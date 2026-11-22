# JDF conversion rule inventory (F# reference): `jdf-to-bundle` without the post estimator

Status: **proposal for review**. Nothing here is implemented. The inventory describes what the F#
JrUtil (`E:/Git/obehy/jrutil`) does when `jdf-to-bundle` turns a merged JDF batch into a
production package with post estimation **disabled** (no `--routing-osm-pbf`), and what the
Python filtered feed (`repo/src/obehy/filtered_jdf.py`) does with that package. The verdicts are
proposals for the architect.

This file owns `JDF-R400` to `JDF-R599`. Neighbouring inventories:

- `docs/rules/jdf-read.md` (being written): reading, version conversion (1.8/1.9/1.10 → 1.11) and
  `fix-jdf`. Everything up to a parsed `JdfBatch` is theirs.
- `docs/rules/jdf-merge.md` (being written): merging, stop identity and the stop registry, line
  versions (`JrutilRouteVersions.txt`, overlap resolution, GVD bounding), calendars as the merger
  bounds them, and the international filter (`JdfInternationalFilter`). The filter runs inside
  `jdf-to-bundle` (stage 4 below) but its rules are theirs; this file only records what it hands
  over.
- `docs/rules/serving.md`: what the package writer does with what the converter hands it
  (sections 2, 5, 6, 7.5, 7.6 and its verdicts). Referred to as `serving §n` / `SERVING-Rnnn`;
  not repeated here.
- `docs/rules/czptt.md`: the rail compiler; referred to as `CZPTT-Rnnn`.

Sources read in full: `jrutil/src/Jdf/JdfToGtfs.fs`, `JdfGtfsRules.fs`, `JdfBundle.fs`,
`JdfBundleInput.fs`, `JdfBundleManifest.fs`, `JdfBundleModel.fs`, `JdfBundleTables.fs`,
`JdfCalendar.fs`, `JdfPostPlan.fs` (the parts used without estimation), `JdfModel.fs`, the
helpers of `Jdf.fs` (`parseAttributes`, `normalizeZoneTokens`, `tripIsReverse`,
`jdfCompilationParser`), `JdfCallStore.tripSpans`, `Holidays.fs`, `GeoData/RouteRules.fs`,
`GeoData/StopRegistry.fs` (constant only), `Gtfs/Gtfs.fs`, `Gtfs/GtfsModel.fs` (fields used),
`Serving/TripWriter.fs`, `TripCallWriter.fs`, `NoteWriter.fs`, `AssignmentWriter.fs`,
`PackageSemanticRelations.fs`, `PackageKeyRelations.fs`, the JDF parts of
`PackageGtfsRelations.fs` and `PackageRows.fs`, `jrutil-multitool/Commands/ConvertCommands.fs`
(`jdfToBundle`), the tests `JdfToGtfsTests.fs`, the non-post cases of `JdfBundleTests.fs`, the
JDF cases of `ServingContractTests.fs`, the trip-key cases of `JdfMergerTests.fs`;
`ExecutionTests.fs` pins no conversion behaviour (worker plans, memory budgets and ordered maps
only). Documents: `jrutil/README.md` ("Czech schedule facts", "Oběhy JDF conversion bundles"),
`repo/JDF_SEMANTICS.md`, `jrutil/docs/PRODUCTION_CONTRACT.md`. Data files:
`jrunify-ext-geodata/routes/transport-modes.csv`, `routes/presentation.csv`,
`filtered-jdf/rules-v1.json`. Python: `repo/src/obehy/filtered_jdf.py`,
`national_jdf.verify_gtfs_stops`, the `cli.py` call site.

Ground truth ("golden"): `work/overlay-golden/out/baseline/merge.zip` (merged JDF 1.11, 907 route
versions, 93,001 trips, 2,052,189 calls, 6,200 stops) and its F# `jdf-to-bundle` output
`…/baseline/bundle` (policy `regional-adjacent`, estimated posts `disabled`; 901 routes, 92,705
trips, 1,901,717 trip calls, 11,934 locations). Every count quoted as "golden" comes from these two
files (read with pyarrow / the `csv` module; no compiler was run).

Conventions used below:

- `esc(x)` = .NET `Uri.EscapeDataString(x)`: RFC 3986 unreserved `A–Z a–z 0–9 - . _ ~` kept,
  every other UTF-8 byte `%XX` upper-case hex. Identical to `jr_model::identity::escape`.
- "version" = one merged route version, the pair (licence `Linky.id`, distinction
  `Linky.idDistinction`). The merger assigns distinctions; they are not stable across runs.
- "licence" = the CIS line number `Linky.id` (6 digits in practice, typed as text).
- "output route" = the route ID after grouping (JDF-R429), which may cover several versions.
- "call" = one `Zasspoje` row; "emitted call" = a call that becomes a `trip_call` (JDF-R441).
- "retained trip" = a trip of the filtered batch that has at least one service day (JDF-R413).
- "attribute" = a `Pevnykod` fixed code referenced by number from `Zastavky`, `Zaslinky`,
  `Spoje` or `Zasspoje`.
- Proposed policy keys live in `policy/jdf.toml` (prefix `jdf.`); proposed data files in
  `data/jdf/`. Reviewed rule files stay in `jrunify-ext-geodata/routes/`.
- Diagnostic codes are the F# `diagnostics.json` codes where one exists; proposed new codes are
  marked "(new)".

---

## Architect's verdicts

**These override the "Proposed verdict" lines below.** Where a rule is not listed, its proposed
verdict stands. Rule IDs in the Rust code are the IDs of this file; the calendar rules are owned
by `jdf-merge.md` (R255–R268), and R409–R414 here refer to them.

The road port is a parity port: what F# publishes is reproduced, crashes become diagnostics, and
the semantic gaps this inventory found are **recorded, not closed**. Closing one (request stops
from the line's route stops, typed `Caskody` designations, line-level reservation notes,
alternate operators) is a deliberate change of what Oběhy serves, to be made on its own after the
switch.

| Q | Decision |
|---|---|
| 1 | Calendars: `jdf-merge.md`. |
| 2 | Port F# (undated odd/even weeks). |
| 3 | Port F#: pickup and drop-off come from the stop's and the call's own codes only. B1 and B2 stand as limitations. |
| 4 | Port: the office phone number. |
| 5 | As decided in `jdf-merge.md` (Q9, Q10): no chunks; a hash of the published validity for a second version starting the same day; `:pN` for side-by-side versions. Route groups keep `jdf:route:<line>` for the group holding the line's earliest-starting version, `…:detour` for detours, and eight hex characters of the group's semantics for any other group, as today. |
| 6 | Port (trip number parity decides direction). |
| 7 | Keep the trip short name. |
| 8 | Keep the 0-based ordered index as the call sequence. |
| 9 | The presentation pass runs twice, as in F#: after the road compiler (so the filtered feed and the overlay see presented routes) and again after the overlay. |
| 10 | The JDF time windows are not carried. |
| 11 | Typed `Caskody` designations are not published (port). Recorded as a limitation. |
| 12 | `Mistenky` rows for trip `0` are dropped with a diagnostic (port). Recorded as a limitation. |
| 13 | A claim on a call that is not served is dropped with a diagnostic (port). No `origin_sequence = 0` fallback: a claim whose origin cannot be found is dropped with a diagnostic too (serving question 18). |
| 14 | Note and claim IDs are built from the trip or route ID and the source row's own number, not from a merge-assigned distinction. They are generated IDs and are compared by content. |
| 15 | Alternate operators are not published. The filtered feed reads them from the road compiler's own facts. |
| 16 | Yes: the manifest records the policy the filter ran with. |
| 17 | One diagnostic per dropped fact, with a code; the package summary counts them. |

Model (section 5): the timetable gained connection claims, travel restrictions and the
operator's registration number (IČO). Everything else in the list is either a source key
(`cis:line`, `cis:line_trip`), stays inside the road compiler (route versions, the route stop a
call came from, alternate operators, integrated-system details), or is not carried. A travel
restriction written on a line's route stop is stored with the calls it reaches, so the published
slot comes from the calls and nothing has to carry the source's route-stop key; a restriction on
a trip's call points at that call (fixes serving question 9).

Further:

- No ID is parsed back (section 4): presentation rules read the registration number and the
  `cis:line` key.
- Bad references (B13, B14) are diagnostics that drop the affected trip or fact, never a crash.
- B7 cannot occur: an ID is built once.
- R467 and the other `cz_*` / `source_*` tables are not built.
- R430: a route none of whose trips runs is not published (F# published 17 such routes on the
  golden slice), nor are its notes and its integrated-transport membership.
- R452 with Q14: versions of a line published as one route number their `Udaje` rows
  independently. A note equal to one already published under its number is published once; a
  differing one is a further note (`identity::jdf::note_variant`). Note text is trimmed.
- R453: a typed `Caskody` row without text has shaped the calendar and is nothing to diagnose.
- R417: the stop's `@` holds for each of its boarding points. Unknown is null, not GTFS's `0`.
- R466: a key holds for the period its line version is in force (the dates the version won),
  not for the bounds of the trip's calendar row.
- R461: a `Navaznosti` target of `0` is absent, not an identifier (B10).
- SERVING-R046: the relation keeps an operator without a website as null; `agency.txt` gets the
  feed publisher's URL, which GTFS requires.

---

## 1. Data flow

The command `jdf-to-bundle` (`ConvertCommands.jdfToBundle`) builds `BundleOptions` from the
command line (`--snapshot-descriptor`, `--converter-version`, `--international-route-policy`,
`--transport-mode-rules`, `--route-presentation-rules`, `--gvd-year`, `--stop-registry`, the post
options) and calls `JdfBundle.execute`. With no routing PBF the execution mode is `Disabled` and
`writeBundleCore` runs:

| # | Stage | Input → output | F# function |
|---|---|---|---|
| 0 | Option checks | workers, routing workers, memory budget > 0; mode matrix (`postInferenceEvidenceOnly`, `routingPbfPath`, `estimatedPosts`) → `Disabled` | `JdfBundle.execute` |
| 1 | Snapshot check | descriptor JSON + input path → validated `SnapshotDescriptor`; input digest must match (JDF-R400, R401) | `JdfBundleInput.loadSnapshotDescriptor`, `validateSnapshot` |
| 2 | Output reservation | output path must not exist; temp sibling `.<name>.tmp-<guid>` (serving §1.1) | `writeBundleCore` |
| 3 | Parse | directory or ZIP (`validateZip`) → `JdfBatch` with `tripStops` in a memory-mapped `JdfCallStore` (jdf-read.md) | `JdfBundleInput.withJdfInput`, `Jdf.jdfCompilationParser` |
| 4 | Calendars on the source batch | `JdfBatch` → `CalendarPreparation { tripsToDelete; schedules }` keyed by trip ID (JDF-R409–R413) | `JdfCalendar.prepareGtfsCalendarWithWorkersAndProgress` |
| 5 | International filter | policy + calendars + batch → filtered batch, per-version decisions, `RegionalInternational` route type on qualifying versions (jdf-merge.md) | `JdfInternationalFilter.applyInternationalRoutePolicyWithCalendarWorkersAndProgress` |
| 6 | Transport-mode rules | filtered batch → corrected batch (route modes) + decisions (JDF-R405–R407) | `JdfGtfsRules.applyTransportModeRules` |
| 7 | Drop post evidence | `postCandidateEvidence = [||]` when estimation is off | `writeBundleCore` |
| 8 | Retained calendars | source calendars filtered to the trip IDs of the corrected batch (JDF-R404) | `JdfCalendar.filterCalendarPreparation` |
| 9 | Preparation | public line numbers (JDF-R408), empty post plan | `JdfToGtfs.prepareGtfsFeedForStreamingBundleWithCalendar` |
| 10 | Call scan | one pass over all calls → emitted-call count, trip-call restriction rows, filtered/unjoinable restriction IDs, singleton restriction diagnostics, conflicting post numbers (JDF-R422, R464, R465) | `JdfBundleTables.scanCallDerivedFacts` |
| 11 | Stop list for parents | every potential stop/boarding point → `stop_id → parent` map | `JdfToGtfs.getGtfsStopsWithPlan` |
| 12 | Calls | trips in ordinal trip-ID order → ordered, emitted calls with times, boarding point, pickup/drop-off, kilometre, route-stop key; records each trip's endpoint stop and the first emitted sequence of every (trip, route stop) a `Navaznosti` row names (JDF-R440–R447) | `JdfToGtfs.getStreamingBundleStopTimeRows` → `TripCallWriter.fromGtfs` → native `trip_call.parquet` |
| 13 | Remaining feed | agencies, retained stops, routes, trips with deduplicated services, calendars, feed info, Czech tables (JDF-R414–R439) | `JdfToGtfs.finishStreamingFeedWithUniqueCalendars` (`assembleGtfsFeed`) |
| 14 | Presentation | reviewed overrides on routes (JDF-R431) | `JdfToGtfs.applyRoutePresentationRules` |
| 15 | Filler values | missing agency URL → `jrutil://invalid`, missing coordinates → 0/0 (JDF-R416, R426; SERVING-R046) | `Gtfs.fillStandardRequiredFields` |
| 16 | Coordinate check | any stop outside ±90/±180 fails the build | `JdfBundleManifest.validateStopCoordinates` |
| 17 | Native trips | GTFS trips → `trip.parquet` | `Serving.TripWriter.write` |
| 18 | Sidecars and semantics | `source_*` Parquet sidecars, notes, trip features, route integrations (JDF-R424, R425, R449, R452–R465) | `JdfBundleTables.getTableProducers` |
| 19 | Native notes and assignments | notes → `service_note.parquet`; note links + route integrations + trip features → `assignment.parquet` | `Serving.NoteWriter.write`, `NoteWriter.assignment`, `AssignmentWriter.write` |
| 20 | Diagnostics | bundle + international + transport-mode + presentation diagnostics, sorted by (code, source object) (JDF-R470) | `JdfBundleManifest.diagnostics`, `writeDiagnostics` |
| 21 | Compiler manifest | JDF section of the manifest (JDF-R471) | `JdfBundleManifest.writeManifest`, `recordManifestGvd` |
| 22 | Package | `CompilerOutput` (GTFS tables except `stop_times`, Czech tables, sidecars, manifest, diagnostics) + native relations + transfer sequences → package (serving §1) | `Serving.PackageWriter.writePackage` |
| 23 | Activation | temp → output (`Directory.Move`) | `writeBundleCore` |
| 24 | Filtered feed (Python, later) | package `gtfs.zip` + merged JDF + `rules-v1.json` + reference date → `jdf-filtered/gtfs.zip`, `filter-report.json`, `line-snapshot.json` (section 6) | `filtered_jdf.build_filtered_jdf` |

What reaches the package writer:

- GTFS tables `agency.txt`, `stops.txt`, `routes.txt`, `trips.txt`, `calendar.txt`,
  `calendar_dates.txt`, `feed_info.txt` (no `stop_times.txt`, no `transfers.txt`, no
  `shapes.txt`).
- Czech tables `cz_routes.txt`, `cz_trips.txt`, `cz_stops.txt`, `cz_stop_zones.txt`
  (`cz_trip_stop_zones` is `None`).
- Native files `trip_call`, `trip`, `service_note`, `assignment`, plus
  `NativeCallArtifacts.transferSequences` (a side channel: (trip ID, source route stop) → first
  emitted sequence).
- Sidecars `source_route_metadata`, `source_stop_metadata`,
  `source_location_feature_metadata`, `source_route_stop_zone_metadata`,
  `source_transfer_metadata`, `source_travel_restriction_metadata` (JDF-R472).

The serving writer then derives `agency`, `location`, `route`, `service_calendar`,
`service_exception`, `source_key`, `connection_claim`, `travel_restriction`, the location
features, the route-stop order and zones, and `gtfs.zip` (serving §2, §4, §5, §7, §9).

---

## 2. Entity by entity

Every output field and what it is derived from. "GTFS" columns are the intermediate F# tables;
"serving" columns are the published relation (serving §2 for the generic derivations).

### 2.1 Agencies (`Dopravci` → `agency`)

One per `Dopravci` row of the filtered batch (the international filter keeps agencies used by a
kept version or a kept `Altdop` row). Golden: 67 rows in the merged JDF, 64 published.

