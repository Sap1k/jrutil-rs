# Oběhy and jrutil-rs

Oběhy's production build switched from F# JrUtil to jrutil-rs on 2026-11-23, for both packages
at once (Oběhy `001ccdf`, jrutil-rs `5934475`). This document records how that was done and how
Oběhy runs the compiler now. Parity on the golden subset (`docs/golden.md`) was the
precondition; one national comparison per package was the check.

## How Oběhy runs it

- `obehy build` writes `<run>/build.toml` (`src/obehy/static_build.py`) with both packages and
  runs `jrutil build --spec <run>/build.toml --progress-events` once. The `JRUTIL_PROGRESS
  {"event":"phase",…}` lines on stderr feed Oběhy's progress reporter; `--jobs` becomes
  `RAYON_NUM_THREADS`. `jrutil build` refuses to leave an invalid package in place.
- The specification it writes has `[czptt]` with one `[[czptt.train_keys]]` (IDS JMK), and
  `[jdf]` with the two `[[jdf.exports]]`, `[jdf.posts]`, `[jdf.overlay]` with PID and IDS JMK and
  `[jdf.filtered]`; `[czptt.shapes]` and `[jdf.shapes]` when route shapes are on. The national comparison specifications below
  have the same shape.
- The compiler is built from source: `build.yml` checks out `Sap1k/jrutil-rs` at
  `jrutil_rs_ref` (default `main`) and `obehy build` runs `cargo build --release --locked -p
  jrutil`; locally the checkout is `[jrutil] directory` in `obehy.local.toml`. Build and run
  share a machine, so the projection's `sin`, `cos`, `atan2` come from the same `libm`.
- `manifest.json` names the compiler (`jrutil-rs`, the version or commit) and each source by ID
  and SHA-256; retrieval times, URLs and licences stay in Oběhy's `release.json`.
- The gazetteers (`cz_regions.json.gz`, `cz_towns.json.gz`, `eur_towns_countries.csv`) are in
  `jrunify-ext-geodata/gazetteer` (moved from the F# checkout's `jrutil/data` on 2026-11-24,
  byte-identical); Oběhy reads them from its geodata checkout. The route-shapes companion
  (`route-shapes/`, C++ on osr) moved here from the F# repository the same day; its workflow
  publishes `route-shapes-<source hash>` releases, and Oběhy's build downloads the one matching
  its jrutil-rs checkout. Oběhy needs nothing from the F# JrUtil any more.

## 1. The national comparison

Made once per package, with the maintainer, on the snapshots of one production run (rail
2026-11-22, road 2026-11-22/23; results in `docs/golden.md`). To repeat it, run
`obehy build --keep-work` (F# JrUtil) so that the run directory keeps its sources and its F#
packages, then build the same inputs with `jrutil build` and compare. Nothing here writes to the
run directory: the specification lives next to it and writes below `rs/`.

### Rail

```toml
# <run>/rs-czptt.toml
[czptt]
snapshot = "national-czptt/sources"        # sources.json, annual/, changes/, kadr/, sr70/
osm = "<workdir>/osm/railway-locations.osm.pbf"
output = "rs/czptt"

[czptt.shapes]                             # only if the F# build made shapes
companion = "<route-shapes companion>"
graph = "<workdir>/cache/route-shapes/graph"
cache = "<workdir>/cache/route-shapes/results"

[[czptt.train_keys]]                       # IDS JMK train trip IDs as keys (OVL-R132)
profile = "ids-jmk"
feed = "<run>/sources/regional/ids-jmk-gtfs.zip"
```

```powershell
jrutil build --spec <run>\rs-czptt.toml
```

```powershell
jrutil compare <release>\czptt <run>\rs\czptt --expect parity\czptt.toml
```

The ledger's `max` counts are the golden subset's; on the national feed the counts of the known
categories grow with the feed, so the result to read is the list of categories: every one must
be a category of `parity/czptt.toml`, and a new one is a finding. Look in particular at what
the subset could not cover: trains with a negative day offset (CZPTT-R066), explicit note
occurrences (CZPTT-R071), and route shapes.

### Road

```toml
# <run>/rs-jdf.toml
[jdf]
catalogues = "<geodata>/other"
gazetteer = "<geodata>/gazetteer"
osm = "<workdir>/osm/jdf-post-candidates.osm.pbf"
registry = "<geodata>/registry"
route_rules = "<geodata>/routes"
timetable_year = 2027                      # the run's GVD year
reference_date = "2026-12-01"              # the run's reference date
output = "rs/jdf"
registry_candidates = "rs/stop-registry/stop-candidates.csv"

[[jdf.exports]]                            # the two national exports, as downloaded
name = "vld"
archive = "sources/JDF_VLD.zip"

[[jdf.exports]]
name = "drahy"
archive = "sources/JDF_drahy.zip"

[jdf.posts]
routing = "<workdir>/osm/jdf-transit-routing-base.osm.pbf"   # unclipped: the demand clip is gone
policy = "<obehy>/src/obehy/data/post-inference/learned-v1.json"
reports = "rs/posts-reports"               # post_candidates.csv, post_decisions.csv
cache = "<workdir>/cache/jrutil-rs/routing"

[jdf.overlay]
reports = "rs/overlay-reports"             # overlay_place_candidates.csv and the review tables

[[jdf.overlay.sources]]
profile = "pid"
feed = "sources/pid-gtfs.zip"
overrides = "<geodata>/overlay/pid-stop-overrides.csv"

[[jdf.overlay.sources]]
profile = "ids-jmk"
feed = "sources/ids-jmk-gtfs.zip"

[jdf.shapes]                               # only if the F# build made shapes
companion = "<route-shapes companion>"
graph = "<workdir>/cache/route-shapes/graph"
cache = "<workdir>/cache/route-shapes/results"

[jdf.filtered]
rules = "<geodata>/filtered-jdf/rules-v1.json"
output = "rs/jdf-filtered"
```

```powershell
jrutil build --spec <run>\rs-jdf.toml
```

```powershell
jrutil compare <release>\jdf <run>\rs\jdf --expect parity\jdf-full.toml
```

Read the result as for rail. The estimated posts are judged by accuracy, not by agreement
(`docs/golden.md`): `tools/golden/posts_accuracy.py` against the regional feeds' real posts,
for both packages. The filtered feed is compared with Python's (`gtfs.zip` member by member,
`line-snapshot.json`). Note the wall time and the peak memory of both builds.

