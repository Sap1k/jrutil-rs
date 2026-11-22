# Architecture

jrutil-rs turns pinned source snapshots into Oběhy's `jrutil-production` packages. It is a
compiler: it reads files it is given, never the network, and the same inputs give the same bytes.
`AGENTS.md` holds the rules that keep this shape; this document explains the shape.

## Shape

```
 bytes ──► jr-formats ──► source facts ──► compiler ──► Timetable ──► passes ──► jr-publish ──► package
           (no policy)                     (jr-czptt,     ▲    │      (Edits)     (no decisions)
                                            jr-jdf)       └────┘
                                                        jr_model::apply
```

- **Readers** (`jr-formats`) parse a format into typed records that say what the source says and
  nothing more. A missing value is `None`, not a default.
- **Compilers** (`jr-czptt`, `jr-jdf`) turn one source family's facts into a `Timetable`:
  merging messages, resolving versions, repairing or rejecting what the source gets wrong.
  Every such decision is a named rule (`docs/rules/`).
- **Passes** (`jr-enrich`, `jr-posts`, `jr-overlay`) improve a timetable from further evidence:
  coordinates, boarding points, a regional feed's facts, connections. A pass returns `Edits`;
  `jr_model::apply` is the only code that changes a timetable.
- **Projections** (`jr-publish`) write the serving relations, `gtfs.zip`, the manifest and the
  diagnostics. They choose nothing; anything that looks like a choice belongs in a pass.

The pipeline order is declared once, in `crates/jrutil/src/pipeline.rs`:

```
czptt:  compile → coordinates → replacement-bus connections → route-stop order → [shapes]
        → publish
jdf:    read batches → stop matching ┐ (per batch, in parallel)
                        merge ◄──────┘ (in batch-name order; calls only for versions that win)
        → international filter → assemble → [post estimator] → presentation
        → [regional overlay → presentation] → route-stop order → [shapes] → publish
        (+ the filtered feed, from the timetable as it stands before the overlay and the shapes)
```

Stages in brackets run when the build specification asks for them. Planned and not built: rail
platforms, connections from the infrastructure manager's list, a foreign rail overlay
(`[rail platforms]`, `[connections]`, `[foreign overlay]` after the rail coordinates).

## Crates

| Crate | Holds | File system |
|---|---|---|
| `jr-model` | `Timetable`, handles, `ServiceTime`, `DateSet`, the ID builder, `Edits` and `apply`, rules and diagnostics | no |
| `jr-formats` | JDF, CZPTT XML, KADR, GTFS, XLSX, OSM PBF, SR70, curated CSV/TOML, registry, the post evidence pack | no: decodes readers it is handed |
| `jr-geo` | name normalisation and matching, spatial grid, administrative areas, road/tram graph and router | no |
| `jr-czptt` | the rail compiler | no |
| `jr-jdf` | the road compiler | no |
| `jr-enrich` | coordinates, route rules and presentation, connections, rail platforms, route-stop order, shapes hand-off | no |
| `jr-posts` | post estimator: capture → evidence store → policy and scorer → edits | no |
| `jr-overlay` | GTFS overlay engine: source profiles, matching, projection → edits | no |
| `jr-publish` | serving relations, GTFS projection, manifest, diagnostics, `validate`, `compare` | yes |
| `jrutil` | CLI, build spec, pipeline order, cache files, progress, the shapes companion process | yes |

## The model

`jr_model::Timetable` is one feed's timetable in dense tables addressed by typed handles.

- **Places and boarding points.** A `Place` is a stop place, a station or a railway operational
  point. It owns `BoardingPoint`s of a kind: `post`, `platform`, `track`, `unspecified`. A call
  has both `boarding_point` (what a passenger is shown) and `track` (what a rail source assigns);
  they are equal until something maps tracks to platforms.
