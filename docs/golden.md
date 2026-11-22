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

Still to come on this slice: the post estimator (`bundle-posts`) and the regional overlay
(`overlay`).

## Rules

- One F# reference run per chain; it is pinned, not re-measured.
- While working, run only the stage being changed, once.
- A full national build happens once per package switch, with the maintainer.
