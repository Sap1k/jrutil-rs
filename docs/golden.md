# The golden subset

Changes are judged on a bounded, frozen subset of real data, never on the full national feed.
Until the switch (2026-11-23) the subset was where parity with the F# JrUtil was established;
the F# references below are pinned and are not rebuilt. The subset lives outside the repository, under one root
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

Not covered by the subset: paths with a negative day offset (CZPTT-R066) and explicit note
occurrences (CZPTT-R071), both seen in the national comparison below, and the route-shapes
phase, which has not run on real data.

### The national rail comparison (2026-11-22)

Made once, with the maintainer, on the full snapshot of timetable year 2026 (`<root>/czptt/full`,
221,736 messages; `<root>/czptt/build-full.toml`). Both compilers without route shapes.

| | F# JrUtil `aa2f1db` | jrutil-rs |
|---|---|---|
| ordering the messages | 6 min 15 s (Python `flatten_messages`) | part of the build |
| build | 5 min 11 s, 9.1 GB peak | 36 s, 2.3 GB peak |
| result | 81,874 trips, 1,252,358 calls | the same trips and calls |

`jrutil compare <root>/czptt/fs-full/package <root>/czptt/rs-full/package --expect
parity/czptt.toml` passes: no trip or call is missing or extra, and in `gtfs.zip` the
`stop_times.txt` (669,050 rows), `routes.txt`, `agency.txt` and the transfers are identical.
The eight categories of the subset appear with national counts, and the national feed adds:

- **Five paths with a negative day offset** (cross-border trains shortly before midnight, ten
  trips). F# moved their stops' times a day forward and left the dates, so they ran a day late,
  and left the railway points between them unmoved. They run from the day before, with times
  past 24:00.
- **Explicit occurrences** (21 note and IDS endpoints): a train that reverses at its terminus
  names the later visit of a station as occurrence `1`. The first verdict read this 1-based, on
  the belief that no real message has one; the comparison showed F#'s zero-based reading is the
  one under which the notes reach the last stop, and the resolver was corrected (the only code
  change the comparison caused).
- **Intervals over repeated points** (verdict Q3): three notes whose end point is visited
  before the start as well as after it now cover their interval instead of the whole trip, and
  17 reversing trains get the IDS zones F# could not resolve; with those, eleven route stops
  state their zones once for the route instead of per call.

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

### The national road comparison (2026-11-22/23)

Made once, with the maintainer: the production build `obehy build --estimated-posts` with F#
JrUtil `aa2f1db`, then the switched pipeline (one `jrutil build`) on the sources that run
fetched, with its reference date. 14,145 JDF batches, PID and IDS JMK, no route shapes.

| | F# JrUtil | jrutil-rs |
|---|---|---|
| whole `obehy build` | 36 min | 13 min with a cold routing cache, under 4 min warm |
| road compile alone | 22 min (fix, merge, bundle, overlay) | 10 min cold (500 s of it routing 138,775 contexts), 3 min warm |
| peak memory | not measured | 8.3 GB |
| road `gtfs.zip` | 125 MB | 63 MB |

`jrutil compare <F# release>/jdf <jrutil-rs release>/jdf --expect parity/jdf-full.toml` passes.
Folding F#'s chunk trips, the 260,674 national trips run on 17.4 million trip-dates, and every
date that differs is one of three kinds:

- **JDF 1.8/1.9 batches** (465 of them; the golden slice has two). F# lost the type of their
  time codes on upgrade (jdf-read B1), so "does not run on 25 December" was ignored: 1,405
  trips ran on 26,782 trip-dates they do not run, missed 279 they do, and 40 trips that never
  run were published.
- **Conflicts on dates a regional feed owns** (overlay Q3, B23): 1,325 national trips keep
  2,327 trip-dates F# dropped, and one trip F# removed entirely stays.
- **Three trips with a broken source time** (a time an hour off mid-route) are dropped with
  `jdf_time_backwards`; F# published them with a 24-hour jump.

What the comparison changed in the compiler:

- The overlay profiles keep source calls (`keep_source_calls`), as production's policy has
  since 2026-11-21; the golden references had been built with the older policy. That was the
  bulk of the first run's differences.
- Stop matching (jdf-read Q20): rows without an okres no longer contradict rows with one
  (R048), and a stop nothing else places takes its reviewed gap-fill rows (R082). With five
  gap-fill rows added the same day, no road stop is published with an estimated position (F#:
  27). No stop lost its registry number; the one F# stop missing is served only by trips that
  never run.
- A stop whose district is written as blanks has no district (three stops published `" "`).
- `gtfs.zip` is written at the default deflate level.

Routing runs on the unclipped routing extract: the demand clip needed rows only F# wrote. On
the golden slice the package is identical with and without the clip; the cost is about 1.5 GB
of memory.

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
- Since the national road comparison the overlay profiles keep source calls, so the golden
  overlay ledger (`jdf-overlay.toml`) no longer passes against the F# reference built with the
  older policy; `parity/jdf-full.toml` with its national counts is
  the judge. Compare a change against the previous jrutil-rs build of the same slice instead.
