# The golden subset

Parity with the F# JrUtil and speed are judged on a bounded, frozen subset of real data, never
on the full national feed. The subset lives outside the repository, under one root
(`E:/Git/obehy/work/jrutil-rs-golden` on the development machine).

## Rail (CZPTT)

`tools/golden/czptt.py` (run with Oběhy's Python environment):

1. `fetch <root>` downloads the timetable year's CZPTT objects from the CIS JŘ mirror into
   `<root>/czptt/full`, in the source snapshot layout the compiler reads:
   `annual/JR<year>.zip`, `changes/<yyyy-mm>/*.xml.zip`, `sr70/SR70.csv`, `sources.json`.
2. `subset <root>` keeps the messages of the trains whose TR identifier hashes into one bucket
   of twenty (about 5%), as `<root>/czptt/subset` in the same layout, and writes
   `<root>/czptt/fs/derived/messages.zip`, the pre-ordered form the F# compiler takes.
3. `kadr <root>` snapshots the KADR catalogue (`kadr/catalog.json`) into both. It is the only
   step that calls an SŽ service; run it once, deliberately.

The F# reference package is produced once with `czptt-to-bundle` on that `messages.zip` and kept
as `<root>/czptt/fs/package`. The Rust compiler reads `<root>/czptt/subset` directly: ordering
the messages is part of the compiler, so the comparison also covers it.

### Reference and result (2026-11-22)

F# JrUtil `aa2f1db`, `czptt-to-bundle --operational-points=sidecar`, no route shapes (there is no
osr graph on the development machine): 16 s, about 1.0 GB peak, 4,054 trips and 59,365 calls.
`jrutil build` on the same subset: 0.9 s, 152 MB peak. `jrutil compare … --expect
parity/czptt.toml` passes: eight explained categories; trips, calls, times, route stops, zones,
notes and the GTFS stop_times, trips, routes and transfers are identical.

Not covered by the subset, to be looked at in the full build made with the maintainer: paths
with a negative day offset (CZPTT-R066), explicit note occurrences (CZPTT-R071), and the
route-shapes phase.

## Road (JDF, posts, overlay)

The slice is the one JrUtil's `scripts/golden/prepare.py` selected for the overlay work
(`<root>/jdf/inputs`, a link to `work/overlay-golden/inputs`): 2,487 JDF batches (2,229 of
version 1.11, 256 of 1.10, 2 of 1.8), the stop catalogues, the OSM stop extract, the reviewed
route rules and the PID and IDS JMK snapshots. `<root>/jdf/build.toml` is the build
specification; the gazetteers come from the JrUtil checkout and the stop registry from the
`jrunify-ext-geodata` checkout.