| Output | Derivation |
|---|---|
| `agency_id` | `jdf:agency:{esc(IČO)}:{idDistinction}` (`Dopravci.id`, `idDistinction`); IČO kept with leading zeros (`jdf:agency:00005886:1`) |
| `name` | `Dopravci.name` verbatim |
| `url` | `website`: if it does not contain `https?://` anywhere (unanchored regex) → `"http://" + website`, else verbatim; absent → `jrutil://invalid` (filler, SERVING-R046). Golden: 19 of 64 are `jrutil://invalid` |
| `timezone` | constant `Europe/Prague` |
| `language` | constant `cs` |
| `phone` | `officePhoneNum` (`Some` even when blank → null after the writer's blank-to-null). `controlPhoneNum`, `infoPhoneNum` ignored. Golden: 0 of 64 have a phone, although `infoPhoneNum` is filled (B5) |
| `fare_url` | null |
| `email` | `Dopravci.email` |
| ignored | `taxId`, `companyType`, `personName`, `officeAddress`, `controlPhoneNum`, `infoPhoneNum`, `faxNum` |

### 2.2 Stop places (`Zastavky` → `location` kind `stop_place`)

One GTFS stop per `Zastavky` row; kept only when a retained call references it or one of its
boarding points (JDF-R423). Golden: 6,200 stops in the merged JDF, 5,967 published.

| Output | Derivation |
|---|---|
| `location_id` | `jdf:stop:{Zastavky.id}` (decimal int64; the merger's or the registry's number, jdf-merge.md) |
| `kind` | `stop_place` (GTFS `location_type = 1`) |
| `name` | `town[,district][,,nearbyPlace]` exactly: `(None,None)` → `town`; `(d,None)` → `town,d`; `(None,np)` → `town,,np`; `(d,np)` → `town,d,np` (no spaces added); ` [?]` appended when the chosen `Polzast` row is not `S` (JDF-R415, R416) |
| `municipality_name`, `district_name`, `nearby_place`, `country_code`, `district_code` | `source_stop_metadata` `town`, `district`, `nearby_place`, `country`, `okres` (= `Zastavky.regionId`) (JDF-R424) |
| `coordinate_precision` | `missing` if no `Polzast` row or lat = lon = 0; else `S` → `exact` (sidecar `stop`), `E` → `estimated` |
| `coordinate_source` | `PolzastZdroj.source` text mapped by substring (SERVING-R005); golden: `catalogue` 11,468 (incl. boarding points), `osm` 306, `gapfill` 142, `route_time` 16, `route_end` 2 |
| `longitude`, `latitude` | chosen `Polzast` row (`StopPrecise` before `Estimated`, then source order); none → 0/0 filler → published null |
| `timezone` | constant `Europe/Prague` |
| `wheelchair_boarding` | `1` if the stop's attributes include `@`, else `0` (never `2`) (JDF-R417) |
| `description`, `url`, `public_code` | null |
| GTFS `zone_id` (not published) | the zone code when the stop has exactly one distinct (route-scoped zone ID, code) membership, else null (JDF-R450) |
| ignored | `Zastavky.attributes` beyond `@` and the flags of JDF-R444 and the features of JDF-R425 |

### 2.3 Boarding points

Every boarding point is a GTFS child stop whose fields are copied from the parent stop place
(name with any ` [?]`, coordinates, timezone, wheelchair value, zone), then overridden:

| Kind | ID | `public_code` | Coordinates (estimation off) | When it exists |
|---|---|---|---|---|
| unspecified | `jdf:stop:{stop}:unspecified` | null | parent's | one per stop; kept when a call uses it (JDF-R418) |
| authored post (`Oznacniky`) | `jdf:stop:{stop}:post:id:{stopPostId}` | `Oznacniky.postName` trimmed; else the single textual number calls give this post; else `string stopPostId` | parent's | one per `Oznacniky` row; kept when a call uses it (JDF-R419) |
| textual post (`Zasspoje`) | `jdf:stop:{stop}:post:{esc(stopPostNum trimmed)}` | the trimmed number | parent's | one per distinct (stop, number) among calls with a number and no post ID (JDF-R420) |
| inferred post | `jdf:stop:{stop}:est:{k}` | null | estimator | only with estimation (out of scope) |

A call picks: its `stopPostId` (authored) if set; else its non-blank `stopPostNum` (textual);
else (estimation only) an inferred post; else `:unspecified` (JDF-R421). Every call therefore has
a boarding point; `trip_call.location_id` is the parent, `boarding_point_id` the child.

Golden: no `Oznacniky` rows and no post IDs or numbers in `Zasspoje`; all 5,967 boarding points
are `:unspecified`. The post forms are exercised only by the fixture
(`data/jdf/obehy_extensions`).

`Oznacniky.name`, `description`, `reserved1/2` are ignored; the post's name is the stop place's.

### 2.4 Routes (`Linky` versions → output routes → `route`)

Versions are grouped into output routes (JDF-R429): per licence, regular and detour versions
separately, versions with equal semantic key together. Golden: 907 versions → 901 output routes
(678 regular, 223 detour), none hash-suffixed.

| Output | Derivation |
|---|---|
| `route_id` | `jdf:route:{esc(licence)}` / `jdf:route:{esc(licence)}:detour` for the group holding the earliest-starting version of its kind; other groups append `:{8 hex}` (JDF-R429) |
| `agency_id` | representative version's `jdf:agency:{esc(agencyId)}:{agencyDistinction}` |
| `short_name` | public line number of the representative version (JDF-R408), then presentation override (JDF-R431) |
| `long_name` | representative `Linky.name` verbatim |
| `description` | `Výlukový jízdní řád` when `detour`, else null |
| `gtfs_route_type` | (effective transport mode, JDF route type) table (JDF-R427) |
| `mode` | `servingMode(route type)` (SERVING-R006): a round trip mode → type → mode |
| `color`, `text_color` | mode/route-type/public-line table (JDF-R428), then presentation override |
| `timetable_kind` | `detour` when the version's `detour` flag is set (sidecar `source_route_metadata.detour = "True"`), else `regular` |
| `url`, `sort_order` | null |
| Czech `cz_routes` | `route_id`, `cis_line_id = licence`, `public_line_number` (before presentation), `source_provenance = "jdf:{VerzeJDF.version}"` |

The representative version of a group is the one with the smallest (`timetableValidFrom`,
`idDistinction`) after merge bounding. Every group is published, also when none of its trips has
a service day (golden: 17 routes without trips, B31). Ignored `Linky` fields: `grouped`, `usesStopPosts`,
`oneWay` (16 golden versions have it; all their trips are odd), `reserved1`, `licenceNum`,
`licenceValidFrom/To` (JDF-R432).

### 2.5 Trips (`Spoje` → `trip`)

One per `Spoje` row of the filtered batch that has service days (JDF-R413). Golden: 93,001
trips, 296 dropped as `filtered_trip`, 92,705 published.

| Output | Derivation |
|---|---|
| `trip_id` | `jdf:trip:{tripVersionKey}:{Spoje.id}` (JDF-R433): `{esc(licence)}:{yymmdd}[:det][:{8 hex}][:p{n}]` |
| `route_id` | output route of the trip's version |
| `service_id` | deduplicated `jdf:service:{bitmap\|exc}:{n}` (JDF-R414) |
| `direction` | `0` for even trip numbers, `1` for odd (JDF-R434); golden 45,667 / 47,038 |
| `headsign` | `getStopName` (no ` [?]`) of the stop of the trip's endpoint call (JDF-R435) |
| `short_name` | `"{licence} {trip number}"`, e.g. `199004 3042` (JDF-R436) |
| `block_key`, `shape_id` | null; `SpojSkup` and `Spoje.tripGroupId` ignored (JDF-R439) |
| `run_key`, `run_part` | null (JDF trips are not parts) |
| `wheelchair_accessible` | `1` if the trip's attributes include `@` or `{`, else `2` (JDF-R437); golden 81,832 / 10,873 |
| `bikes_allowed` | `1` if `O`, else `2` (JDF-R438); golden: all 92,705 are `2` |
| Czech `cz_trips` | `trip_id`, `cis_line_id = licence`, `cis_trip_id = Spoje.id`, `train_number` null, `source_trip_ids = trip_id`, `coverage_sources = "jdf"` |

### 2.6 Calls (`Zasspoje` → `trip_call`)

| Output | Derivation |
|---|---|
| order | calls of one trip sorted by `routeStopId` ascending (odd trip) or descending (even trip) (JDF-R440) |
| emitted? | not when `departureTime` is `\|` (passes) or `<` (does not pass), nor when arrival and departure are both blank (JDF-R441) |
| `sequence` | 0-based index of the call in the ordered list **including** unemitted calls, so sequences start at 0 and have gaps (JDF-R443); golden: every trip starts at 0, maximum 82 |
| `location_id` / `boarding_point_id` | parent stop place / chosen boarding point (2.3) |
| `scheduled_arrival`, `scheduled_departure` | `HHmm` → seconds plus a carried day offset; a missing side takes the other side's value (JDF-R442); golden: 9,671 arrivals ≥ 86,400; 26,662 calls with arrival ≠ departure |
| `pickup_type`, `dropoff_type` | from the attributes of the stop (`Zastavky`) and of the call (`Zasspoje`) only (JDF-R444); golden: (0,0) 1,858,269; (3,3) 41,954; (1,0) 1,262; (2,2) 204; (0,1) 25; (1,3) 3 |
| `timepoint` | always `true` (JDF-R445) |
| `stop_headsign` | null |
| `source_kilometre` | `Zasspoje.kilometer` as double (JDF-R446); golden: never null |
| `route_stop_id` | transient key `compositeKey [route; distinction; routeStopId]` (`jdf:route:100100/1/…` form), replaced by the route-stop pass (JDF-R447, serving §7.4) |
| `passenger_service` | `true` |
| `shape_distance_traveled`, `subsidiary_*`, `active_line_code` | null |
| ignored | `minArrivalTime`, `maxDepartureTime` (golden: 1,731 calls carry both), the call's restriction codes (go to JDF-R464), `Zasspoje.stopId` cross-check against `Zaslinky.stopId` |

### 2.7 Services (calendars)

Per trip a date bitmap over the version's validity (merge-bounded `Linky.timetableValidFrom/To`)
is computed from the trip's calendar attributes (`X`, `+`, digits) and its typed `Caskody` rows
(JDF-R409–R411). A trip with no day is dropped (JDF-R413). Equal (calendar entry, exceptions)
pairs share one service `jdf:service:{weekday bitmap or exc}:{n}` (JDF-R412, R414). Golden: 514
services, 3,416 exceptions.

### 2.8 Zones

Each `Zaslinky.zone` text is split on `,`, trimmed, emptied tokens dropped and duplicates removed
(first occurrence kept) (JDF-R448). The tokens become `source_route_stop_zone_metadata` rows
(output route, distinction, route stop, `jdf:zone:…` ID, code, 0-based order) (JDF-R449). The
serving route-stop pass gives each passenger call the zones of its route-stop key and publishes
them per slot or per call with `zone_system` null (serving §7.5). Golden: 34,044
`route_stop_zone` rows, 0 `call_zone` rows. The `jdf:zone:` IDs are never published.

### 2.9 Notes (`Udaje`, `Caskody`, `Mistenky` → `service_note` + note `assignment`)

| Source | Kept when | `note_id` | `kind` | `label` | `text` | validity | `service_note_type` | link |
|---|---|---|---|---|---|---|---|---|
| `Udaje` | text not blank and output route retained | `jdf:notice:route:{esc(lic)}:{dist}:{Udaje.id}` | `route_information` | null | `Udaje.text` | null | null | `route` scope, output route |
| `Caskody` | not (typed and no text) and (text or designation) and trip retained | `jdf:notice:trip:{esc(lic)}:{dist}:{trip}:{Caskody.id}` | `timetable_note` | `designation` (non-blank) | `note` (non-blank) | `dateFrom`, `dateTo` | type name (JDF-R453) | `trip` scope |
| `Mistenky` | note not blank and trip retained | `jdf:notice:reservation:{esc(lic)}:{dist}:{trip}:{ordinal}` | `reservation_note` | null | `note` | null | null | `trip` scope |

Golden: 2,992 `route_information`, 564 `timetable_note` (all untyped), 0 `reservation_note`
(all 7 `Mistenky` rows have trip number `0`, B3), and 78,760 typed `Caskody` rows without text
not published (B4).

### 2.10 Features (`Pevnykod` codes → `assignment`)

| Scope | Source | Kinds | ID | `source_object_id` |
|---|---|---|---|---|
| trip | `Spoje` attributes of retained trips | table in JDF-R457 (incl. `calendar_designation` for `X`, `+`, digits) | `feedId("jdf","trip-feature",[trip; code; kind])` | `{trip_id}:attribute:{esc(code)}` |
| location | `Zastavky` attributes of retained stop places | table in JDF-R425 | `feedId("jdf","location-feature",[location; code; kind])` (built by serving from the sidecar) | `{stop_id}:attribute:{esc(code)}` |
| route | every `LinExt` row of a retained output route | `integrated_transport`, `source_code` = `transportSystemId` | `feedId("jdf","route-integration",[route; system])` | `{route_id}:integration:{system}` |

Golden: `calendar_designation` 115,995; `wheelchair_accessible_vehicle` 81,832;
`reservation_available` 97; `on_request` 23; `conditional` 8; route `integrated_transport` 881;
location `urban_transport_interchange` 627, `rail_interchange` 55, `refreshments` 32, `toilet` 16,
`metro_interchange` 6, `wheelchair_accessible` 6, `line_interchange` 2, `accessible_toilet` 2.

Not featurised: call attributes (`Zasspoje`), route-stop attributes (`Zaslinky`), any
`Caskody` link (`note_id` is always null on features) (JDF-R459, section 8).

### 2.11 Calendar designations

Only the trip's calendar attributes (`X`, `+`, `1`–`7`) are published, as trip-scope
`calendar_designation` assignments whose `source_code` is the code (`X`, `+`, the digit). The
typed `Caskody` designation (`Caskody.designation`, e.g. `71`) of a text-free row is lost
(B4).

### 2.12 Connection claims (`Navaznosti` → `connection_claim`)

Every `Navaznosti` field and where it goes (sidecar `source_transfer_metadata`, then
`PackageSemanticRelations.semanticRelations`, SERVING-R025):

| `Navaznosti` field (`JdfModel.Transfer`) | Output |
|---|---|
| `transferType` (`m`/`M`) | `direction`: `m` → `waits_for`, `M` → `connects_to`, other text verbatim (rejected later by validation) |
| `routeId`, `routeDistinction`, `tripId` | `origin_trip_id` = that trip's ID; ordinal of the row among the trip's rows (source order, 1-based) in `connection_id` |
| `routeStopId` | joins the trip's emitted call: `origin_sequence` = first emitted sequence with this route stop; a row whose call is not emitted is dropped (`unjoinable_call_enrichment`) |
| `transferRouteId` (int64) | `target_source_route_id` as decimal text (`"0"` kept; leading zeros of a licence lost, B10) |
| `transferStopId` (int64) | `target_source_stop_id` text; per the model comment a CIS stop-register number, not a local `Zastavky` ID |
| `transferStopPostId` | `target_source_post_id` |
| `transferEndStopId` | `target_source_end_stop_id` |
| `transferEndStopPostId` | `target_source_end_post_id` |
| `waitMinutes` | `wait_minutes` |
| `note` | `note` (non-blank, verbatim) |
| — | `connection_id = source_object_id = jdf:transfer:{esc(lic)}:{dist}:{trip}:{ordinal}`; `service_id`, `target_source_trip_id`, `target_public_line`, `target_destination_text`, `target_route_id`, `target_trip_id`, `target_location_id` null; `target_derivation = structured`; `resolution_status = unresolved` |

Golden: 44,566 rows, 43,712 claims (22,646 `waits_for`, 21,066 `connects_to`); 42,188 carry no
target field and no note, 1,513 a note only, 11 every target field as `0` and a note.

### 2.13 Travel restrictions (`§`, `A`, `B`, `C` → `travel_restriction`)

| Scope | Source | Row |
|---|---|---|
| `route_stop` | each `Zaslinky` row of a retained output route whose attributes include an exclusion code; one row per code | `gtfs_route_id` = output route, `source_route_version` = distinction, `source_route_stop_id`, `group_code` |
| `trip_call` | each call of a retained trip that is emitted and whose `Zasspoje` attributes include an exclusion code; one row per code | `gtfs_trip_id`, `source_route_stop_id`, `group_code` |

Codes map `§` → group `§`, `A` → `A`, `B` → `B`, `C` → `C`. Serving builds the ID and resolves
slots (serving §5.4, §7.6). Golden: 457 rows, all `trip_call` (`§` 220, `A` 193, `B` 32, `C` 12),
none resolved to a call (serving open question 9).

### 2.14 Reservation notes (`Mistenky`)

See 2.9. JDF gives no date range and no call scope; the row is a trip note (or, with trip number
`0`, apparently a line note that the converter cannot place, B3).

### 2.15 Alternate operators and line names (`Altdop`, `Altlinky`)

**Not converted.** `Altdop` (alternate operator per trip: `agencyId`, attributes, `timeType`,
`dateFrom`, `dateTo`) is read, filtered by the international filter and then ignored; trips are
published under the route's agency. `Altlinky` (alternate line number per country) is ignored.
Golden: 184 `Altdop` rows, 0 `Altlinky` rows. The Python filtered feed does read `Altdop`
(section 6).

### 2.16 Integrated systems (`LinExt`)

- Public line number: the single `preferential` row's `routeName` (JDF-R408).
- Route features: every row (preferential or not) → one `integrated_transport` route assignment
  per (output route, `transportSystemId`) (JDF-R458). The preferential flag, `entryNum` and
  `routeName` of non-preferential rows are not published.
- Golden: 887 rows (all preferential; systems 30621 ×511, 30001 ×367, 30041 ×9) → 881
  assignments.

### 2.17 Transfers

None. `transfers.txt` is `None`; the serving `transfer` relation is empty for JDF (golden: 0).
`Navaznosti` becomes claims only.

### 2.18 Source keys

Built by serving from the Czech tables (serving §4.1 rows 3 and 6):

- `cis:line_trip` per trip: identifier `PadLeft(licence, 6, '0') + ":" + Int64(trip number)`,
  only when the licence is 1–6 ASCII digits; validity = the trip's service bounds; golden 92,705.
- `cis:line` per output route with trips: identifier `PadLeft(licence, 6, '0')`, validity = union
  of its trips' bounds; golden 884 (901 routes, 17 without retained trips).
- `cz_stops.source_ids` (`jdf:stop:N`, `jdf:stop:N:post:id:K[,jdf:stop:N:post:{esc(num)}]`) are
  written but never read (JDF-R467).

---

## 3. Rules

### Stage 1 — input

#### JDF-R400 — Snapshot descriptor
- **What:** `--snapshot-descriptor` JSON must have `schema_version` = number `1`;
  `retrieved_at` a non-blank string ending in `Z` or `±hh:mm` and parseable as
  `DateTimeOffset`; `payload_kind` `zip` or `directory-tree`; `payload_sha256` 64 hex
  (lower-cased); `payload_bytes` integer ≥ 0; `source_uri` absent/null or an absolute URI;
  `source_id`, `retrieval_method`, `licence` non-blank. Any failure → `ArgumentException`.
- **Where:** `JdfBundleInput.loadSnapshotDescriptor`.
- **Why:** provenance recorded in `manifest.sources` (README "Snapshot descriptor schema
  version 1").
- **Pinned by:** `JDF bundle validates snapshots and ZIP packaging atomically`.
- **Constants:** schema version `1`; offset regex `(?:Z|[+-][0-9]{2}:[0-9]{2})$`; kinds
  `zip`, `directory-tree`; hash regex `^[0-9a-f]{64}$`.
- **Diagnostic:** build error.
- **Proposed verdict:** replace — the build spec (SERVING-R040 verdict: canonical build spec
  with each input's ID and digest) carries the source identity; `jrutil` validates it once for
  every compiler.

#### JDF-R401 — Input digest and ZIP safety
- **What:** a directory input is hashed as SHA-256 over files in ordinal relative-path order,
  appending each path (UTF-8, `/` separators), a NUL byte and the file's SHA-256; bytes = sum of
  sizes. A ZIP is hashed as a file. Kind, digest and size must equal the descriptor. A ZIP must
  contain no rooted path, no `..` or empty segment, no case-insensitive duplicate, exactly one
  `VerzeJDF.txt`, and every file under that file's directory.
- **Where:** `directoryTreeIdentity`, `validateSnapshot`, `validateZip`.
- **Why:** reproducibility and archive safety (README).
- **Pinned by:** `JDF bundle validates snapshots and ZIP packaging atomically`.
- **Constants:** none beyond the rules.
- **Diagnostic:** build error.
- **Proposed verdict:** port into `jrutil`'s input layer (ZIP safety is shared with every ZIP
  reader; coordinate with jdf-read.md, which may own it).

#### JDF-R402 — Execution mode
- **What:** mode `Disabled` iff no routing PBF; capture-only and live modes need the PBF and
  `estimatedPosts`; other combinations are errors; worker counts and memory budget must be > 0.
- **Where:** `JdfBundle.execute`, `writeBundleCore`.
- **Why:** the post estimator is optional (README).
- **Pinned by:** `Capture-only dispatch never enters policy evaluation or GTFS publication`
  (estimator side).
- **Constants:** none.
- **Diagnostic:** build error.
- **Proposed verdict:** replace — the `posts` pass is a pipeline step that is present or absent
  (`ARCHITECTURE.md`); no mode matrix in the compiler.

#### JDF-R403 — Stage order and the international filter hand-off
- **What:** calendars are computed on the unfiltered batch (stage 4); the international filter
  (stage 5) then drops versions/trips and sets `routeType = RegionalInternational` on versions
  with qualifying cross-border trips; transport-mode rules (stage 6) run on the filtered batch;
  diagnostics `filtered_international_trips` / `filtered_international_route` (warning,
  `source_object_id = jdf:route:{esc(lic)}:{dist}`) and the manifest block
  `international_route_filter` are written by the bundle.
- **Where:** `writeBundleCore`, `JdfBundle.fs` 573–584.
- **Why:** the filter needs service days to classify trips (jdf-merge.md).
- **Pinned by:** jdf-merge.md (`InternationalRouteFilterTests`).
- **Constants:** the manifest repeats the filter thresholds as literals `120`, `60`, `200`, `80`
  km (duplicating the filter's own constants, section 9).
- **Diagnostic:** `filtered_international_trips`, `filtered_international_route`.
- **Proposed verdict:** port the filter per jdf-merge.md; the manifest must read the thresholds
  from the policy the filter used.

#### JDF-R404 — Trip version keys of derived batches
- **What:** trip IDs (JDF-R433) are cached per batch object. The international filter registers
  its parent's keys for the filtered batch (`inheritTripVersionKeys`), but stage 6 and stage 7
  each build a new batch record (`{ batch with routes = … }`, `{ … with postCandidateEvidence =
  [||] }`), so every later stage recomputes the keys from the filtered route list. Calendars
  (keyed by parent IDs) are then filtered by the recomputed IDs (`filterCalendarPreparation`).
- **Where:** `JdfGtfsRules.inheritTripVersionKeys`, `applyTransportModeRules`, `writeBundleCore`
  336–341.
- **Why:** comment: "A batch derived by dropping route versions keeps its parent's keys, so trip
  ids prepared on the parent stay valid for the derived batch."
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** none (B7: a mismatch crashes with `KeyNotFoundException` in
  `finishStreamingFeedWithUniqueCalendars` or silently loses calendars).
- **Proposed verdict:** replace — the model holds route versions and trip identities as fields
  computed once by the compiler; no per-object caches.

### Stage 2 — route rules (transport mode)

#### JDF-R405 — Transport-mode rule file
- **What:** CSV (UTF-8, header required exactly `agency_id,licence,public_line,expected_mode,
  effective_mode,reason`, cells trimmed). `agency_id` empty or all ASCII digits (IČO);
  `licence` a pattern (JDF-R406); `public_line` empty, `N` or `N-M` with `N ≤ M` (Int32);
  `expected_mode`/`effective_mode` one of `A` Bus, `E` Tram, `T` Trolleybus, `L` CableCar, `M`
  Metro, `P` Ferry; `reason` non-empty. Errors name the 1-based line (`index + 2`). The file's
  SHA-256 is kept for the manifest.
- **Where:** `JdfGtfsRules.loadTransportModeRules`, `parseTransportMode`,
  `RouteRules.readRuleCsv`, `parseSelector`.
- **Why:** reviewed corrections of city trams/trolleybuses exported as buses
  (`jrunify-ext-geodata/README.md` "Route rules"; `routes.py` docstring).
- **Pinned by:** `Transport mode rule CSV is validated and checksummed`.
- **Constants:** the header; mode letters. Data: `routes/transport-modes.csv` (7 rows today:
  `61974757,915001-915019,1-19,A,E` Ostrava trams, `61974757,915101-915113,101-113,A,T` Ostrava
  trolleybuses, `47311975,545002|545003|545004|545005|545011,<line>,A,E` Liberec trams).
- **Diagnostic:** build error.
- **Proposed verdict:** port (reader in `jr-formats`, rules in `jr-enrich` route rules; the
  reviewed file stays in `jrunify-ext-geodata`).

#### JDF-R406 — Licence patterns and specificity
- **What:** pattern grammar: `N*` (prefix of ASCII digits, `*` alone = every licence), `L-H`
  (both all digits, equal length, `L ≤ H` ordinal), or a single all-digit licence. Matching:
  licence equal; range: equal length and `L ≤ licence ≤ H` ordinal; prefix: `StartsWith`. A
  selector with an agency also requires the route's agency IČO to be equal. Specificity
  (larger wins): (kind: licence 3, range 2, prefix 1; narrowness: range `-(H-L)`, prefix its
  length, licence 0; agency present). Among the candidates that set a field, the most specific
  wins; two equally specific ones → `InvalidOperationException("Equally specific route rules …
  set … of …")`.
- **Where:** `RouteRules.parseLicencePattern`, `licenceMatches`, `selectorMatches`,
  `specificity`, `mostSpecific`.
- **Why:** README of jrunify-ext-geodata "Route rules"; module comment.
- **Pinned by:** `The most specific transport mode rule wins`, `Presentation rules override
  fields by specificity`.
- **Constants:** kind ranks 3/2/1.
- **Diagnostic:** build error on a tie.
- **Proposed verdict:** port (one selector implementation for both rule files and for the
  Python checker `routes.py`, which reimplements it).

#### JDF-R407 — Transport-mode rule application
- **What:** per version: candidates = rules whose selector matches (route `agencyId` raw IČO,
  licence). Guarded = candidates whose `expected_mode` equals the version's current mode and
  whose `public_line` range (if any) contains the version's public line number parsed as Int32
  (a non-numeric public line never satisfies a range). Most specific guarded rule → mode set to
  `effective_mode`, decision `corrected = true`, message = the rule's `reason`. Candidates but no
  guarded rule → decision `corrected = false`, message `reviewed rule guard mismatch`, mode
  unchanged. No candidate → nothing. Runs before grouping, so route type, colours and semantic
  key use the effective mode; `source_route_metadata` keeps both modes.
- **Where:** `JdfGtfsRules.applyTransportModeRules`.
- **Why:** "The rule applies only while the source still has `expected_mode` and, when
  `public_line` … is given, a numeric public line in it" (geodata README).
- **Pinned by:** `Reviewed transport mode rule requires every guard`, `The most specific
  transport mode rule wins`.
- **Constants:** message `reviewed rule guard mismatch`.
- **Diagnostic:** `corrected_transport_mode` (info, `{reason}; effective_mode={mode}`),
  `transport_mode_rule_mismatch` (warning), both `source_object_id = jdf:route:{esc(lic)}:{dist}`;
  manifest `transport_mode_corrections { rules_sha256, corrected_routes, guard_mismatches,
  by_effective_mode }`. Golden: 0 / 0.
- **Proposed verdict:** port as a route-rules step of the compiler (it changes a source fact
  before routes are formed) with the same diagnostics.

#### JDF-R408 — Public line number
- **What:** per version, `LinExt` rows of the version with `preferential = true`:
  exactly one → its `routeName` trimmed; empty → warning log and `None`; all ASCII digits →
  leading zeros removed (`"0"` if nothing remains); otherwise verbatim. None → if the licence is
  exactly six ASCII digits → its last three digits with leading zeros removed (`100100` →
  `100`, `721001` → `1`), else warning log and `None`. Two or more → warning log and `None`.
- **Where:** `JdfGtfsRules.computePublicLineNumbers`, `normalizeNumericDesignation`.
- **Why:** README: "A preferred JDF `LinExt` designation wins; otherwise the last three digits
  of the six-digit CIS line ID are used. Numeric designations have leading zeroes removed, while
  alphanumeric designations retain their spelling and case."
- **Pinned by:** `JDF conversion emits Oběhy identities and public line numbers`,
  `Ambiguous or malformed public line identity is not guessed`.
- **Constants:** regexes `^[0-9]+$`, `^[0-9]{6}$`; `Substring(3)`.
- **Diagnostic:** `missing_public_line_number` (warning, per version,
  `jdf:route:{esc(lic)}:{dist}`); the three log messages are not coded. Golden: 0.
- **Proposed verdict:** port; the three cases become one coded diagnostic with a reason field.

### Stage 3 — calendars

These run in `jdf-to-bundle` on the merged batch. jdf-merge.md owns version bounding; if it
also claims the date-bitmap rules, the architect decides the owner (Q1).

#### JDF-R409 — Trip date bitmap
- **What:** for each day `d` of the version's `[timetableValidFrom, timetableValidTo]`, with the
  trip's typed `Caskody` rows (each with its interval, JDF-R410), the trip's attribute set and
  Czech holidays (JDF-R411), the first matching line decides:
  1. a `ServiceOnly` (type 3) row covers `d` → runs;
  2. the trip has any type-3 row → does not run;
  3. a `NoService` (4) row covers `d` → does not run;
  4. a `ServiceAlso` (2) row covers `d` → runs;
  5. an odd-weeks row (5 or 7) covers `d` and the ISO week number of `d` is even → does not run;
  6. an even-weeks row (6 or 8) covers `d` and the ISO week is odd → does not run;
  7. the trip has a `Service` (1) row and none covers `d` → does not run;
  8. attribute `+` and (`d` is a holiday or a Sunday) → runs;
  9. attribute `X` and `d` is not a holiday, Saturday or Sunday → runs;
  10. a digit attribute equal to the ISO weekday of `d` (1 = Monday … 7 = Sunday) → runs;
  11. otherwise → runs iff the trip has no calendar attribute (`X`, `+`, digit).
  Untyped `Caskody` rows do not affect dates.
- **Where:** `JdfCalendar.tripDateBitmap`.
- **Why:** JDF `Caskody` semantics (JDF 1.11 specification, `Caskody` section, referenced by
  JDF_SEMANTICS); no comment beyond the code.
- **Pinned by:** `Shared inactive schedules delete every source trip` (type 4 over the whole
  validity), `Shared calendars are identical across workers` (determinism only).
- **Constants:** type codes `1`–`8` (`JdfModel.ServiceNoteType`); ISO week rule
  (`WeekYearRules.Iso`); weekday digits as ISO day numbers.
- **Diagnostic:** none.
- **Proposed verdict:** port as one function producing a `DateSet`, tested per ladder step
  (`JDF-R409` cases 1–11).

#### JDF-R410 — Caskody interval defaults
- **What:** a typed row's interval is `[dateFrom ?? version validFrom, dateTo ?? (dateFrom ??
  validFrom)]`: a row without `dateTo` covers one day.
- **Where:** `tripDateBitmap` (preprocessing).
- **Why:** "jede/nejede dne …" rows give one date (unknown for the odd/even types).
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port for types 1–4; for types 5 and 6 (whole-validity odd/even weeks)
  replace the default with "the whole version validity" if the specification says so (B12, Q2).
  Golden has no type 5–8 rows; types seen: 4 (49,606), 2 (19,046), 1 (9,587), 3 (521).

#### JDF-R411 — Czech public holidays
- **What:** holidays in `[validFrom, validTo]`: fixed (day, month) `1.1, 8.5, 5.7, 6.7, 28.9,
  28.10, 17.11, 1.5, 24.12, 25.12, 26.12`, plus Good Friday and Easter Monday computed from
  Easter Sunday (the "New Scientist" algorithm) by a "days since March 1" arithmetic that
  treats every month as 31 days.
- **Where:** `Holidays.czechHolidays`, `easterSundayDate`, `czechHolidayDates`.
- **Why:** Czech state holidays (code comment: "I'm pretty sure this only works by accident").
- **Pinned by:** none.
- **Constants:** the date list → `data/calendar/cz-holidays.toml` (fixed dates; Easter-relative
  offsets −2 and +1).
- **Diagnostic:** none.
- **Proposed verdict:** replace with a correct Easter computation and date arithmetic (B11:
  "April 0" for Easter on 2 April, first in 2051; Easter Monday "April 0" for Easter on 30 March,
  first in 2059). One holiday calendar for every compiler.

#### JDF-R412 — Calendar representation
- **What:** per trip, the bitmap is published as (a) nothing (no day), (b) only added dates when
  `diff(bitmap, weekday calendar) > true days`, or (c) a calendar entry (weekdays from the
  attributes: `X` → Mon–Fri, `+` → Sun, digits; none → all seven) over the version validity plus
  added/removed exceptions.
- **Where:** `prepareGtfsCalendarWithWorkersAndProgress.calculate`.
- **Why:** "Pick the most efficient representation (calendar + exceptions vs just exceptions)".
- **Pinned by:** `Shared calendars are identical across workers`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** drop — the model stores a `DateSet`; weekday masks are derived in
  `jr-publish` (AGENTS "weekday masks exist only in `jr-publish`").

#### JDF-R413 — Trips without service days
- **What:** a trip whose bitmap has no day is removed from trips, calls, Czech tables, features,
  notes, claims and restrictions.
- **Where:** `prepareGtfsCalendar…` (`tripsToDelete`), `assembleGtfsFeed`,
  `getStreamingBundleStopTimeRows`, `getCzTrips`, `getTableProducers`.
- **Why:** a trip with no day is not a trip.
- **Pinned by:** `Shared inactive schedules delete every source trip`.
- **Constants:** none.
- **Diagnostic:** `filtered_trip` (warning, trip ID, "Trip has no retained service dates and was
  omitted"); its enrichments → `filtered_enrichment` (warning): trip `Caskody` rows with text or
  untyped with a designation, all `Mistenky` rows, all `Navaznosti` rows, and call-level
  restriction IDs `jdf:restriction:…`. Golden: 296 / 547.
- **Proposed verdict:** port (model: a trip with an empty `ServiceId` is not published; one
  diagnostic per dropped trip, enrichments counted on it rather than one row each).

#### JDF-R414 — Service identity
- **What:** distinct (calendar entry without ID, sorted exceptions without ID) pairs, visited in
  ordinal trip-ID order, are numbered `n = 0, 1, …` and named `jdf:service:{weekday bitmap as
  seven 0/1 characters Mon…Sun | "exc"}:{n}`.
- **Where:** `JdfCalendar.materializeUniqueCalendars`.
- **Why:** comment "Map iteration matches v1's ordinal service-ID traversal, preserving IDs".
- **Pinned by:** `Shared calendars are identical across workers`; `JDF bundle is deterministic
  and contains normalized sidecars` (`jdf:service:` prefix).
- **Constants:** `jdf:service:`, `exc`.
- **Diagnostic:** none.
- **Proposed verdict:** replace with `identity::service(feed, dates)` (serving verdict 5); the
  IDs change whenever any trip is added or removed today.

### Stage 4 — stop places and boarding points

#### JDF-R415 — Stop place identity and name
- **What:** one stop place per `Zastavky` row, `jdf:stop:{id}`; name as in 2.2 (commas, no
  spaces, empty parts skipped as shown); GTFS `location_type = 1`; `timezone = Europe/Prague`.
- **Where:** `JdfToGtfs.getGtfsStopsWithPlan`, `JdfGtfsRules.jdfStopId`, `getStopName`.
- **Why:** "This tries to mimic how IDOS displays these names"; README "JDF stop numbers are
  local to the export, never global CIS identifiers" (code comment).
- **Pinned by:** `JDF conversion normalizes zones and preserves both post forms`.
- **Constants:** separators `,` and `,,`; timezone.
- **Diagnostic:** none.
- **Proposed verdict:** port; the display name is a compile rule (`jdf.place_name`), the parts
  stay separate fields (municipality, district name, nearby place).

#### JDF-R416 — Stop coordinates and the ` [?]` marker
- **What:** per stop, the `Polzast` rows sorted by precision (`S` before `E`, stable) → first.
  None → no coordinate; the GTFS filler writes 0/0 and serving publishes it `missing`. A chosen
  row with precision `E` appends ` [?]` (once) to the stop's name and therefore to every child's.
  After filling, any stop with lat outside [−90, 90] or lon outside [−180, 180] fails the build.
- **Where:** `getGtfsStopsWithPlan`, `Gtfs.markApproximateStopName`,
  `Gtfs.fillStandardRequiredFields`, `JdfBundleManifest.validateStopCoordinates`.
- **Why:** README "estimated coordinate appends ` [?]`" (serving §2.2); coordinate selection is
  merge-time evidence (jdf-merge.md).
- **Pinned by:** `Estimated coordinates mark stop places and boarding points with question mark`.
- **Constants:** ` [?]`; precision letters `S`, `E`.
- **Diagnostic:** `missing_stop_coordinates` (warning, stop place ID, "Referenced stop has no
  resolved coordinates and was serialized as 0,0"); a log line with the count. Golden: 0;
  estimated 9 stop places.
- **Proposed verdict:** replace — the model's `Place.position` holds coordinate, precision and
  source; ` [?]` is added when publishing (as CZPTT-R103's verdict); a missing position is
  `None` with a diagnostic (CZPTT Q18 decision); no 0/0.

#### JDF-R417 — Stop wheelchair boarding
- **What:** `wheelchair_boarding = 1` if the stop's attributes contain `@`, else `0`; never `2`
  ("there's no corresponding JDF attribute"). Children copy the parent's value.
- **Where:** `getGtfsStopsWithPlan`.
- **Why:** code comment.
- **Pinned by:** none.
- **Constants:** `@`.
- **Diagnostic:** none.
- **Proposed verdict:** port on the place (`Tristate::Yes`/`Unknown`); boarding points stay
  `Unknown` unless a source says otherwise (the copy is a GTFS artefact). Golden: 6 stop places
  `1`.

#### JDF-R418 — The unspecified boarding point
- **What:** each stop has a child `jdf:stop:{id}:unspecified` (GTFS `location_type = 0`,
  `parent_station` = the stop), used by calls without a post.
- **Where:** `getGtfsStopsWithPlan`, `jdfUnspecifiedStopId`.
- **Why:** every call needs a boarding point (`README` "Typed `location` … preserve the
  place/boarding-point hierarchy").
- **Pinned by:** `JDF conversion normalizes zones and preserves both post forms`.
- **Constants:** suffix `unspecified`.
- **Diagnostic:** none.
- **Proposed verdict:** port (`BoardingPointKind::Unspecified`, as CZPTT-R052).

#### JDF-R419 — Authored posts (`Oznacniky`)
- **What:** one child per `Oznacniky` row, `jdf:stop:{stopId}:post:id:{stopPostId}`;
  `public_code` = `postName` trimmed when non-blank, else the unique textual number that calls
  with this post ID give (JDF-R422), else the decimal `stopPostId`; coordinates = parent's
  (without estimation); all other fields copied from the parent.
- **Where:** `getGtfsStopsWithPlan`, `jdfStopPostId`, `stopPostNumbersById`.
- **Why:** README: "`:post:id:` is an authoritative `Oznacniky` ID".
- **Pinned by:** `JDF conversion normalizes zones and preserves both post forms` (`postName "B"`
  wins over the textual `A`), `Legacy geometry cannot position authored posts without routed
  evidence`.
- **Constants:** `post`, `id`.
- **Diagnostic:** none.
- **Proposed verdict:** port (`BoardingPointKind::Post`, `source_code` = post ID,
  `public_code` per the cascade, `position = None` = at the place).

#### JDF-R420 — Textual posts (`Zasspoje` station numbers)
- **What:** for every distinct (stop, trimmed non-blank `stopPostNum`) among calls without a
  `stopPostId`: child `jdf:stop:{stopId}:post:{esc(number)}`, `public_code` = the number.
- **Where:** `JdfGtfsRules.derivedStopPosts`, `jdfStopPostNumId`.
- **Why:** README: "bare `:post:<value>` is a textual `Zasspoje` designation"; model comment
  "Standard says this should be an int, but it's a string in practice".
- **Pinned by:** `JDF conversion normalizes zones and preserves both post forms`.
- **Constants:** `post`.
- **Diagnostic:** none.
- **Proposed verdict:** port; the model must distinguish authored from textual posts (section 5).

#### JDF-R421 — A call's boarding point
- **What:** `stopPostId` set → authored post (even when the call also has a number); else a
  non-blank trimmed `stopPostNum` → textual post; else (estimation only) inferred; else
  `:unspecified`. A post ID absent from `Oznacniky` yields an ID with no stop row and crashes
  the bundle (`parents.[stopTime.stopId]`, B14).
- **Where:** `getGtfsStopTimeRowsInternal.convertedStopId`, `JdfBundle.fs` 478.
- **Why:** as R419/R420.
- **Pinned by:** `JDF conversion normalizes zones and preserves both post forms`.
- **Constants:** none.
- **Diagnostic:** none (crash).
- **Proposed verdict:** port; a dangling post ID is a diagnostic (`jdf_unknown_post`, new) and
  the call falls back to `:unspecified`.

#### JDF-R422 — Conflicting station numbers of one authored post
- **What:** calls giving one (stop, post ID) more than one distinct trimmed number: no number is
  used for its `public_code`.
- **Where:** `stopPostNumbersById` (log), `scanCallDerivedFacts` (diagnostic).
- **Why:** do not guess (README).
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** `conflicting_post_numbers` (warning, `jdf:stop:{stop}:post:id:{post}`,
  "Authoritative post has conflicting display numbers: {sorted, comma-joined}") plus a log
  warning. Golden: 0.
- **Proposed verdict:** port (one detection, one diagnostic).

#### JDF-R423 — Only referenced stops are published
- **What:** stops kept = boarding points referenced by retained emitted calls ∪ their parents;
  `cz_stops` and `cz_stop_zones` filtered the same way; location features and stop metadata only
  for kept stop places.
- **Where:** `assembleGtfsFeed`, `getTableProducers` (`retainedStopIds`).
- **Why:** unknown (implicit GTFS hygiene; `verify_gtfs_stops` in Oběhy rejects unreferenced
  boarding points).
- **Pinned by:** `JDF conversion normalizes zones and preserves both post forms` (stop set =
  referenced ∪ parents).
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port as a publish-time rule of `jr-publish` or a final compiler rule;
  later passes (posts, overlay) may add references, so prune at the end.

#### JDF-R424 — Stop metadata sidecar
- **What:** `source_stop_metadata` per kept stop place: `gtfs_stop_id`, `town`, `district`,
  `nearby_place`, `country`, `okres` (= `regionId`), `stop_id_provisional` = (`id ≥
  1,000,000,000`), `coordinates_missing` (no `Polzast` row or 0/0), `coordinate_precision`
  (`missing` / `stop` / `estimated`), `coordinate_source` (`PolzastZdroj.source` verbatim). Serving
  maps these onto `location` and lets boarding points inherit them (SERVING-R004, R005, R008).
- **Where:** `JdfBundleTables.getTableProducers.stopPlaces`.
- **Why:** README "Stop ID registry": "`source_stop_metadata.parquet` records `okres` and
  `stop_id_provisional`".
- **Pinned by:** `JDF bundle is deterministic and contains normalized sidecars` (presence).
- **Constants:** `StopRegistry.provisionalBase = 1_000_000_000`; precision words.
- **Diagnostic:** none.
- **Proposed verdict:** replace — the facts are `Place` fields (`municipality`,
  `district_name`, `district_code`, `nearby_place`, `country`, `position`), the provisional flag
  a field (section 5); no sidecar.

#### JDF-R425 — Location features
- **What:** per kept stop place, each distinct attribute of `Zastavky` mapped:
  `@` → `wheelchair_accessible`, `%` → `refreshments`, `W` → `toilet`, `w` →
  `accessible_toilet`, `x` → `request_stop`, `~` → `urban_transport_interchange`, `$` →
  `border_control_only`, `}` → `visually_impaired_accessible`, `t` → `accessibility_terminal`,
  `v` → `rail_interchange`, `b` → `line_interchange`, `U` → `metro_interchange`, `S` →
  `ship_terminal`, `J` → `airport_nearby`, `P` → `park_and_ride`; every other code none. Row:
  `gtfs_stop_id`, `source_code` (the code's text), `feature_kind`,
  `source_object_id = {stop_id}:attribute:{esc(code)}`; serving builds `assignment` scope
  `location`, ID `feedId("jdf","location-feature",[location; code; kind])`.
- **Where:** `getTableProducers.locationFeatureKind`, `locationFeatures`;
  `PackageSemanticRelations.semanticRelations`.
- **Why:** JDF_SEMANTICS "`assignment`: location features" (kind list identical).
- **Pinned by:** none.
- **Constants:** the table → `data/jdf/attribute-features.toml` (shared with JDF-R457).
- **Diagnostic:** none.
- **Proposed verdict:** port (scope `Location(Place)`).

### Stage 5 — agencies

#### JDF-R426 — Agency fields
- **What:** as 2.1: ID, verbatim name, URL with the unanchored `https?://` test and `http://`
  prefix, constant timezone and language, phone = `officePhoneNum`, email; missing URL →
  `jrutil://invalid`.
