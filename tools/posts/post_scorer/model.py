"""Conditional-logit post scorer with an explicit "no correct candidate" option. Training and
judging run from `train_scorer.py`; the Rust scorer (`jr-posts`, `evaluate/scorer.rs`) reproduces
the exported model.

Each context is a choice among its candidates plus a null alternative whose
utility is a learned constant. Softmax probabilities therefore express both
"which post" and "none of these"; a candidate is published when its
probability reaches the calibrated threshold. Training/validation split is by
stop (deterministic hash), so contexts of one terminal never straddle it.
"""

from __future__ import annotations

import hashlib
from dataclasses import dataclass

import numpy as np
import polars as pl
from scipy.optimize import minimize

from post_scorer.collective import COLLECTIVE_COLUMNS, collective_features
from post_scorer.posts import FEATURE_COLUMNS, same_post_pairs

MODEL_FORMAT = "post-scorer-conditional-logit-v1"


@dataclass
class ContextBatch:
    """Candidates sorted by context; `starts` delimits each context's rows."""

    context_ids: np.ndarray
    features: np.ndarray
    y: np.ndarray
    starts: np.ndarray
    null_target: np.ndarray
    context_weight: np.ndarray | None = None

    @classmethod
    def from_table(cls, table: pl.DataFrame, columns: list[str]) -> ContextBatch:
        table = table.sort("context_id", "hypothesis_id")
        context_ids = table["context_id"].to_numpy()
        starts = np.flatnonzero(np.r_[True, context_ids[1:] != context_ids[:-1]])
        y = table["y"].to_numpy().astype(float) if "y" in table.columns else np.zeros(table.height)
        positives = np.add.reduceat(y, starts) if len(starts) else np.zeros(0)
        return cls(
            context_ids=context_ids[starts],
            features=table.select(columns).to_numpy().astype(float),
            y=y,
            starts=starts,
            null_target=(positives == 0).astype(float),
            context_weight=(table["weight"].to_numpy().astype(float)[starts]
                            if "weight" in table.columns and len(starts) else None),
        )


def stable_fold(stop_id: int, folds: int = 5) -> int:
    return int(hashlib.sha256(str(stop_id).encode()).hexdigest()[:8], 16) % folds


def _segment_softmax(utilities: np.ndarray, null_utility: float, starts: np.ndarray):
    """Per-context softmax over candidates plus the null option."""
    maxima = np.maximum(np.maximum.reduceat(utilities, starts), null_utility)
    counts = np.diff(np.r_[starts, len(utilities)])
    shifted = np.exp(utilities - np.repeat(maxima, counts))
    null_exp = np.exp(null_utility - maxima)
    denominators = np.add.reduceat(shifted, starts) + null_exp
    return shifted / np.repeat(denominators, counts), null_exp / denominators


def fit(batch: ContextBatch, l2: float = 1.0) -> tuple[np.ndarray, float]:
    columns = batch.features.shape[1]

    def objective(parameters: np.ndarray):
        weights, null_utility = parameters[:columns], parameters[columns]
        probabilities, null_probability = _segment_softmax(batch.features @ weights, null_utility, batch.starts)
        # A context may have several correct candidates (one physical post
        # mapped more than once); the target is the total mass on them.
        chosen = np.add.reduceat(batch.y * probabilities, batch.starts) + batch.null_target * null_probability
        chosen = np.maximum(chosen, 1e-300)
        # Traffic weights scale each context's term (and its gradient); unweighted = 1.
        context_weight = batch.context_weight if batch.context_weight is not None else np.ones(len(chosen))
        loss = -(context_weight * np.log(chosen)).sum() + 0.5 * l2 * weights @ weights
        # d(-log sum_pos p)/dw = sum_j p_j x_j - sum_pos (p_k / sum_pos p) x_k
        counts = np.diff(np.r_[batch.starts, len(batch.y)])
        row_weight = np.repeat(context_weight, counts)
        responsibility = batch.y * probabilities / np.repeat(chosen, counts)
        expected = ((row_weight * probabilities)[:, None] * batch.features).sum(axis=0)
        observed = ((row_weight * responsibility)[:, None] * batch.features).sum(axis=0)
        gradient = expected - observed + l2 * weights
        # The null option's own gradient: sum w p_null - sum over null targets of w.
        null_gradient = (context_weight * null_probability).sum() - (context_weight * batch.null_target).sum()
        return loss, np.r_[gradient, null_gradient]

    result = minimize(objective, np.zeros(columns + 1), jac=True, method="L-BFGS-B")
    if not result.success:
        raise RuntimeError(f"Optimisation failed: {result.message}")
    return result.x[:columns], float(result.x[columns])


