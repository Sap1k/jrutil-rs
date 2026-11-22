# Switching Oběhy to jrutil-rs

Two switches: rail (the `czptt` package), then road (the `jdf` package with estimated posts,
the regional overlay and the filtered feed). Each has the same three steps: one national build
compared with the F# build of the same snapshots, the Oběhy-side change, the first production
build watched. Parity on the golden subset (`docs/golden.md`) is the precondition and holds for
both packages.

## 1. The national comparison

Made once per switch, with the maintainer, on the snapshots of one production run. Run
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
gazetteer = "<jrutil>/jrutil/data"
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
routing = "<workdir>/osm/jdf-transit-routing-demand.osm.pbf"
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

## 2. What changes in Oběhy

Only at the switch, in one commit each. The serving contract, the loader, the API and
`obehy-rt` do not change.

### Both switches

- `pipeline/jrutil.py`: the runtime is the `jrutil` binary of a pinned jrutil-rs ref.
  `build.yml` checks the ref out and runs `cargo build --release --locked -p jrutil`, as it
  builds the F# multitool from its checkout today; a local build does the same from
  `[jrutil] directory`. The build and the run happen on the same machine, so the platform's
  `libm` is the same for both (the projection uses `sin`, `cos`, `atan2`).
- The compiler is invoked as `jrutil build --spec <file> --progress-events`. With the flag it
  prints the `JRUTIL_PROGRESS {"event":"phase",…}` lines `pipeline/process.py` already reads;
  `--jobs` becomes `RAYON_NUM_THREADS`, and there is no memory budget to pass.
- `jrutil validate <package>` replaces `validate-package`; `jrutil build` already refuses to
  leave an invalid package in place.
- Provenance: `manifest.json` names the compiler (`jrutil-rs`, its version) and each source by
  ID and SHA-256. Retrieval times, URLs and licences stay where Oběhy records them
  (`release.json`); the snapshot descriptors are no longer passed to the compiler.

### Switch 1 (rail)

| Today | After |
|---|---|
| `flatten_messages` → `derived/messages.zip` | gone: the compiler orders the messages of the snapshot |
| `czptt-to-bundle --catalog-snapshot --sr70 --osm-pbf --operational-points=sidecar` | `[czptt] snapshot`, `osm` |
| `--route-shapes*` | `[czptt.shapes]` |
| `--diagnostics-out` and `_verify_foreign_coordinate_acceptance` | gone: `diagnostics.json` carries the coordinate summary, and an unplaceable station makes the package ineligible |
| `_verify_gtfs_stops` | stays (it reads the package's GTFS) |

`NATIONAL_CZPTT.md` and `STATIC_PIPELINE.md` are updated in the same commit.

### Switch 2 (road)

| Today | After |
|---|---|
| `stage_nested_jdf_batches` | gone: `[[jdf.exports]]` names the downloaded archives |
| `fix-jdf`, `merge-jdf`, `jdf-to-bundle`, `regional-gtfs-overlay` | one `[jdf]` build |
| `--ext-geodata` | `catalogues`, `registry`, `route_rules`, the overlay `overrides` |
| `--cz-pbf` | `osm` |
| `--routing-osm-pbf`, `--post-inference-policy`, `--routing-cache` | `[jdf.posts] routing`, `policy`, `cache` |
| `--capture-post-inference-evidence` | `[jdf.posts] evidence` |
| `--stop-registry-candidates` (three commands) | `registry_candidates`, `post_candidates.csv` in the posts reports, `overlay_place_candidates.csv` in the overlay reports |
| `--policy` (overlay) | the source profiles shipped with the compiler (`policy/overlay/*.toml`) |
| `filtered_jdf.py` | `[jdf.filtered]` |
| `--international-route-policy=regional-adjacent`, `--strict`, `--batch-output`, `--memory-budget` | gone: the first is the compiler's only behaviour, the others have no counterpart |

`national_jdf.py`, `regional_overlay.py` and `filtered_jdf.py` collapse to writing the
specification and one invocation. The gazetteers (`cz_regions.json.gz`, `cz_towns.json.gz`,
`eur_towns_countries.csv`) live in the F# checkout today; they move to `jrunify-ext-geodata`
before the `converters/jrutil` submodule is removed. The route-shapes companion keeps its
repository. `STATIC_PIPELINE.md`, `BASE_PLAN.md` §11 and `AGENTS.md` are updated in the same
commit.

## 3. Decisions to confirm before a switch

Differences from F# that are deliberate and visible to a consumer; each is a ledger category
with its reason, listed here so that the maintainer can veto one before it reaches production.

Rail (national comparison done 2026-11-22, `docs/golden.md`):
- a train with a negative day offset moves its dates, not only its times (F# bug fixed; five
  paths nationally);
- a note or IDS entry over a point the path visits twice resolves to the first visit, for an end
  the first at or after the start (17 reversing trains gain zones, three notes their interval);
- trip-to-trip transfers of replacement buses are published in serving;
- a station that cannot be placed makes the package ineligible instead of publishing it at 0,0.

Road:
- a route without a published trip is not published (which also lets IDS JMK own line 444, whose
  authority F# withheld because of such a route);
- `cis:line_trip` keys are valid for their line version's period;
- an unknown agency URL is null in serving and the publisher's URL in GTFS;
- boarding points carry no timezone of their own (their place's applies);
- an unknown wheelchair value is null, not 0;
- on a conflict between two source trips on a date the source owns the line, the national trip
  keeps the date (F# lost it);
- the estimator's bar is labelled accuracy not worse than F#'s, not agreement with it.