- **Where:** `JdfToGtfs.convertToGtfsAgency`, `Gtfs.fillStandardRequiredFields`.
- **Why:** "This will have to be adjusted for slovak datasets" (timezone); filler: "some
  GTFS-consuming software actually needs them filled" (Gtfs.fs).
- **Pinned by:** `JDF conversion emits Oběhy identities and public line numbers` (prefix only).
- **Constants:** `Europe/Prague`, `cs`, `http://`, regex `https?://`, `jrutil://invalid`.
- **Diagnostic:** none.
- **Proposed verdict:** port with fixes: anchored scheme test, null URL (serving verdict 7);
  phone choice is Q4 (B5).

### Stage 6 — routes

#### JDF-R427 — GTFS route type
- **What:** (effective mode, JDF route type) → type: Bus + `A`/`B` → `704`; Bus + `M`/`N`/`P` →
  `201`; Bus + `V`/`W`/`Z`/`RegionalInternational` → `701`; Bus + `D` → `202`; Tram → `900`;
  CableCar → `1701`; Metro → `401`; Ferry → `1000`; Trolleybus → `800`.
- **Where:** `JdfGtfsRules.getGtfsRouteType`.
- **Why:** extended GTFS route types; comments "Local bus", "International coach", "Regional
  bus", "National coach".