@dataclass
class Model:
    columns: list[str]
    mean: np.ndarray
    scale: np.ndarray
    weights: np.ndarray
    null_utility: float
    threshold: float = 0.5

    def probabilities(self, table: pl.DataFrame) -> pl.DataFrame:
        batch = ContextBatch.from_table(table, self.columns)
        standardized = (batch.features - self.mean) / self.scale
        probabilities, null_probability = _segment_softmax(standardized @ self.weights, self.null_utility, batch.starts)
        counts = np.diff(np.r_[batch.starts, len(batch.y)])
        return table.sort("context_id", "hypothesis_id").with_columns(
            pl.Series("probability", probabilities),
            pl.Series("null_probability", np.repeat(null_probability, counts)),
        )

    def to_json(self) -> dict:
        return {
            "format": MODEL_FORMAT,
            "features": self.columns,
            "mean": self.mean.tolist(),
            "scale": self.scale.tolist(),
            "weights": self.weights.tolist(),
            "null_utility": self.null_utility,
            "threshold": self.threshold,
        }

    @classmethod
    def from_json(cls, data: dict) -> Model:
        if data.get("format") != MODEL_FORMAT:
            raise ValueError(f"Unsupported model format: {data.get('format')}")
        return cls(
            columns=list(data["features"]),
            mean=np.asarray(data["mean"]),
            scale=np.asarray(data["scale"]),
            weights=np.asarray(data["weights"]),
            null_utility=float(data["null_utility"]),
            threshold=float(data["threshold"]),
        )


def train(table: pl.DataFrame, columns: list[str] = FEATURE_COLUMNS, l2: float = 1.0) -> Model:
    table = table.filter(pl.col("labelled"))
    features = table.select(columns).to_numpy().astype(float)
    mean = features.mean(axis=0)
    scale = features.std(axis=0)
    scale[scale < 1e-9] = 1.0
    standardized = table.with_columns(
        [((pl.col(column) - mean[index]) / scale[index]).alias(column) for index, column in enumerate(columns)]
    )
    weights, null_utility = fit(ContextBatch.from_table(standardized, columns), l2)
    return Model(columns, mean, scale, weights, null_utility)


def post_mass(scored: pl.DataFrame) -> pl.DataFrame:
    """Per candidate: probability summed over its same-post duplicates.

    One physical post is often mapped by several sources a few metres apart and
    the softmax splits its probability across those points, so the decision
    uses the post's mass. Duplicates follow `posts.same_post_pairs`, which
    keeps distinct bays apart even when they are close together.
    """
    if "post_id" not in scored.columns and not {"latitude", "longitude", "sources", "local_ref"} <= set(scored.columns):
        return scored.with_columns(pl.col("probability").alias("post_probability"))
    probabilities = scored.select(
        "context_id", pl.col("hypothesis_id").alias("same_post_id"), pl.col("probability").alias("n_probability")
    )
    mass = (
        same_post_pairs(scored)
        .join(probabilities, on=["context_id", "same_post_id"])
        .group_by("context_id", "hypothesis_id")
        .agg(pl.col("n_probability").sum().clip(upper_bound=1.0).alias("post_probability"))
    )
    return scored.join(mass, on=["context_id", "hypothesis_id"], how="left")


COMPLEX_STOP_POSTS = 3


def threshold_expression(threshold, columns) -> pl.Expr:
    """A float, or {"simple": x, "complex": y} split by the stop's number of physical posts."""
    if isinstance(threshold, dict):
        if "stop_post_count" not in columns:
            return pl.lit(threshold["complex"])
        return (pl.when(pl.col("stop_post_count") >= COMPLEX_STOP_POSTS)
                .then(pl.lit(threshold["complex"])).otherwise(pl.lit(threshold["simple"])))
    return pl.lit(float(threshold))