The F# reference is `<root>/jdf/out/fs-ref`: `fix-jdf`, then `merge-jdf --gvd-year=2026
--reference-date=2026-11-21 --stop-registry=…`, then `jdf-to-bundle` without post estimation
(`fix`, `merge-reg`, `bundle-reg`; production always passes the registry, so the runs without it
are not the reference).

### Base package: reference and result (2026-11-22)

F# JrUtil: `fix-jdf` 65 s (3.1 GB peak), `merge-jdf` 32 s, `jdf-to-bundle` 32 s: 129 s for
92,705 trips and 1,901,717 calls. `jrutil build` on the same inputs: 17 s, 0.95 GB peak, the
whole chain in one process. `jrutil compare <root>/jdf/out/fs-ref/bundle-reg
<root>/jdf/out/rs/package --expect parity/jdf.toml` passes: fifteen explained categories; every
trip, call, time, calendar, route stop and zone is identical, as are the stop numbers and the
registry candidates but one (a stop no published trip uses is no longer offered).

Batches are read twice: once for stop matching and the merge of everything but the calls, and,
for the batches that still have a running trip once the line versions are resolved, once more
for their calls. The build therefore never holds the calls of versions that lose.

### Filtered feed

The reference is Oběhy's `build_filtered_jdf` run on the F# package and merged JDF with
`<root>/jdf/filter-rules.json` (the production rules' shape with operators this slice
contains; the production file names operators it does not): `<root>/jdf/out/fs-ref/filtered`.
`jrutil build` writes `<root>/jdf/out/rs/filtered` from the same rules: the line selection (843
lines in three queries), the 12 routes, 125 trips, 1,071 stop times and 272 stops are
identical.

### Overlay: result (2026-11-22)

`<root>/jdf/build-overlay.toml` builds the road package with PID and IDS JMK overlaid in 38 s
(the overlay stage 12 s), under 2 GB; F#: 129 s for the base and 234 s, 6.8 GB for the overlay.
`jrutil compare <root>/jdf/out/fs-ref/overlay-reg <root>/jdf/out/rs-overlay/package --expect
parity/jdf-overlay.toml` passes. Trips are paired by schedule, since slices and added trips are
named differently by design: 14,946 by ID, 181,487 by equal content, 1,795 as near pairs, 1,198
by overlapping dates; no F# trip is left unpaired and 39 trips exist only here.

Stage by stage against F#'s review reports: all 11,020 source stop groups go to the same base
place by the same method, with the same 510 context inferences; the candidate scores (98,220),
ambiguous candidates (906) and pattern edits (4,537) are equal row for row; 819,291 of 819,411
source-trip-dates end the same way (bound to the same base trip, added, or neither).

What differs on purpose: a conflict between two source trips on a date the source owns the
line keeps the national trip (1,398 trips keep 3,006 trip-dates F# lost); 39 trips of IDS JMK
line 444 gain authority because the base has no trip-less routes; replaced coordinates say
`overlay`; national keys hold for the line version's period on every slice.

### Estimated posts and the production shape: result (2026-11-22)

`<root>/jdf/build-posts.toml` (the base with estimated posts) builds in 54 s: 12 s for the
routing graph of the national demand extract, 28 s for the estimator. The graph (16.6 M
vertices, 32.9 M edges) holds about 1.5 GB and is freed when the last context is routed: the
build peaks at 2.6 GB while the graph is built, stays at 2.0 GB while routing and at 1.4 GB
after it.

With the routing cache (`[jdf.posts] cache`) a warm build spends 11 s in the estimator instead
of 39 s, and its package, evidence pack and reports are byte-identical to a `--no-cache`
build. The check that the per-region validity is exact: a cache written on the demand extract,
used for a build on `jdf-transit-routing-base.osm.pbf` (another extract of the same country,
with other edge numbers), reuses 19,933 of 20,021 contexts and routes 88 again, and that build
is byte-identical to a `--no-cache` build on the base extract. The cache file is 55 MB.

`<root>/jdf/build-exports.toml` builds the base package from the batches packed as two national
exports (`inputs-exports/JDF_VLD.zip`, `JDF_drahy.zip`, `[[jdf.exports]]`): byte-identical to the
build from the batch directory. `<root>/jdf/build-full.toml` (posts, then the overlay, then the filtered feed: what
production publishes) builds in 68 s. F#: 85 s `fix-jdf`, 34 s `merge-jdf`, 206 s `jdf-to-bundle`
with posts, 234 s overlay.

- `jrutil compare <root>/jdf/out/fs-ref/bundle-posts-reg <root>/jdf/out/rs-posts/package
  --expect parity/jdf-posts.toml` passes.
- `jrutil compare <root>/jdf/out/fs-ref/overlay-posts-reg <root>/jdf/out/rs-full/package
  --expect parity/jdf-full.toml` passes.

The evaluator decides exactly as F# on the pack F# captured (100,925 of 100,925 contexts). With
the Rust capture and router, 96.4 % of calls board at the same point as with F#; the rest are
decisions on a threshold that fall the other way, in both directions (the new router keeps
`f64` where F# stored vertices as `f32`, and the anchor of a stop with two observed positions
is the one nearer the stop, where F# let a hash decide). Judged by the regional feeds' real
posts (`tools/golden/posts_accuracy.py`, 1,636,203 labelled calls):

| | correct (≤ 20 m) | ambiguous | wrong (> 35 m) | no estimate |
|---|---|---|---|---|
| F# | 72.59 % | 4.19 % | 3.34 % | 19.88 % |
| jrutil-rs | 72.43 % | 4.13 % | 3.15 % | 20.30 % |

### Overlay and post estimator references

Both are F# runs on the registry-numbered chain, made once (2026-11-22):

- **Overlay:** `regional-gtfs-overlay` with the production policy on `bundle-reg` (the base
  without estimated posts) → `<root>/jdf/out/fs-ref/overlay-reg` and `overlay-reg-diagnostics`
  (with traces). 234 s, 6.8 GB peak; 102,965 matched and 13,651 unmatched source trips.
- **Post estimator:** `<root>/jdf/reference-posts.ps1`: `fix-jdf` with post candidates,
  `merge-jdf` with the registry, a capture of the evidence pack
  (`<root>/jdf/out/fs-ref/posts-evidence`, 192 s) and the live package with the learned scorer
  (`bundle-posts-reg` and its diagnostics, 206 s): 10,171 estimated posts, 14,804 weak or
  unresolved contexts. The routing extract is the demand-clipped file of the last national run
  (`work/osm/jdf-transit-routing-demand.osm.pbf`): F# accepts only a file with that manifest,
  and both compilers read the same one.

## Rules

- One F# reference run per chain; it is pinned, not re-measured.
- While working, run only the stage being changed, once.
- A full national build happens once per package switch, with the maintainer.