- **Pinned by:** `Extended bus types reserve coach for long-distance routes`.
- **Constants:** the table → `data/jdf/route-types.toml`.
- **Diagnostic:** none.
- **Proposed verdict:** port; with the one mode classifier (AGENTS rule 5) emit `Mode` and the
  type together instead of re-deriving mode from type (SERVING-R006). Golden: 701 ×600, 704 ×198,
  900 ×56, 800 ×30, 202 ×13, 401 ×3, 1701 ×1.

#### JDF-R428 — Route colours
- **What:** Bus: `M`/`N`/`P`/`D` → `004f71`; `RegionalInternational` → `00695c`; other → `0076a3`;
  Tram `7a0200`; Trolleybus `80166f`; CableCar `c8d021` on `1c1745`; Metro by upper-cased public
  line: `A` `00b274`, `B` `fbaf33` on `1c1745`, `C` `d31245`, other `1c1745`; Ferry `00b3cb` on
  `1c1745`; text `ffffff` unless stated. Detours keep the line's colours.
- **Where:** `JdfGtfsRules.getGtfsRouteColors`.
- **Why:** unknown (house style); README "Detour routes keep the line's own colours".
- **Pinned by:** `JDF routes receive default colors by transport mode`, `Route versions collapse
  into one regular and one detour route`.
- **Constants:** the table → `data/presentation/route-colours.toml` (shared with CZPTT-R061).
- **Diagnostic:** none.
- **Proposed verdict:** port as the default of the presentation pass (`ARCHITECTURE` puts
  presentation after the overlay), not in the compiler.

#### JDF-R429 — Grouping versions into output routes
- **What:** for each licence (ordinal order), for `detour` in `[false; true]`: versions of that
  kind grouped by the semantic key = concatenation **without separator** of: licence, agency ID
  (`jdf:agency:…`), public line or `""`, `Linky.name`, route type, colour or `""`, text colour or
  `""`, `string detour` (`True`/`False`). Members sorted by (`timetableValidFrom`,
  `idDistinction`); groups sorted by (first member's `timetableValidFrom`, key ordinal). Group 0
  → `jdf:route:{esc(lic)}` (detour: `…:detour`); group `i > 0` → that + `:` + lower-hex of the
  first 4 bytes of SHA-256(UTF-8 key). Every version maps to its group's ID; the group's
  representative is its first member.
- **Where:** `JdfGtfsRules.buildRouteGrouping`, `getRouteGrouping`, `gtfsRouteId`.
- **Why:** comment "Output GTFS routes group all merged versions of a CIS line that share route
  semantics, keeping detour timetables apart. The group holding the line's earliest-starting
  version (after merge-jdf bounding, the one in force) gets the plain id"; README "Output
  routes group the merged versions of one CIS line".
- **Pinned by:** `Route versions collapse into one regular and one detour route`, `A later
  version with different semantics gets a hashed route id`.
- **Constants:** `detour` suffix; 4-byte hash.
- **Diagnostic:** none.
- **Proposed verdict:** replace — keep `jdf:route:<line>` and `…:detour` (architect); a route
  with different semantics is a new route whose ID is built from a structured key (fields, not a
  concatenation, B8) by `identity`; whether hash suffixes stay is Q5.

#### JDF-R430 — Route attributes from the representative version
- **What:** route fields as in 2.4 come from the group's representative; `timetable_kind` from
  `detour` (`source_route_metadata` per version, then `distinct` per route ID).
- **Where:** `getGtfsRoutesWithPublicLines`, `getCzRoutes`, `getTableProducers.routes`,
  `PackageGtfsRelations` (`timetableKinds`).
- **Why:** README "Serving `route.timetable_kind` is `regular` or `detour` for JDF routes".
- **Pinned by:** `Route versions collapse into one regular and one detour route`.
- **Constants:** `Výlukový jízdní řád` (`detourRouteDescription`) → `data/jdf/texts.toml`.
- **Diagnostic:** none.
- **Proposed verdict:** port (`Route.timetable_kind` exists in the model); a route with no
  published trip is not published (B31), with its notes and integrations.

#### JDF-R431 — Route presentation overrides
- **What:** CSV with header exactly `agency_id,licence,route_short_name,route_color,
  route_text_color,reason` (trimmed); colours must match `^[0-9A-Fa-f]{6}$` and are lower-cased;
  `reason` required; a row setting no field is an error. Per published route with a CIS licence
  (from `cz_routes.cis_line_id`) and the agency IČO **parsed from the agency ID**
  (`^jdf:agency:([^:]+):[0-9]+$`, unescaped): each of short name, colour, text colour
  independently takes the most specific matching rule that sets it (JDF-R406), else keeps the
  computed value. Applied after grouping: IDs never change; `cz_routes.public_line_number`
  keeps the source value.
- **Where:** `RouteRules.loadPresentationRules`, `resolvePresentation`, `presentRoute`,
  `jdfAgencyIco`; `JdfToGtfs.applyRoutePresentationRules`.
- **Why:** geodata README: overrides the published line marking and colours; "Regional-overlay
  route matching still uses the source line marking".
- **Pinned by:** `Presentation rules override fields by specificity`, `Presentation rules
  restyle published routes without changing ids`, `JDF agency ids yield the agency IČO`.
- **Constants:** header; data `routes/presentation.csv` (163 rows: 3 Arriva Express `AEx`
  `1c1745`/`03eafc`, 1 Teplice livery `17196264,*` `283286`/`ffce00`, 159 DÚK lines
  `00843d`/`ffffff`).
- **Diagnostic:** `route_presentation_override` (info, route ID, `"{field}: {before} -> {after}"`,
  one per changed field); manifest `route_presentation_overrides { rules_sha256,
  overridden_routes, by_field }`. Golden: `jdf:route:721341` short name `341 → AEx` and colours.
- **Proposed verdict:** port as the presentation pass after the overlay (pipeline order), with
  the agency IČO as a field (no ID parsing); the filtered feed question follows (Q9).

#### JDF-R432 — Ignored `Linky` fields
- **What:** `grouped`, `usesStopPosts`, `oneWay`, `reserved1`, `licenceNum`,
  `licenceValidFrom/To` are parsed and not used.
- **Where:** absence in `JdfToGtfs`.
- **Why:** unknown (TODO on `grouped`).
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port the absence, but keep `oneWay` as a version fact: JDF-R434's
  parity rule may not hold for one-way timetables (golden: 16 one-way versions, only odd trips,
  all ascending) (Q6).

### Stage 7 — trips

#### JDF-R433 — Trip version key and trip ID
- **What:** per version, `sourceOf` = (`originalValidFrom`, `originalValidTo`,
  `sourceCreationDate`) from `JrutilRouteVersions.txt`, else (`timetableValidFrom`,
  `timetableValidTo`, `VerzeJDF.creationDate`). Per licence: versions grouped by (original
  validFrom, detour); base key `{esc(lic)}:{yyMMdd(validFrom)}[:det]`; within a group, distinct
  sources sorted by (validTo, creation date) → source 0 keeps the base key, source `i > 0` gets
  `:{lower-hex first 4 bytes of SHA-256("{yyMMdd from}|{yyMMdd to}|{yyMMdd created or ''}")}`;
  versions of one source (merger date chunks) sorted by (`timetableValidFrom`, distinction) →
  chunk 0 the source key, chunk `p > 0` `:p{p+1}`. A repeated key throws
  `JDF route {id}/{dist} repeats trip version key {key}`. Trip ID = `jdf:trip:{key}:{Spoje.id}`.
- **Where:** `JdfGtfsRules.buildTripVersionKeys`, `jdfTripId`.
- **Why:** comment and README: "Trip ids name the published schedule rather than the
  merge-assigned distinction, so they stay the same between runs".
- **Pinned by:** `Trip version keys name the published schedule in any merge order`,
  `Versions starting on one day get a hashed trip version key` (`JdfMergerTests`); `JDF
  conversion emits Oběhy identities…` (`jdf:trip:586001:260101:1`).
- **Constants:** `yyMMdd`, `det`, `p`, 4-byte hash.
- **Diagnostic:** build error on a repeat.
- **Proposed verdict:** port the `jdf:trip:<line>:<yymmdd>[:det]:<trip>` part (architect);
  the hash and `:pN` parts are Q5. The model carries licence, original validity start, detour
  flag and trip number as fields (section 5).

#### JDF-R434 — Direction and call order from trip parity
- **What:** even trip number → reverse: calls ordered by descending route stop, `direction = 0`;
  odd → ascending, `direction = 1`.
- **Where:** `Jdf.tripIsReverse`, `getGtfsTripsWithEndpoints`, `getGtfsStopTimeRowsInternal`
  (inline `tripId % 2L = 0L`).
- **Why:** model comment: "For odd trips, the order they visit the stops is lowest-to-highest.
  For even trips, it's in reverse" (JDF convention).
- **Pinned by:** `JDF bundle is deterministic and contains normalized sidecars` (slot IDs with
  direction `1`).
- **Constants:** parity.
- **Diagnostic:** none.
- **Proposed verdict:** port (`jdf.direction` rule), with a diagnostic when an ordered trip's
  times decrease (new, `jdf_order_against_times`), which would expose a one-way timetable (Q6).

#### JDF-R435 — Headsign
- **What:** the endpoint call is the last ordered call, stepping back while the previous call
  has the same route stop ID; headsign = `getStopName` of its stop (no ` [?]`). The endpoint is
  recorded for every trip while streaming calls; if the number of recorded endpoints differs
  from the number of trips, a fallback scans all calls for the maximum signed order instead (and
  throws for a trip without calls, B14). Whether the endpoint call is served is not checked.
- **Where:** `getGtfsStopTimeRowsInternal` (`recordEndpoint`), `getGtfsTripsWithEndpoints`.
- **Why:** unknown (JDF has no headsign field).
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** replace — headsign = name of the last **emitted** call's place (golden:
  identical for all 92,705 trips, because no trip ends on an unserved row).