def decisions(scored: pl.DataFrame, threshold) -> pl.DataFrame:
    """One row per context: the model's post and publication, baseline choice, truth.

    1. Physical: the heaviest physical post reaches the stratum threshold.
    2. Area: otherwise, when `threshold["area"]` is set and the heaviest area
       (posts within 40 m sharing a mode class, e.g. a bus station's bays)
       reaches it, the area's most probable post is published.
    3. Otherwise abstain. Within the chosen post, its OSM platform point is
       published when there is one (best-mapped geometry).
    """
    scored = scored if "post_probability" in scored.columns else post_mass(scored)
    platform = pl.col("is_platform").fill_null(0) if "is_platform" in scored.columns else pl.lit(0)
    has_area = "area_id" in scored.columns
    scored = scored.with_columns(
        platform.alias("_platform"),
        (pl.col("probability").sum().over("context_id", "area_id") if has_area
         else pl.col("post_probability")).alias("area_probability"),
    )
    top = (
        scored.sort(["context_id", "post_probability", "_platform", "probability", "hypothesis_id"],
                    descending=[False, True, True, True, False])
        .group_by("context_id", maintain_order=True).first()
    )
    area_top = (
        scored.sort(["context_id", "area_probability", "post_probability", "_platform", "probability", "hypothesis_id"],
                    descending=[False, True, True, True, True, False])
        .group_by("context_id", maintain_order=True).first()
    )
    second = (
        scored.sort(["context_id", "probability"], descending=[False, True])
        .group_by("context_id", maintain_order=True)
        .agg(pl.col("probability").slice(1, 1).first().fill_null(0.0).alias("second_probability"))
    )
    columns = ["context_id", "stop_id", "hypothesis_id", "probability", "post_probability", "area_probability",
               "null_probability", "is_bay", "stop_bay_candidates", "stop_post_count", "baseline_resolution", "y",
               "area_id", "weight"]
    physical = top.select([c for c in columns if c in top.columns]).with_columns(
        (pl.col("post_probability") >= threshold_expression(threshold, top.columns)).alias("_physical")
    )
    area_threshold = threshold.get("area") if isinstance(threshold, dict) else None
    area = area_top.select([c for c in columns if c in area_top.columns]).with_columns(
        (pl.col("area_probability") >= (area_threshold if area_threshold is not None else 9.0)).alias("_area")
    )
    result = (
        physical.join(area.select("context_id", "_area"), on="context_id")
        .with_columns(pl.when(pl.col("_physical")).then(pl.lit("Physical"))
                      .when(pl.col("_area")).then(pl.lit("Area")).otherwise(pl.lit("Abstain")).alias("model_resolution"))
    )
    # Area decisions publish the area's best post instead of the overall heaviest post.
    area_choice = area.select(
        "context_id", *[pl.col(c).alias(f"area_{c}") for c in ("hypothesis_id", "y", "is_bay", "stop_post_count",
                                                                "post_probability", "area_probability")
                        if c in area.columns]
    )
    is_area = pl.col("model_resolution") == "Area"
    result = result.join(area_choice, on="context_id").with_columns(
        pl.when(is_area).then(pl.col("area_hypothesis_id")).otherwise(pl.col("hypothesis_id")).alias("hypothesis_id"),
        # Probabilities describe the published post (as JrUtil reports them).
        pl.when(is_area).then(pl.col("area_post_probability")).otherwise(pl.col("post_probability"))
        .alias("post_probability"),
        pl.when(is_area).then(pl.col("area_area_probability")).otherwise(pl.col("area_probability"))
        .alias("area_probability"),
        *([pl.when(pl.col("model_resolution") == "Area").then(pl.col("area_is_bay")).otherwise(pl.col("is_bay"))
           .alias("is_bay")] if "is_bay" in result.columns else []),
        (pl.col("model_resolution") != "Abstain").alias("model_published"),
    ).join(second, on="context_id")
    baseline_row = scored.filter(pl.col("baseline_selected")).select(
        "context_id", pl.col("is_bay").alias("baseline_is_bay"),
        *([pl.col("y").alias("baseline_y")] if "y" in scored.columns else []),
    )
    result = result.join(baseline_row, on="context_id", how="left").with_columns(
        (pl.col("baseline_resolution") == "Physical").fill_null(False).alias("baseline_published")
    )
    if "y" in scored.columns:
        truth = scored.group_by("context_id").agg(
            pl.col("labelled").first(), pl.col("no_correct_candidate").first(), (pl.col("y").max() > 0).alias("has_positive")
        )
        area_truth = (scored.group_by("context_id", "area_id").agg((pl.col("y").max() > 0).alias("area_has_truth"))
                      if has_area else None)
        result = result.join(truth, on="context_id").with_columns(
            pl.when(pl.col("model_resolution") == "Area").then(pl.col("area_y")).otherwise(pl.col("y")).alias("model_y")
        )
        if area_truth is not None:
            chosen_area = scored.select("context_id", "hypothesis_id", "area_id").unique(["context_id", "hypothesis_id"])
            result = (result.drop("area_id", strict=False)
                      .join(chosen_area, on=["context_id", "hypothesis_id"], how="left")
                      .join(area_truth, on=["context_id", "area_id"], how="left")
                      .with_columns(pl.col("area_has_truth").fill_null(False)))
    return result.drop([c for c in ("_physical", "_area", "y", "area_y", "area_post_probability",
                                    "area_area_probability") if c in result.columns])

