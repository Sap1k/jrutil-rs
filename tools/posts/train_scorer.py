"""Retrain the learned post scorer on a jrutil-rs training table and judge it against the shipped
model on stops neither model was fitted or calibrated on.

Usage (from `tools/posts`):
    uv run python train_scorer.py <candidates.parquet> <shipped policy.json> <output policy.json>

The split follows the one the shipped model was made with: stops are hashed into five folds by `stable_fold`, fold 0 is the test, folds 1-4 train. The shipped
model was fitted on folds 1-4 too, so on fold 0 neither model has seen a stop.

* Thresholds: each training fold is scored by a model fitted on the other three (out of fold),
  and the thresholds are calibrated on those scores to the call precision the shipped model
  reaches on the same stops: the post thresholds to its precision in each stratum (simple and
  complex stops), the area threshold to its precision overall, then all raised together until
  the whole reaches it as well. The shipped model was fitted there, so the target favours it.
* The candidate is then fitted on folds 1-4 and given those thresholds.
* Both are judged on fold 0 the same way: every published decision counts (a post or an area's
  post), and stage 2 sees every context of the stop, as in production. Counts are call-weighted
  (a context's weight is its published calls), which is what the golden accuracy measures.

The output policy is the shipped document with the candidate model; it is written only when the
candidate has more correct calls on fold 0 and no higher wrong share, so what ships is exactly
what was tested.
"""

import json
import sys
from pathlib import Path

import polars as pl

from post_scorer.model import (
    COMPLEX_STOP_POSTS,
    calibrate_stratified,
    calibrate_threshold,
    decisions,
    metrics,
    post_mass,
    stable_fold,
    strata,
    train_two_stage,
)

FIELDS = ("labelled", "published", "correct", "wrong", "precision", "coverage",
          "weighted_precision", "weighted_coverage", "street_over_bay")


def scored(model, table: pl.DataFrame) -> pl.DataFrame:
    """Production-like: stage 2 sees every context of the stop; labels are applied afterwards."""
    return post_mass(model.probabilities(table)).filter(pl.col("labelled"))


def weighted(result: pl.DataFrame, published: str, y: str) -> dict:
    labelled = result.filter(pl.col("labelled"))
    chosen = labelled.filter(pl.col(published))
    correct = chosen.filter(pl.col(y).fill_null(0) == 1)["weight"].sum()
    total = chosen["weight"].sum()
    return {"calls_published": total, "calls_correct": correct, "calls_wrong": total - correct,
            "calls_labelled": labelled["weight"].sum()}


def judge(result: pl.DataFrame) -> dict:
    stats = metrics(result)
    return {
        "model": {**{k: stats["model"].get(k) for k in FIELDS},
                  **weighted(result, "model_published", "model_y")},
        "baseline": {**{k: stats["baseline"].get(k) for k in FIELDS},
                     **weighted(result, "baseline_published", "baseline_y")},
    }


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    table = pl.read_parquet(sys.argv[1])
    shipped_path, output = Path(sys.argv[2]), Path(sys.argv[3])
    # The shipped model's area decisions publish a post too; count them like the candidate's.
    table = table.with_columns(pl.col("baseline_resolution").replace({"Area": "Physical"}))
    # Stops without a labelled context cannot affect training or the comparison.
    table = table.filter(pl.col("labelled").any().over("stop_id"))
    folds = table.select(pl.col("stop_id").unique()).with_columns(
        pl.col("stop_id").map_elements(stable_fold, return_dtype=pl.Int64).alias("fold")
    )
    table = table.join(folds, on="stop_id")
    training = table.filter(pl.col("fold") != 0)
    test = table.filter(pl.col("fold") == 0).drop("fold")
    print(f"training stops {training['stop_id'].n_unique()}, test stops {test['stop_id'].n_unique()}; "
          f"labelled contexts {training.filter('labelled')['context_id'].n_unique()} / "
          f"{test.filter('labelled')['context_id'].n_unique()}", flush=True)

    out_of_fold = []
    for fold in range(1, 5):
        model = train_two_stage(training.filter(pl.col("fold") != fold).drop("fold"))
        out_of_fold.append(scored(model, training.filter(pl.col("fold") == fold).drop("fold")))
        print(f"  out-of-fold scores for fold {fold}", flush=True)
    out_of_fold = pl.concat(out_of_fold, how="diagonal_relaxed")
    def shipped_precision(part: pl.DataFrame) -> float:
        stats = judge(decisions(part, {"simple": 1.1, "complex": 1.1, "area": 1.1}))["baseline"]
        return stats["calls_correct"] / stats["calls_published"]

    complex_stop = pl.col("stop_post_count") >= COMPLEX_STOP_POSTS
    target = shipped_precision(out_of_fold)
    targets = {"simple": shipped_precision(out_of_fold.filter(~complex_stop)),
               "complex": shipped_precision(out_of_fold.filter(complex_stop))}
    print(f"shipped model's call precision on the training stops: {target:.4f}, "
          f"simple {targets['simple']:.4f}, complex {targets['complex']:.4f}", flush=True)
    # The area threshold as calibrated for the whole; the post thresholds per stratum, to the
    # shipped model's precision in that stratum.
    thresholds = calibrate_stratified(out_of_fold, target)
    for stratum, part in (("simple", out_of_fold.filter(~complex_stop)),
                          ("complex", out_of_fold.filter(complex_stop))):
        thresholds[stratum] = calibrate_threshold(lambda t, part=part: decisions(part, t), targets[stratum])
    # Each stratum reaches the target on its own; area decisions can still pull the whole below
    # it, so every threshold is raised together until the whole reaches it too.
    while True:
        result = judge(decisions(out_of_fold, thresholds))["model"]
        if result["calls_correct"] >= target * result["calls_published"] or min(thresholds.values()) > 1:
            break
        thresholds = {key: round(value + 0.01, 2) for key, value in thresholds.items()}
    print(f"thresholds {thresholds}: out-of-fold call precision "
          f"{result['calls_correct'] / max(result['calls_published'], 1):.4f}", flush=True)

    model = train_two_stage(training.drop("fold"))
    model.thresholds = thresholds
    test_scored = scored(model, test)
    report = {population: judge(decisions(part, thresholds)) for population, part in strata(test_scored)}
    if "region" in test_scored.columns:
        for region in sorted(test_scored["region"].drop_nulls().unique().to_list()):
            report[f"region {region}"] = judge(
                decisions(test_scored.filter(pl.col("region") == region), thresholds))
    print(json.dumps({"thresholds": thresholds, "test_fold": report}, indent=2, default=float))

    new, old = report["all"]["model"], report["all"]["baseline"]
    share = lambda stats: stats["calls_wrong"] / stats["calls_published"] if stats["calls_published"] else 0.0
    better = new["calls_correct"] > old["calls_correct"] and share(new) <= share(old)
    print(f"correct calls {new['calls_correct']:.0f} vs shipped {old['calls_correct']:.0f}; "
          f"wrong share {share(new):.4f} vs {share(old):.4f}: {'better' if better else 'NOT better'}")
    if not better:
        return 1
    document = json.loads(shipped_path.read_text(encoding="utf-8"))
    document["scorer"]["model"] = model.to_json()
    output.write_text(json.dumps(document, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"wrote {output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
