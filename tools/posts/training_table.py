"""The learned post scorer's candidate table, from jrutil-rs's own reports (POST-R141).

Usage:
    training_table.py <reports dir> <overlay package> <output.parquet>

<reports dir> is `[jdf.posts] reports` of a build with `training = true`. Capture it without
the labelled regions' own catalogues (PID.csv, MPVNet_PID.csv, IDSJMK.csv): they list every bay
of the region, and the labels would leak into the candidates (the scorer README's guard).

<overlay package> is a road package with the regional feeds overlaid, built on a base without
estimated posts: for the calls a regional feed was bound to it states the real post. Labels
follow the scorer README (`jrutil/scripts/post-scorer`):

* a context's real post is the majority regional post of its labelled calls (share >= 0.8,
  else the context is unlabelled);
* a candidate within 20 m of it is correct; no candidate within 35 m is "no correct candidate";
  a nearest candidate between 20 and 35 m, or every candidate beyond 300 m, leaves the context
  unlabelled.

The weight of a context is its number of published calls. The baseline columns are the
decision the shipped model made (the `post_decisions` report), so `post_scorer.model` compares
a new model with it on the same stops. Authored contexts are left out: they are not scored.

Needs polars and pyarrow (the post-scorer environment has both).
"""

import collections
import math
import sys
from pathlib import Path

import polars as pl
import pyarrow.parquet as pq

CORRECT_METRES = 20.0
AMBIGUOUS_METRES = 35.0
ALIGNMENT_ERROR_METRES = 300.0
MAJORITY_SHARE = 0.8


def metres(a, b):
    north = (a[0] - b[0]) * 111_320
    east = (a[1] - b[1]) * 111_320 * math.cos(math.radians((a[0] + b[0]) / 2))
    return math.hypot(east, north)


def truth(overlay):
    """(national trip, sequence) -> the regional post's position, where every slice agrees."""
    locations = pq.read_table(
        f"{overlay}/serving/location.parquet", columns=["location_id", "latitude", "longitude"]
    ).to_pydict()
    position = {
        location: (latitude, longitude)
        for location, latitude, longitude in zip(
            locations["location_id"], locations["latitude"], locations["longitude"]
        )
        if latitude is not None
    }
    calls = pq.read_table(
        f"{overlay}/serving/trip_call.parquet",
        columns=["trip_id", "sequence", "boarding_point_id"],
    ).to_pydict()
    found, clashing = {}, set()
    for trip, sequence, post in zip(calls["trip_id"], calls["sequence"], calls["boarding_point_id"]):
        if not post or not post.startswith("overlay:") or post not in position:
            continue
        if not trip.startswith("jdf:trip:"):
            continue
        key = (trip.split(":overlay:")[0], sequence)
        if found.setdefault(key, post) != post:
            clashing.add(key)
    for key in clashing:
        del found[key]
    return {key: (post, position[post]) for key, post in found.items()}


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    reports, overlay, output = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3])
    rows = pl.read_csv(reports / "post_scorer_rows.csv", infer_schema_length=0)
    features = rows.columns[rows.columns.index("longitude") + 1:]
    rows = rows.with_columns(
        pl.col("stop_id").cast(pl.Int64),
        pl.col("stop_post_count").cast(pl.Int64),
        pl.col("is_platform").cast(pl.Int64),
        pl.col("latitude").cast(pl.Float64),
        pl.col("longitude").cast(pl.Float64),
        *[pl.col(name).cast(pl.Float64) for name in features],
        pl.when(pl.col("previous_stop") == "").then(None).otherwise(pl.col("previous_stop")).alias("previous_stop"),
        pl.when(pl.col("next_stop") == "").then(None).otherwise(pl.col("next_stop")).alias("next_stop"),
    )
    decisions = pl.read_csv(reports / "post_decisions.csv", infer_schema_length=0)
    authored = set(decisions.filter(pl.col("authored_post_key") != "")["context_id"].to_list())
    rows = rows.filter(~pl.col("context_id").is_in(authored))

    # Labels: the majority real post of each context's calls.
    real = truth(overlay)
    calls = pl.read_csv(reports / "post_context_calls.csv", infer_schema_length=0)
    weights = collections.Counter()
    posts = collections.defaultdict(collections.Counter)
    where = {}
    for trip, sequence, context in calls.iter_rows():
        weights[context] += 1
        hit = real.get((trip, int(sequence)))
        if hit is not None:
            posts[context][hit[0]] += 1
            where[hit[0]] = hit[1]
    majority = {}
    for context, counter in posts.items():
        post, count = counter.most_common(1)[0]
        if count / sum(counter.values()) >= MAJORITY_SHARE:
            majority[context] = where[post]

    labelled, y = [], []
    candidates_of = collections.defaultdict(list)
    for index, (context, latitude, longitude) in enumerate(
        rows.select("context_id", "latitude", "longitude").iter_rows()
    ):
        candidates_of[context].append((index, (latitude, longitude)))
    is_labelled = [False] * rows.height
    is_correct = [0] * rows.height
    no_correct = [False] * rows.height
    counts = collections.Counter()
    for context, members in candidates_of.items():
        post = majority.get(context)
        if post is None:
            counts["unlabelled: no majority post"] += 1
            continue
        distances = [metres(position, post) for _, position in members]
        nearest = min(distances)
        if nearest > ALIGNMENT_ERROR_METRES:
            counts["dropped: alignment error"] += 1
            continue
        if CORRECT_METRES < nearest <= AMBIGUOUS_METRES:
            counts["dropped: ambiguous"] += 1
            continue
        counts["labelled" if nearest <= CORRECT_METRES else "labelled: no correct candidate"] += 1
        for (index, _), distance in zip(members, distances):
            is_labelled[index] = True
            is_correct[index] = int(distance <= CORRECT_METRES)
            no_correct[index] = nearest > AMBIGUOUS_METRES

    # The shipped model's decisions as the baseline.
    baseline = decisions.select(
        "context_id",
        pl.col("hypothesis_id").alias("baseline_hypothesis"),
        pl.col("kind")
        .replace_strict({"physical": "Physical", "area": "Area"}, default="Abstain")
        .alias("baseline_resolution"),
    )
    bays = rows.group_by("stop_id").agg(
        pl.col("hypothesis_id").filter(pl.col("is_bay") > 0).n_unique().alias("stop_bay_candidates")
    )
    table = (
        rows.with_columns(
            pl.Series("labelled", is_labelled),
            pl.Series("y", is_correct),
            pl.Series("no_correct_candidate", no_correct),
            pl.col("context_id").replace_strict(dict(weights), default=1).cast(pl.Float64).alias("weight"),
        )
        .join(baseline, on="context_id", how="left")
        .join(bays, on="stop_id", how="left")
        .with_columns(
            (pl.col("hypothesis_id") == pl.col("baseline_hypothesis")).fill_null(False).alias("baseline_selected"),
            pl.col("baseline_resolution").fill_null("Abstain"),
        )
        .drop("baseline_hypothesis")
    )
    table.write_parquet(output)
    print(f"{table.height} candidate rows, {table['context_id'].n_unique()} contexts, features {features}")
    for name, count in sorted(counts.items()):
        print(f"  {name}: {count}")


if __name__ == "__main__":
    main()