def metrics(result: pl.DataFrame) -> dict:
    """Label-based metrics (when labels exist) plus the label-free street-over-bay count."""
    report: dict = {"contexts": result.height}
    for system, published, in_station in (
        ("model", "model_published", "is_bay"),
        ("baseline", "baseline_published", "baseline_is_bay"),
    ):
        stats: dict = {"published": int(result[published].sum())}
        # Street-over-bay: publishing a non-station post where station bays exist.
        stats["street_over_bay"] = int(
            result.filter(
                pl.col(published) & (pl.col("stop_bay_candidates") > 0) & (pl.col(in_station).fill_null(0) == 0)
            ).height
        )
        if "labelled" in result.columns:
            labelled = result.filter(pl.col("labelled"))
            y_column = "model_y" if system == "model" else "baseline_y"
            chosen = labelled.filter(pl.col(published))
            correct = int(chosen.filter(pl.col(y_column).fill_null(0) == 1).height)
            answerable = labelled.filter(pl.col("has_positive"))
            unanswerable = labelled.filter(~pl.col("has_positive"))
            if "weight" in labelled.columns:
                chosen_weight = chosen["weight"].sum()
                correct_weight = chosen.filter(pl.col(y_column).fill_null(0) == 1)["weight"].sum()
                answerable_weight = labelled.filter(pl.col("has_positive"))["weight"].sum()
                stats["weighted_precision"] = correct_weight / chosen_weight if chosen_weight else None
                stats["weighted_coverage"] = correct_weight / answerable_weight if answerable_weight else None
            stats.update(
                labelled=labelled.height,
                correct=correct,
                wrong=chosen.height - correct,
                precision=correct / chosen.height if chosen.height else None,
                coverage=correct / answerable.height if answerable.height else None,
                abstain_on_no_candidate=(
                    1.0 - unanswerable.filter(pl.col(published)).height / unanswerable.height
                    if unanswerable.height
                    else None
                ),
            )
            if system == "model" and "model_resolution" in result.columns:
                area = labelled.filter(pl.col("model_resolution") == "Area")
                stats["area_decisions"] = area.height
                stats["area_post_correct"] = int(area.filter(pl.col("model_y").fill_null(0) == 1).height)
                if "area_has_truth" in area.columns:
                    stats["area_contains_truth"] = int(area.filter(pl.col("area_has_truth")).height)
            if system == "model":
                stats["top1_accuracy"] = (
                    answerable.filter(pl.col("model_y") == 1).height / answerable.height if answerable.height else None
                )
        report[system] = stats
    return report


def calibrate_threshold(result_for: callable, target_precision: float | None) -> float:
    """Lowest threshold whose precision reaches the target; never publish when none does."""
    for threshold in np.round(np.arange(0.30, 0.995, 0.01), 2):
        stats = metrics(result_for(float(threshold)))["model"]
        # Calibrate on traffic-weighted precision when weights are present.
        precision = stats.get("weighted_precision", stats.get("precision"))
        if precision is not None and (target_precision is None or precision >= target_precision):
            return float(threshold)
    return 1.01