## 2. What changed in Oběhy

Both switches landed in one commit (`001ccdf`), which replaced every F# invocation with the
single `jrutil build` above and deleted the steps the compiler absorbed. The serving contract,
the loader, the API and `obehy-rt` did not change. The tables map each F# step or flag to what
replaced it.

### Rail

| F# | jrutil-rs |
|---|---|
| `flatten_messages` → `derived/messages.zip` | gone: the compiler orders the messages of the snapshot |
| `czptt-to-bundle --catalog-snapshot --sr70 --osm-pbf --operational-points=sidecar` | `[czptt] snapshot`, `osm` |
| `--route-shapes*` | `[czptt.shapes]` |
| `--diagnostics-out` and `_verify_foreign_coordinate_acceptance` | gone: `diagnostics.json` carries the coordinate summary, and an unplaceable station makes the package ineligible |
| `_verify_gtfs_stops` | stays (it reads the package's GTFS) |

### Road

| F# | jrutil-rs |
|---|---|
| `stage_nested_jdf_batches` | gone: `[[jdf.exports]]` names the downloaded archives |
| `fix-jdf`, `merge-jdf`, `jdf-to-bundle`, `regional-gtfs-overlay` | one `[jdf]` build |
| `--ext-geodata` | `catalogues`, `registry`, `route_rules`, the overlay `overrides` |
| `--cz-pbf` | `osm` |
| `--routing-osm-pbf`, `--post-inference-policy`, `--routing-cache` | `[jdf.posts] routing` (the unclipped base extract; the osmium demand clip goes), `policy`, `cache` |
| `--capture-post-inference-evidence` | `[jdf.posts] evidence` |
| `--stop-registry-candidates` (three commands) | `registry_candidates`, `post_candidates.csv` in the posts reports, `overlay_place_candidates.csv` in the overlay reports |
| `--policy` (overlay) | the source profiles shipped with the compiler (`policy/overlay/*.toml`) |
| `filtered_jdf.py` | `[jdf.filtered]` |
| `--international-route-policy=regional-adjacent`, `--strict`, `--batch-output`, `--memory-budget` | gone: the first is the compiler's only behaviour, the others have no counterpart |

The F# invocations, `filtered_jdf.py`, the message flattening, the osmium demand clip and the
`obehy-national-*` commands are gone; `src/obehy/static_build.py` writes the specification.

## 3. Deliberate differences from F#

Differences that are visible to a consumer and shipped with the switch; each is a ledger
category with its reason. Any of them can still be revisited.

Rail (national comparison done 2026-11-22, `docs/golden.md`):
- a train with a negative day offset moves its dates, not only its times (F# bug fixed; five
  paths nationally);
- a note or IDS entry over a point the path visits twice resolves to the first visit, for an end
  the first at or after the start (17 reversing trains gain zones, three notes their interval);
- trip-to-trip transfers of replacement buses are published in serving;
- a station that cannot be placed makes the package ineligible instead of publishing it at 0,0.

Road (national comparison done 2026-11-22/23, `docs/golden.md`):
- trips of JDF 1.8/1.9 batches follow their time codes (F# ran them every day of the version);
- a version interrupted by another is one trip with a gap in its calendar, not `:p2` trips;
- a trip whose times step back mid-route is dropped with a warning (three nationally);
- stops the catalogues or a reviewed gap-fill row place are no longer estimated (JDF-R048, R082);
- a route without a published trip is not published (which also lets IDS JMK own line 444, whose
  authority F# withheld because of such a route);
- `cis:line_trip` keys are valid for their line version's period;
- an unknown agency URL is null in serving and the publisher's URL in GTFS;
- boarding points carry no timezone of their own (their place's applies);
- an unknown wheelchair value is null, not 0;
- on a conflict between two source trips on a date the source owns the line, the national trip
  keeps the date (F# lost it);
- the estimator's bar is labelled accuracy not worse than F#'s, not agreement with it.