- **Calendars are `DateSet`s**: bitsets of day offsets from the horizon's start. Trips share
  interned sets (`ServiceId`). Weekday masks and exception lists are derived when publishing.
  Cancelling days, resolving overlapping line versions or letting a regional feed own some days
  is set algebra, and equal calendars are equal values.
- **Trips hold their calls.** A `Call` is 64 bytes; the national road timetable's calls fit in
  half a gigabyte. Stop patterns are an index a pass derives when it needs one, not a storage
  layer, so changing one trip never forks shared state.
- **Times** are `ServiceTime`: seconds from local midnight of the service date.
- **IDs.** `PublicId`s are built in `jr_model::identity` and never parsed. Rows without a
  natural key get a content-derived ID.
- **Invariants.** `Timetable::check` states what every pass may rely on: handles resolve, a
  call's boarding point belongs to its place, sequences increase, time never goes backwards,
  IDs are unique and carry the feed prefix. `apply` re-checks after every batch.

## Edits

A pass builds a batch with `EditBuilder`, which hands out the handles new rows and newly interned
values will have. `apply` refuses a batch built against a different timetable shape, applies the
edits in order, records `(pass, rule)` against each changed entity (`Timetable::history`, read by
`jrutil explain`), consults the `Precedence` table where sources compete for one value, and
checks the invariants.

The `Edit` vocabulary is closed. Extending it is an architecture decision, because every kind
is something all later passes and the projections must be able to live with.

## Rules

A special case is a `Rule` constant with an ID, a doc comment saying what in the source data
makes it necessary, thresholds in `policy/*.toml`, a diagnostic code, a test named after the ID
and a row in `docs/rules/<area>.md`. The ledger also records, for every rule the F# JrUtil had,
whether it was ported, replaced or dropped, and why.

## Caches

A cache changes nothing but time: `jrutil build --no-cache` writes the same bytes, and the
golden runs say so (`docs/golden.md`). One is built where a measurement shows it pays.

**Routed post contexts** (`[jdf.posts] cache`). Routing is the longest stage of the road build,
and the OSM extract changes every day, so validity is per region, not per file. Every routing
algorithm reads the graph through a reader that notes where in the plane it read
(`jr_geo::routing::reader`); the graph is cut into tiles, each with a digest of the edges that
touch it (`region`); a stored result is reused when the digest of the tiles it read is the same
on the current graph (`cache`). The file's name carries a hash of the router's sources (taken
at build time, so nobody has to remember a version bump) and of the routing policy. The
graph is still built on every run: the digests need it.

**The shapes companion's results** (`[*.shapes] cache`) are the companion's own, keyed by
pattern content.

Not cached, by measurement: a parsed and matched JDF batch, a CZPTT source object, an overlay
source snapshot. On the golden slice the whole road build without posts takes 17 s and the rail
build under a second; a batch's stop matching depends on the registry, the catalogues and the
OSM stops, so any geodata change would invalidate every batch anyway. If the national build
shows a stage worth caching, it gets one then.

## Parity

Until a package has switched, `jrutil compare <f#> <rust> --expect parity/<package>.toml` judges
it on the golden subset (`docs/golden.md`) at two levels: by ID where IDs are in the kept scheme,
and as schedules (service date + journey key → calls, times, boarding points, presentation,
attached semantics) where they are not. Every difference falls into a ledger category with a
reason and an expected count; anything else fails.

## Extension seams

| Extension | Reader | Pass |
|---|---|---|
| SŽ "Přípoje mezi vlaky" (XLSX) | `jr-formats` connection facts | `connections`: station + train number + dates → connection claims and waiting transfers |
| Track → platform from OSM | OSM platform and track facts, a reviewed override file | `rail_platforms`: sets `Call::boarding_point`, leaves `Call::track` |
| Post estimator changes | none | `jr-posts` policy and scorer versions, judged on the evidence pack |
| Foreign rail GTFS | GTFS + a rail source profile, a stop crosswalk | `jr-overlay` with rail matching, extending calls past the border |
| Something else | a new reader | a new pass emitting existing edits, or a new projection |