#### JDF-R436 — Trip short name
- **What:** `"{licence} {trip number}"`.
- **Where:** `getGtfsTripsWithEndpoints`.
- **Why:** comment "This is kind of arbitrary".
- **Pinned by:** none.
- **Constants:** format.
- **Diagnostic:** none.
- **Proposed verdict:** port for parity, or drop (Q7): the value restates the source keys.

#### JDF-R437 — Wheelchair-accessible trips (closed world)
- **What:** `@` or `{` among the trip's attributes → `1`, else `2`.
- **Where:** `getGtfsTripsWithEndpoints`.
- **Why:** comment "JDF is a closed-world source for trip accessibility. An absent @/{
  designation means the trip is not accessible"; JDF_SEMANTICS notes the collapse of `@`/`{`.
- **Pinned by:** none.
- **Constants:** `@`, `{`.
- **Diagnostic:** none.
- **Proposed verdict:** port (`Tristate`); the `partly_…` distinction survives as a trip feature
  (JDF-R457).

#### JDF-R438 — Bikes allowed (closed world)
- **What:** `O` → `1`, else `2`.
- **Where:** `getGtfsTripsWithEndpoints`; `TripWriter` maps 0/1/2.
- **Why:** JDF_SEMANTICS: "`O` on a trip | Correctly projected to `bikes_allowed=1`; absence
  becomes `bikes_allowed=2`".
- **Pinned by:** none.
- **Constants:** `O`.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### JDF-R439 — Trip groups, blocks, shapes
- **What:** `SpojSkup` and `Spoje.tripGroupId` ignored; block and shape null.
- **Where:** absence.
- **Why:** comment "TripGroups" listed under "Information that is lost in the conversion".
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port the absence; record trip groups as a gap (golden: none).

### Stage 8 — calls

#### JDF-R440 — Call order and trip spans
- **What:** a trip's calls must be contiguous in `Zasspoje` (one span per (licence,
  distinction, trip)); spans are processed in ordinal trip-ID order (the native writer requires
  strictly increasing (trip ID, sequence)); within a trip, calls sorted by `routeStopId × (−1 if
  even)` with `Array.sortBy` (unstable for equal route stops).
- **Where:** `getGtfsStopTimeRowsInternal` (`tripStopGroups`), `JdfCallStore.tripSpans`,
  `TripCallWriter.Writer.Append`.
- **Why:** "Calls of one trip are contiguous (JdfCallStore spans)".
- **Pinned by:** `Streamed call metadata retains ordinal trip ordering and GTFS sequences`.
- **Constants:** none.
- **Diagnostic:** a non-contiguous trip → `Trip calls are not strictly ordered at …` (build
  error).
- **Proposed verdict:** replace — the reader groups calls by trip; order by route stop with a
  stable tie-break; duplicate route stops in one trip are a diagnostic (golden: none).

#### JDF-R441 — Unserved calls are not published
- **What:** a call is not emitted when its departure is `|` (passes without stopping) or `<`
  (does not pass), or arrival and departure are both blank. An arrival of `|`/`<` with a
  departure time throws `failwith "Invalid data"` (B13).
- **Where:** `getGtfsStopTimeRowsInternal`; same predicate in `JdfBundleTables.callIsEmitted`,
  `JdfPostPlan.callIsUsable`, `JdfInternationalFilter` (section 9).
- **Why:** comment "The JDF specification allows stops that aren't served to have a blank
  arrival and departure time. In GTFS, such stops are just omitted"; PRODUCTION_CONTRACT "JDF
  pass-through stops are not published, because the source gives them no time".
- **Pinned by:** none directly.
- **Constants:** `|`, `<`.
- **Diagnostic:** none.
- **Proposed verdict:** port; the inconsistent arrival/departure pair becomes a diagnostic
  (`jdf_inconsistent_pass`, new) and the call is treated as served with its one time. Golden:
  (`<`,`<`) 143,644 and (`|`,`|`) 1,258 unserved; no inconsistent pair.

#### JDF-R442 — Times and midnight
- **What:** per trip, in call order, arrival then departure of each emitted call: a time earlier
  than the previous time read adds 86,400 s to the running offset; value = `H×3600 + M×60` +
  offset. A missing arrival takes the departure, a missing departure the arrival.
- **Where:** `getGtfsStopTimeRowsInternal.adjustTime`.
- **Why:** comment "We have to deal with stop times for each trip separately, because we have to
  count 23:59 -> 00:00 crossings … Not even this is enough" (a gap of more than 24 h is
  undetectable).
- **Pinned by:** none.
- **Constants:** `HHmm` pattern; 86,400.
- **Diagnostic:** none.
- **Proposed verdict:** port with a threshold: a backwards step larger than
  `jdf.midnight.max_backstep_minutes` (proposal 12 h) is a data error, diagnosed
  (`jdf_time_backwards`, new) instead of becoming +24 h (B18; golden: no gap > 6 h, no trip >
  8 h).

#### JDF-R443 — Call sequence
- **What:** `sequence` = 0-based index in the ordered call list including unserved calls.
- **Where:** `stopSequence = i`.
- **Why:** unknown; the test comment "JDF calls keep their source sequence, so no call needs a
  key" relies on it.
- **Pinned by:** `JDF bundle is deterministic and contains normalized sidecars` (0 call keys),
  `Streamed call metadata retains ordinal trip ordering and GTFS sequences`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port (stable and gap-tolerant; model allows gaps). Q8 asks whether to
  use the route stop number instead.

#### JDF-R444 — Pickup and drop-off
- **What:** flags = attributes of the call's stop (`Zastavky`) ∪ attributes of the call
  (`Zasspoje`); route-stop (`Zaslinky`) and trip (`Spoje`) attributes are **not** used. Service
  = `x` → 3 (ask driver); else `!` → 2; else `T` → 2; else 0. Pickup = 1 if `(` or `$`, else
  service; drop-off = 1 if `)` or `$`, else service. Unknown attribute numbers on calls are
  ignored.
- **Where:** `getGtfsStopTimeRowsInternal` (`attributeFlagsById`, `flagsForAttributes`).
- **Why:** comments: "`$` marks a border/customs stop where passengers may neither board nor
  alight"; "ConditionalService is a very broad attribute … PhoneBefore implies some human
  interaction, so that's my choice". JDF_SEMANTICS: "`x`, `T`, `!`, `(`, `)` at a stop/call |
  Usefully approximated through GTFS pickup/drop-off values".
- **Pinned by:** `Timed calls at border-only stops are kept without boarding or alighting`.
- **Constants:** code → boarding table → `data/jdf/attribute-boarding.toml`.
- **Diagnostic:** none.
- **Proposed verdict:** port with the route-stop and trip attributes included (B1, B2; Q3).
  Golden: 1,018,252 emitted calls lie on a `Zaslinky` route stop coded `x` without the call being
  coded `x`; 23 trips coded `T` and 8 coded `!` have regular calls.

#### JDF-R445 — Timepoint and time windows
- **What:** `timepoint = true` for every call; `minArrivalTime`/`maxDepartureTime` ignored.
- **Where:** `exactTimepoint`; comment "This will be dynamic when support for JDF's min/max times
  comes".
- **Why:** comment.
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port the timepoint; carry the time window as call facts (section 5; golden
  1,731 calls) and decide its publication separately (Q10).

#### JDF-R446 — Kilometre
- **What:** `Zasspoje.kilometer` (decimal) → `source_kilometre` (double).
- **Where:** `shapeDistTraveled = jdfTripStop.kilometer`; `TripCallWriter.fromGtfs`.
- **Why:** PRODUCTION_CONTRACT: "a source's own distance is `trip_call.source_kilometre`: the JDF
  kilometre value of the call (tariff distance)".
- **Pinned by:** `JDF kilometres are kept as source_kilometre and restored in the overlay's base
  view`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port (`Call.kilometre`).

#### JDF-R447 — Route-stop key and the transfer side channel
- **What:** each emitted call carries `compositeKey [output route; distinction; routeStopId]`
  in `trip_call.route_stop_id` until the route-stop pass replaces it; for each (trip ID,
  `routeStopId`) named by a `Navaznosti` row, the first emitted call's sequence goes into
  `transferSequences`.
- **Where:** `JdfBundle.fs` 483–488; `Serving.Identity.routeStopKey`.
- **Why:** README "route-stop keys include the version (`<route>/<distinction>/<route stop>`)
  because route stop numbers are unique only within one version".
- **Pinned by:** `Route stop keys stay unique across versions of one route`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** replace — the call carries its source route stop number and the trip its
  version (section 5); zones, claims and restrictions join through those fields.

### Stage 9 — zones

#### JDF-R448 — Zone token normalisation
- **What:** `Zaslinky.zone` split on `,`, each token trimmed, empty tokens dropped, repeats
  dropped (first kept), order kept. No IDS system is inferred.
- **Where:** `Jdf.normalizeZoneTokens`.
- **Why:** README "Raw zone lists are split, trimmed and deduplicated without attempting to infer
  their IDS system, so a JDF `zone_system` is null".
- **Pinned by:** `JDF conversion normalizes zones and preserves both post forms` (`6,193`);
  `JDF bundle is deterministic…` (slot zones `6`, `193`).
- **Constants:** separator `,`.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### JDF-R449 — Route-stop zones
- **What:** `source_route_stop_zone_metadata` row per (route stop, token): `gtfs_route_id`
  (output route), `source_route_version`, `source_route_stop_id`, `zone_id =
  jdf:zone:{esc(lic)}:{dist}:{esc(code)}`, `zone_code`, `zone_order` (0-based); distinct by
  (route, version, route stop, zone ID); sorted. Serving applies them to calls by their
  route-stop key (serving §7.5 precedence 2).
- **Where:** `getTableProducers.routeStopZones`.
- **Why:** README "The tokens, in source order, land on the route stop slot, or on its calls
  where line versions disagree".
- **Pinned by:** `JDF bundle is deterministic and contains normalized sidecars`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** replace — the compiler sets `Call.zones` (interned ordered list, system
  `None`) from the call's route stop; the route-stop pass decides slot vs call publication.

#### JDF-R450 — GTFS `stops.zone_id`
- **What:** a stop place gets `zone_id` = the code when all its route-stop memberships are one
  (route-scoped zone ID, code) pair, else none; `cz_stop_zones` rows (stop, zone ID, code, output
  route, `ids_system_id` null, provenance `jdf:{version}`) per membership.
- **Where:** `getGtfsStopsWithPlan` (`zonesByStop`), `getCzStopZones`.
- **Why:** comment "GTFS only permits one zone_id".
- **Pinned by:** `JDF conversion normalizes zones and preserves both post forms`.
- **Constants:** none.
- **Diagnostic:** `standard_zone_omitted` (warning, per stop place with more than one distinct
  zone ID). Golden: 4,110 — because zone IDs are per version, almost every stop on two lines
  fires.
- **Proposed verdict:** drop — `zone_id` is not in `location` or `gtfs.zip`; `cz_stop_zones`
  rows always have a route and therefore never feed serving's location fallback (B16). Same
  verdict as CZPTT-R087.

#### JDF-R451 — One route stop, several stops
- **What:** (licence, distinction, `routeStopId`) rows of `Zaslinky` naming more than one stop.
- **Where:** `JdfBundleManifest.diagnostics` (`conflictingRouteStops`).
- **Why:** a route-stop number must identify one stop for zones to apply.
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** `conflicting_route_stop_zone_mapping` (severity `error` but the build
  continues, `jdf:route-stop:{esc(lic)}:{dist}:{rs}`). Golden: 0.
- **Proposed verdict:** port as a compile-time check; the zone is not applied for such a route
  stop (or the route stop is quarantined); a reader-level duplicate check may belong to jdf-read.md.

### Stage 10 — notes

#### JDF-R452 — Route information (`Udaje`)
- **What:** per `Udaje` row with non-blank text whose version's output route is retained: note
  as in 2.9, linked by a `route` assignment.
