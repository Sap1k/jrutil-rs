"""The model's input columns and its physical posts: which candidates of a stop are one post.

The training table (`training_table.py`, from the Rust estimator's `post_scorer_rows`) carries
the features; `post_mass` sums a post's duplicates, found here, as the Rust scorer does.
"""

from __future__ import annotations

import numpy as np
import polars as pl

# Model inputs, in a fixed order. Source identity (sourceKind, catalogue name,
# source-support weight and network-specific OSM refs) is excluded on purpose.
FEATURE_COLUMNS = [
    # Frozen 2026-09-28 by leave-one-out ablation on PID validation; the 17-feature set beat the
    # full 34 (log-loss 0.699 vs 0.704, same coverage at 0.9 precision). JrUtil's port computes
    # exactly these, in this order.
    "side",
    "alignment",
    "proximity",
    "corridor_distance",
    "routed_fit",
    "routed_excess_metres_capped",
    "excess_minus_context_min",
    "eligible",
    "has_local_ref",
    "is_bay",
    "post_support",
    "post_support_deficit",
    "is_tram",
    "is_trolleybus",
    "anchor_previous_missing",
    "anchor_next_missing",
    "service_edge_count",
]


MAXIMUM_POST_DIAMETER_METRES = 25.0


def _cluster_stop(latitude, longitude, sources, local_refs, maximum_diameter=MAXIMUM_POST_DIAMETER_METRES):
    """Source-constrained complete-linkage clustering of one stop's candidates.

    Every source maps a physical post at most once, so a post may contain each
    source only once: two OSM platforms 4 m apart stay separate posts, while
    four catalogue/OSM points of one post spread over 17 m merge. Merges run
    closest-first, the merged diameter stays within `maximum_diameter`, and
    differing local_ref values never merge. Returns a cluster index per input.
    """
    count = len(latitude)
    lat = np.asarray(latitude, dtype=float)
    lon = np.asarray(longitude, dtype=float)
    y = lat * 110_540.0
    x = lon * 111_320.0 * np.cos(np.radians(lat.mean() if count else 50.0))
    distance = np.hypot(x[:, None] - x[None, :], y[:, None] - y[None, :])
    clusters = [{index} for index in range(count)]
    cluster_sources = [set(sources[index]) for index in range(count)]
    cluster_refs = [{local_refs[index]} - {None} for index in range(count)]
    while True:
        best = None
        for left in range(len(clusters)):
            for right in range(left + 1, len(clusters)):
                if cluster_sources[left] & cluster_sources[right]:
                    continue
                refs = cluster_refs[left] | cluster_refs[right]
                if len(refs) > 1:
                    continue
                diameter = max(distance[i, j] for i in clusters[left] for j in clusters[right])
                if diameter > maximum_diameter:
                    continue
                key = (diameter, min(clusters[left] | clusters[right]))
                if best is None or key < best[0]:
                    best = (key, left, right)
        if best is None:
            break
        _, left, right = best
        clusters[left] |= clusters.pop(right)
        cluster_sources[left] |= cluster_sources.pop(right)
        cluster_refs[left] |= cluster_refs.pop(right)
    assignment = np.zeros(count, dtype=int)
    for index, members in enumerate(clusters):
        for member in members:
            assignment[member] = index
    return assignment


def post_clusters(facts: pl.DataFrame) -> pl.DataFrame:
    """Physical-post id, source support and diameter for every candidate, per stop."""
    rows = []
    for (stop_id,), group in facts.group_by(["stop_id"], maintain_order=True):
        group = group.sort("hypothesis_id")
        sources = [(value or "?").split(";") for value in group["sources"].to_list()]
        assignment = _cluster_stop(group["latitude"].to_list(), group["longitude"].to_list(),
                                   sources, group["local_ref"].to_list())
        for hypothesis_id, cluster, candidate_sources in zip(group["hypothesis_id"].to_list(), assignment, sources):
            rows.append((hypothesis_id, f"{stop_id}:{cluster}", candidate_sources))
    table = pl.DataFrame(rows, schema=["hypothesis_id", "post_id", "candidate_sources"], orient="row")
    support = table.explode("candidate_sources", empty_as_null=True).group_by("post_id").agg(
        pl.col("candidate_sources").n_unique().alias("post_support")
    )
    return table.drop("candidate_sources").join(support, on="post_id")


def same_post_pairs(candidates: pl.DataFrame, key: str = "context_id") -> pl.DataFrame:
    """Pairs (including each candidate with itself) that belong to the same physical post.

    Uses the table's `post_id` (from `post_clusters`) when present; otherwise
    clusters the candidates of each `key` group directly.
    """
    if "post_id" in candidates.columns:
        posts = candidates.select(key, "hypothesis_id", "post_id")
    else:
        facts = candidates.select(
            pl.col(key).alias("stop_id"), "hypothesis_id", "latitude", "longitude", "local_ref", "sources"
        ).unique(["stop_id", "hypothesis_id"])
        posts = (
            post_clusters(facts).join(facts.select(pl.col("stop_id").alias(key), "hypothesis_id"), on="hypothesis_id")
            .select(key, "hypothesis_id", "post_id")
        )
    return posts.join(posts.rename({"hypothesis_id": "same_post_id"}), on=[key, "post_id"]).select(
        key, "hypothesis_id", "same_post_id"
    )
