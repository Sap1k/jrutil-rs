"""Retrain the learned post scorer on a jrutil-rs training table and judge it against the shipped
model on stops neither saw in training or calibration.

Usage (from `jrutil/scripts/post-scorer`, whose `post_scorer` package this uses):
    uv run --with pyarrow python <this> <candidates.parquet> <shipped policy.json> <output policy.json>

Stops are split by `post_scorer.model.stable_fold` (five folds): folds 2-4 train the two-stage
model, fold 1 calibrates its thresholds to the precision the shipped model reaches on fold 1
(so both are compared at the same operating point), fold 0 is the test. On the test fold the
new model is compared with the shipped model's own decisions (the table's baseline columns). The output policy is the shipped document with the new model;
it is written only when the new model has more correct decisions and no higher wrong share
there, so what ships is exactly what was tested.
"""

import json
import sys
from pathlib import Path

import polars as pl

from post_scorer.model import (
    decisions,
    calibrate_stratified,
    metrics,
    post_mass,
    stable_fold,
    strata,
    train_two_stage,
)


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    table = pl.read_parquet(sys.argv[1])
    shipped_path, output = Path(sys.argv[2]), Path(sys.argv[3])
    folds = table.select(pl.col("stop_id").unique()).with_columns(
        pl.col("stop_id").map_elements(stable_fold, return_dtype=pl.Int64).alias("fold")
    )
    table = table.join(folds, on="stop_id")
    training = table.filter(pl.col("fold") >= 2).drop("fold")
    calibration = table.filter(pl.col("fold") == 1).drop("fold")
    test = table.filter(pl.col("fold") == 0).drop("fold")

    model = train_two_stage(training)
    calibrated = post_mass(model.probabilities(calibration.filter(pl.col("labelled"))))
    shipped = metrics(decisions(calibrated, {"simple": 1.1, "complex": 1.1}))["baseline"]
    target = shipped["precision"]
    print(f"shipped model's precision on the calibration fold: {target:.4f}")
    model.thresholds = calibrate_stratified(calibrated, target)
    scored = post_mass(model.probabilities(test.filter(pl.col("labelled"))))
    report = {}
    for population, part in strata(scored):
        result = metrics(decisions(part, model.thresholds))
        report[population] = {
            system: {
                key: result[system].get(key)
                for key in ("labelled", "published", "correct", "wrong", "precision", "coverage",
                            "weighted_precision", "weighted_coverage", "street_over_bay")
            }
            for system in ("model", "baseline")
        }
    print(json.dumps({"thresholds": model.thresholds, "test_fold": report}, indent=2))

    new, old = report["all"]["model"], report["all"]["baseline"]
    wrong_share = lambda stats: stats["wrong"] / stats["published"] if stats["published"] else 0.0
    better = new["correct"] > old["correct"] and wrong_share(new) <= wrong_share(old)
    print(f"new correct {new['correct']} vs shipped {old['correct']}; "
          f"wrong share {wrong_share(new):.4f} vs {wrong_share(old):.4f}: "
          f"{'better' if better else 'NOT better'}")
    if not better:
        return 1
    document = json.loads(shipped_path.read_text(encoding="utf-8"))
    document["scorer"]["model"] = model.to_json()
    output.write_text(json.dumps(document, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"wrote {output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