- **Where:** `getTableProducers.notices`; `NoteWriter.assignment`.
- **Why:** JDF_SEMANTICS "Retain route information … as verbatim Unicode".
- **Pinned by:** `JDF bundle is deterministic…` (3 notes in the fixture).
- **Constants:** kind `route_information`.
- **Diagnostic:** blank → `blank_notice` (warning, note ID, "Textual notice is blank and was
  omitted"). Golden: 0.
- **Proposed verdict:** port (`NoteKind::RouteInformation`, `Scope::Route`). Several versions in
  one output route keep separate notes even when the text is equal (golden: 2,992 notes for 901
  routes).

#### JDF-R453 — Timetable notes (`Caskody`)
- **What:** per `Caskody` row: text = non-blank `note`; label = non-blank `designation`. Skipped
  when typed and text-free (calendar handled, **silently**), or when neither text nor label;
  emitted for retained trips with `service_note_type` = `service`, `service_also`,
  `service_only`, `no_service`, `odd_weeks`, `even_weeks`, `odd_weeks_from_to`,
  `even_weeks_from_to` for types 1–8, validity `dateFrom`/`dateTo`.
- **Where:** `getTableProducers.notices`, `serviceNoteTypeName`.
- **Why:** comment "Bundle sidecars retain otherwise-unhandled textual service notes";
  JDF_SEMANTICS asks for the opposite on text-free designations (section 8).
- **Pinned by:** `JDF bundle is deterministic…` (count only).
- **Constants:** the type names → model enum.
- **Diagnostic:** untyped, designation and text blank → `blank_notice`; for omitted trips →
  `filtered_enrichment` (text present or untyped with designation). Typed text-free rows: none.
- **Proposed verdict:** replace — every `Caskody` row is retained (typed ones as calendar-designation
  facts with their designation, interval and type; text rows as notes), per JDF_SEMANTICS (B4,
  Q11).

#### JDF-R454 — Reservation notes (`Mistenky`)
- **What:** per `Mistenky` row: ordinal = 1-based position among rows of the same (licence,
  distinction, trip) in source order; note kept when text non-blank and the trip
  `jdf:trip:{key}:{tripId}` is retained. A row with trip number `0` names no trip: dropped
  silently by the international filter (`regional-adjacent`) or as `filtered_enrichment`
  (`keep-all`).
- **Where:** `getTableProducers.notices`, `withOwnerOrdinals`; `JdfInternationalFilter` 309.
- **Why:** JDF_SEMANTICS "Retain … `Mistenky`".
- **Pinned by:** none.
- **Constants:** kind `reservation_note`.
- **Diagnostic:** `blank_notice`, `filtered_enrichment`.
- **Proposed verdict:** port with trip `0` = all trips of the version (route scope) if the
  specification confirms (B3, Q12). Golden: 7 of 7 rows have trip `0`.

#### JDF-R455 — Note links
- **What:** `assignment` per note: ID `feedId("jdf","note-assignment",[note; scope; route;
  trip])`, scope `trip` when a trip is set else `route`, `route_id` only for route notes,
  `trip_id`, `note_id`.
- **Where:** `NoteWriter.assignment`.
- **Why:** serving 5.2.
- **Pinned by:** none (serving).
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port (model `Assignment { kind: Note }`; ID per serving verdict 12).

### Stage 11 — features

#### JDF-R456 — Attribute references
- **What:** `Jdf.parseAttributes` resolves attribute numbers of stops and trips through
  `Pevnykod` with `Array.find` (an unknown number throws, B14) into a set (duplicates collapse);
  call and route-stop flags and restriction masks use dictionaries and ignore unknown numbers.
  A `Pevnykod` value of only digits is `DayOfWeekService n` for any `n` (B28).
- **Where:** `Jdf.parseAttributes`, `JdfModel.Attribute.CsvParse`, `flagsForAttributes`,
  `restrictionMask`.
- **Why:** none given.
- **Pinned by:** none.
- **Constants:** the code table of `JdfModel.Attribute` (section 2 of jdf-read.md).
- **Diagnostic:** none (crash or silence).
- **Proposed verdict:** replace — one resolver; an unknown number or a digit outside 1–7 is a
  diagnostic (`jdf_unknown_attribute`, new) and the reference is dropped.

#### JDF-R457 — Trip features
- **What:** per retained trip, each distinct attribute: `X`, `+`, digit → `calendar_designation`;
  `R` → `reservation_available`; `#` → `reservation_required`; `|` → `not_stopping`; `<` →
  `diversion`; `@` → `wheelchair_accessible_vehicle`; `{` → `partly_wheelchair_accessible_vehicle`;
  `%` → `refreshments_on_vehicle`; `[` → `luggage_transport`; `O` → `bicycle_transport`; `x` →
  `request_stop`; `(` → `exit_only`; `)` → `boarding_only`; `T` → `on_request`; `!` →
  `conditional`; `I` → `integrated_transport`; `s` → `self_service_ticketing`; `W` → `toilet`;
  `w` → `accessible_toilet`; `~` → `urban_transport_interchange`; `$` → `border_control_only`;
  `}` → `visually_impaired_accessible`; `t` → `accessibility_terminal`; `v` → `rail_interchange`;
  `b` → `line_interchange`; `U` → `metro_interchange`; `S` → `ship_terminal`; `J` →
  `airport_nearby`; `P` → `park_and_ride`; `§`, `A`, `B`, `C` → none. Row: scope `trip`,
  `source_code` = the code text (digit for weekdays), `source_object_id =
  {trip}:attribute:{esc(code)}`, ID `feedId("jdf","trip-feature",[trip; code; kind])`, no note.
- **Where:** `getTableProducers.tripFeatureKind`, `tripFeatures`.
- **Why:** comment "JDF attribute codes as typed `assignment` kinds (JDF_SEMANTICS.md). Travel
  exclusions (§, A-C) are `travel_restriction`, not features"; PRODUCTION_CONTRACT "JDF calendar
  codes (`X`, `+`, `1`–`7`) are `calendar_designation` assignments".
- **Pinned by:** none.
- **Constants:** the table → `data/jdf/attribute-features.toml`.
- **Diagnostic:** none.
- **Proposed verdict:** port (`AssignmentKind::Feature` / `CalendarDesignation`, `Scope::Trip`);
  every kind exists in `jr_model::Feature`.

#### JDF-R458 — Integrated systems as route features (`LinExt`)
- **What:** distinct (output route, `transportSystemId`) over all `LinExt` rows of retained
  routes, sorted → assignment scope `route`, kind `integrated_transport`, `source_code` = system
  number, `source_object_id = {route}:integration:{system}`, ID
  `feedId("jdf","route-integration",[route; system])`.
- **Where:** `getTableProducers.routeIntegrations`.
- **Why:** comment "LinExt: the integrated systems a line belongs to, by their CIS code (30001 =
  PID)".
- **Pinned by:** `JDF bundle is deterministic…` (`jdf:route:586001`, `30001`; none for
  `582486`).
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port, with the preferential flag carried (section 5; the filtered feed
  distinguishes preferential rows).

#### JDF-R459 — Feature scopes
- **What:** features exist only at trip, route and location scope. Call attributes (`Zasspoje`
  `x`, `(`, `)`, `T`, `!`, …), route-stop attributes (`Zaslinky`) and `Caskody` links are not
  published as assignments.
- **Where:** absence in `getTableProducers`.
- **Why:** unknown.
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** replace — call-scope features for call attributes and route-stop
  attributes expanded to the version's calls (or a route-stop scope), as JDF_SEMANTICS requires
  (Q3). Golden call codes: `x` 43,052, `(` 1,267, `T` 204, `)` 25 (plus the exclusion codes);
  route-stop codes: `x` 10,814, `(` 1, `)` 1.

### Stage 12 — connection claims

#### JDF-R460 — `Navaznosti` → connection claims
- **What:** rows owned by (licence, distinction, trip) get a 1-based ordinal in source order;
  ID `jdf:transfer:{esc(lic)}:{dist}:{trip}:{ordinal}`; kept when the trip is retained and the
  call with `routeStopId` is emitted; fields as in 2.12; `origin_sequence` = first emitted
  sequence of that route stop (fallback `0`, which is also a valid first sequence, B9).
- **Where:** `getTableProducers.transfers`, `JdfBundle.fs` (`transferCallQueries`,
  `emittedTransferCalls`), `PackageSemanticRelations.semanticRelations`.
- **Why:** JDF_SEMANTICS "`connection_claim`: Retain every `Navaznosti` row even if it cannot
  identify a final target journey"; PRODUCTION_CONTRACT "`m` is `waits_for`, `M` is
  `connects_to`".
- **Pinned by:** `JDF bundle is deterministic…` (1 claim; the fixture's second row, on an
  unemitted call, is dropped).
- **Constants:** `m` → `waits_for`, `M` → `connects_to`; `target_derivation = structured`;
  `resolution_status = unresolved`.