def calibrate_stratified(scored: pl.DataFrame, target_precision: float) -> dict:
    """Post thresholds per stratum, then an area threshold for the contexts they leave unpublished.

    Area decisions count as correct when the true post lies in the chosen area.
    """
    complex_stop = pl.col("stop_post_count") >= COMPLEX_STOP_POSTS
    thresholds = {
        "simple": calibrate_threshold(lambda t: decisions(scored.filter(~complex_stop), t), target_precision),
        "complex": calibrate_threshold(lambda t: decisions(scored.filter(complex_stop), t), target_precision),
    }
    if "area_id" not in scored.columns:
        return thresholds
    for area_threshold in np.round(np.arange(0.50, 0.995, 0.01), 2):
        result = decisions(scored, {**thresholds, "area": float(area_threshold)})
        area = result.filter(pl.col("labelled") & (pl.col("model_resolution") == "Area"))
        if "weight" in area.columns:
            share = (area.filter(pl.col("area_has_truth"))["weight"].sum() / area["weight"].sum()) if area.height else 0.0
        else:
            share = area.filter(pl.col("area_has_truth")).height / area.height if area.height else 0.0
        if area.height and share >= target_precision:
            thresholds["area"] = float(area_threshold)
            return thresholds
    thresholds["area"] = 1.01
    return thresholds


TWO_STAGE_FORMAT = "post-scorer-two-stage-v1"


@dataclass
class TwoStageModel:
    """First stage scores each context alone; the second adds related contexts' beliefs."""

    stage1: Model
    stage2: Model
    thresholds: dict

    def probabilities(self, table: pl.DataFrame) -> pl.DataFrame:
        first = self.stage1.probabilities(table).drop("null_probability")
        return self.stage2.probabilities(collective_features(first).drop("probability"))

    def to_json(self) -> dict:
        return {"format": TWO_STAGE_FORMAT, "stage1": self.stage1.to_json(), "stage2": self.stage2.to_json(),
                "thresholds": self.thresholds}

    @classmethod
    def from_json(cls, data: dict) -> TwoStageModel:
        if data.get("format") != TWO_STAGE_FORMAT:
            raise ValueError(f"Unsupported model format: {data.get('format')}")
        return cls(Model.from_json(data["stage1"]), Model.from_json(data["stage2"]), dict(data["thresholds"]))


def _with_folds(table: pl.DataFrame, folds: int = 5) -> pl.DataFrame:
    return table.join(
        table.select(pl.col("stop_id").unique()).with_columns(
            pl.col("stop_id").map_elements(lambda stop: stable_fold(stop, folds), return_dtype=pl.Int64).alias("fold")
        ),
        on="stop_id",
    )


def train_two_stage(table: pl.DataFrame, l2: float = 1.0) -> TwoStageModel:
    """Stage 2 is fitted on out-of-fold stage-1 scores (folds by stop), never on in-sample ones."""
    table = _with_folds(table.filter(pl.col("labelled")), folds=5)
    out_of_fold = []
    for fold in sorted(table["fold"].unique().to_list()):
        held_out = table.filter(pl.col("fold") == fold).drop("fold")
        out_of_fold.append(train(table.filter(pl.col("fold") != fold).drop("fold"), l2=l2)
                           .probabilities(held_out).drop("null_probability"))
    second_stage = collective_features(pl.concat(out_of_fold)).drop("probability")
    stage2 = train(second_stage, columns=FEATURE_COLUMNS + COLLECTIVE_COLUMNS, l2=l2)
    stage1 = train(table.drop("fold"), l2=l2)
    return TwoStageModel(stage1, stage2, {"simple": 0.5, "complex": 0.5})


def strata(scored: pl.DataFrame):
    complex_stop = pl.col("stop_post_count") >= COMPLEX_STOP_POSTS
    return (("all", scored), ("simple_stops", scored.filter(~complex_stop)),
            ("complex_stops", scored.filter(complex_stop)),
            ("stops_with_bays", scored.filter(pl.col("stop_bay_candidates") > 0)),
            ("trams", scored.filter(pl.col("mode") == "E")))
