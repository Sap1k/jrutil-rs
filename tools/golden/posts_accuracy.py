"""How good are estimated boarding posts, judged by a regional feed that knows the real ones?

Usage:
    posts_accuracy.py <overlay package> <package with estimated posts>...

The overlay package (the road timetable with regional feeds overlaid, built on a base
*without* estimated posts) states, for the calls a regional feed was bound to, the feed's own
boarding point and its position. Each package given after it is a road package with estimated
posts built from the same national inputs. For every call of a national trip whose real post
is known, the estimate is:

    correct    an estimated post within 20 m of the real one
    ambiguous  between 20 and 35 m
    wrong      farther than 35 m
    none       no estimated post (the call keeps the stop's unspecified point)

The thresholds are those of the scorer's training labels (`tools/posts/README.md`). With two
packages the transitions between them are printed as well.

On the golden slice this is not a measure of the estimator: the regional catalogues the labels
come from are among the candidates, so results are not comparable with the held-out test
(`tools/posts/train_scorer.py`); see `docs/golden.md`.

Needs pyarrow (the Oběhy environment has it: `uv run python posts_accuracy.py ...`).
"""

import collections
import math
import sys

import pyarrow.parquet as pq


def locations(package):
    table = pq.read_table(
        f"{package}/serving/location.parquet", columns=["location_id", "latitude", "longitude"]
    ).to_pydict()
    return {
        location: (latitude, longitude)
        for location, latitude, longitude in zip(
            table["location_id"], table["latitude"], table["longitude"]
        )
        if latitude is not None
    }


def calls(package):
    table = pq.read_table(
        f"{package}/serving/trip_call.parquet",
        columns=["trip_id", "sequence", "location_id", "boarding_point_id"],
    ).to_pydict()
    return zip(
        table["trip_id"], table["sequence"], table["location_id"], table["boarding_point_id"]
    )


def metres(a, b):
    north = (a[0] - b[0]) * 111_320
    east = (a[1] - b[1]) * 111_320 * math.cos(math.radians((a[0] + b[0]) / 2))
    return math.hypot(east, north)


def labels(overlay):
    """(national trip, sequence, stop place) -> the regional post, where every date slice of
    the trip agrees on it."""
    positions = locations(overlay)
    found, clashing = {}, set()
    for trip, sequence, place, post in calls(overlay):
        if not post or not post.startswith("overlay:") or post not in positions:
            continue
        if not trip.startswith("jdf:trip:"):
            continue
        key = (trip.split(":overlay:")[0], sequence, place)
        if found.setdefault(key, post) != post:
            clashing.add(key)
    for key in clashing:
        del found[key]
    return {key: positions[post] for key, post in found.items()}, len(clashing)


def judge(package, truth):
    positions = locations(package)
    verdicts = {}
    for trip, sequence, place, point in calls(package):
        real = truth.get((trip, sequence, place))
        if real is None:
            continue
        if point is None or ":est:" not in point:
            verdicts[(trip, sequence)] = "none"
            continue
        distance = metres(positions[point], real)
        verdicts[(trip, sequence)] = (
            "correct" if distance <= 20 else "ambiguous" if distance <= 35 else "wrong"
        )
    return verdicts


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    truth, clashing = labels(sys.argv[1])
    print(f"labelled calls of national trips: {len(truth)} ({clashing} dropped: slices disagree)")
    results = []
    for package in sys.argv[2:]:
        verdicts = judge(package, truth)
        results.append(verdicts)
        counts = collections.Counter(verdicts.values())
        total = len(verdicts)
        summary = ", ".join(
            f"{name} {counts[name]} ({counts[name] / total:.2%})"
            for name in ("correct", "ambiguous", "wrong", "none")
        )
        print(f"{package}: {total} calls: {summary}")
    if len(results) == 2:
        first, second = results
        moves = collections.Counter(
            (first[key], second[key]) for key in first.keys() & second.keys()
        )
        print("transitions, first package -> second:")
        for (before, after), count in sorted(moves.items(), key=lambda item: -item[1]):
            if before != after:
                print(f"  {before:9} -> {after:9} {count}")


if __name__ == "__main__":
    main()