- **Diagnostic:** omitted trip → `filtered_enrichment`; call not emitted →
  `unjoinable_call_enrichment` (warning, "Call-scoped enrichment cannot join an emitted GTFS
  call"). Golden: 371 unjoinable (claims and restrictions together).
- **Proposed verdict:** port as a model relation (section 5); an unknown direction letter is a
  diagnostic, not a verbatim value; a claim on an unserved call is kept with the nearest
  emitted call or quarantined (Q13).

#### JDF-R461 — Claim target fields
- **What:** target route, stop, post, end stop, end post are `int64 option` and published as
  decimal text; `0` (the merged files' placeholder) is kept as `"0"`; a target licence loses
  leading zeros; target stop numbers are CIS register numbers (model comment), not `jdf:stop`
  numbers.
- **Where:** `JdfModel.Transfer`, `getTableProducers.transfers`.
- **Why:** model comment "All these IDs point to the global stop register, *not* to local stop
  IDs".
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** replace — target licence kept as 6-digit text; `0` read as absent (in
  jdf-read.md); stop/post numbers kept as source codes for later resolution (B10).

### Stage 13 — travel restrictions

#### JDF-R462 — Exclusion codes
- **What:** attribute `§` → bit 1 (group `§`), `A` → 2, `B` → 4, `C` → 8; a mask yields groups in
  the order `§`, `A`, `B`, `C`.
- **Where:** `JdfBundleTables.restrictionLookup`, `restrictionMask`, `restrictionGroupCodes`.
- **Why:** README "calls sharing the same effective group code (`§`, `A`, `B` or `C`) may not be
  used as both the boarding and alighting endpoints".
- **Pinned by:** `JDF bundle is deterministic…` (5 restrictions).
- **Constants:** the four codes.
- **Diagnostic:** none.
- **Proposed verdict:** port (group as a closed enum).

#### JDF-R463 — Route-stop restrictions
- **What:** per `Zaslinky` row of a retained output route, one `route_stop` row per group (2.13).
- **Where:** `getTableProducers.restrictions`.
- **Why:** README "`Zaslinky` produces `route_stop` assignments".
- **Pinned by:** `JDF bundle is deterministic…` (restriction slot IDs resolve).
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port; the model holds the restriction against (version, route stop) and
  the route-stop pass maps it to slots (serving §7.6).

#### JDF-R464 — Trip-call restrictions
- **What:** per call whose own attributes carry exclusion codes: trip not retained → ID
  `jdf:restriction:{esc(lic)}:{dist}:{trip}:{rs}` to `filtered_enrichment`; call not emitted →
  same ID to `unjoinable_call_enrichment`; else one `trip_call` row per group (trip ID, route
  stop, group), distinct, sorted.
- **Where:** `scanCallDerivedFacts`, `getTableProducers.restrictions`.
- **Why:** README "`Zasspoje` produces `trip_call` assignments".
- **Pinned by:** `JDF bundle is deterministic…`.
- **Constants:** none.
- **Diagnostic:** as stated.
- **Proposed verdict:** port with `Scope::Call { trip, sequence }` resolved at compile time
  (fixes serving open question 9: all 457 golden rows are unresolved today).

#### JDF-R465 — Singleton restriction groups
- **What:** per retained trip, over emitted calls, group → set of route stops using the union of
  the route-stop and call masks; a group with exactly one route stop → diagnostic.
- **Where:** `scanCallDerivedFacts.flushTrip`.
- **Why:** a group needs two calls to restrict a journey.
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** `singleton_travel_restriction` (warning, `{trip}:restriction-group:{esc(group)}`,
  "Effective travel-exclusion group has only one emitted call"). Golden: 0.
- **Proposed verdict:** port.

### Stage 14 — keys and source identity

#### JDF-R466 — National keys
- **What:** `cz_trips` (licence, trip number) and `cz_routes` (licence) feed `cis:line_trip` and
  `cis:line` (2.18; SERVING-R017). A licence that is not 1–6 ASCII digits gets no key, silently.
- **Where:** `getCzTrips`, `getCzRoutes`, `PackageKeyRelations.keyRelations`.
- **Why:** PRODUCTION_CONTRACT "`582492:143` for CIS line + trip".
- **Pinned by:** `JDF bundle is deterministic…` (`cis:line_trip` present).
- **Constants:** `PadLeft(6,'0')`.
- **Diagnostic:** none (new: `jdf_no_cis_key` for a non-numeric licence).
- **Proposed verdict:** port in the compiler as `SourceKey` rows (CZPTT-R118 analogue); keys
  stay (architect).

#### JDF-R467 — `cz_stops` source identities
- **What:** `cz_stops` rows for every kept stop and boarding point: `stop_place_id`, `post_id`
  (post ID or number), `source_ids` (`jdf:stop:N`; `jdf:stop:N:post:id:K` plus
  `jdf:stop:N:post:{esc(num)}` when a unique number exists, comma-joined; textual posts their own
  ID; unspecified and inferred none), `cis_stop_id` and `asw_id` always null.
- **Where:** `JdfToGtfs.getCzStopsWithPlan`.
- **Why:** README "Typed `location` and `source_entity_map` rows preserve … source identity".
- **Pinned by:** `JDF conversion normalizes zones and preserves both post forms`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** drop — no serving relation reads `cz_stops` (only the overlay's
  compiler view rebuilds it from serving).

#### JDF-R468 — Version-level source IDs
- **What:** `jdf:route:{esc(lic)}:{dist}` names a version in `source_route_metadata`
  (`source_route_id`) and in international, transport-mode and public-line diagnostics.
- **Where:** `JdfGtfsRules.jdfRouteId`/`jdfSourceRouteId`.
- **Why:** diagnostics need the version; the output route covers several.
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** —
- **Proposed verdict:** replace — diagnostics name the version by fields (licence,
  distinction, original validity) and the output route by handle; no second ID in the
  `jdf:route:` namespace (it can collide with an 8-digit hash suffix, B27).

### Stage 15 — feed and package

#### JDF-R469 — Feed info
- **What:** `feed_version` = `jdf-{VerzeJDF.version}` + (`:{batchId}` if non-blank) +
  (`:{yyyyMMdd(creationDate)}` if present); start = min(calendar starts ∪ added dates), end =
  max(calendar ends ∪ added dates); publisher `Oběhy project (via JrUtil)`,
  `https://obehy.cz`, `cs`, contact `admin@obehy.cz`.
- **Where:** `JdfToGtfs.feedInfo`, `Gtfs.obehyFeedInfo`.
- **Why:** SERVING-R039 pass-through.
- **Pinned by:** `JDF conversion emits Oběhy identities…` (`jdf-1.11:OBEHY-EXT:20260718`),
  `Feed contact round-trips and legacy feed info remains readable`.
- **Constants:** publisher strings (shared with CZPTT-R081) → `policy/publish.toml`.
- **Diagnostic:** none.
- **Proposed verdict:** port (`Timetable.source_version`; dates derived in `jr-publish`).

#### JDF-R470 — Bundle diagnostics
- **What:** the codes `filtered_trip`, `filtered_enrichment`, `unjoinable_call_enrichment`,
  `blank_notice`, `singleton_travel_restriction`, `conflicting_route_stop_zone_mapping`
  (`error`), `missing_public_line_number`, `standard_zone_omitted`, `conflicting_post_numbers`,
  `missing_stop_coordinates`, plus `filtered_international_trips`,
  `filtered_international_route`, `corrected_transport_mode` (`info`),
  `transport_mode_rule_mismatch`, `route_presentation_override` (`info`); all others `warning`;
  sorted by (code, `source_object_id`); serving summarises them (SERVING-R047).
- **Where:** `JdfBundleManifest.diagnostics`, `JdfBundle.fs` 573–605.
- **Why:** —
- **Pinned by:** none.
- **Constants:** messages as quoted in the rules.
- **Diagnostic:** —
- **Proposed verdict:** replace — `jr_model::diag` codes per rule, summarised by
  `jr-publish` (serving verdict 15).

#### JDF-R471 — Compiler manifest section
- **What:** `bundle_format = obehy-jrutil-jdf`, `bundle_version = 1`, `source_snapshot` (the
  descriptor), `source_format { kind = jdf, version, du_number, region, batch_id,
  creation_date, generator }`, `conversion { tool = jrutil, version, international_route_filter
  {…}, transport_mode_corrections {…}, route_presentation_overrides {…},
  route_type_distribution {type: count} (ordinal keys), estimated_posts {…} }`, then `gvd {
  year, start_date, end_date }` (`yyyyMMdd`) when `--gvd-year` is given. Serving turns `gvd`
  into `service_horizon` (golden `2026`, `20251214`–`20261212`).
- **Where:** `JdfBundleManifest.writeManifest`, `recordManifestGvd`.
- **Why:** provenance.
- **Pinned by:** `JDF bundle is deterministic…` (production manifest fields).
- **Constants:** `obehy-jrutil-jdf`, `BundleVersion = 1`, `ParquetSchemaVersion = 7` (sidecar
  metadata), filter literals 120/60/200/80.
- **Diagnostic:** none.
- **Proposed verdict:** replace — `jr-publish` writes the manifest (serving §10.1); the
  compiler contributes its policy digests and counts as data; the horizon comes from the build
  spec.

#### JDF-R472 — Sidecars
- **What:** written to the package input: `source_route_metadata` (`gtfs_route_id`,
  `source_route_id`, `route_distinction`, `source_agency_id`, `source_agency_distinction`,
  `source_transport_mode`, `effective_transport_mode`, `valid_from`, `valid_to`, `detour`; only
  `detour` is read), `source_stop_metadata` (R424), `source_location_feature_metadata` (R425),
  `source_route_stop_zone_metadata` (R449), `source_transfer_metadata` (R460),
  `source_travel_restriction_metadata` (R463, R464); Parquet metadata `obehy.bundle_version`,
  `obehy.schema_version = 7`, `obehy.source_id`, `obehy.snapshot_id = sha256:{payload}`; Snappy,
  65,536-row groups.
- **Where:** `getTableProducers`, `writeParquet`, `writeBundleCore` 539–556.
- **Why:** comment "Parquet is deliberately limited to source facts that cannot be reconstructed
  from standard GTFS plus the Oběhy extension tables".
- **Pinned by:** `JDF bundle is deterministic and contains normalized sidecars`.
- **Constants:** schema versions.
- **Diagnostic:** none.
- **Proposed verdict:** drop — no sidecars (AGENTS rule 1); every fact is a model field.

#### JDF-R473 — Dangling references crash
- **What:** a call's stop missing from `Zastavky` (`stopFlagsById.[…]`), a call's route stop
  missing from `Zaslinky` (`routeRestrictions.[…]`), a call's post ID missing from `Oznacniky`
  (`parents.[…]`), a trip without calls (headsign fallback), a version without its route
  (`modesByRoute.[…]`) throw `KeyNotFoundException`; a trip with one emitted call is published
  (`assert` is compiled out).
- **Where:** `getGtfsStopTimeRowsInternal`, `scanCallDerivedFacts`, `JdfBundle.fs` 478,
  `getGtfsTripsWithEndpoints`.
- **Why:** relies on `fix-jdf`/`merge-jdf` (jdf-read.md, jdf-merge.md).
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** replace — each case is a coded diagnostic and the entity is dropped or
  falls back (AGENTS "A malformed source entity is a diagnostic"); a trip with fewer than two
  emitted calls is dropped (model invariant) with `jdf_short_trip` (new).

### Stage 16 — filtered feed (Python)

#### JDF-R474 — Filter rule file
- **What:** `rules-v1.json`: `operators` (exact agency names), `ids_codes` (integers),
  `line_prefixes` (must be all digits, else `PipelineError`); `ids_names`, `source`,
  `schema_version` are not read.
- **Where:** `filtered_jdf.load_rules`.
- **Why:** module docstring: mirrors `0xaa55h/gtfs-processor` ("drops CIS lines that other
  feeds already publish").
- **Pinned by:** Oběhy unit tests (not read here).
- **Constants:** data file `jrunify-ext-geodata/filtered-jdf/rules-v1.json`: operators `FlixBus
  CZ s.r.o.`, `FlixBus DACH GmbH`, `FlixBus Polska sp. z o.o.`, `Plzeňské městské dopravní
  podniky, a.s.`, `Dopravní podnik města Olomouce, a.s.`; IDS `30001`, `30621`, `30722`;
  prefixes `199`, `205`, `289`, `445`, `737`, `755`, `805`, `825`, `826`, `835`.
- **Diagnostic:** `PipelineError`.
- **Proposed verdict:** port (reader of a reviewed data file).

#### JDF-R475 — Line snapshot
- **What:** section 6.2.
- **Where:** `filtered_jdf.jdf_line_snapshot`, `normalize_line`, `_jdf_rows`, `_jdf_date`.
- **Why:** docstring "reproduces the portal.radekpapez.cz queries gtfs-processor uses".
- **Pinned by:** Oběhy tests.
- **Constants:** field counts `Linky` 17, `LinExt` 7, `Dopravci` 13, `Altdop` 15.
- **Diagnostic:** `PipelineError` (wrong field count, invalid date, empty query group); a
  non-numeric licence raises `ValueError` (B30).
- **Proposed verdict:** replace — computed from the model's version facts (licence, operator
  IČO and name, alternate operators, preferential LinExt systems, validity) instead of re-reading
  the merged JDF.

#### JDF-R476 — GTFS filter
- **What:** section 6.3.
- **Where:** `filtered_jdf.filter_gtfs`.
- **Why:** docstring "drops calls at stops without coordinates".
- **Pinned by:** Oběhy tests.
- **Constants:** minimum calls per trip `2`.
- **Diagnostic:** counts in `filter-report.json`; `PipelineError` when no trip is kept.
- **Proposed verdict:** replace — a projection of the pre-overlay timetable in `jr-publish`
  (section 6).

#### JDF-R477 — Filtered feed outputs
- **What:** section 6.4.
- **Where:** `build_filtered_jdf`, `national_jdf.verify_gtfs_stops`.
- **Why:** —
- **Pinned by:** Oběhy tests.
- **Constants:** report schema 2, snapshot schema 2.
- **Diagnostic:** `PipelineError`.
- **Proposed verdict:** port the outputs from `jr-publish` (deterministic ZIP, report, snapshot).

---

## 4. Identity

All built with string interpolation in F#; `esc` as defined above. "Decided" marks the
architect's decision (registry stop/post numbers, `jdf:trip:<line>:<yymmdd>:<trip>`,
`jdf:route:<line>` and all source keys stay; hash suffixes and `:pN` chunks may go).

| Entity | Format | Inputs | Exists because of / notes |
|---|---|---|---|
| Agency | `jdf:agency:{esc(IČO)}:{agencyDistinction}` | `Dopravci.id`, `idDistinction` | distinction is merge-assigned (jdf-merge.md); IČO parsed back (below) |
| Stop place | `jdf:stop:{n}` | `Zastavky.id` (int64) | decided: registry numbers; provisional ≥ 1,000,000,000 |
| Unspecified point | `jdf:stop:{n}:unspecified` | | |
| Authored post | `jdf:stop:{n}:post:id:{k}` | `Oznacniky.stopPostId` | decided: post numbers stay |
| Textual post | `jdf:stop:{n}:post:{esc(num)}` | trimmed `Zasspoje.stopPostNum` | a number `id` would collide in shape with `post:id:…` only if escaped text were `id:…`; `:` is escaped, so no collision |
| Inferred post | `jdf:stop:{n}:est:{k}` | estimator | out of scope |
| Output route | `jdf:route:{esc(lic)}`, `…:detour`, `…[:detour]:{8 hex}` | licence; hash = first 4 bytes SHA-256 of the concatenated semantic key | decided: `jdf:route:<line>` stays; hash suffix may go (Q5) |
| Version (diagnostics, sidecar) | `jdf:route:{esc(lic)}:{dist}` | licence, distinction | merge artefact; overlaps the route namespace (B27) |
| Trip version key | `{esc(lic)}:{yyMMdd}[:det][:{8 hex}][:p{n}]` | original validity from, detour; hash of `"{from}|{to}|{created}"`; chunk index | `:pN` exists because the merger splits one published version into date chunks (jdf-merge.md); the hash because several published versions share a start day |
| Trip | `jdf:trip:{key}:{Spoje.id}` | key, trip number (decimal) | decided: `jdf:trip:<line>:<yymmdd>:<trip>` stays |
| Service | `jdf:service:{Mon..Sun 0/1 \| exc}:{n}` | distinct calendars, ordinal trip order | replaced by `identity::service` |
| Zone (unpublished) | `jdf:zone:{esc(lic)}:{dist}:{esc(code)}` | | merge artefact; drop |
| Route note | `jdf:notice:route:{esc(lic)}:{dist}:{Udaje.id}` | | distinction is merge-assigned: note IDs change between runs |
| Trip note | `jdf:notice:trip:{esc(lic)}:{dist}:{trip}:{Caskody.id}` | | same |
| Reservation note | `jdf:notice:reservation:{esc(lic)}:{dist}:{trip}:{ordinal}` | ordinal per trip, source order | same |
| Connection claim | `jdf:transfer:{esc(lic)}:{dist}:{trip}:{ordinal}` | ordinal per trip, source order | same |
| Restriction source ID (diagnostics) | `jdf:restriction:{esc(lic)}:{dist}:{trip}:{rs}` | | |
| Route-stop source ID (diagnostics) | `jdf:route-stop:{esc(lic)}:{dist}:{rs}` | | |
| Restriction | `feedId("jdf","restriction",[scope; route; trip; route_stop; group])` (serving §6.1) | | `source_object_id = compositeKey [scope; route; trip; route_stop; group]` (e.g. `trip_call//jdf:trip:721308:260830:1/6/A`) |
| Route-stop key (transient) | `compositeKey [route; dist; rs]` → `jdf:route:100100/1/6` | | merge artefact (distinction); replaced by the slot ID |
| Trip feature | `feedId("jdf","trip-feature",[trip; code; kind])` | | |
| Location feature | `feedId("jdf","location-feature",[location; code; kind])` | | |
| Route integration | `feedId("jdf","route-integration",[route; system])` | | serving.md calls this `route-feature` (section 9) |
| Note link | `feedId("jdf","note-assignment",[note; scope; route; trip])` | | |
| Feature source objects | `{trip}:attribute:{esc(code)}`, `{stop}:attribute:{esc(code)}`, `{route}:integration:{system}` | | |
| Singleton restriction diagnostic | `{trip}:restriction-group:{esc(group)}` | | |
| `cz_stops.source_ids` | see JDF-R467 | | duplicate builders |
| Feed version | `jdf-{version}[:{batchId}][:{yyyyMMdd}]` | `VerzeJDF` | |
| `cz_routes.source_provenance`, `cz_stop_zones.source_provenance` | `jdf:{version}` | | |
| `cis:line_trip` key | `{PadLeft(lic,6,'0')}:{Int64(trip)}` | | decided: stays |
| `cis:line` key | `{PadLeft(lic,6,'0')}` | | decided: stays |
| Shapes | none in this compiler | | |

Escaping notes: the licence is escaped once inside the trip key, and the key's own `:` are
separators; a Rust builder must escape each component separately (`PublicId::build` does, giving
the same text for numeric licences). Stop, post and trip numbers are decimal and never escaped.
`.NET Uri.EscapeDataString` and `jr_model::identity::escape` agree byte for byte.

### Places that parse an ID back

1. `RouteRules.jdfAgencyIco` parses `^jdf:agency:([^:]+):[0-9]+$` and unescapes it to get the
   IČO for presentation rules (JDF-R431).
2. `JdfBundleTables.authoredTargetId` parses the internal post keys `id:{n}` / `num:{text}`
   (`Int64.Parse` of the rest) to rebuild a stop ID (diagnostics artifact only).
3. `filtered_jdf._route_line` splits the route ID on `:` and takes part 3 when parts are `jdf`,
   `route`, digits → the licence for line filtering.
4. `PackageReader.prepareCompilerView` parses the `cis:line_trip` identifier at its first `:`
   (serving §6.5 #10) to rebuild `cz_trips` for the overlay.
5. `RouteStopWriter` takes the restriction ID's text before the first `:` as the feed prefix
   (serving §6.5 #3).
6. `Serving.Validation` and `repo/src/obehy/reference/station_pairs.py` test the `jdf:` prefix
   (feed classification, not information recovery).

All but 6 conflict with AGENTS rule 6; 1, 3 and 4 need the licence and IČO as fields.

---

## 5. What the model must carry

What a JDF timetable needs that `jr_model::Timetable` does not have today (field names are
proposals):

1. **Route versions** (new table `route_versions`, or fields on `Trip` + a version handle):
   licence (`Box<str>`, 6-digit text), merge distinction (for diagnostics only), effective
   validity (`Date..=Date`), published validity (`original_valid_from/to`), source creation
   date, `detour`, JDF route type (enum incl. `RegionalInternational`), source and effective
   transport mode, `one_way`, the operator (agency handle), public line number as published
   (before presentation), name. Needed for trip IDs, `cis:line` keys, presentation, the
   filtered feed and diagnostics. `Route` gains `version`s (or the version gains `route`).
2. **Route**: `licence` (CIS line, for keys, presentation and the filtered feed without parsing
   IDs) and `source_short_name` (public line before presentation, for overlay matching).
3. **Agency**: `source_code` (IČO, leading zeros kept) and its distinction or an equivalent
   handle for `Linky`/`Altdop` references; contact fields beyond one phone if Q4 chooses so.
4. **Trip**: `version` handle, `trip_number: u32` (source trip number), the trip's attribute
   codes needed after compile (or only the derived features), alternate operators from `Altdop`
   (agency handle + `DateSet` + attributes + time type) — new, none exists.
5. **Call**: `source_route_stop: u32` (the version's route stop number; joins zones, claims,
   restrictions and route-stop facts without IDs), `time_window: Option<(ServiceTime,
   ServiceTime)>` (`minArrivalTime`/`maxDepartureTime`), and enough to publish call-scope
   features (assignments with `Scope::Call` suffice if the compiler emits them).
6. **Boarding point**: a way to tell an authored `Oznacniky` post from a textual `Zasspoje`
   number (`BoardingPointKind::Post` with an `authored: bool`, or a `PostSource` enum), plus
   the authored post ID (`source_code`) and the post name (`source_name`).
7. **Place**: `provisional: bool` (registry status), the raw coordinate provenance text or a
   `CoordSource` from it at compile time (today mapped by substring in serving), `wheelchair_boarding`
   (exists).
8. **Connection claims** (new table): `id`, origin trip, origin call sequence, direction
   (`WaitsFor`/`ConnectsTo`), target licence (text), target stop / post / end stop / end post
   (source codes, CIS register numbers), wait minutes, note text, derivation
   (`None`/`Structured`/`SpecNote`), resolution (`Unresolved`/`Pattern`/`Resolved` with optional
   target route/trip/location), optional `ServiceId`, `source_object`.
9. **Travel restrictions** (new table): group (`§`/`A`/`B`/`C`), scope `RouteStop { version,
   source_route_stop }` or `Call { trip, sequence }`, `source_object`; the route-stop pass
   derives slot rows (serving §7.6).
10. **Calendar designations from `Caskody`**: typed rows retained with designation, type (1–8),
    interval and trip — either `Note` rows of a new kind (`calendar_note`) or assignment
    `CalendarDesignation` with an interval; today `Assignment` has no dates.
11. **Note scope for line-level reservation notes** (`Mistenky` trip `0`): `Scope::Route` exists;
    only the compile rule is missing.
12. **LinExt facts**: integrated system per route with `preferential: bool` and the system's line
    designation (`routeName`); `Assignment` has no slot for either (a `source_code` holds the
    system).
13. **Zones**: nothing new (`Call.zones`, `Zone.system = None`), provided the compiler sets them
    per call from the route stop.
14. **Source keys**: nothing new (`SourceKey` with `Entity::Trip`/`Route`, namespaces
    `cis:line_trip`, `cis:line`).
15. **Feed version**: `Timetable.source_version` exists.
16. **Per-entity diagnostics** with the codes of JDF-R470 (in `jr_model::diag`).
17. **Trip groups** (`SpojSkup`): not used by F#; carry only if Q-gap is accepted (none in golden).

---

## 6. The filtered feed

### 6.1 Purpose and inputs

A second GTFS for consumers that merge the national road feed with regional feeds: lines the
regional feeds publish are removed, and calls at unpositioned stops are dropped. F# does not
produce it; Oběhy's `build_filtered_jdf` runs after `jdf-to-bundle` on the bundle's `gtfs.zip`
(post estimation and the bundle-time presentation rules applied, no overlay), the merged JDF
ZIP the bundle came from, `rules-v1.json` and the build's reference date. The rewrite produces
it from the pre-overlay timetable (`ARCHITECTURE.md` pipeline).

### 6.2 Line snapshot (which lines other feeds publish)

From the merged JDF (every relation read as CP1250 CSV, trailing `;` stripped, empty lines
skipped, exact field counts enforced):

1. `names[(IČO, distinction)] = Dopravci.name`.
2. `operators[(licence, version distinction)]` = the (IČO, distinction) pairs of the version's
   `Altdop` rows (if the file exists).
3. `preferred[(licence, version distinction)]` = `transportSystemId`s of `LinExt` rows with
   `preferential == "1"`.
4. For each `Linky` row whose `timetableValidTo` (field 15, `ddMMyyyy`) ≥ reference date: the
   line = licence zero-filled to six digits (only digits, ≤ 6 characters; else `ValueError`); it
   matches by operator if the name of the version's own operator or of any `Altdop` operator is
   one of `operators` (exact string); it matches each listed IDS code found in
   `preferred[version]`.
5. Groups: `operators` (when the list is non-empty) and one per IDS code; any group with no line
   fails the build (`Merged JDF has no lines for {kind}={value} from {reference}`).
6. `line-snapshot.json`: `schema_version 2`, `source "jdf-linext"`, `merged_jdf_sha256`,
   `created_at`, `reference_date`, `queries [{kind, value, lines (sorted)}]`.
7. Removed lines = union of all groups' lines.

Docstring rationale: "Every line version still valid on or after `reference` counts, so a line
whose current version is a short detour still matches through its regular version. An operator
matches as the line's operator or as an alternative operator (`Altdop.txt`). An integrated system
matches only through the preferred `LinExt` row, as on the line portal, so a line that merely also
accepts another system's tariff is kept."

### 6.3 Filter

On the extracted bundle GTFS:

1. Invalid stops: `stop_lat`/`stop_lon` unparsable or both 0, or the parent station's are.
2. Removed routes: route IDs `jdf:route:{digits}…` whose licence is a removed line or starts
   with a listed prefix (`199`, `205`, `289`, `445`, `737`, `755`, `805`, `825`, `826`, `835`).
   Other route IDs (overlay IDs would be, but the input is pre-overlay) are never removed.
3. Trips of removed routes dropped (`trips_removed_by_line`).
4. A remaining trip is kept when it has at least 2 calls at valid stops
   (`trips_removed_too_few_calls`).
5. `stop_times`: rows of kept trips at valid stops (`calls_removed_missing_coordinates`); the
   other rows' sequences keep their values (gaps).
6. `trips`, `routes` (routes with a kept trip), `stops` (used boarding points ∪ their parents),
   `agency` (agencies of kept routes), `calendar`/`calendar_dates` (services of kept trips),
   `shapes` (shapes of kept trips), `transfers` (both stops emitted and any trip/route columns
   kept) filtered; every other file (`feed_info.txt`) copied byte for byte.
7. CSV written with Python `csv` defaults (minimal quoting, LF) under the source header, not
   JrUtil's all-quoted form.

### 6.4 Outputs and checks

`verify_gtfs_stops`: every `stop_times.stop_id` exists, every non-station stop is referenced,
every parent exists, no station without children, coordinates in range (0/0 only warned).
No kept trip → `PipelineError("Filtered JDF GTFS contains no trips")`. `gtfs.zip` via
`deterministic_zip`. `filter-report.json`: `schema_version 2`, `created_at`,
`source_gtfs_sha256`, `rules {path, sha256}`, `line_snapshot_sha256`, `matched_lines`, `gtfs
{bytes, sha256}`, `routes_removed`, `lines_removed`, `routes_kept`, `trips_kept`, `stops_kept`,
`stops_without_coordinates`, the counters above, `copied:{file}` = 1.

### 6.5 As a projection of the timetable

Proposed `jr-publish` projection `filtered_gtfs(timetable_pre_overlay, rules, reference)`:

- Line selection from model facts: route versions with `valid_to ≥ reference`; operator names
  of the version's agency and of its trips' alternate operators; preferential integrated
  systems; licence prefixes. Needs section 5 items 1–4 and 12.
- Removal by route licence (field), not by ID.
- Unpositioned = `Place.position` is `None` (no 0/0 test); a boarding point is unpositioned when
  its place is.
- Trips need ≥ 2 remaining calls; sequences unchanged.
- The rest is the ordinary GTFS projection restricted to kept entities (serving §9), with one
  CSV writer for both feeds.
- Diagnostics: the report counters; an empty selection group and an empty result stay errors.
- Pipeline position: before the overlay; whether before or after the presentation pass is Q9.

---

## 7. Known or probable bugs

- **B1 — Route-stop attributes are ignored for pickup/drop-off.** `combinedFlags` uses the stop's
  and the call's attributes only (`JdfToGtfs.fs` 532–534); `Zaslinky` attributes reach only the
  restriction code. Evidence (golden): 10,814 route stops are coded `x`; 1,018,252 emitted calls
  lie on such a route stop without carrying `x` themselves and are published regular, against
  41,954 published as request stops. The JDF meaning of a route-stop `x` (request stop for every
  trip of the version) should be confirmed against the specification before fixing (Q3).
- **B2 — Trip-level `T` and `!` do not reach the calls.** A trip coded `T` (on request only) or
  `!` (conditional) has regular calls; only the trip features say so. Golden: 23 and 8 trips.
- **B3 — `Mistenky` rows with trip number `0` are lost.** Golden: all 7 rows; with
  `regional-adjacent` they vanish in the international filter without a diagnostic, with
  `keep-all` they become `filtered_enrichment`.
- **B4 — Typed `Caskody` designations are dropped.** The note filter skips typed rows without
  text, with no diagnostic. Golden: 78,760 rows. Contradicts JDF_SEMANTICS and README (section
  8).
- **B5 — Agency phone uses the office number.** Golden: 0 of 64 agencies publish a phone although
  `infoPhoneNum` is filled (e.g. `567 301 321`, `296 191 817 (296 192 150)`).
- **B6 — Agency URL handling.** The `https?://` test is unanchored and the prefix is always
  `http://`; a missing URL becomes `jrutil://invalid` in serving (19 of 64 golden agencies;
  SERVING-R046).
- **B7 — Trip keys of derived batches.** JDF-R404: `applyTransportModeRules` and the evidence
  drop build new batch objects, so `inheritTripVersionKeys` is defeated. When the international
  filter drops a version that shares its published start day (and detour flag) with a kept
  version of the same licence, the kept version's key changes between the calendar stage and the
  conversion → `KeyNotFoundException` or lost services. Not exercised by golden (0 dropped
  versions).
- **B8 — Ambiguous route semantic key.** Fields are concatenated without a separator
  (`String.Join("", …)`), so e.g. public line `1` + name `0 X` equals public line `10` + name
  ` X`. Low probability.
- **B9 — `origin_sequence` fallback `0` is a real sequence.** JDF sequences start at 0, so the
  sentinel cannot be told from a claim at the first call. Golden: 10,214 claims at sequence 0,
  all genuine; the fallback is unreachable on the native path.
- **B10 — Claim targets as integers.** `transferRouteId` is `int64`: a licence such as `001401`
  becomes `1401`; the merged files' `0` placeholders are published as `"0"` (11 golden claims).
- **B11 — Easter arithmetic.** `Holidays.czechHolidays` yields day 0 of April for Good Friday when
  Easter is 2 April (2051, 2056) and for Easter Monday when Easter is 30 March (2059, 2070, 2081,
  2092): `LocalDate` throws, the conversion fails.
- **B12 — Whole-validity odd/even week rows apply to one day.** Types 5/6 without dates get
  `dateFrom = validFrom`, `dateTo = dateFrom`. Golden: no such rows.
- **B13 — Arrival `|`/`<` with a departure time crashes** (`failwith "Invalid data"`). Golden: 0.
- **B14 — Dangling references crash instead of being diagnosed** (JDF-R473; parseAttributes
  `Array.find`).
- **B15 — One-call trips are published** (the `assert` is a debug-only check). Golden: 0.
- **B16 — `standard_zone_omitted` is noise.** 4,110 golden warnings about a field that is not
  published; zone IDs are per version, so any stop on two versions "has multiple zones".
- **B17 — Headsign from an unserved last row** (JDF-R435). Latent: golden has no trip whose first
  or last row is unserved, and all 92,705 headsigns equal the last served stop.
- **B18 — Any backwards time step is a midnight** (JDF-R442). Latent: golden has no inter-call
  gap over 6 h.
- **B19 — Unstable order for repeated route stops** (`Array.sortBy`). Latent: golden has none.
- **B20 — `conflicting_route_stop_zone_mapping` is severity `error` but nothing stops.**
- **B21 — `Altdop` is ignored.** Trips run by an alternate operator on some dates are published
  under the route's agency (golden: 184 rows), while the filtered feed uses `Altdop` to decide
  which lines to drop.
- **B22 — Trip-call restrictions never point at their call** (serving open question 9; golden
  457/457).
- **B23 — Location features and pickup flags only for the stop's own attributes.** A request
  stop coded on `Zastavky` makes every call at it a request stop for every line (golden: no stop
  coded `x`), while the route-stop coding (B1) is ignored — the precedence is inverted relative
  to specificity.
- **B24 — Notes and claims keyed by merge distinction.** `jdf:notice:…` and `jdf:transfer:…`
  embed the merge-assigned distinction, so their IDs move whenever the merger renumbers versions
  (unlike trip IDs, which were fixed for this reason).
- **B25 — Unknown `Navaznosti` direction letters are published verbatim** and then rejected by
  package validation (SERVING-R025).
- **B26 — Calendar IDs depend on the whole feed** (`jdf:service:{bitmap}:{n}`, JDF-R414).
- **B27 — `jdf:route:{lic}:{dist}` shares the output route namespace**; an 8-digit decimal
  distinction is indistinguishable from an all-digit hash suffix.
- **B28 — Digit attributes outside 1–7** count as calendar attributes but never match a weekday,
  so such a trip runs only on `Caskody` days.
- **B29 — Presentation rules parse the agency ID** (JDF-R431, ID parse 1).
- **B30 — The filtered feed crashes on a non-numeric licence** in `Linky` (`normalize_line`
  raises `ValueError`, not `PipelineError`); and it re-reads the merged JDF while the bundle may
  have dropped versions (international filter), so a line can be "published elsewhere" on the
  strength of a version the bundle does not contain.
- **B31 — Routes without trips are published.** Output routes come from every version of the
  filtered batch, whether or not any of its trips survives JDF-R413; their route notes and
  `LinExt` assignments are published too (`retainedRouteIds` is the route list, not the routes
  with trips). Golden: 17 of 901 routes have no trip (e.g. `jdf:route:721331`, whose 6 trips are
  all `filtered_trip`); they get no `cis:line` key because key validity needs trips.

---

## 8. Documentation vs. code

| Document | Says | Code |
|---|---|---|
| README "Czech schedule facts" | relations include `location_zone` and `transfer` "Standard transfers plus waiting limits" | no `location_zone` relation in serving 5.2; JDF emits no transfers |
| README | "Typed `location` and `source_entity_map` rows preserve … source identity" | no `source_entity_map`; `source_key` holds only `cis:*` keys; `cz_stops.source_ids` is unread |
| README | "Deduplicated GTFS service patterns use the derived `gtfs:service:<weekday-bitmap>:<ordinal>` namespace" | JDF uses `jdf:service:…` (`materializeUniqueCalendars`) |
| README | "The internal JDF extraction maps … all typed `Caskody` including text-free calendar designations to trip facts" | text-free typed rows are skipped (B4) |
| README enrichment schemas | `source_route_stop_zone_metadata (gtfs_route_id, source_route_stop_id, zone_id, zone_order)`; `source_notice_metadata`; restriction sidecar without version | sidecar also has `source_route_version`, `zone_code`; notes are written natively (no `source_notice_metadata`); restriction sidecar has `source_route_version` |
| README | "`source_call_metadata` maps a GTFS call to its JDF route-stop" | JDF writes no `source_call_metadata` (a test asserts its absence); a side channel carries transfer sequences |
| README | "Production Parquet relations use serving schema 5.0 … relations are not sorted" | 5.2 (serving verdicts) |
| README route grouping | "Versions that share agency, public line number, name, route type and colours form one route" | key also includes licence and the detour flag; versions are grouped per kind first |
| JDF_SEMANTICS table | "`x`, `T`, `!`, `(`, `)` at a stop/call | Usefully approximated through GTFS pickup/drop-off" | only stop and call attributes; route-stop and trip-level codes are not (B1, B2) |
| JDF_SEMANTICS "location features" | "Call-specific `x`, `(`, and `)` are call-scoped assignments" | no call-scope assignment exists (JDF-R459) |
| JDF_SEMANTICS "service features" | "`T` and `!` must retain whether they apply to the whole trip or only marked calls. `O`, `[`, `T`, and `!` must link to their applicable `Caskody` text" | trip scope only; `note_id` always null |
| JDF_SEMANTICS service features kind list | 17 kinds | code also emits location-type kinds at trip scope (`toilet`, `urban_transport_interchange`, …) and `calendar_designation` |
| JDF_SEMANTICS `service_note` | "Retain route information, all `Caskody`, and `Mistenky` … Calendar compilation does not justify discarding a text-free source designation" | typed text-free `Caskody` dropped; `Mistenky` trip `0` dropped (B3, B4) |
| JDF_SEMANTICS `connection_claim` | "Retain every `Navaznosti` row" | rows of omitted trips and of unemitted calls are dropped (diagnosed) |
| JDF_SEMANTICS status 2026-10-06 | "Every JDF code has a typed `assignment.kind`" | true for trip and stop codes; codes used only on calls or route stops have no assignment |
| PRODUCTION_CONTRACT "Route stops and zones" | "a trip-scoped one points at its call" | never (B22) |
| PRODUCTION_CONTRACT "Typed semantics" | "`connection_claim` keeps every JDF `Navaznosti` row" | as JDF_SEMANTICS row above |
| PRODUCTION_CONTRACT "Calls" | "Its passenger calls are exactly GTFS `stop_times`, with `stop_sequence = sequence`" | true; sequences are 0-based with gaps for JDF (not stated) |
| serving.md §5.2 | "[jdf] route feature (LinExt integrated systems) \| `feedId(feed, "route-feature", …)`" | LinExt rows are native assignments with `feedId("jdf","route-integration",[route; system])`; the `route-feature` path reads a sidecar JDF no longer writes |
| JdfGtfsRules comment "Information that is lost" | lists textual route notes, `Caskody` designations, accessibility details, transfer attributes, stop exclusivity, trip groups | route notes, accessibility details, exclusivity and transfers are now carried; designations and trip groups are still lost |

---

## 9. Duplicated logic

### 9.1 Within this area

| Concept | Copies |
|---|---|
| "Call is served" | `getGtfsStopTimeRowsInternal` match, `JdfBundleTables.callIsEmitted`, `JdfPostPlan.callIsUsable`, `JdfInternationalFilter` (line 30) |
| Authored post key `id:`/`num:` | `JdfPostPlan.authoredPostKey`, inline in `scanCallDerivedFacts`, `convertedStopId`, `derivedStopPosts`, `stopPostNumbersById`; parsed back by `authoredTargetId` |
| Conflicting post numbers | `stopPostNumbersById` (log) and `scanCallDerivedFacts` (diagnostic) |
| Stop ID builders | `JdfGtfsRules.jdfStopId`/`jdfUnspecifiedStopId`/`jdfStopPostId`/`jdfStopPostNumId`; `getCzStopsWithPlan.sourceStopId`/`sourcePostId`/`sourcePostNum`; inline `jdf:stop:{id}:post:id:{post}` (conflicting posts), `{stop}:unspecified` (centroid assignments), `JdfPostPolicyEvaluation.stopText` |
| Route ID builders | `jdfRouteId` and its alias `jdfSourceRouteId`; inline `jdf:route:%s` base in `buildRouteGrouping` |
| Precise-first stop location | `getGtfsStopsWithPlan`, `getTableProducers.stopLocations` |
| Zone normalisation + zone ID | `zonesByStop`, `getCzStopZones`, `routeStopZones` |
| Transport mode letters | `parseTransportMode`, `transportModeCode`, Python `routes.py` `MODES` |
| Trip parity | `Jdf.tripIsReverse`, inline `% 2L = 0L` (twice) |
| Call ordering | `getGtfsStopTimeRowsInternal`, fallback in `getGtfsTripsWithEndpoints` (different tie handling) |
| Restriction mask | `scanCallDerivedFacts` and `getTableProducers.restrictions` |
| Owner ordinals for IDs | `withOwnerOrdinals` in notices, transfers and three diagnostic passes (must agree) |
| Public line number consumers | grouping, routes, `cz_routes`, transport-mode guards, colours (Metro) |
| Licence pattern grammar | `RouteRules.fs` and `jrunify-ext-geodata/routes.py` |
| Line normalisation | `PackageKeyRelations.cisLine` (`PadLeft`) and `filtered_jdf.normalize_line` (`zfill`) |
| LinExt preference | `computePublicLineNumbers` and `filtered_jdf.jdf_line_snapshot` |
| Filter thresholds | `JdfInternationalFilter` and the manifest literals |

### 9.2 Against the rail inventory (`czptt.md`)

| Concept | JDF | CZPTT |
|---|---|---|
| Unspecified boarding point | JDF-R418 | CZPTT-R052 |
| ` [?]` marker | JDF-R416 (`Gtfs.markApproximateStopName`) | CZPTT-R103 |
| Route colours | JDF-R428 | CZPTT-R061 |
| Route type / mode | JDF-R427 | CZPTT-R035, R045 |
| Agency defaults (timezone, language) | JDF-R426 | CZPTT-R080 |
| Feed info | JDF-R469 (`Gtfs.obehyFeedInfo`) | CZPTT-R081 |
| Filler values | JDF-R416, R426 | CZPTT-R114 |
| Service dedup | JDF-R414 (`materializeUniqueCalendars`) | CZPTT-R113 (`Gtfs.deduplicateCalendarWithPrefix`) — two implementations of one idea |
| Pickup/drop-off codes | JDF-R444 | CZPTT-R067 |
| Wheelchair and bicycle trip flags | JDF-R437, R438 | CZPTT-R070 |
| Request-stop / on-request features | JDF-R457 (`request_stop`, `on_request`) | CZPTT-R078 (`on_request` for `0030`) — same fact, different kinds |
| `stops.zone_id` unanimity | JDF-R450 | CZPTT-R087 |
| National keys | JDF-R466 | CZPTT-R118 |
| Czech holiday calendar | JDF-R411 | none (CZPTT uses bitmaps) — one holiday source for any future need |

### 9.3 Against the serving inventory (`serving.md`)

- Route-stop key (JDF-R447) vs slot ID (SERVING-R003); serving §14.1 already lists it.
- `NoteWriter.assignment` used natively and again for sidecar notes in
  `PackageSemanticRelations` (dead for JDF: `source_notice_metadata`,
  `source_trip_feature_metadata`, `source_route_feature_metadata` are never written).
- `TripWriter`/`TripCallWriter.fromGtfs` duplicate the generic GTFS projection (tests assert
  equality).
- Coordinate precision/source: the converter writes words (`stop`, `estimated`, raw provenance
  text) that serving re-maps by substring (SERVING-R004/R005).
- Detour → `timetable_kind` via a `"True"` string round trip (SERVING-R007).

---

## 10. Open questions for the architect

1. **Calendar ownership.** JDF-R409–R414 run in `jdf-to-bundle`; jdf-merge.md covers calendars as
   the merger bounds them. Which file owns the date-bitmap rules?
2. **Odd/even week rows without dates (types 5/6).** Whole version validity (B12) or keep F#?
   Needs the specification text.
3. **Attribute scope.** Should route-stop (`Zaslinky`) and trip-level (`T`, `!`) codes set
   pickup/drop-off (B1, B2), and should call and route-stop codes become call-scope
   assignments (JDF-R459)? Fixing B1 changes about a million golden calls to request stops.
4. **Agency phone.** `officePhoneNum` (F#, always blank in golden), `infoPhoneNum`, or
   both as separate fields?
5. **Hash suffixes and `:pN`.** May go (architect). What replaces them for (a) a second regular
   route group of one licence with different semantics, (b) two published versions starting on
   one day, (c) merger date chunks of one version? Options: the published validity in the key, a
   registry, or making chunks one trip with a combined `DateSet`.
6. **One-way timetables.** Trust parity for `oneWay` versions, or derive direction from times
   (JDF-R432, R434)?
7. **Trip short name** `"{licence} {trip}"`: keep for parity or drop?
8. **Call sequence.** Keep the 0-based ordered index (JDF-R443), or publish the route stop number
   (which would make `call_key` rows unnecessary as well and survive inserted stops)?
9. **Filtered feed and presentation.** F#'s filtered feed sees bundle-time presentation
   overrides; the Rust pipeline places presentation after the overlay. Generate the filtered
   feed after a presentation step, or accept the difference (only short names and colours)?
10. **JDF time windows** (`minArrivalTime`/`maxDepartureTime`, 1,731 golden calls): carry
    only, or publish (timepoint false, a call note)?
11. **Typed `Caskody` designations** (78,760 golden rows): retain as notes of a new kind, as
    interval-bearing calendar-designation assignments, or not at all (contract change either way)?
12. **`Mistenky` trip `0`:** route-scope note for all trips of the version?
13. **Claims on unserved calls** (`unjoinable_call_enrichment`): drop (F#), attach to the nearest
    served call, or keep with `origin_sequence` null?
14. **Note and claim IDs embed the merge distinction** (B24). Re-key them on the trip ID / the
    published version (like trip IDs), or accept churn?
15. **`Altdop`.** Publish alternate operators (model item 4) or keep ignoring them? The filtered
    feed depends on them.
16. **International filter thresholds** belong to jdf-merge.md; confirm the manifest reads them
    from that policy.
17. **Diagnostics granularity.** One row per dropped enrichment (F#: 547 `filtered_enrichment`
    golden rows) or counts per dropped trip?

---

## 11. Rules added by the rewrite

#### JDF-R478 — A version without a known transport mode
- **What:** a version whose `Linky` transport mode is empty or not one of the JDF letters (JDF
  1.10/1.11 only; 1.8/1.9 batches take the default of JDF-R028) is published with the policy's
  fallback mode (`[modes] fallback`, bus), before the transport-mode rules run.
- **Why:** F# could not read such a batch at all (the union parse failed and the batch was lost);
  the reader now keeps the empty field, and a route needs a mode.
- **Constants:** `jdf_convert.toml` `[modes] fallback = "A"`.
- **Diagnostic:** `jdf_transport_mode_missing` (warning), per version.
