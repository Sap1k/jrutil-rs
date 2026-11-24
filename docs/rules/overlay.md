# Regional GTFS overlay rule inventory (F# reference): `regional-gtfs-overlay`

Status: **proposal for review, nothing implemented**. This file describes what the F# JrUtil
(`E:/Git/obehy/jrutil`) does when `regional-gtfs-overlay` overlays the PID and IDS JMK GTFS feeds
on the national road package, and proposes a verdict for every rule. The verdicts are for the
architect.

This file owns `OVL-R001` to `OVL-R199`. Neighbouring inventories: `docs/rules/serving.md`
(`SERVING-Rnnn`, the package writer the overlay hands its tables to), `docs/rules/jdf-convert.md`
(`JDF-Rnnn`, the base package the overlay reads).

Sources read in full:

- `jrutil/src/Gtfs/RegionalGtfsOverlay.fs` and every file of `jrutil/src/Gtfs/RegionalOverlay/`:
  `Types.fs`, `Scratch.fs`, `DateSet.fs`, `Model.fs`, `Runtime.fs`, `Values.fs`, `Names.fs`,
  `GtfsFiles.fs`, `GtfsRows.fs`, `TripScoring.fs`, `StopGroups.fs`, `OutputIds.fs`, `Support.fs`,
  `Shapes.fs`, `Policy.fs`, `MultiSourcePreparation.fs`, `BasePreparation.fs`,
  `InputPreparation.fs`, `StopMatching.fs`, `RouteMatching.fs`, `SourceAnalysis.fs`,
  `TripMatching.fs`, `Projection.fs`, `Reports.fs`, `BundleOutput.fs`, `BundleCalls.fs`,
  `BundleGtfs.fs`, `BundleTransfers.fs`, `BundleExtensions.fs`, `BundleWriter.fs` (compile order
  from `jrutil.fsproj` lines 45–99).
- Callers and dependencies: `jrutil-multitool/Commands/PackageCommands.fs` (`overlay`,
  `parseOverlayBindings`), `Program.fs` usage lines; `Serving/PackageReader.fs`
  (`prepareCompilerView`), `Serving/PackageKeyRelations.fs`, `Serving/PackageBaseRelations.fs`,
  the overlay-relevant parts of `Serving/PackageWriter.fs` (`writePackage`,
  `writeDiagnosticsSummary`, `writeManifest`, `zoneInputs`, `writeDiagnosticArtifact`),
  `Serving/PackageGtfsRelations.fs` (locations, routes, calendars, `writeOrderedTripCalls`),
  `Serving/PackageRows.fs` (`sourceNamespace`, `bindingMethod`, `coordinateSource`),
  `Serving/Identity.fs` (`feedId`), `Serving/RouteStopWriter.fs` (call zones),
  `GeoData/StopRegistry.fs` (overlay places), `GeoData/RouteRules.fs` (`jdfAgencyIco`),
  `GeoData/StopMatcherSynonyms.txt`, `Core/Geo.fs`, `Core/Hashing.fs`, `DateUtils.gvdBounds`,
  `Gtfs/Gtfs.fs` (`markApproximateStopName`), `Jdf/JdfToGtfs.fs` lines 98–115.
- Tests: `jrutil.tests/RegionalGtfsOverlayTests.fs` (all 25 tests),
  `RegionalOverlayStorageTests.fs` (8 tests), the overlay cases of `ServingContractTests.fs`,
  `StopRegistryTests.fs`.
- Policies: `work/jrutil-rs-golden/jdf/inputs/config/{pid,ids-jmk,pid-ids-jmk}-production-v1.json`
  and `config-candidate/` (section 2.3 says how it differs).
- Reviewed data: `work/jrutil-rs-golden/jdf/inputs/geodata/overlay/pid-stop-overrides.csv`
  (identical to `jrunify-ext-geodata/overlay/pid-stop-overrides.csv`),
  `jrunify-ext-geodata/registry/overlay_places.csv` and `jrunify-ext-geodata/README.md`
  ("overlay_places.csv" paragraph).
- Source descriptors `work/jrutil-rs-golden/jdf/inputs/overlay/*-descriptor.json` and the member
  list plus header line of every file in `pid-gtfs.zip` and `ids-jmk-gtfs.zip`.
- Oběhy side (read-only): `repo/src/obehy/regional_overlay.py` (the task named
  `pipeline/regional_overlay.py`; the module lives at `src/obehy/regional_overlay.py`),
  `STATIC_PIPELINE.md` (sections "Commands and build identity", "Route shapes", "Overlay policy"),
  `BASE_PLAN.md` sections 6, 6.1, 8, 9.1, 11, 12, 14, 15.3, 17, `AGENTS.md`.
- Scripts `jrutil/scripts/audit_regional_overlay_references.py`, `audit_duplicate_journeys.py`.
- Reference output `work/overlay-golden/out/baseline/overlay/manifest.json`, the head of
  `diagnostics.json`, the file list of `overlay-diagnostics/`, and the timings in
  `work/overlay-golden/perf/*.json`.

## Architect's verdicts

**These override the "Proposed verdict" lines below.** Where a rule is not listed, its proposed
verdict stands. Rule IDs in the Rust code are the IDs of this file.

The overlay is ported in two steps, and this file rules on the first. **Step one is a parity
port of what the F# overlay decides**, in the rewrite's shape: one engine over the `Timetable`,
sources described by profiles, the result returned as edits. Its output is compared with the F#
overlay on the golden slice (`out/fs-ref/overlay-reg`), trips matched by schedule where the IDs
differ. Changes of what the overlay decides (a one-to-one trip assignment, zones scoped to their
tariff, authored thresholds per source) are made afterwards, one at a time, each measured on the
same slice. Only defects that lose service or publish something false are fixed in step one.

| Q | Decision |
|---|---|
| Q1 | Port the F# matching (R065–R076): nearest candidate per date, the availability pass, no time limit. B1 is recorded as the first change to make after the switch. |
| Q2 | Each source has its own profile and is governed by it. The profiles shipped with the rewrite state the values that are effective today (PID with the 25 m / 10 m coordinate tier and the union of capabilities and tiers), so step one decides as F# does and the files say what really applies. The merged template (R011, R022) is not built. |
| Q3 | Fix B23: a quarantined conflict on an owned line-date keeps the national trip on that date, as the diagnostic says. Ledgered. |
| Q4 | New edits: `SplitTrip` (a copy of a trip under a new ID and service, with everything stated for the trip), `ReplaceTripCalls` (the whole call list, with the map from old to new sequences so what was stated for a call follows it), `SetTripRoute`, `SetTripHeadsign`. Call zones travel with the calls. |
| Q5 | Change once. Services are `identity::service`. A slice is `<base trip>:overlay:<12 hex of its date set>`; an added trip `overlay:<source>:trip:<20 hex of source trip and date set>`, the date set being `DateSet::to_bytes`. Posts, routes, agencies and stop places keep their F# form: they are keyed on source identifiers and the registry. |
| Q6 | The registry's `group_key` is built exactly as today, in `identity::overlay`. Migrating the file is a separate change with the registry tooling. |
| Q7 | Port (R062 with its maximum and exclusions as profile data). |
| Q8 | The facts of the base the engine relies on (after-midnight fold, mode compatibility, detour preference, the estimated-stop correction, CIS lines, integrated systems) come from a base profile, so a rail base can supply its own. |
| Q9 | Mappings are not written: source keys and call keys carry them. The review reports (`stop_group_matches`, `unmatched_trip_reasons`, `headsigns`, candidate scores, coverage) are returned as typed tables and written by the CLI as a diagnostics artifact. The manifest lists each source with its payload, descriptor and profile hashes (fixes B3). Unpublished `cz_*` markers are dropped. |
| Q10 | Port (place-level zones for every call at the place). B6 is recorded as a change to make after the switch. |
| Q11 | A source whose data cannot be used (bad `api.txt`, agencies, references) is left out with an error diagnostic and the package is not eligible for publication; the build continues. A checksum or profile error stays fatal. |
| Q12 | Headsigns are a capability of the profile; the shipped profiles grant it where F# applied it (authority slices). |
| Q13 | Fix B4: a coordinate the overlay replaced is `CoordSource::Overlay`, exact. Ledgered. |
| Q14 | Source shapes are not ported (R110 dropped; off in production). |
| Q15 | Port (`trip_equivalent_tie_expanded`). |

Further:

- There is no combined input, no compiler view and no intermediate GTFS (R012, R021–R024,
  R093–R106 as file writers, R111, R119 mappings): the engine reads the base `Timetable` and each
  source's `jr_formats::gtfs::Feed`, and returns edits. What those stages decided is kept as
  rules of the stage that now decides it.
- Source quirks (R015–R020) are named hooks of a profile, selected by the profile and not by an
  adapter name in code: the revision in a trip ID, the stop group and post key columns, the
  companion CIS-line join, the operational crosswalk, the integrated-system scopes.
- The approximate-stop marker (R026) is `Position.precision`, never a name suffix.
- B8 cannot occur: what is stated for a call follows the call through `ReplaceTripCalls`.
- B12: the synonym table is the one data file every name matcher shares (`data/names/`).
- B18: a capability naming an unknown tier is a profile error.
- The validations of the base package (R002, twice) are the publisher's check of what the build
  itself wrote; the overlay does not validate its input timetable again.
- Determinism (R124) and memory (R125): no scratch directory, no forced collections.

---

## Summary

1. The command takes a finished national road package (the "base"), an overlay-all policy naming
   the sources (PID and IDS JMK), one GTFS snapshot plus descriptor per source, and reviewed data
   (stop overrides, the overlay-places registry, route presentation rules).
2. Every run is a combined run: the sources' GTFS tables are rewritten into one namespaced GTFS
   directory (`<source>:<id>`), source quirks are baked into extra columns (PID revision, ASW node
   and post keys, IDS JMK `api.txt`), and the source policies are merged into one generated policy.
3. The base package is turned back into GTFS plus `cz_*.txt` tables (the "compiler view").
4. Source stops are grouped into stop places and matched to base stop places: reviewed override,
   name plus distance, coordinate identity, then trip context; unmatched stops of trips the
   source may run on its own become source-native stop places (pinned by `overlay_places.csv`).
5. Source routes get candidate base routes from a companion CIS-line table (PID) or from public
   line number plus mode.
6. A source "owns" a CIS line on the dates it runs a complete trip set for it (trip-set
   authority); PID also owns every line of integrated system 30001 inside its feed window.
7. Each active source trip is compared, date by date, with base trips on candidate routes: exact
   call pattern with or without times, superset patterns, or a small edit distance; candidates are
   ranked by time deviations; equal-best candidates of different meaning go to an availability
   pass, the rest is quarantined. There is no one-to-one assignment and no time tolerance.
8. Projection merges the claims per base trip and date, keeps the newest source revision or
   quarantines conflicts, removes national trips on owned line-dates that no source trip claimed,
   splits base trips into date slices with new IDs, and adds source trips nobody matched.
9. The writer streams new `stop_times`, trips, routes, stops, agencies, calendars, transfers and
   Czech tables, plus mapping CSVs (`base_to_output_trips.csv`, `source_to_output_*.csv`) and report
   CSVs; the serving package writer turns these into relations, projects base semantics and keys
   through the mappings, and derives `source_key` / `call_key`.

Conventions:

- Paths without a directory are in `jrutil/src/Gtfs/RegionalOverlay/`. Line ranges are those of
  the file named.
- "Combined input" is the scratch directory `combined-source/` written by
  `MultiSourcePreparation.prepare`; "source ID" is `pid-gtfs` or `ids-jmk-gtfs`; a "namespaced" ID
  is `<source ID>:<original ID>`.
- `sha16(x)` = first 16 lower-case hex characters of SHA-256 over the UTF-8 bytes of `x`;
  `shaN(x)` likewise for N characters.
- `dateKey(d)` = full lower-case hex SHA-256 of the packed date set `d` (section 4.1).
- "Window" = the GVD window `DateUtils.gvdBounds year` = second Sunday of December of `year-1`
  to the day before the second Sunday of December of `year` (GVD 2026: 2025-12-14 – 2026-12-12,
  364 days).
- Diagnostic codes are the F# codes (all published with severity `warning`).
- Proposed policy keys live under `overlay.` in `policy/overlay.toml` and per source in a source
  profile (`data/overlay/<source>.toml`); new diagnostic codes are marked "(new)".

---

## 1. Data flow

Entry: `PackageCommands.overlay` → `RegionalGtfsOverlay.compile` (`RegionalGtfsOverlay.fs`
23–60) → `BundleWriter.write` → `Serving.PackageWriter.writePackage`.

| # | Stage | In → out | Rules | F# function |
|---|---|---|---|---|
| 0 | CLI | `--policy --gvd-year --converter-version (--source ID=PATH --source-descriptor ID=PATH)… [--stop-registry DIR] [--stop-registry-candidates FILE] [--route-presentation-rules FILE] [--overrides-root DIR] [--route-shapes… ] [--diagnostics-out DIR] [--diagnostic-traces] <base> <out>` → `CompilationOptions`; `auditDate` is always `None` | R001 | `PackageCommands.parseOverlayBindings`, `overlay` |
| 1 | Base validation (1st) | base package → full `Validation.validatePackage` | R002 | `PackageCommands.overlay` |
| 2 | Scratch | `%TEMP%/.overlay-scratch-<guid>` owned by `Scratch.Storage`, deleted on dispose | R125 | `RegionalGtfsOverlay.compile` |
| 3 | Multi-source policy | overlay-all JSON + each profile JSON → validated policies | R004–R006 | `MultiSourcePreparation.loadPolicy`, `Policy.loadPolicy` |
| 4 | Per-source checks | descriptor JSON, payload hash (file SHA-256 or tree hash), snapshot skew against base `manifest.json` | R007, R008 | `MultiSourcePreparation.prepare` 251–267 |
| 5 | **Combined input** (conversion 1: N source GTFS → one namespaced GTFS dir) | for each of `agency, routes, trips, stops, stop_times, calendar, calendar_dates, shapes, transfers`: read every source table (dictionary per row), add `overlay_*` columns, write `combined-source/<table>` as quoted CSV. **Full read+write of every source `stop_times.txt` and `shapes.txt`** | R011–R017 | `writeCombinedTables` |
| 6 | Companion tables | PID `route_sub_agencies.txt` + `trips.txt` → `overlay_route_join.txt`; IDS JMK `api.txt` + `trips.txt` → `operational_trip_candidates.txt`; feed windows → `overlay_ids_authority.txt` | R018–R020 | `writeRouteJoin`, `writeIdsJmkOperational`, `idsAuthorityScopes` |
| 7 | Embedded metadata | `_overlay/policies/*.json`, `_overlay/descriptors/*.json`, `overlay_sources.json` | R023 | `prepare` 277–303 |
| 8 | Combined overrides | each profile's override CSV → `combined-policy/combined-stop-overrides.csv` with rewritten `source_id` | R021 | `writeCombinedOverrides` |
| 9 | Generated policy | merged `OverlayPolicy` with `source_id = regional-all` | R022 | `prepare` 308–360 |
| 10 | Combined hash | `sha256Tree combined-source` (**hashes every combined file again**, incl. `stop_times`), combined descriptor `combined-descriptor.json` (max `retrieved_at`) | R023 | `prepare` 361–367 |
| 11 | Input checks (again) | binding = combined; checksum = the hash from 10 (not recomputed); skew check against 3660 days; GVD year vs base `service_horizon.year`; audit date | R008–R010 | `InputPreparation.prepare` 95–126 |
| 12 | **Base compiler view** (conversion 2: package → GTFS + `cz_*.txt`) | `Validation.validatePackage` (2nd time); extract `gtfs.zip`; rewrite `transfers.txt` with `max_waiting_time` from `serving/transfer`; **rewrite `stop_times.txt`** with `shape_dist_traveled = trip_call.source_kilometre` (scan of base `stop_times` + `trip_call`); write `cz_routes`, `cz_stops`, `cz_trips` (parsed keys), empty `cz_stop_zones`, `cz_trip_stop_zones` (scan of `trip_call` + `call_zone`) | R024 | `Serving.PackageReader.prepareCompilerView` |
| 13 | Override load | combined override CSV → `OverrideBinding[]` | R040 | `Support.loadOverrides` |
| 14 | Shape spool | `shapes.txt` → external sort by (shape_id, sequence) into scratch, only when capability `shapes` (never in production) | R110 | `Shapes.spool` |
| 15 | Base indexes | base `stops/routes/trips/calendar*`, `cz_routes`, `cz_stops` → dictionaries, base stop groups, spatial grid | R025–R027 | `BasePreparation.prepare`, `InputPreparation.prepare` 136–166 |
| 16 | Source tables | combined `agency/stops/routes/trips/calendar*` → eligible routes and trips, calendars | R028–R031 | `SourceAnalysis.analyze` 571–620 |
| 17 | **Source stop_times scan 1** | which source stops active trips call (dictionary per row) | R030 | `analyze` 622–636 |
| 18 | Stop grouping and matching | source groups → base stop places (override, name+geo, coordinate) | R039–R043 | `StopGroups.groupSourceStops`, `StopMatching.matchStops` |
| 19 | Route candidates | source routes → candidate base routes per tier | R049–R052 | `RouteMatching.buildCandidates` 55–123 |
| 20 | **Base stop_times scan** | calls of candidate base trips → `BaseSignature` per trip (3 SHA-256 + 1 digest each) | R053, R055 | `buildCandidates` 125–180 |
| 21 | **Source stop_times scan 2** | calls of eligible source trips, stop place resolved | R054 | `buildCandidates` 182–202 |
| 22 | After-midnight fold | trips departing ≥ 24:00 move to `<service>@next-day` and lose 24 h (mutates rows and calls) | R056 | `foldAfterMidnightTrips` |
| 23 | Context inference | iterate until stable: unresolved groups placed from trip context; approximate base stops corrected or split | R044–R046 | `resolveStopsFromContext` |
| 24 | Source-native places | unmatched stops of source-native-allowed trips → new stop places | R047 | `nativeStopPlaces` |
| 25 | Trip-set authority | authority identity per trip, candidates, blocked line-dates, IDS silence, projections | R057–R064 | `tripSetAuthority`, `extendIdsAuthority` |
| 26 | Registry candidates | unpinned native places → CSV (optional) | R048 | `stopPlaceRegistryCandidates` |
| 27 | Trip matching | per source trip and date: candidates, tiers, ranking, decisions; availability passes; coverage counts | R065–R076 | `TripMatching.matchTrips` |
| 28 | GC boundary | forced full blocking compacting collection | R125 | `Runtime.releaseAnalysisMemory` |
| 29 | Projection | capability gates, output boarding points, selections, per-date claim resolution, slices, service interning, additions, centroids | R077–R092 | `Projection.resolve` |
| 30 | **Output calls** (conversion 3: model → GTFS text) | **base stop_times scan** (rows rewritten per slice, repeats inserted) + **source stop_times scan 3** (added trips) → external sort by (trip_id, sequence) through spill files in a second scratch next to the output → `gtfs-intermediate/stop_times.txt` | R093–R096 | `BundleCalls.write` |
| 31 | GTFS tables | `trips, routes, stops, agency, calendar_dates, feed_info, shapes, transfers` | R097–R103, R107–R110 | `BundleGtfs.*`, `BundleTransfers.write` |
| 32 | Czech tables | `extensions/cz_routes, cz_trips, cz_stops, cz_stop_zones, cz_trip_stop_zones` (re-reads the base `cz_*`) | R101, R104, R105 | `BundleExtensions.write` |
| 33 | Mappings and reports | `mappings/*.csv`, `reports/*.csv` from final decisions (re-reads source `transfers`, `trips`, `routes`, `pathways`, `levels`, `operational_trip_candidates`) | R119–R122 | `Reports.write` |
| 34 | Manifest draft | overlay manifest object; policies and descriptors copied for diagnostics | R118 | `BundleWriter.compile` 123–177 |
| 35 | **Package** (conversion 4: GTFS + cz + CSV → relations) | `CompilerOutput.Output { feed = "jdf"; gtfs; czech; mappings; reports; basePackage }` → serving relations (`trip_call` from `stop_times` again, keys, base semantics projected through `base_to_output_trips.csv`), `gtfs.zip`, `manifest.json`, `diagnostics.json`; optional route shapes | R111–R118, R123 | `PackageWriter.writePackage` |
| 36 | Diagnostics artifact | `--diagnostics-out`: `events/*.csv` (reports minus coverage), `projection/czech/*.txt`, `traces/*.csv` (mappings; `base_to_output_*` only with `--diagnostic-traces`), `inputs/policies`, `inputs/source-descriptors` | R119 | `PackageWriter.writeDiagnosticArtifact` |
| 37 | Activation | delete compiler temp, move `<out>.tmp-<guid>.production` → `<out>`; on failure keep the compiler temp for diagnosis | R003 | `BundleWriter.write` |

Scans of `stop_times`: source tables are read 5 times (stage 5 read and write, stage 10 hash,
17, 21, 30) plus the hash of each original ZIP (stage 4); the base `stop_times` is read 4 times
by the overlay (12 twice: restore and rewrite; 20; 30) and again by the package writer
(`trip_call`, keys), and `Validation.validatePackage` reads the base package twice (stages 1, 12).

Temporary files: `combined-source/` (full copy of all sources), `combined-policy/`,
`combined-descriptor.json`, `base-production-view/{gtfs,extensions}` (full copy of the base GTFS),
row logs for diagnostics, candidate scores, ambiguous candidates and headsign provenance
(`Scratch.RowLog`), shape spill and store files, external-sort spill files for output calls, the
compiler output tree `.<out>.tmp-<guid>/` (`gtfs-intermediate/`, `extensions/`, `mappings/`,
`reports/`, `policy/`, `source-metadata/`) and the production temp `<that>.production`.

Multi-source versus single source: there is no single-source path. `compile` always goes through
`MultiSourcePreparation`; a one-source run (the tests) writes a one-source combined input. With
two sources: tables are concatenated in source-ID order; the generated policy merges the
profiles (R022), so per-source thresholds other than authority modes, IDS scopes, the route
join, revisions and overrides are **not** per source; claims from different sources on one base
trip and date are coalesced when compatible (`cross_source_fact_coalesced`) and quarantined
otherwise (`cross_source_fact_conflict`); two sources' stops on one place with one platform code
share a structural post ID (R079); equivalent added trips of different sources collapse (R090).

---

## 2. Inputs and outputs, field by field

### 2.1 Read from the base package

| Source | Fields | Used for |
|---|---|---|
| `manifest.json` | `source_snapshot.retrieved_at`, else `sources[0].retrieved_at` (required) | skew check (R008), snapshot date and default audit date (R009) |
| `manifest.json` | `service_horizon.year` (optional) | must equal `--gvd-year` (R009) |
| `manifest.json` | whole file SHA-256 | overlay manifest draft `base_manifest_sha256` (dropped, B3); diagnostics `evidence_references` |
| `gtfs.zip` `stops.txt` | `stop_id, stop_name, stop_lat, stop_lon, location_type, parent_station, zone_id, wheelchair_boarding, platform_code` + all columns copied | base stop groups (R025), output stops (R100) |
| `gtfs.zip` `routes.txt` | `route_id, agency_id, route_type, route_short_name` + all copied | route candidates (R049–R051), output routes (R098) |
| `gtfs.zip` `trips.txt` | all GTFS trip fields (`route_id, service_id, trip_id, trip_headsign, trip_short_name, direction_id, block_id, shape_id, wheelchair_accessible, bikes_allowed`) | everything |
| `gtfs.zip` `calendar.txt`, `calendar_dates.txt` | all | base date sets (R029 semantics) |
| `gtfs.zip` `stop_times.txt` | `trip_id, stop_id, arrival_time, departure_time, stop_sequence, shape_dist_traveled, stop_headsign` (matching); all columns copied (output) | R053, R093 |
| `gtfs.zip` `agency.txt`, `feed_info.txt`, `transfers.txt` | all | R102, R103, R107 |
| `serving/trip_call` | `trip_id, sequence, passenger_service, source_kilometre` | restores JDF kilometres as `shape_dist_traveled` (R024) |
| `serving/transfer` | six selectors + `maximum_waiting_time` | restores `max_waiting_time` (R024) |
| `serving/source_key` | `namespace, public_id, identifier` for `cis:line`, `cis:line_trip`, `czptt:train_number` | `cz_routes.cis_line_id`, `cz_trips` (R024) |
| `serving/route` | `route_id, timetable_kind` | detour routes (R058) |
| `serving/assignment` | `scope = route, kind = integrated_transport, route_id, source_code` | `cz_routes.ids_system_ids` (R062) |
| `serving/location` | `location_id, parent_location_id` | `cz_stops.stop_place_id` (R025) |
| `serving/route_stop_zone`, `serving/call_zone` | zones | `cz_trip_stop_zones` (R105) |
| at package time: `serving/service_note, assignment, connection_claim, travel_restriction, source_key, call_key, location, route` | | projected base semantics, keys, location metadata, `timetable_kind` (R111, R115–R117) |

The base must have been compiled by JDF: the overlay relies on `cis:line` keys, `timetable_kind
= detour`, LinExt `integrated_transport` assignments, `jdf:agency:<IČO>:<n>` agency IDs (R099),
the ` [?]` suffix on estimated stop names (R026, written by `JdfToGtfs.fs` 103–108), JDF trip
dating by departure (R056) and JDF kilometres (R024). Section 5 lists what a rail base would
need.

### 2.2 Read from a regional GTFS

Members of the golden snapshots (header line read only):

- PID: `agency, calendar, calendar_dates, fare_attributes, fare_rules, feed_info, levels,
  pathways, routes, route_stops, route_sub_agencies, shapes (138 MB), stops, stop_times (114 MB),
  transfers, trips, vehicle_allocations, vehicle_boardings, vehicle_categories,
  vehicle_couplings`.
- IDS JMK: `trips, agency, calendar, calendar_dates, api.txt, routes, stop_times (28 MB), stops,
  transfers`. Times are not zero-padded (`4:32:00`).

| File | Columns read | Notes |
|---|---|---|
| `agency.txt` (required non-empty) | `agency_id`; all columns present in the **base** agency header are copied for source-native agencies | R098 |
| `routes.txt` (required) | `route_id, agency_id, route_type, route_short_name, route_long_name, route_color, route_text_color`; base-header columns copied for native routes (PID `route_url`) | PID `is_night, is_regional, is_substitute_transport` unused |
| `trips.txt` (required) | `route_id, service_id, trip_id, trip_headsign, trip_short_name, direction_id, block_id, shape_id, wheelchair_accessible, bikes_allowed`; PID `sub_agency_id` (route join key) | PID `exceptional, headsign_icons` unused; `direction_id` never used for matching |
| `stops.txt` (required) | `stop_id, stop_name, stop_lat, stop_lon, location_type, parent_station, platform_code, wheelchair_boarding, zone_id` (IDS JMK zones), `level_id` (namespaced only); PID `asw_node_id, asw_stop_id` | PID `zone_region_type, stop_url` unused |
| `stop_times.txt` | `trip_id, stop_id, arrival_time, departure_time, stop_sequence, shape_dist_traveled, stop_headsign`; for added trips every column of the **base** `stop_times` header (`pickup_type, drop_off_type, timepoint`) is copied | PID `trip_operation_type, bikes_allowed, stop_icons, headsign_icons` dropped |
| `calendar.txt`, `calendar_dates.txt` | all | R029 |
| `feed_info.txt` | `feed_start_date, feed_end_date` (first row) | IDS authority window (R020) |
| `shapes.txt` | five standard columns | only with capability `shapes` (off in production) |
| `transfers.txt` | `from_stop_id, to_stop_id, from_trip_id, to_trip_id, transfer_type, min_transfer_time, max_waiting_time` (`from/to_route_id` namespaced, ignored) | R108 |
| PID `route_sub_agencies.txt` | `route_id, sub_agency_id, route_licence_number` | via policy `route_join` (R018) |
| IDS JMK `api.txt` | lines `Linka/CVlaku = trip_id: <line>/<course> = <trip>` | hard-coded for adapter `ids-jmk-v1` (R019) |
| `pathways.txt`, `levels.txt` | row count only | `exclusions.csv` |
| not read | `fare_*`, `route_stops`, `vehicle_*` | |

### 2.3 Policies

Overlay-all policy (`pid-ids-jmk-production-v1.json`): `schema_version` (must be 1), `sources[]`
with `source_id`, `policy` (profile path relative to this file), `adapter` (`pid-v1` or
`ids-jmk-v1`). All read.

Source profile (`schema_version` 4). "Effective" is what a combined run uses (R022).

| Key | PID | IDS JMK | Read? Effective in a combined run |
|---|---|---|---|
| `source.source_id` | `pid-gtfs` | `ids-jmk-gtfs` | read; must equal the overlay-all entry; replaced by `regional-all` |
| `excluded_route_types` | `["2"]` | `["2"]` | union, compared as raw `route_type` strings |
| `maximum_snapshot_skew_days` | 7 | 7 | per source (R008) |
| `route_join.{table,source_keys,lookup_keys,value_column}` | `route_sub_agencies.txt`, `[route_id, sub_agency_id]` ×2, `route_licence_number` | `null` | per source (R018); replaced by the generated `overlay_route_join.txt` join |
| `route_join.target_namespace` | `cis_line_id` | – | validated only (must be `cis_line_id`) |
| `stop_match.group_column` / `post_column` | `asw_node_id` / `asw_stop_id` | `""` / `""` | **ignored**: replaced by `overlay_group_id` / `overlay_post_id`; the PID columns are hard-coded in the adapter (R016, R017) |
| `stop_match.maximum_distance_metres` | 300 | 300 | first source's value (template) |
| `stop_match.coordinate_identity_maximum_metres` | absent (0) | 25 | **maximum** over sources → 25 for PID too |
| `stop_match.coordinate_identity_minimum_margin_metres` | absent (0) | 10 | maximum → 10 |
| `stop_match.split_flat_groups_by_name` | true | true | template |
| `stop_match.contextual_inference.enabled` / `minimum_mapped_calls` | true / 1 | true / 1 | template |
| `stop_match.contextual_inference.maximum_unresolved_groups_per_trip` | absent | absent (candidate: 2) | maximum; `max 1 value` at use |
| `trip_match.time_resolution_seconds` | 60 | 60 | template |
| `trip_match.exact_pattern_proximity` | true | true | template |
| `trip_match.keep_source_calls` | absent (candidate: true) | absent (candidate: true) | true if any source |
| `trip_match.pattern_edit.{enabled,maximum_edits,minimum_agreement,require_same_endpoints}` | true, 3, 0.9, true | same | template |
| `trip_match.source_revision.{column,regex,date_format}` | `trip_id`, `_(?<revision>\d{6})(?:_\|$)`, `yyMMdd` | `null` | validated, then **ignored**: replaced by column `overlay_revision`, regex `(?<revision>\d{6})`, `yyMMdd`; PID extraction is hard-coded (R015) |
| `trip_match.minimum_capability_tier` | `call_boarding_points: pattern_edit_nearest, schedules: pattern_nearest, transfers: pattern_nearest` | same | union; unequal values for one capability fail the build |
| `trip_set_authority.modes` | bus, tram, metro, trolleybus, ferry | bus, tram, trolleybus, ferry | per source via trip flags (R014); union as fallback |
| `trip_set_authority.source_native_modes` | same five | ferry | per source via trip flags |
| `trip_set_authority.ids_system_ids` / `excluded_cis_lines` / `maximum_silenced_lines` | `["30001"]` / `["199041","199042"]` / 20 | absent | per source via `overlay_ids_authority.txt` |
| `route_match_tiers` | companion_assertion, structural_trip_evidence | structural_trip_evidence | union in canonical order |
| `trip_match_tiers` | all five | all five | union in **canonical** order (profile order is ignored) |
| `capabilities` | stop_coordinates, boarding_points, call_boarding_points, route_short_name, route_long_name, route_color, route_text_color, transfers, schedules | the same + stop_zones | union (so PID runs with `stop_zones`, gated by a source-ID check, R101) |
| `overrides.stops` | `pid-stop-overrides.csv` | `""` | per source (R021) |

`config-candidate/` differs only in `trip_match.keep_source_calls: true` (both profiles) and IDS
JMK `contextual_inference.maximum_unresolved_groups_per_trip: 2` (which, through the maximum,
also applies to PID). The overlay-all file is identical.

### 2.4 Reviewed data

`pid-stop-overrides.csv` (6 rows): `source_namespace` (`pid-stop-group`, required non-empty, never
interpreted), `source_id` (a PID group ID as the profile sees it: parent station `U1000S1` or flat
group `group:539:národní-třída`), `target_namespace` (`jdf-stop-place-name` in all rows;
`jdf-stop-place` also supported), `target_id` (`Praha,Letňany`), `valid_from`, `valid_to`
(`yyyyMMdd`), `review_note` (required non-empty). Read by R021, R040.

`overlay_places.csv` (119 rows: 109 PID, 10 IDS JMK, all `active`): `source_id, group_key,
place_id, status, note`. Loaded by `StopRegistry.parseOverlayPlaces` (required fields, unique
`(source_id, group_key)`, status validated, `status` otherwise ignored, `note` ignored). The
README says a native place's default ID "is a hash of the source group, and that hash changes
when the stop is renamed". Every `place_id` in the file is exactly
`overlay:<source_id>:stop-place:sha16(group_key)` (checked for one PID and one IDS JMK row), so
the registry pins the current hash. `group_key` embeds F# strings (section 4.2).

### 2.5 Output relative to the base

Every row of the output package is either a base row passed through, a base row changed, or a
new row. The overlay writes GTFS + `cz_*.txt`; the relation column says what the serving writer
publishes from it (serving §2).

| Relation | Change relative to the base |
|---|---|
| `agency` | base agencies referenced by a used route, plus source-native agencies `overlay:<src>:agency:sha16(original agency_id)` with the source's columns (R098, R102); unused base agencies are dropped |
| `location` (stop places) | base places that are used (called, or parent of a used stop) are kept; name replaced by the source group name where an approximate base stop was corrected (R046); coordinates replaced by the source group centroid where `stop_coordinates` and exactly one distinct centroid claims it (R091); `coordinate_source` is **not** changed (B4); new source-native places `overlay:<src>:stop-place:…` or the pinned registry ID, `location_type 1`, centroid coordinates, `coordinate_source = overlay` (R047, R117); unused base places dropped |
| `location` (boarding points) | base posts kept when used; new posts `overlay:<src>:post:sha16(original stop_id)` or `overlay:regional-all:post:sha16(place⟨US⟩PLATFORM)` with parent = matched place, name = parent's name, coordinates (with `boarding_points`), `platform_code`, `wheelchair_boarding` from the source (R079, R100); posts not referenced by a call are pruned |
| `route` | only routes with an output trip; base routes get `route_short_name`, `route_long_name`, `route_color`, `route_text_color` from the bound source routes when each capability is on and the non-empty source values agree (R098); new derived routes `overlay:<src>:route:sha16(original route_id \| cis)` for source-mode or source-native service (R059); presentation rules applied last (R099); `timetable_kind` inherited from the base by route ID |
| `trip` | base trips kept on retained dates; split into slices `<base trip>:overlay:sha12(…)` where dates differ in selection (R087); `service_id` always replaced by `overlay:service:<16 hex>`; `shape_id` cleared (or an overlay shape); `route_id` switched to a derived route where the source mode differs (R088); headsign from the source when the selection comes from an authority trip (R089); retired on owned line-dates without a claim (R086); new source trips `overlay:<src>:trip:sha20(src\|trip\|dateKey)` with source fields (R090) |
| `trip_call` | per slice: boarding point replaced by the source post (`call_boarding_points`), arrival/departure from the source (`schedules`), `stop_headsign` from the source for authority trips, `shape_dist_traveled` (published as `source_kilometre`) from the source only with an overlay shape; extra source calls inserted after their base call and later sequences shifted (R094); added trips carry the source calls (R095) |
| `service_calendar`, `service_exception` | every service is exceptions-only (`calendar_dates` with `exception_type 1` per day, no `calendar.txt`), weekday mask 0 (R103) |
| `shape`, `shape_point` | overlay shapes `overlay:shape:sha20(points)` only with capability `shapes` (off); otherwise generated by the route-shapes phase |
| `transfer` | base transfers re-pointed to every pair of slices of their trips (R107); source trip-to-trip transfers projected and reconciled (R108, R109) |
| `service_note` | base notes copied (R111) |
| `assignment`, `connection_claim`, `travel_restriction` | base rows re-targeted to every output trip cut from their trip, IDs re-derived `jdf:<kind>-projection:<32 hex>` when the trip changed, sequences shifted by repeats (R111) |
| `call_zone`, `route_stop_zone` | base call zones copied per slice (resequenced); IDS JMK `zone_id` per stop place as place-level zones with system `ids-jmk` (R101, R105) |
| `source_key` | new: `<sys>:gtfs_trip_id` per source trip and contiguous date run, `ids-jmk:line_course`, `ids-jmk:gtfs_trip_line_course`, `<sys>:gtfs_route_id`, `<sys>:gtfs_stop_id`; national keys re-derived from `cz_*`; other base keys re-targeted (R112–R115, R131) |
| `call_key` | new: `<sys>:gtfs_trip_id` calls whose source sequence differs from the output sequence; base call keys follow their trips (R116) |
| `route_stop` | re-derived by the writer (serving) |
| authority / coverage markers | `cz_trips.coverage_sources` and `source_trip_ids` on added trips and `cz_stops.asw_id, post_id, source_ids` on new stops are written but **not published** (only the diagnostics `projection/czech/` copy has them); there is no published authority marker |
| `manifest.json` | base source plus each regional source (`source_id, payload_sha256, descriptor_sha256, retrieved_at, descriptor_file, adapter`); `compiler`, `service_horizon`; the overlay's own fields are dropped (B3) |
| `diagnostics.json` | `counts_by_code`, `examples_by_code` (≤10 per code) of `reports/diagnostics.csv`, `coverage_populations` from eight coverage CSVs (R123) |
| diagnostics artifact | `events/{ambiguous_trip_candidates, diagnostics, headsigns, pattern_edits, post_pruning, source_trip_additions, stop_context_inference, stop_group_matches, trip_candidate_scores, unmatched_trip_reasons}.csv`, `traces/source_to_output_{calls,routes,stops,trips}.csv`, `operational_to_source_trips.csv` (and `base_to_output_trips.csv`, `resequenced_calls.csv` with traces), `projection/czech/*`, `inputs/policies/*`, `inputs/source-descriptors/*` |

---

## 3. Rules

### 3.1 Inputs and validation

#### OVL-R001 Command line and source bindings

- **What**: `--source` and `--source-descriptor` are repeated `SOURCE_ID=PATH` values (split at the
  first `=`; an empty side is an error). At least one source; source IDs unique; descriptor IDs
  unique; exactly one descriptor per source. `--gvd-year` must parse as an integer. The base is
  validated before anything else (R002). `auditDate` is always `None` from the CLI.
- **Where**: `jrutil-multitool/Commands/PackageCommands.fs` `parseOverlayBindings` 54–76,
  `overlay` 78–105.
- **Why**: CLI contract (`Program.fs` usage line 18).
- **Pinned by**: `Combined PID and IDS JMK overlay is order independent and preserves JMK metadata`
  (duplicate and missing bindings).
- **Constants**: none.
- **Diagnostic**: exceptions only.
- **Proposed verdict**: port into the `jrutil` CLI / build spec; sources become build-spec
  entries, not CLI pairs.

#### OVL-R002 Base package validated

- **What**: `Validation.validatePackage base` runs in the command and again in
  `prepareCompilerView`.
- **Where**: `PackageCommands.fs` 81; `Serving/PackageReader.fs` 88.
- **Why**: "Materialize a private compiler view from a validated production package" (comment).
- **Pinned by**: none directly.
- **Constants**: none. **Diagnostic**: exception.
- **Proposed verdict**: drop. The Rust overlay is a pass over the in-memory `Timetable`; the base
  is never re-read.

#### OVL-R003 Immutable output, atomic activation

- **What**: the output path must not exist (`validateInputs`). The compiler writes into
  `.<name>.tmp-<guid>` next to the output; the package writer writes `<that>.production`; on
  success the compiler temp is deleted and the production temp moved to the output. A failure in
  compile deletes the temp; a failure in finalisation keeps the compiler temp and logs its path.
- **Where**: `Support.fs` 150–160; `BundleWriter.fs` 53–60, 181–206.
- **Why**: "overlay bundles are immutable"; "The compiler scratch … is the only evidence for a
  finalization failure".
- **Pinned by**: `Overlay refuses checksum mismatch without activating output`; `Evidence flush
  failure leaves neither scratch nor activated output`; `Spill failure removes all scratch files`;
  the last assertions of `Overlay enriches non-rail facts and deterministically splits validity`.
- **Constants**: none. **Diagnostic**: exception.
- **Proposed verdict**: replace with `jr-publish`'s package activation (one implementation for all
  packages).

#### OVL-R004 Overlay-all policy

- **What**: JSON (snake_case, case-insensitive). `schema_version` must be 1; `sources` non-empty;
  each `source_id` non-blank and unique; `adapter` ∈ {`pid-v1`, `ids-jmk-v1`}; each profile
  (path relative to the overlay-all file unless rooted) loads (R005) and its `source.source_id`
  equals the entry's.
- **Where**: `MultiSourcePreparation.fs` `loadPolicy` 35–52.
- **Why**: none stated.
- **Pinned by**: `Combined PID and IDS JMK overlay is order independent…`.
- **Constants**: schema 1 (`Types.OverlayAllPolicySchemaVersion`), adapter names.
- **Diagnostic**: `ArgumentException`.
- **Proposed verdict**: replace with a TOML build-spec list of source profiles; the adapter name
  becomes the profile's set of declared hooks (section 5).

#### OVL-R005 Source profile validation

- **What**: `schema_version` = 4; `source` present; `source_id` non-blank; `excluded_route_types`
  not null; `maximum_snapshot_skew_days` ≥ 0; if `route_join` present: `target_namespace =
  cis_line_id`, `table` and `value_column` non-blank, `source_keys` and `lookup_keys` non-null,
  non-empty, equal length, all non-blank; `stop_match` present with `maximum_distance_metres > 0`,
  `coordinate_identity_maximum_metres ≥ 0`, `coordinate_identity_minimum_margin_metres ≥ 0`,
  `contextual_inference` present with `minimum_mapped_calls ≥ 1`; `trip_match`,
  `trip_set_authority`, `pattern_edit` present; `source_revision` (if present) with non-blank
  column, regex, date format and a named `revision` group; `maximum_edits ≥ 0`;
  `0 ≤ minimum_agreement ≤ 1`; authority `modes` and `source_native_modes` non-null, each in
  {bus, tram, metro, trolleybus, ferry, other-guided}, every native mode also in `modes`; with
  `ids_system_ids` non-empty each non-blank and `maximum_silenced_lines ≥ 1`; route tiers ⊆
  {companion_assertion, structural_trip_evidence} and non-empty; trip tiers ⊆ the five
  configurable tiers and non-empty; `capabilities` ⊆ `Values.capabilityNames` (R077). Not
  validated: `minimum_capability_tier` keys or values (B18), `time_resolution_seconds` (checked
  at first use: must be > 0), `group_column`/`post_column`.
- **Where**: `Policy.fs` `loadPolicy` 19–107; `TripScoring.fs` 14–15.
- **Why**: none stated.
- **Pinned by**: `Configured companion join validates namespace and requires its table`.
- **Constants**: schema 4 (`OverlayPolicySchemaVersion`); mode and tier vocabularies.
- **Diagnostic**: `ArgumentException`.
- **Proposed verdict**: port as serde-validated profile types; validate every key including the
  capability-tier map.

#### OVL-R006 Bindings equal the policy's sources

- **What**: duplicate binding IDs fail; the set of binding IDs must equal the set of policy source
  IDs.
- **Where**: `MultiSourcePreparation.fs` 244–248.
- **Pinned by**: `Combined PID and IDS JMK overlay is order independent…`.
- **Constants**: none. **Diagnostic**: `ArgumentException`.
- **Proposed verdict**: port (build-spec validation).

#### OVL-R007 Payload checksum

- **What**: descriptor JSON must have `retrieved_at` (or `retrievedAt`, parsed by
  `DateTimeOffset.Parse`) and `payload_sha256` (or `payloadSha256` or `sha256`, lower-cased).
  The payload hash is SHA-256 of the file, or for a directory `sha256Tree`: files sorted by path
  (ordinal), line `relative/path⟨US⟩length⟨US⟩sha256\n` per file, SHA-256 of the concatenation.
  Mismatch fails the run.
- **Where**: `GtfsFiles.fs` `readDescriptor` 95–110; `Values.fs` `sha256Tree` 39–49;
  `MultiSourcePreparation.fs` 258–260; `InputPreparation.fs` 99–106 (re-check against the
  combined hash).
- **Why**: BASE_PLAN 11: "Sources reach JrUtil only as checksum-pinned local snapshots".
- **Pinned by**: `Overlay refuses checksum mismatch without activating output`; `Combined …`
  (bad checksum).
- **Constants**: separator U+001F.
- **Diagnostic**: `InvalidOperationException "Source checksum mismatch for <id>"`.
- **Proposed verdict**: port into the build-spec snapshot loader shared with JDF and CZPTT
  (`jrutil` crate); not overlay logic.

#### OVL-R008 Snapshot skew

- **What**: `|source.retrieved_at − base.retrieved_at|` in days (fractional) must not exceed the
  source profile's `maximum_snapshot_skew_days`. The base value comes from
  `manifest.source_snapshot.retrieved_at`, else `sources[0].retrieved_at` (invariant culture).
  The second check in `InputPreparation` uses the generated policy's 3660 days and the combined
  descriptor (max `retrieved_at`), so it never fires.
- **Where**: `Support.fs` `descriptorRetrievedAtFromBase` 133–148; `MultiSourcePreparation.fs`
  263–265; `InputPreparation.fs` 107–110.
- **Why**: none stated (keeps base and source from different timetable states).
- **Pinned by**: `Combined …` (skewed binding).
- **Constants**: 7 days (PID, IDS JMK), 3660 (generated).
- **Diagnostic**: `InvalidOperationException`.
- **Proposed verdict**: port per source; drop the dead second check.

#### OVL-R009 GVD window and audit date

- **What**: `2000 ≤ gvdYear ≤ 9999`. If the base manifest has `service_horizon.year` it must equal
  `gvdYear`. The window is `gvdBounds gvdYear`. `snapshotDate` = base `retrieved_at` converted to
  `Europe/Prague` date. `auditDate` = option or `snapshotDate`; it must lie in the window.
- **Where**: `Support.fs` 151; `InputPreparation.fs` 112–126; `Values.fs` `gvdWindow` 85–90.
- **Why**: "A base bounded to another GVD has no service in this window; never overlay across the
  cutover."
- **Pinned by**: `Overlay rejects a base bounded to another GVD`.
- **Constants**: 2000, 9999, `Europe/Prague`.
- **Diagnostic**: `ArgumentException`.
- **Proposed verdict**: port; the window is the `Timetable` horizon. The audit date only labels
  report rows (R122); keep it as a build-spec value.

#### OVL-R010 Existence checks

- **What**: base directory exists and contains `gtfs.zip`; payload file or directory exists;
  descriptor exists.
- **Where**: `Support.fs` `validateInputs` 150–160.
- **Proposed verdict**: replace (the `jrutil` crate opens files; the pass sees values).
- **Why / Pinned by / Constants / Diagnostic**: none / none / none / exceptions.

### 3.2 Combined source preparation

#### OVL-R011 Source order and template

- **What**: sources are processed sorted by `source_id` (ordinal); tables are concatenated in that
  order; the generated policy's template (every field not explicitly merged) is the **first
  source's** profile, i.e. `ids-jmk-gtfs` whenever IDS JMK is present.
- **Where**: `MultiSourcePreparation.fs` 253–256, 308.
- **Why**: determinism ("order independent", test).
- **Pinned by**: `Combined PID and IDS JMK overlay is order independent…`.
- **Constants**: none.
- **Diagnostic**: none.
- **Proposed verdict**: replace: each source profile governs its own entities (B2, Q2). Keep a
  sorted source order for determinism.

#### OVL-R012 Namespacing and column union

- **What**: for each of the nine tables, the output header is the union (first-seen order, then
  distinct) of every source's header, then `overlay_source_id` and table-specific columns:
  `agency`: `overlay_original_agency_id`; `routes`: `overlay_original_route_id`,
  `overlay_original_agency_id`; `trips`: `overlay_original_{route,service,trip,block,shape}_id`,
  `overlay_revision`, `overlay_source_native_allowed`, `overlay_trip_set_authority_allowed`;
  `stops`: `overlay_original_{stop_id,parent_station,level_id}`, `overlay_group_id`,
  `overlay_post_id`; `stop_times`: `overlay_original_{trip,stop}_id`; `calendar*`:
  `overlay_original_service_id`; `shapes`: `overlay_original_shape_id`; `transfers`:
  `overlay_original_` + each of the six selectors. Every non-blank ID value in those columns is
  rewritten to `<source>:<value>` and the original kept. `shapes.txt` is written only if some
  source has a non-empty one. All values are written quoted. `sourceIdentity` reads
  `overlay_source_id` (fallback: the binding ID `regional-all`); `originalIdentity c` reads
  `overlay_original_c` (fallback: column `c`).
- **Where**: `MultiSourcePreparation.fs` `normalizeRow` 71–119, `columnsFor` 121–146,
  `writeCombinedTables` 148–171; `Values.fs` 56–64.
- **Why**: "The sources are merged into one namespaced input and resolved in a single pass"
  (`RegionalGtfsOverlay.fs` 20–22).
- **Pinned by**: `Combined …`.
- **Constants**: separator `:`.
- **Diagnostic**: none.
- **Proposed verdict**: drop. A source is decoded by `jr-formats` into typed facts that carry
  their source ID; no combined text input, no `overlay_*` columns.

#### OVL-R013 Heavy rail never enters

- **What**: combined `routes.txt` omits routes whose `modeClass route_type = heavy-rail`; trips of
  such routes stay in `trips.txt` (their route lookup reads the source's own routes; a trip whose
  route is missing from the source throws `KeyNotFoundException`). Source analysis drops routes in
  `excluded_route_types` (raw string compare) or of class heavy-rail, and trips whose route was
  dropped. Reported in `exclusions.csv` as `heavy_rail_permanently_protected`.
- **Where**: `MultiSourcePreparation.fs` 160, 163; `SourceAnalysis.fs` 575–584;
  `Reports.fs` 426–445.
- **Why**: BASE_PLAN 12: "No regional static rail data is used".
- **Pinned by**: `Overlay enriches non-rail facts…` (`railtrip` absent); `Combined …` ("Heavy rail
  must remain excluded").
- **Constants**: heavy-rail class (R038).
- **Diagnostic**: `exclusions.csv` row.
- **Proposed verdict**: port as a source-profile mode filter (`modes_excluded`), not a hard-coded
  class: the planned foreign-rail extension needs rail.

#### OVL-R014 Per-trip authority flags

- **What**: each combined trip gets `overlay_source_native_allowed = 1` iff its route's mode class
  is in that source's `source_native_modes`, and `overlay_trip_set_authority_allowed = 1` iff in
  `modes` (else `0`). Analysis reads the flags; if absent it falls back to the generated policy's
  union lists.
- **Where**: `MultiSourcePreparation.fs` 153–167; `SourceAnalysis.fs` 199–207.
- **Proposed verdict**: replace with the source profile's mode sets read directly by the engine.
- **Why / Pinned by / Constants / Diagnostic**: keeps per-source modes through the merged policy /
  `Combined …` / – / –.

#### OVL-R015 Source revision (PID hook)

- **What**: for adapter `pid-v1`, `overlay_revision` = the 6 digits of the first match of
  `_(?<revision>\d{6})(?:_|$)` in the original `trip_id` (PID `991_11915_260207` → `260207`), else
  `700101`; for any other adapter always `700101`. The generated policy reads `overlay_revision`
  with regex `(?<revision>\d{6})` and format `yyMMdd`; a trip's revision is the parsed date
  (cached). A non-match or unparsable capture gives `DateTime.MinValue` and
  `source_revision_unresolved` (cannot happen in combined runs). Used only by R082.
- **Where**: `MultiSourcePreparation.fs` 85–91, 345; `SourceAnalysis.fs` 586–614.
- **Why**: PID publishes revisions of one trip with different validity; the newer wins (test
  "The newest overlapping source revision must provide authoritative times").
- **Pinned by**: `Overlay enriches non-rail facts…`; `Equal or unresolved revisions quarantine
  conflicting schedules` (both cases are revision 700101); `Generic feed needs neither companion
  table nor revision identifiers`.
- **Constants**: regexes above, `700101`, `yyMMdd`.
- **Diagnostic**: `source_revision_unresolved` (dead).
- **Proposed verdict**: port as a source-profile hook `revision_from_trip_id { regex, format }`
  (data), absent for IDS JMK; drop the 700101 sentinel (absent revision = no ordering).

#### OVL-R016 Stop group key column (PID hook)

- **What**: `overlay_group_id` = for `pid-v1`: `<src>:group:<asw_node_id>` if non-blank, else
  `<src>:group:<original parent_station>`, else `<src>:group:<original stop_id>`; for others:
  `<src>:group:<parent>` else `<src>:group:<stop_id>`. The generated policy sets
  `group_column = overlay_group_id`.
- **Where**: `MultiSourcePreparation.fs` 95–102, 336.
- **Why**: BASE_PLAN 14.1: "a documented stop-place key such as PID `asw_node_id`".
- **Pinned by**: `Overlay enriches non-rail facts…` (flat group `group:B1:beta` override).
- **Constants**: `group:` prefix.
- **Diagnostic**: none.
- **Proposed verdict**: port as a source-profile hook `group_key_column = "asw_node_id"`; the key
  string format matters for the registry (section 4.2).

#### OVL-R017 Post key column (PID hook)

- **What**: `overlay_post_id` = `""` for `location_type = 1`; PID: `asw_stop_id` if non-blank;
  otherwise the original `stop_id`. Written to `cz_stops.post_id` of new posts (unpublished).
- **Where**: `MultiSourcePreparation.fs` 103–106; `BundleExtensions.fs` 95.
- **Proposed verdict**: port as `post_key_column` in the profile only if the model carries a
  post key; otherwise drop (it is not published).
- **Why / Pinned by / Constants / Diagnostic**: – / none / – / –.

#### OVL-R018 Companion CIS-line join (PID)

- **What**: for each source with `route_join`: the join table is required non-empty
  (`requireTable`); rows are grouped by the **separator-less** concatenation of the lookup
  columns (B10); a trip whose concatenated source-key columns find exactly one distinct non-blank
  value gets a row `(namespaced trip_id, value)` in `overlay_route_join.txt`. If no row is written
  in any source, the generated policy has no route join. RouteMatching then joins on `trip_id`
  (U+001F-joined) and a trip has a CIS line iff exactly one distinct value exists.
- **Where**: `MultiSourcePreparation.fs` `writeRouteJoin` 175–195, 330–333;
  `RouteMatching.fs` 55–77.
- **Why**: PID's `route_sub_agencies.route_licence_number` is the CIS licence of a route
  operated by a sub-agency (BASE_PLAN 12.3, "documented CIS line/trip claims").
- **Pinned by**: `Configured companion join validates namespace and requires its table`;
  `Overlay enriches non-rail facts…` (`srbus` has two licences, `sub1` picks one).
- **Constants**: table/column names from the PID profile.
- **Diagnostic**: missing table → `InvalidOperationException`.
- **Proposed verdict**: port as a profile hook `cis_line_claim { table, keys, value }` producing a
  typed source fact (trip → CIS line), with a proper composite key.

#### OVL-R019 IDS JMK operational crosswalk

- **What**: for adapter `ids-jmk-v1`: every non-blank, trimmed line of `api.txt` must match
  `^Linka/CVlaku = trip_id: (?<line>[^/]+)/(?<course>.+) = (?<trip>[^=\s]+)$`; the trip must exist
  in `trips.txt` and appear once; the number of mapped trips must equal the number of trips. Any
  failure aborts the run. Rows `(source_id, line, course, namespaced trip, original trip)` go to
  `operational_trip_candidates.txt`; Reports turns them into
  `operational_to_source_trips.csv` rows per output date range of the source trip (R113). The
  Oběhy downloader also rejects an IDS JMK ZIP without `api.txt`.
- **Where**: `MultiSourcePreparation.fs` `writeIdsJmkOperational` 197–217; `Reports.fs`
  175–191; `repo/src/obehy/regional_overlay.py` `download_gtfs`.
- **Why**: BASE_PLAN 6.1: "IDS JMK `api.txt` … maps (source line code, course/train number) to a
  GTFS `trip_id`".
- **Pinned by**: `Combined …` (malformed `api.txt` throws; `"10","42","j1"` mapping).
- **Constants**: the regex.
- **Diagnostic**: `InvalidOperationException`.
- **Proposed verdict**: port as an IDS JMK profile hook `operational_crosswalk = "ids-jmk-api"`
  decoded by `jr-formats`; replace the all-or-nothing failure by per-line diagnostics (Q11).

#### OVL-R020 IDS authority scopes and feed window

- **What**: for each source with `ids_system_ids`: the feed window is `feed_info` first row's
  `(feed_start_date, feed_end_date)` if both non-empty, else `(min, max)` over calendar
  `start_date`/`end_date` and `calendar_dates` dates with `exception_type 1`; none → no scope. One
  scope per system with modes, excluded lines, window and maximum. Written to
  `overlay_ids_authority.txt` (`;`-joined lists, `yyyyMMdd` dates) and read back in R062.
- **Where**: `Policy.fs` 110–155; `MultiSourcePreparation.fs` 272–275.
- **Why**: "Feed windows are per source, so the scopes travel as a table."
- **Pinned by**: `A line of the source's integrated system that the source does not run is not
  running`.
- **Constants**: none.
- **Proposed verdict**: port the window rule; drop the table hand-off.
- **Diagnostic**: none.

#### OVL-R021 Combined stop overrides

- **What**: each profile's `overrides.stops` (non-blank) resolves against `--overrides-root` when
  given and relative, else relative to the profile file; must exist. Every row is copied with
  `source_id` rewritten by `combinedStopGroupId`: `group:X` → `group:<src>:group:X`, `stop:X` →
  `stop:<src>:X`, anything else → `<src>:X`. Output columns: `source_namespace, source_id,
  target_namespace, target_id, valid_from, valid_to, review_note`. The combined CSV is later
  copied into the package diagnostics (`inputs/policies/combined-stop-overrides.csv`).
- **Where**: `MultiSourcePreparation.fs` 60–63, 219–240; `BundleWriter.fs` 129–131.
- **Why**: "Translate a profile's stop-group ID into the combined input's group ID."
- **Pinned by**: `Reviewed presentation overrides win over source display fields` (overrides
  root); `Overlay enriches non-rail facts…`.
- **Constants**: prefixes `group:`, `stop:`.
- **Diagnostic**: missing file → exception.
- **Proposed verdict**: port the file as reviewed data in `data/overlay/`; drop the ID rewriting
  (the engine matches by source-local group key).

#### OVL-R022 Generated (merged) policy

- **What**: `source_id = regional-all`; `excluded_route_types` = sorted union;
  `maximum_snapshot_skew_days = 3660`; `route_join` = generated join or none; `stop_match` =
  template with `group_column = overlay_group_id`, `post_column = overlay_post_id`, both
  coordinate-identity values = maximum over sources, `contextual_inference` = template with
  `maximum_unresolved_groups_per_trip` = maximum; `trip_match` = template with
  `keep_source_calls` = any, `minimum_capability_tier` = union (conflict → `ArgumentException`),
  `source_revision` = R015; `trip_set_authority` = template with sorted union of `modes` and
  `source_native_modes`, IDS lists cleared; route and trip tiers = canonical order filtered by the
  union; `capabilities` = sorted union; `overrides` = combined CSV.
- **Where**: `MultiSourcePreparation.fs` 308–360.
- **Why**: none stated.
- **Pinned by**: `Combined …` indirectly.
- **Constants**: canonical tier orders `[companion_assertion, structural_trip_evidence]`,
  `[full_signature, pattern_endpoints, pattern_first, pattern_nearest, pattern_edit_nearest]`.
- **Diagnostic**: exception on unequal capability tiers.
- **Proposed verdict**: drop. Per-source profiles drive the engine (Q2); the differences this
  causes today are ledgered.

#### OVL-R023 Embedded metadata and combined hash

- **What**: copies the overlay-all policy to `_overlay/policies/overlay-all-policy.json`, each
  profile to `<src>-policy.json`, each descriptor to `_overlay/descriptors/<src>-descriptor.json`;
  writes `overlay_sources.json` `{schema_version 1, policy_sha256 (overlay-all file), sources[]
  {source_id, payload_sha256, descriptor_sha256, retrieved_at ("O" format), descriptor_file,
  adapter}}`; hashes the whole combined directory (`sha256Tree`); writes a combined descriptor.
- **Where**: `MultiSourcePreparation.fs` 277–303, 361–373.
- **Why**: provenance in the manifest and diagnostics.
- **Pinned by**: `Combined …` (three manifest sources).
- **Proposed verdict**: replace: the build spec records sources and policy hashes once; the
  publisher writes them to the manifest.
- **Constants / Diagnostic**: – / –.

### 3.3 Base preparation

#### OVL-R024 Base compiler view (package → GTFS + Czech tables)

- **What**: (a) extract `gtfs.zip` (entries must be flat); (b) `transfers.txt`: append/replace
  `max_waiting_time` from `serving/transfer` keyed by the six selectors joined by U+001F
  (`from_location_id`/`to_location_id` as stop IDs); (c) `stop_times.txt`: rows must align
  one-to-one, in order, with `trip_call` rows where `passenger_service = True` (else exception);
  `shape_dist_traveled` := `source_kilometre`; (d) `cz_routes.txt` = one row per `cis:line` key
  `(route, line)` sorted, `public_line_number ""`, `source_provenance serving`, `timetable_kind`
  from `serving/route`, `ids_system_ids` = sorted distinct `;`-joined `source_code` of route
  assignments `integrated_transport`; (e) `cz_stops.txt`: `stop_place_id` = `parent_location_id`
  or the location itself; (f) `cz_trips.txt`: from `cis:line_trip` keys, **parsing the identifier
  at its first `:`** into CIS line and CIS trip, plus `czptt:train_number`; (g) empty
  `cz_stop_zones.txt`; (h) `cz_trip_stop_zones.txt` = every `route_stop_zone` expanded onto the
  calls of that slot plus `call_zone` rows.
- **Where**: `Serving/PackageReader.fs` `prepareCompilerView` 87–219.
- **Why**: "gtfs.zip carries no JDF kilometres … restore trip_call.source_kilometre as the
  compiler's stop distance"; "Restore it so the compiler can carry base constraints".
- **Pinned by**: `JDF kilometres are kept as source_kilometre and restored in the overlay's base
  view`; `Overlay compiler view is rebuilt from GTFS ZIP and serving identities`.
- **Constants**: namespaces `cis:line`, `cis:line_trip`, `czptt:train_number`;
  `integrated_transport`.
- **Diagnostic**: exceptions.
- **Proposed verdict**: drop entirely. The overlay reads the `Timetable` (route `cis:line` key,
  `timetable_kind`, route features, call kilometres, zones) directly; no ID is parsed.

#### OVL-R025 Base indexes and base stop groups

- **What**: base calendars into the window (R029); `baseCisByRoute` from `cz_routes` rows with a
  non-blank CIS line; `baseRoutesByCis` grouped; `baseIdsSystemsByRoute`; detour routes
  (`timetable_kind = detour`); `basePlaceByStop` from `cz_stops`. Base stop groups: base
  `stops.txt` grouped by `stop_place_id` (else `parent_station`, else own ID); representative =
  the row whose ID is the group ID, else the first member; name = representative's; lat/lon =
  representative's, else the average of members'. Trip text values are interned (memory).
- **Where**: `BasePreparation.fs` 35–121; `StopGroups.fs` `groupBaseStops` 70–85.
- **Proposed verdict**: replace: places and boarding points are model entities.
- **Why / Pinned by / Constants / Diagnostic**: – / indirectly all / – / –.

#### OVL-R026 Approximate base stop marker

- **What**: a base stop group whose name ends with ` [?]` is "approximate". The JDF compiler
  appends that suffix when the stop's position precision is not exact (`JdfToGtfs.fs` 103–108).
  Approximate targets bypass the name check in context inference (R045), may be renamed and
  repositioned by the source (R046) and suppress the distance warning.
- **Where**: `InputPreparation.fs` 139.
- **Why**: "PID name and geodata replace a context-matched approximate JDF stop" (test).
- **Pinned by**: `PID name and geodata replace a context-matched approximate JDF stop`.
- **Constants**: ` [?]`.
- **Diagnostic**: see R046.
- **Proposed verdict**: replace with the model fact `Position.precision == Estimated` (or no
  position). The Rust road compiler writes no ` [?]` (`jr-jdf/src/assemble/places.rs` R416), so
  a name test would never fire.

#### OVL-R027 Spatial grid

- **What**: cell of a point = `(floor(lat / 0.002), floor(lon / 0.002))` (as `float`); every
  distinct member point of every base group is indexed. A query visits cells within
  `radius = max(1, ceil(maximum_distance_metres / 100))` in both axes (300 m → ±3 cells).
- **Where**: `InputPreparation.fs` 151–166; `StopMatching.fs` 83–92.
- **Why**: "0.002 degrees is roughly 140 m longitudinally in Czechia. Use a conservative dynamic
  neighbourhood so the configured radius is real."
- **Constants**: 0.002°, 100 m per cell.
- **Proposed verdict**: replace with `jr-geo`'s spatial grid; candidates are then filtered by
  exact distance, so results are identical.
- **Pinned by / Diagnostic**: `Configured radius finds a unique stop…` / –.

### 3.4 Source analysis

#### OVL-R028 Eligible source routes and trips

- **What**: see R013. All other active trips are eligible regardless of agency or direction.
- **Where**: `SourceAnalysis.fs` 575–585.
- **Proposed verdict**: port (profile mode filter).
- **Why / Pinned by / Constants / Diagnostic**: as R013.

#### OVL-R029 Calendars into the window

- **What**: per `service_id` a boolean per window day. `calendar.txt`: for window dates in
  `[start_date, end_date]`, value = weekday flag (`"1"`); later rows of the same service
  overwrite. `calendar_dates.txt` (after all calendar rows): date in window → value =
  `exception_type = "1"` (so `2` clears, any other value clears too). Dates outside the window
  are dropped. Each set is interned into a packed `Dates` by its SHA-256 key.
- **Where**: `GtfsRows.fs` `parseCalendar` 13–42; `DateSet.fs`.
- **Why**: GTFS semantics.
- **Pinned by**: indirectly; `Packed dates preserve canonical bits and identifiers across GVD
  boundaries`.
- **Constants**: none.
- **Diagnostic**: invalid date → exception.
- **Proposed verdict**: replace with `jr-formats` GTFS calendar facts and `jr_model::DateSet`.

#### OVL-R030 Scheduled source stops

- **What**: only source stops that at least one active eligible trip (service with any day in
  the window) calls are grouped and matched (a full scan of source `stop_times`).
- **Where**: `SourceAnalysis.fs` 622–637.
- **Proposed verdict**: port (derived from the decoded calls, no separate scan).
- **Why / Pinned by / Constants / Diagnostic**: – / – / – / –.

#### OVL-R031 Contiguous calls required

- **What**: the base and source `stop_times` must list each candidate/eligible trip's calls
  contiguously; otherwise `InvalidOperationException` ("must keep each trip's calls contiguous
  for streaming overlay matching"). Calls are sorted by `stop_sequence` per trip.
- **Where**: `RouteMatching.fs` 168–180, 190–202.
- **Proposed verdict**: drop (in-memory trips hold their calls).
- **Why / Pinned by / Constants / Diagnostic**: streaming / – / – / exception.

### 3.5 Names, labels and modes

#### OVL-R032 Simple name normalisation (`normalizeName`)

- **What**: `ToLowerInvariant`; every UTF-16 char that is not `Char.IsLetterOrDigit` becomes a
  space; split on spaces, empties removed. Diacritics kept. Used for: flat group split by name
  (`-`-joined, R039, **part of registry keys**), the stable base name key (U+001F-joined, R040),
  the `target_stable_digest` report column.
- **Where**: `Names.fs` 11–16; `InputPreparation.fs` 146.
- **Pinned by**: registry rows (`kounice-č-p-129-virtuální-zastávka-nm`).
- **Constants**: none.
- **Proposed verdict**: port **exactly** inside the registry-key builder (it defines stored
  keys); use the shared `jr-geo` normaliser elsewhere.
- **Why / Diagnostic**: – / –.

#### OVL-R033 Stop-name equivalence rank (`stopNameMatchRank`)

- **What**: returns `Some rank` (lower is closer) when two names say the same place, else `None`.
  1. Segments: split on `,`, trim; a segment containing `rozc` (case-insensitive) loses a trailing
     `\s*\d+[.,]\d+\s*$` (CIS junction distance `rozc.4.0`); tokenise; drop empty segments.
  2. Tokens: lower-case; letters/digits accumulate; any other char ends the word, and the word is
     *dotted* iff that char is `.`; end of text ends a non-dotted word.
  3. Word equality `sameWord a b`: equal text; or `a` dotted and `b` starts with `a`; or `b` dotted
     and `a` starts with `b`; or both ≥ 3 chars and one starts with the other.
  4. `nameDifference(L, R)` (memoised on `(i, j)`): both exhausted → 0; one exhausted → none;
     otherwise the minimum of: `sameWord` → rest + (0 if texts equal else 1); `L[i]` expands to
     a word list matching `R[j..]` → rest + 1; symmetric.
  5. Rank = difference of all tokens; if none, for each side as "longer" and each
     `dropped ∈ 1..segments−1`: difference(longer segments `dropped..`, all of shorter) + number of
     tokens in the dropped leading segments; minimum; none if no option.
- **Where**: `Names.fs` 18–144.
- **Why**: "Rank two stop names that say the same place … 0 for the same words, more for
  abbreviations spelled out and for leading locality segments one side leaves out."
- **Pinned by**: `Czech stop-name abbreviations match their verbose IDS JMK forms`; `Wider radius
  does not merge a stop with a different facility prefix`; `Headsigns resolve full names
  conservatively`.
- **Constants**: prefix length 3; regex above; `rozc`.
- **Diagnostic**: none.
- **Proposed verdict**: port into `jr-geo` as the one stop-name matcher, with the expansion list
  as data (R034). Note it differs from the CZPTT and JDF matchers (section 8).

#### OVL-R034 Abbreviation expansions

- **What**: dictionary abbreviation → word lists. Built-ins: `žst` → `železniční stanice`, `zd` →
  `zemědělské družstvo`, `mš` → `mateřská škola`, `zš` → `základní škola`, `kd` → `kulturní dům`.
  Then `GeoData/StopMatcherSynonyms.txt` (if the file exists at the **compile-time source path**
  `__SOURCE_DIRECTORY__/StopMatcherSynonyms.txt`, B12): per line, text before `#`, lower-cased,
  split on `,`, trimmed, empties dropped; with > 1 members, abbreviations = first member and every
  member ending in `.`; full forms = members not ending in `.`; add every (abbreviation, full form)
  pair that differ. Add rule: key = abbreviation trimmed and stripped of trailing `.`; skip empty
  keys, keys containing a space, empty expansions, and one-word expansions equal to the key.
- **Where**: `Names.fs` 49–85.
- **Why**: "Expansions that a prefix cannot find: acronyms and the like."
- **Pinned by**: `Czech stop-name abbreviations…` (`ZD`, `MŠ`).
- **Constants**: the five pairs; the 59-line synonyms file.
- **Proposed verdict**: port as a data file in `data/` (or `jrunify-ext-geodata`), loaded
  explicitly; never from a source path.
- **Diagnostic**: none.

#### OVL-R035 Full headsign resolution

- **What**: `resolveFullHeadsignDetailed raw destinations`: tokens = regex `[\p{L}\p{N}]+` over
  the lower-cased text; forms(x) = tokens(x) and, if `x` has a comma, tokens(after the first
  comma), non-empty, distinct. Raw matching `(?i)\b(via|přes)\b|[→;]` → `(None,
  via_or_composite)`. Else, in order: exactly one distinct destination with tokens equal to raw's
  → `exact_full_name`; exactly one with `stopNameMatchRank raw d` some → `locality_prefix`; the
  **last** destination matches (same token count, pairwise equal or by the table `žel →
  železniční`, `st → stanice`, `aut → autobusové|autobusová`, `nám → náměstí`, `sídl →
  sídliště`, `zast → zastávka`) → `terminal_name`; exactly one matching → `known_abbreviation`;
  none → `no_destination_match`; else `ambiguous_destination`. The overlay calls it with a single
  destination (R092).
- **Where**: `Support.fs` 163–205.
- **Why**: "Resolve display abbreviations only when they identify one downstream full name."
- **Pinned by**: `Headsigns resolve full names conservatively`.
- **Constants**: regexes and the six-entry table.
- **Diagnostic**: reason strings go to `headsigns.csv`.
- **Proposed verdict**: port, with the abbreviation table merged into R034's data file.

#### OVL-R036 Compatible mode classes

- **What**: `left = right`, or both in {bus, trolleybus}.
- **Where**: `Names.fs` 146–148.
- **Why**: "JDF represents trolleybus service as road/bus" (`SourceAnalysis.fs` 492).
- **Pinned by**: `Czech stop-name abbreviations…`; `Matched trolleybus slice retains its
  source-mode route`.
- **Constants**: the pair.
- **Proposed verdict**: port as a profile/base-family table (`compatible_modes`), since a rail
  overlay needs other pairs.
- **Diagnostic**: none.

#### OVL-R037 Structurally compatible route labels

- **What**: normalise = upper-case invariant, keep letters and digits. Equal, or both non-empty
  and one is a single letter that prefixes the other with only digits after it (`H` ~ `H4`, not
  `2` ~ `25`).
- **Where**: `Names.fs` 150–162.
- **Pinned by**: `Czech stop-name abbreviations…`.
- **Proposed verdict**: port as a named rule of the route-matching tier.
- **Why / Constants / Diagnostic**: – / – / –.

#### OVL-R038 Mode class of a GTFS route type

- **What**: non-integer → `other:<text>`; `0` or 900–906 → tram; `1` or 400–405 → metro; `2` or
  100–117 → heavy-rail; `3`, 200–209, 700–716 → bus; `4` or 1000–1021 → ferry; `11` or `800` →
  trolleybus; `12` or 1200–1207 → other-guided; else `other:<n>` (cable 5/6/7/1300–1307/1400 are
  `other:*`).
- **Where**: `GtfsFiles.fs` `modeClass` 150–162.
- **Why**: "JDF long-distance lines (Linky type D) are coach (200-209); PID publishes the same line
  as plain bus (3)."
- **Pinned by**: `Bus slice of a long-distance CIS line keeps the CIS route`.
- **Proposed verdict**: replace with `jr_model::Mode` (one mode classifier); the overlay compares
  model modes.
- **Constants / Diagnostic**: ranges above / –.

### 3.6 Stops

#### OVL-R039 Source stop grouping

- **What**: scheduled source stops (R030) grouped by key: non-blank `parent_station` →
  the (namespaced) parent ID; else non-blank group column (`overlay_group_id`) → with
  `split_flat_groups_by_name`: `group:<group value>:<normalizeName(stop_name) joined by '-'>`,
  without: `group:<group value>`; else `stop:<stop_id>` (unreachable in combined runs).
  Representative: the explicit parent row if present in `stops.txt`, else the first member. Group
  name = representative's `stop_name`; lat/lon = average of members' parseable values (decimal).
  Group points = distinct member coordinate pairs, else the group centroid.
- **Where**: `StopGroups.fs` `groupSourceStops` 41–68, `stopGroupPoints` 27–39,
  `averageCoordinate` 23–25.
- **Why**: BASE_PLAN 14.1 grouping order.
- **Pinned by**: `Overlay enriches non-rail facts…`; registry rows.
- **Constants**: `group:`, `stop:`, `-`.
- **Diagnostic**: none.
- **Proposed verdict**: port the grouping order as source-profile data; produce the group key
  string exactly as today for the registry (section 4.2).

#### OVL-R040 Reviewed stop override tier

- **What**: overrides by `source_id` (combined group ID). Rows whose `[valid_from, valid_to]`
  overlaps the window at all apply (whole window). Target resolution: `jdf-stop-place-name` →
  stable name key of `target_id` must find exactly one base group (else
  `stop_override_target_ambiguous` / `stop_override_target_missing`); `jdf-stop-place` → the ID as
  given; other namespaces → `stop_override_namespace_unsupported`. Distinct targets: one → if a
  base group has that ID, match with method `reviewed_override` (no distance check), else
  `stop_override_target_missing`; several → `stop_override_ambiguous` and the group stays
  unmatched; **none → the group falls through to automatic matching** (B11).
- **Where**: `StopMatching.fs` 41–82; `Support.fs` `loadOverrides` 39–61.
- **Why**: override notes: "PID platforms remain just beyond the automatic 125 m radius".
- **Pinned by**: `Overlay enriches non-rail facts…` (`reviewed_override`).
- **Constants**: namespaces `jdf-stop-place-name`, `jdf-stop-place`.
- **Diagnostic**: the five codes above.
- **Proposed verdict**: port as tier 1 of stop matching; a failed override quarantines the group
  (fix B11); `jdf-stop-place` targets become registry stop numbers, not IDs.

#### OVL-R041 Override for an unknown group

- **What**: an override whose `source_id` is not a scheduled source group →
  `stop_override_source_missing`.
- **Where**: `StopMatching.fs` 58–61.
- **Proposed verdict**: port.
- **Why / Pinned by / Constants**: – / – / –. **Diagnostic**: `stop_override_source_missing`.

#### OVL-R042 Name plus distance tier

- **What**: nearby = base groups in the grid neighbourhood of any source point. Candidates =
  nearby groups whose name ranks (R033) and that have some pair of points within
  `maximum_distance_metres` (haversine, R = 6 371 000 m). Keep those with the minimum rank.
  Exactly one → match, method `name_geo_unique`, distance = minimum pairwise distance. More →
  `stop_match_ambiguous` ("<n> name/geography candidates") and **no further tier**. None → R043.
- **Where**: `StopMatching.fs` 83–117.
- **Pinned by**: `Configured radius finds a unique stop beyond the jdf:service:old 125 metre
  limit`; `Wider radius does not merge a stop with a different facility prefix`.
- **Constants**: 300 m (policy `stop_match.maximum_distance_metres`).
- **Diagnostic**: `stop_match_ambiguous`.
- **Proposed verdict**: port as tier 2 using `jr_model::geo` distance.
- **Why**: BASE_PLAN 14.1 point 5.

#### OVL-R043 Coordinate identity tier

- **What**: only when `coordinate_identity_maximum_metres > 0` and the source group has points:
  nearby groups with minimum pairwise distance ≤ that value, sorted by (distance, group ID). One →
  match; several → match the first iff runner-up − best ≥ `coordinate_identity_minimum_margin_metres`;
  method `coordinate_identity_unique`, diagnostic `stop_coordinate_identity_selected`
  (`<source name> -> <target name> at <d:F1> m`). Candidates without margin →
  `stop_match_ambiguous` ("No coordinate candidate has the required <m:F1> m margin"); none →
  `stop_match_unresolved`.
- **Where**: `StopMatching.fs` 118–147.
- **Why**: IDS JMK names differ a lot from CIS names (test name "verbose IDS JMK forms").
- **Pinned by**: `Combined …` (`coordinate_identity_unique`).
- **Constants**: 25 m and 10 m (IDS JMK profile; applied to PID by the merge, B2).
- **Diagnostic**: three codes above.
- **Proposed verdict**: port as tier 3, per source.

#### OVL-R044 Contextual stop inference

- **What**: if enabled, repeat until a pass accepts nothing: for every eligible source trip active
  in the window with calls: unresolved groups = groups of its calls without a match; require
  `1 ≤ distinct unresolved ≤ max(1, maximum_unresolved_groups_per_trip)`, `calls − unresolved
  calls ≥ minimum_mapped_calls`, and each unresolved group called exactly once. Candidate base
  trips: on the source route's primary ∪ structural routes (R049–R051), **same call count**,
  whose service overlaps the source service on any date, and whose pattern equals the source's at
  every resolved position. Score with identity alignment (R069), keep all with the minimum rank
  (R070). For each unresolved group, if all best candidates have the same place at its ordinal →
  proposal (method `trip_context_unique`, call ordinal 1-based). Then R045. After each accepting
  pass, all source calls' places are refreshed.
- **Where**: `SourceAnalysis.fs` 229–325, 355–368.
- **Why**: "Place stop groups that names and coordinates left unresolved from the trips calling
  them".
- **Pinned by**: `Overlay enriches non-rail facts…` (`trip_context_unique`, context-inferred
  centroid).
- **Constants**: `minimum_mapped_calls` 1; `maximum_unresolved_groups_per_trip` 0→1 (candidate 2).
- **Diagnostic**: report `stop_context_inference.csv`; see R045.
- **Proposed verdict**: port as tier 4 (iterative fixed point; order-independent).

#### OVL-R045 Accepting a context proposal

- **What**: per group, distinct proposed targets: exactly one, existing in the base, and either
  approximate (R026) or name-compatible (R033) → accept (method `trip_context_gtfs_authoritative`
  for approximate targets, else `trip_context_unique`; distance recorded; if > maximum distance
  and not approximate → `stop_context_distance_warning`); one but incompatible name →
  `stop_context_name_conflict`; several → `stop_context_conflict`. No distance limit applies.
- **Where**: `SourceAnalysis.fs` 326–353.
- **Pinned by**: `PID name and geodata replace a context-matched approximate JDF stop`.
- **Constants**: maximum distance (warning only).
- **Diagnostic**: three codes above.
- **Proposed verdict**: port.
- **Why**: "Strong route/call context accepted a compatible established stop" (message).

#### OVL-R046 Approximate base stop correction or split

- **What**: after inference, group matches by target; for targets that are approximate: exactly
  one source group → correction (the target's output name becomes the source group name, R100;
  its coordinates the source centroid via R091), method gains `+gtfs_authoritative`,
  `stop_approximate_corrected`; several source groups → all of them lose their match (and member
  mappings), `stop_approximate_split`.
- **Where**: `SourceAnalysis.fs` 369–394.
- **Why**: "A provisional JDF row cannot collapse multiple distinct source places."
- **Pinned by**: `PID name and geodata replace a context-matched approximate JDF stop`.
- **Diagnostic**: `stop_approximate_corrected`, `stop_approximate_split`.
- **Proposed verdict**: port, keyed on estimated position (R026); emit `SetPlaceName` and
  `SetPlacePosition` with source `Overlay`.
- **Constants**: none.

#### OVL-R047 Source-native stop places

- **What**: for every eligible trip allowed source-native (R014) and active in the window, each
  call whose stop has no mapped place creates/uses a native place for its group: ID = the pinned
  registry `place_id` for `(source ID, group ID)` if present, else
  `overlay:<src>:stop-place:sha16(group ID)`; method `source_native`; all members map to it. This
  happens before trip-set authority and regardless of whether the trip is later projected; places
  never referenced by an output call are pruned at write time. The call records keep their stale
  place IDs (B14).
- **Where**: `SourceAnalysis.fs` `nativeStopPlaces` 398–430; `RegionalGtfsOverlay.fs` 38–44.
- **Why**: "Stop places only the source knows, for trips it may run on its own."
- **Pinned by**: `Source-native mode imports route agency and unmatched stop place` (pinned and
  hashed IDs).
- **Constants**: none.
- **Diagnostic**: report `stop_group_matches.csv` status `native`.
- **Proposed verdict**: port; ID via `identity::overlay::stop_place` (section 4).

#### OVL-R048 Registry candidates

- **What**: with `--stop-registry-candidates`: one row per native place not pinned: `source_id,
  group_key, place_id, stop_name, lat, lon` (`0.000000` format), `reason = new`,
  `alias_of_suggestion` = `;`-joined sorted pinned place IDs of the same source whose key starts
  with the group's prefix up to its last `:` (only for `group:` keys); rows sorted by (source,
  group).
- **Where**: `SourceAnalysis.fs` 718–745; `RegionalGtfsOverlay.fs` 46–50.
- **Why**: "A group split by name suggests pinned places of the same source group, which is how a
  renamed stop keeps its ID."
- **Pinned by**: `Source-native mode imports route agency and unmatched stop place`.
- **Constants**: header `StopRegistry.overlayPlaceCandidatesHeader`.
- **Proposed verdict**: port (registry review output of the `jrutil` crate).
- **Diagnostic**: none.

### 3.7 Routes

#### OVL-R049 Companion assertion route tier

- **What**: a source trip's direct base routes = base routes of its asserted CIS line (R018) that
  exist and whose mode class is compatible (R036), distinct, sorted. A source route's direct routes
  = union over its trips. If non-empty and the tier is enabled → primary candidates with method
  `companion_assertion`.
- **Where**: `RouteMatching.fs` 78–93, 98–119.
- **Why**: BASE_PLAN 12.3 step 1.
- **Pinned by**: `Overlay enriches non-rail facts…`.
- **Proposed verdict**: port.
- **Constants / Diagnostic**: – / –.

#### OVL-R050 Structural route tier

- **What**: otherwise primary candidates = base routes of compatible mode whose
  `route_short_name` equals the source's exactly — **or every compatible-mode base route when the
  source label is blank** (B21) — with method `structural_trip_evidence` (empty if the tier is
  disabled).
- **Where**: `RouteMatching.fs` 103–109, 120–122.
- **Why**: BASE_PLAN 12.3 step 3 ("public designation").
- **Pinned by**: `Generic feed needs neither companion table nor revision identifiers`.
- **Proposed verdict**: port; a blank label yields no candidates (diagnosed).
- **Constants / Diagnostic**: – / –.

#### OVL-R051 Relaxed structural candidates

- **What**: for every source route, structural candidates = compatible-mode base routes whose label
  is structurally compatible (R037) (blank label → all). Used as the fallback candidate set in
  trip matching (R065) and context inference (R044), never for authority.
- **Where**: `RouteMatching.fs` 110–117.
- **Proposed verdict**: port.
- **Why / Pinned by / Constants / Diagnostic**: – / – / – / –.

#### OVL-R052 Unmatched routes

- **What**: a source route with no candidates is not an error; its trips get
  `trip_route_candidate_unresolved` (R073) or become source-native (R059). A base route loses
  nothing because no source route maps to it; it disappears from the output only when none of its
  trips survives (R098).
- **Where**: `TripMatching.fs` 309–310; `BundleGtfs.fs` 90–91.
- **Proposed verdict**: port.
- **Why / Pinned by / Constants / Diagnostic**: – / – / – / `trip_route_candidate_unresolved`.

### 3.8 Calls and signatures

#### OVL-R053 Base calls loaded for matching

- **What**: candidate base trips = trips on any primary or structural candidate route whose service
  has a window day. Their calls (sorted by sequence) carry `stopPlaceId` = `cz_stops.stop_place_id`
  of the stop or the stop ID. Per trip a `BaseSignature` (R055) with `pattern` (place IDs),
  `arrivals`, `departures` (seconds, −1 if blank).
- **Where**: `RouteMatching.fs` 125–180.
- **Proposed verdict**: replace by an index the pass derives from the `Timetable` (trip → place
  pattern, times).
- **Why / Pinned by / Constants / Diagnostic**: – / – / – / –.

#### OVL-R054 Source call place resolution

- **What**: a source call's place = the matched base place (or native place) of its stop, else the
  stop's own namespaced ID (never equal to a base place). Refreshed after context inference and
  after the approximate split; not after native places (B14).
- **Where**: `RouteMatching.fs` 182–202; `SourceAnalysis.fs` 221–226.
- **Proposed verdict**: port (unresolved = `None`, not a fake ID).
- **Why / Pinned by / Constants / Diagnostic**: – / – / – / –.

#### OVL-R055 Signatures

- **What**: `t(x)` = `floor(seconds(x) / resolution) × resolution` as text (`""` if blank); times
  `H:MM:SS` with any digit counts. `full` = SHA-256 of `"|" + Σ(place;t(arr)/t(dep);)`;
  `endpoints` = SHA-256 of `"|" + Σ(place;) + "|" + t(first dep) + "|" + t(last arr)`; `first` =
  the same without the last arrival. `patternKey` = SHA-256 of places joined by U+001F.
  `overlayEquivalenceDigest` = SHA-256 of `Σ(place/arrSeconds/depSeconds;)` (exact seconds).
- **Where**: `TripScoring.fs` 14–54.
- **Constants**: 60 s (policy `trip_match.time_resolution_seconds`).
- **Proposed verdict**: replace the hashes by direct comparison of interned patterns and rounded
  time vectors (same equivalence, no hashing).
- **Why / Pinned by / Diagnostic**: – / `Winner selection preserves tier precedence…` / –.

#### OVL-R056 After-midnight fold

- **What**: after loading source calls: a trip whose first call departs at ≥ 86 400 s moves to
  service `<service>@next-day` (its dates shifted +1 day; dates leaving the window are dropped)
  and every call time loses 24 h (`HH:MM:SS` re-formatted). Applies to every source.
- **Where**: `SourceAnalysis.fs` `foldAfterMidnightTrips` 83–120, 659–660.
- **Why**: "GTFS dates a trip by its service day, so PID runs Monday night's 00:30 tram as Monday
  24:30. JDF dates it by departure: Tuesday 00:30."
- **Pinned by**: `An after-midnight source trip meets its next-day CIS twin`; `An after-midnight
  source trip without a CIS twin is added at its next-day time`.
- **Constants**: 86 400.
- **Diagnostic**: log line only (`trips=<n>`).
- **Proposed verdict**: port as a base-family rule ("JDF dates a trip by its first departure"),
  not a source rule; handle a first call arriving before midnight (B13).

### 3.9 Trip-set authority

#### OVL-R057 Authority identity of a source trip

- **What**: (1) asserted CIS line (R018) → `(cis, preferRegular(direct routes))`; (2) else if the
  route's candidates are structural and `preferRegular` leaves exactly one route with a CIS line →
  `(that CIS, [route])`; (3) else if source-native allowed → `(source:<src>:<original route_id>,
  [])`; else none.
- **Where**: `SourceAnalysis.fs` 454–470.
- **Why**: "Decide on which CIS lines and dates the source owns the trip set".
- **Pinned by**: `Generic feed needs neither companion table…` (structural authority).
- **Constants**: prefix `source:`.
- **Proposed verdict**: port; a pseudo-line becomes a typed `AuthorityScope::SourceRoute`, not a
  string.
- **Diagnostic**: none.

#### OVL-R058 Regular routes preferred over detours

- **What**: `preferRegular routes` = routes not in the detour set, unless that is empty.
- **Where**: `SourceAnalysis.fs` 449–453.
- **Why**: "A CIS line can have a regular and a detour (výluka) base route. Source trips belong to
  the regular one; trip matching still considers both."
- **Pinned by**: `Detour base routes take source display fields but never line authority`.
- **Proposed verdict**: port (reads `TimetableKind::Detour`).
- **Constants / Diagnostic**: – / –.

#### OVL-R059 Target route of an authority trip

- **What**: one direct route of the same mode class → that base route; one direct route of a
  compatible but different class (trolleybus on a JDF bus route) → derived route
  `overlay:<src>:route:sha16(original route_id | cis)` recorded with the CIS line; otherwise, if
  source-native allowed → the same derived route; otherwise none → `trip_set_authority_withheld`
  ("The asserted CIS line has no compatible base route") and the line-dates are blocked.
- **Where**: `SourceAnalysis.fs` 487–507.
- **Why**: "Preserve the source mode on a derived route while retaining the proven national CIS
  identity".
- **Pinned by**: `Matched trolleybus slice retains its source-mode route`; `Source-native mode
  imports route agency and unmatched stop place`; `Bus slice of a long-distance CIS line keeps the
  CIS route`.
- **Diagnostic**: `trip_set_authority_withheld`.
- **Proposed verdict**: port (`AddRoute` with `identity::overlay::route`).
- **Constants**: none.

#### OVL-R060 Authority candidates and blocking

- **What**: for each eligible trip whose mode is authority-allowed and whose service has window
  days: identity and calls present and target route found and **every call's stop mapped** →
  candidate; target missing, unmapped call ("At least one source call has no resolved stop
  place"), no call pattern ("The active source trip has no call pattern") → block the line on all
  the trip's active dates; no identity → block every CIS line other trips of the same source route
  assert ("The active source trip has no unique companion CIS line assertion"). Each failure emits
  `trip_set_authority_withheld`. Because native places (R047) are created first, the "unmapped"
  failure only happens for modes that are not source-native (IDS JMK bus, tram, trolleybus).
- **Where**: `SourceAnalysis.fs` 471–522.
- **Why**: partial evidence must not silence national trips.
- **Pinned by**: `Trip-set authority emits complete source pattern and removes stale national
  instance`.
- **Diagnostic**: `trip_set_authority_withheld` (29 054 on the golden run).
- **Proposed verdict**: port.
- **Constants**: none.

#### OVL-R061 Authority dates per line

- **What**: `authority[cis]` = union of active dates of the line's candidates.
- **Where**: `SourceAnalysis.fs` 523–533.
- **Proposed verdict**: port (`DateSet` union).
- **Why / Pinned by / Constants / Diagnostic**: – / – / – / –.

#### OVL-R062 Integrated-system silence (PID system 30001)

- **What**: per scope (R020): lines = distinct sorted CIS lines of base routes filed under the
  system (LinExt), whose route mode class is in the scope's modes, not excluded, and that run on
  some date inside the window. Silenced = lines without authority dates. If silenced count >
  `maximum_silenced_lines` → `trip_set_authority_silence_withheld` (first 20 listed) and nothing
  changes for this scope; else every line gets authority on **every window date inside the feed
  window** (lines without dates get `trip_set_authority_silent`). So a line the source runs on
  some days becomes authoritative for the whole feed window.
- **Where**: `SourceAnalysis.fs` `extendIdsAuthority` 126–183.
- **Why**: "tram 16 (2026-10) is cancelled for trackworks, CIS still publishes it and PID simply
  leaves it out"; "The rule is withheld when it would silence more lines than this, so a truncated
  source cannot wipe out its region" (`Types.fs` 70–72).
- **Pinned by**: `A line of the source's integrated system that the source does not run is not
  running` (maximum 1 and 2).
- **Constants**: `30001`, `199041`, `199042`, 20 (PID profile).
- **Diagnostic**: `trip_set_authority_silent`, `trip_set_authority_silence_withheld`.
- **Proposed verdict**: port as named rule with profile data (Q7 on its window semantics).

#### OVL-R063 Blocked line-dates win

- **What**: after R062, every blocked `(cis, date)` is cleared from the authority dates.
- **Where**: `SourceAnalysis.fs` 535–538.
- **Proposed verdict**: port.
- **Why / Pinned by / Constants / Diagnostic**: – / – / – / –.

#### OVL-R064 Trip projections

- **What**: each candidate becomes a projection with dates = active ∧ authority[cis] (dropped if
  empty), keeping source ID, original trip reference, source and target routes, CIS line, calls,
  shape ID and the trip row.
- **Where**: `SourceAnalysis.fs` 539–563.
- **Proposed verdict**: port.
- **Why / Pinned by / Constants / Diagnostic**: – / – / – / log only.

### 3.10 Trip matching

#### OVL-R065 Candidate generation

- **What**: primary routes and method from R049/R050; structural routes = R051 minus primary.
  For each set: base trips on those routes (distinct); if some have exactly the source pattern
  (pattern index) only those are tried, else all. With `keep_source_calls` and a source pattern
  containing consecutive calls at one place, a folded view (consecutive equal places merged:
  first arrival, last departure) is also tried and used iff its fewest edits is strictly lower than
  the unfolded view's fewest edits (> 0). Matching never looks at agency, direction or block.
- **Where**: `TripMatching.fs` `tripCandidates` 181–219, `matchSourceTrip` 253–258;
  `TripScoring.fs` `collapseRepeatedPlaces` 107–120.
- **Why**: "The base usually has one call where the source calls several posts of a stop place."
- **Pinned by**: `A trip with a call the base trip lacks binds to it and keeps the call`.
- **Proposed verdict**: port.
- **Constants / Diagnostic**: – / –.

#### OVL-R066 Tier of a candidate

- **What**: for a base trip with signature: exact pattern and equal `full` and tier enabled →
  `full_signature`; exact and equal `endpoints` → `pattern_endpoints`; exact and equal `first` →
  `pattern_first`; exact and `exact_pattern_proximity` → `pattern_nearest`; else, with
  `keep_source_calls ∧ exact_pattern_proximity ∧ pattern_nearest` enabled, a superset alignment
  (R068) → `pattern_nearest` with edit count = extra calls; else an edit alignment (R067) →
  `pattern_edit_nearest`; else no candidate. Exact tiers use the identity alignment.
- **Where**: `TripMatching.fs` `candidateFor` 124–169.
- **Pinned by**: `Overlay enriches non-rail facts…`; `Winner selection preserves tier
  precedence equivalent ties and ambiguity`.
- **Constants**: tier names; policy `trip_match_tiers`, `exact_pattern_proximity`.
- **Proposed verdict**: port.
- **Why / Diagnostic**: – / –.

#### OVL-R067 Pattern edit alignment

- **What**: none if disabled or a side is empty, or `require_same_endpoints` and first/last places
  differ, or `|len diff| > maximum_edits`, or quick support `max(positional matches, common prefix
  + common suffix) < min(len) − maximum_edits`. Levenshtein (substitution, insertion, deletion
  cost 1); agreement = `1 − edits / max(len)`; none if edits > maximum or agreement < minimum.
  Backtrack preferring diagonal, then source deletion, then target insertion; a target call is
  aligned only on an equal diagonal step.
- **Where**: `TripScoring.fs` `patternEditAlignment` 58–102.
- **Constants**: 3 edits, 0.9 agreement, same endpoints (policy `trip_match.pattern_edit.*`).
- **Pinned by**: `Overlay enriches non-rail facts…` (edited pattern `st3`).
- **Proposed verdict**: port.
- **Why / Diagnostic**: – / report `pattern_edits.csv`.

#### OVL-R068 Superset alignment

- **What**: source has `extra = len(src) − len(target)` more calls, `0 < extra ≤ maximum_edits`,
  same first and last place; greedy: each target call except the last takes the first later source
  call with its place (never the last source call); the last target call takes the last source
  call. Fails if any target call is not found.
- **Where**: `TripScoring.fs` `supersetAlignment` 125–146.
- **Why**: "Bind a source trip to a base trip whose every call it also makes … and keep the
  source's further calls" (`Types.fs` 91–94).
- **Pinned by**: `A trip with a call the base trip lacks binds to it and keeps the call`.
- **Constants**: `maximum_edits` 3.
- **Proposed verdict**: port.
- **Diagnostic**: none.

#### OVL-R069 Time features

- **What**: first-departure delta = |first non-negative source departure − first non-negative
  target departure| (`Int32.MaxValue/4` if either missing); duration delta = |(last src arr −
  first src dep) − (last tgt arr − first tgt dep)| (same sentinel); over aligned pairs where both
  times exist: Σ|Δarr| + Σ|Δdep| (aggregate), max, Σ Δ² (both `Int64.MaxValue/4` when no pair);
  aligned count. Seconds, unrounded.
- **Where**: `TripScoring.fs` `candidateTimeScore` 152–184.
- **Constants**: the sentinels. **There is no maximum deviation anywhere** (B1).
- **Pinned by**: `Overlay enriches non-rail facts…` ("Aligned intermediate call times must
  resolve an edited-pattern tie").
- **Proposed verdict**: port features; add a policy maximum (Q1).
- **Why / Diagnostic**: – / report `trip_candidate_scores.csv`.

#### OVL-R070 Rank and margin

- **What**: rank = lexicographic `(edits, firstDelta, aggregate, duration, maximum, squared,
  −aligned)`. Runner-up margin = difference in the first differing component (aligned:
  best − runner).
- **Where**: `TripScoring.fs` 186–203.
- **Pinned by**: `Winner selection preserves tier precedence equivalent ties and ambiguity`.
- **Proposed verdict**: port.
- **Why / Constants / Diagnostic**: – / – / –.

#### OVL-R071 Per-date decision

- **What**: for every window date the source trip runs: candidates active that day; primary ones
  if any, else structural ones (method `structural_trip_evidence`). Keep the best tier (rank in
  `trip_match_tiers` order), sort by rank, take all equal to the first. If they fall into > 1
  equivalence classes (`cis:<line>` or `route:<id>` + exact call digest) → `Ambiguous`
  (`trip_same_date_ambiguous`, pending); else `Accepted`: **every** best candidate gets the date
  (`trip_equivalent_tie_expanded` when > 1), with the margin to the first strictly worse
  candidate.
- **Where**: `TripMatching.fs` `chooseCandidates` 57–67, `matchSourceTrip` 263–302.
- **Why**: BASE_PLAN 12.3 ("compare calendars").
- **Pinned by**: `Overlay enriches non-rail facts…` (`trip_equivalent_tie_expanded`,
  `jdf:trip:bt1dup`); `Winner selection preserves tier precedence…`.
- **Diagnostic**: `trip_same_date_ambiguous`, `trip_equivalent_tie_expanded`.
- **Proposed verdict**: port (Q1, Q15).
- **Constants**: none.

#### OVL-R072 Bindings, no one-to-one assignment

- **What**: accepted (candidate, method, tier, edits, deltas) keys accumulate dates; one
  `MatchBinding` per key with interned dates. Different source trips may bind the same base trip
  on the same date; nothing prevents it here (resolved per date in R082).
- **Where**: `TripMatching.fs` 286–298, 318–319.
- **Pinned by**: none.
- **Proposed verdict**: replace by a one-to-one assignment per date (Q1).
- **Why / Constants / Diagnostic**: – / – / –.

#### OVL-R073 Unmatched trip classification

- **What**: a source trip without any accepted date gets the first applicable code:
  `trip_stop_unresolved` (some call unmapped), `trip_route_candidate_unresolved` (no base trips on
  candidate routes), `trip_signature_unresolved` (no candidate under any tier),
  `base_snapshot_gap` (candidates exist, none active on a shared date; also recorded),
  `trip_validity_unresolved`. An active trip without parsed calls →
  `trip_call_pattern_unavailable`.
- **Where**: `TripMatching.fs` 305–317, 517–518.
- **Diagnostic**: the six codes.
- **Proposed verdict**: port as diagnostics.
- **Why / Pinned by / Constants**: – / – / –.

#### OVL-R074 Availability resolution

- **What**: claims = `(base trip, date) → source trips` from bindings, restricted to pending
  candidates. Repeat: for each pending `(source trip, date)` the candidates not claimed by another
  source trip; if exactly one, propose it; a proposal accepted iff it is the only proposal for that
  `(base trip, date)`; accepted → binding for that one date (margin none), candidate-score row with
  method suffix `+target_availability`, claim added, `trip_target_availability_resolved`. Stop
  when a pass accepts nothing.
- **Where**: `TripMatching.fs` `resolveByAvailability` 324–389.
- **Pinned by**: `Overlay enriches non-rail facts…` (`st4amb_251201` → `bt4b`).
- **Diagnostic**: `trip_target_availability_resolved`.
- **Proposed verdict**: port (or subsume in the assignment of Q1).
- **Why / Constants**: – / –.

#### OVL-R075 Capacity gap and ambiguous rows

- **What**: a remaining pending date whose candidates are all claimed by others →
  `base_snapshot_capacity_gap`; a trip with no binding and all pending dates such → capacity-gap
  trip. Every remaining candidate goes to `ambiguous_trip_candidates.csv`.
- **Where**: `TripMatching.fs` 391–415.
- **Diagnostic**: `base_snapshot_capacity_gap`.
- **Proposed verdict**: port.
- **Why / Pinned by / Constants**: – / – / –.

#### OVL-R076 Coverage counts

- **What**: source call counts per trip, totals, matched calls (trips with a binding or
  projection), and per mode the scheduled groups with a match and call totals, taken while indexes
  live.
- **Where**: `TripMatching.fs` `coverage` 418–482.
- **Proposed verdict**: port into the diagnostics summary.
- **Why / Pinned by / Constants / Diagnostic**: – / – / – / `coverage*.csv`.

### 3.11 Projection

#### OVL-R077 Capabilities and their minimum tiers

- **What**: capabilities ⊆ {stop_coordinates, boarding_points, call_boarding_points, shapes,
  route_short_name, route_long_name, route_color, route_text_color, transfers, stop_zones,
  schedules}; anything else is base-only. A binding may use capability `c` iff `c` has no minimum
  tier, or both the binding's tier and the minimum are in `trip_match_tiers` and the binding's
  index ≤ the minimum's index. A misspelt tier silently disables `c` (B18). Bindings of tier
  `context_stop_inference` cannot occur.
- **Where**: `Values.fs` 32–37; `Projection.fs` 52–61; `Support.fs` 63.
- **Why**: "Enabled capabilities; everything else is inherited from the base" (`Types.fs` 116).
- **Pinned by**: most integration tests.
- **Constants**: PID/IDS JMK minimums (section 2.3).
- **Proposed verdict**: port as per-source capability table with tiers; validate names.
- **Diagnostic**: none.

#### OVL-R078 Accepted source stops

- **What**: stops of source calls aligned to a target call, for bindings eligible for
  `call_boarding_points`; with `keep_source_calls` and a fully aligned binding, every mapped source
  call; plus every call of every authority projection.
- **Where**: `Projection.fs` 62–73.
- **Proposed verdict**: port (derived).
- **Why / Pinned by / Constants / Diagnostic**: – / – / – / –.

#### OVL-R079 Output boarding point of a source stop

- **What**: for accepted stops with a mapped place: `location_type 1` or not both
  `boarding_points` and `call_boarding_points` → the place itself; else if two or more sources'
  stops map to the same `place⟨US⟩upper(trim(platform_code))` → shared post
  `overlay:regional-all:post:sha16(that key)`; else `overlay:<src>:post:sha16(original stop_id)`.
- **Where**: `Projection.fs` 111–141.
- **Why**: BASE_PLAN 14 ("resolves or creates the post, binds the source post ID").
- **Pinned by**: `Overlay enriches non-rail facts…`; `Generic feed…` (`overlay:generic-gtfs:post:`).
- **Constants**: `regional-all`.
- **Proposed verdict**: port; posts become `AddBoardingPoint(kind = Platform/Post)`; the
  cross-source shared post becomes a match between sources' posts (Q5 on IDs).
- **Diagnostic**: report `post_pruning.csv`.

#### OVL-R080 Selection of a binding

- **What**: per target call (aligned ordinal or none): output stop = the source call's output
  boarding point (R079, else its place) when `call_boarding_points` eligible, else `""` (keep
  base). Distances = source `shape_dist_traveled` only if a valid overlay shape was selected
  (eligible for `shapes`) and all aligned distances are present and non-decreasing (else
  `shape_call_distance_invalid`). Arrivals/departures when `schedules` eligible: arrival of the
  first source call of the run; departure of its last (or of the call itself when repeats are
  kept). Repeats are kept iff schedules ∧ call boarding points ∧ every target call aligned ∧ fewer
  target calls than source calls ∧ first aligned source ordinal 0 ∧ ordinals strictly increasing ∧
  every source call mapped; then each source call between two aligned ones becomes a repeated call
  after the preceding target ordinal with its own output stop, times and (shape) distance.
  `factKey` = SHA-256 of `selectionEncoding` (section 4).
- **Where**: `Projection.fs` `selectionForBinding` 162–248.
- **Why**: "The source's further calls are kept only where it may state both where and when the
  trip calls…"
- **Pinned by**: `A trip with a call the base trip lacks binds to it and keeps the call`.
- **Diagnostic**: `shape_call_distance_invalid`.
- **Proposed verdict**: port; emits `SetCallBoardingPoint`, call-time edits and call insertion
  (new `Edit` kinds, Q4).
- **Constants**: none.

#### OVL-R081 Selection compatibility and merge

- **What**: two selections are compatible iff for every position the stop values are equal or one
  is `""`, shape IDs, distances, arrivals, departures equal or one absent, and repeat lists equal
  ignoring source ordinals. Merge: order by (source ID, source trip ID); take the first non-empty
  value per position; `sourceIds` sorted union; new fact key.
- **Where**: `Projection.fs` 252–276.
- **Proposed verdict**: port.
- **Why / Pinned by / Constants / Diagnostic**: – / `Combined …` (coalesced) / – / –.

#### OVL-R082 Per target-date claim resolution

- **What**: per base trip and date, distinct selections by fact key. All pairwise compatible →
  merged (`cross_source_fact_coalesced` if > 1 source). Else, if one source and exactly one claim
  with the newest revision (R015) → that claim (`overlay_newer_source_selected`). Else no claim
  wins (`cross_source_fact_conflict` or `overlay_fact_conflict`, "national value retained") and
  every claiming source trip counts as represented on that date — on a line-date under authority
  the base trip date is then retired as well (B23).
- **Where**: `Projection.fs` `resolveTarget` 293–331.
- **Why**: messages above.
- **Pinned by**: `Overlay enriches non-rail facts…`; `Equal or unresolved revisions quarantine
  conflicting schedules`; `Generic feed…`; `Combined …` (cross-source conflict keeps 09:00).
- **Diagnostic**: four codes above (4 020 `overlay_fact_conflict` and 318
  `overlay_newer_source_selected` on the golden run).
- **Proposed verdict**: port with B23 fixed (Q3).
- **Constants**: none.

#### OVL-R083 Incomplete authority bindings

- **What**: a binding of an authority-projected source trip that does not cover every source call
  (aligned + repeats = all calls) or lacks `schedules` or `call_boarding_points` eligibility
  removes its resolved dates (so the projection is added as its own trip instead).
- **Where**: `Projection.fs` 333–350.
- **Why**: "Partial evidence cannot represent a complete authoritative source instance."
- **Pinned by**: `Trip-set authority emits complete source pattern and removes stale national
  instance`.
- **Proposed verdict**: port.
- **Constants / Diagnostic**: – / –.

#### OVL-R084 One source instance, one base variant

- **What**: across base trips (processed in base trip ID order), the first resolved
  `(authority source trip, date)` wins; later base trips selected by the same source trip on that
  date lose the date (`equivalent_baseline_instance_removed`).
- **Where**: `Projection.fs` 351–365.
- **Why**: "One <source> instance … must not produce multiple baseline variants".
- **Pinned by**: `Authoritative service deduplicates baseline variants and scopes semantic
  evidence`.
- **Diagnostic**: `equivalent_baseline_instance_removed`.
- **Proposed verdict**: port.
- **Constants**: none.

#### OVL-R085 Represented dates

- **What**: a binding's source trip is represented on a date where its target has a resolved
  selection compatible with the binding's.
- **Where**: `Projection.fs` 366–372.
- **Proposed verdict**: port.
- **Why / Pinned by / Constants / Diagnostic**: – / – / – / –.

#### OVL-R086 Retiring national trips on owned line-dates

- **What**: a base trip's retained dates = active ∧ (¬authority[cis(route)] ∨ resolved selection on
  that date). A trip without retained dates disappears.
- **Where**: `Projection.fs` 375–381, 400–409.
- **Why**: authority semantics (`Types.fs` 75–76: "inside its feed window, such a line in one of
  its modes runs only what the source runs").
- **Pinned by**: `Trip-set authority emits complete source pattern…`; `A line of the source's
  integrated system…`.
- **Proposed verdict**: port (`SetTripService` with the reduced `DateSet`).
- **Constants / Diagnostic**: – / –.

#### OVL-R087 Date slices

- **What**: no selection on any retained date → one slice, same trip ID. Otherwise retained dates
  grouped by selection fact key (`base` for none); one group → same ID; several → each slice ID
  `splitTripId(base, dates, selectionEncoding or "base")`, slices sorted by ID. Every slice's
  service is re-interned `overlay:service:<16 hex>`; `shape_id` cleared unless an overlay shape.
- **Where**: `Projection.fs` 383–467; `OutputIds.fs` 15, 29–31.
- **Pinned by**: `Overlay enriches non-rail facts…` (two `jdf:trip:bt1:overlay:` slices);
  `Packed dates preserve canonical bits and identifiers across GVD boundaries`.
- **Proposed verdict**: port as trip split (`AddTrip` + `SetTripService`), IDs per Q5.
- **Why / Constants / Diagnostic**: – / – / –.

#### OVL-R088 Route switch to the source mode

- **What**: a slice whose selection comes from an authority projection whose source mode class
  differs from the base route's moves to the projection's target route (the derived route).
- **Where**: `Projection.fs` 444–451.
- **Pinned by**: `Matched trolleybus slice retains its source-mode route`.
- **Proposed verdict**: port (new `SetTripRoute` edit, Q4).
- **Why / Constants / Diagnostic**: R059 / – / –.

#### OVL-R089 Headsign from authority trips

- **What**: a slice whose selection comes from an authority projection takes headsign
  `fullHeadsign(source trip, −1, source trip_headsign)` (R092); other slices keep the base headsign.
  Stop headsigns of such slices' calls come from the source call (R093).
- **Where**: `Projection.fs` 454–460; `BundleCalls.fs` 40–44.
- **Pinned by**: none directly (`Overlay enriches non-rail facts…` asserts the base headsign is
  kept for a non-authority binding).
- **Proposed verdict**: port as an explicit capability `headsigns` (Q12).
- **Why / Constants / Diagnostic**: – / – / –.

#### OVL-R090 Source trip additions

- **What**: each projection's dates minus represented dates; empty → none. Group by
  `mode | route_short_name | dateKey | Σ(place@arr@dep;)`; representative = first by (source ID,
  original trip ID); `sourceIds` and trip references sorted unions. Trip = source trip row with ID
  `overlay:<src>:trip:sha20(src | original trip | dateKey)`, route = projection target route,
  service interned, shape = overlay shape or none, headsign = R092. Every member's source trip maps
  to the addition.
- **Where**: `Projection.fs` 472–529; `OutputIds.fs` 33–35.
- **Pinned by**: `Trip-set authority emits complete source pattern…` (equivalent trips → one
  output trip); `Source-native mode imports…`; `An after-midnight source trip without a CIS
  twin…`.
- **Diagnostic**: report `source_trip_additions.csv`.
- **Proposed verdict**: port (`AddTrip`).
- **Why / Constants**: – / –.

#### OVL-R091 Stop coordinates from source centroids

- **What**: with `stop_coordinates`: for every source group that is context-inferred or has an
  accepted member stop and is matched (incl. native), add its centroid to its target's list if
  within ±90/±180 (else `stop_coordinate_invalid`). At write time, exactly one distinct centroid
  replaces the base place's coordinates; more → `stop_coordinate_conflict` and base kept.
- **Where**: `Projection.fs` 533–553; `BundleGtfs.fs` 246–254.
- **Pinned by**: `Overlay enriches non-rail facts…` (context-inferred centroid); `PID name and
  geodata replace…`.
- **Diagnostic**: `stop_coordinate_invalid`, `stop_coordinate_conflict` (56 on golden).
- **Proposed verdict**: port (`SetPlacePosition`, source `Overlay`; fixes B4).
- **Why / Constants**: – / ±90, ±180.

#### OVL-R092 Full headsign of a source trip

- **What**: `fullHeadsign(trip, sequence, raw)`: terminal = last call with sequence ≥ `sequence`;
  its canonical name = correction name, else base group name of its place, else source group
  name, else `stop_name`. Blank raw → terminal name (`terminal_default`); R035 resolves raw against
  the terminal → terminal name (`terminal_stop`); else raw kept (`distinct_source_headsign`); no
  terminal → raw with `headsign_unresolved`. Each call appends a provenance row (with the audit-day
  flag).
- **Where**: `Projection.fs` 76–110.
- **Why**: "Expand a display-shortened terminal to the full canonical place name. Preserve text
  which is genuinely different (bilingual, via, or an intermediate destination)."
- **Pinned by**: `Source-native mode imports…` (`headsigns.csv`, `audit_day`).
- **Diagnostic**: `headsign_unresolved`; `headsigns.csv`.
- **Proposed verdict**: port (provenance as diagnostics only).
- **Constants**: none.

### 3.12 Output writing

#### OVL-R093 Base calls per slice

- **What**: base `stop_times` streamed; for a base trip with slices, each row is emitted once per
  slice (copied if > 1 slice) with `trip_id` = slice ID and, if the slice has a selection and the
  call's ordinal (position within the trip in file order) is within it: `stop_id` (non-empty),
  `shape_dist_traveled` (when a distance), arrival, departure, and stop headsign from the source
  call (authority trips). Base trips without slices are dropped.
- **Where**: `BundleCalls.fs` 63–77, 101–142.
- **Proposed verdict**: replace by edits on calls.
- **Why / Pinned by / Constants / Diagnostic**: – / integration tests / – / –.

#### OVL-R094 Repeated calls and resequencing

- **What**: in a slice with repeats, each base row becomes the base call followed by its repeats
  (clones of the base row, B9: kilometre, pickup/drop-off and stop headsign of the base row stay
  unless overwritten), with `stop_sequence = base sequence + repeats written so far + index`. The
  map `(slice trip, base sequence) → output sequences` feeds `cz_trip_stop_zones` (R105),
  `resequenced_calls.csv` (rows where the first output sequence differs) and base semantics
  (R111).
- **Where**: `BundleCalls.fs` 47–62, 118–138; `Reports.fs` 262–270.
- **Pinned by**: `A trip with a call the base trip lacks binds to it and keeps the call`
  (sequences 1, 2, 3; times `09:01:30,09:02:00`).
- **Proposed verdict**: port as a call-list replacement; sequences renumbered by the model.
- **Why / Constants / Diagnostic**: – / – / –.

#### OVL-R095 Added trip calls

- **What**: source `stop_times` streamed; rows of the representative source trip of an addition
  are emitted with `trip_id` = addition ID; arrival/departure from the projected (possibly folded)
  call with the same sequence; `stop_headsign` via R092 if non-blank; `stop_id` = output boarding
  point; `shape_dist_traveled` blanked without a shape. Only base-header columns survive; the
  source `stop_sequence` is kept.
- **Where**: `BundleCalls.fs` 78–95, 143–151.
- **Pinned by**: `An after-midnight source trip without a CIS twin is added at its next-day time`.
- **Proposed verdict**: port (`AddTrip` with calls).
- **Why / Constants / Diagnostic**: "Times come from the projected calls, not the raw row" / – / –.

#### OVL-R096 Output call order

- **What**: all output rows external-sorted by (trip ID ordinal, integer sequence), stable,
  64 MiB buffers, 128-way merge, spill files in a scratch next to the output.
- **Where**: `BundleCalls.fs` 157–178; `Scratch.fs` 68–127.
- **Pinned by**: `External sort handles interleaved groups quoted fields and many spill runs`.
- **Proposed verdict**: drop (in memory; the publisher orders).
- **Why / Constants / Diagnostic**: "Normalize native output once" / 64 MiB, 128 / –.

#### OVL-R097 Trips table

- **What**: base trips in base order (their slices), then additions sorted by ID; base trip
  header; routes of written trips marked used.
- **Where**: `BundleGtfs.fs` `writeTrips` 24–45.
- **Proposed verdict**: drop (publisher).
- **Why / Pinned by / Constants / Diagnostic**: – / – / – / –.

#### OVL-R098 Routes table

- **What**: used base routes cloned; for each route the source routes of bindings with a selected
  slice overlapping the binding's dates and of additions on it; for each enabled display
  capability, the distinct non-empty source values: one → set; several → `route_display_conflict`.
  Used native routes: base route header filled from the source route, ID = derived route,
  `agency_id` = `overlay:<src>:agency:sha16(original agency_id)`; missing source agency →
  exception. Sorted by route ID.
- **Where**: `BundleGtfs.fs` `writeRoutes` 48–130.
- **Pinned by**: `Detour base routes take source display fields…`; `Overlay enriches non-rail
  facts…`; `Source-native mode imports…`.
- **Diagnostic**: `route_display_conflict`.
- **Proposed verdict**: port (`SetRoutePresentation`, `AddRoute`, `AddAgency`).
- **Why / Constants**: – / –.

#### OVL-R099 Presentation rules after the overlay

- **What**: with rules: licence = `baseCisByRoute` or the native route's CIS line (non-blank;
  `source:*` pseudo-lines included); agency IČO **parsed** from `agency_id` by
  `^jdf:agency:([^:]+):[0-9]+$`; `RouteRules.presentRoute` changes are applied and each emits
  `route_presentation_override` (`<field>: <before> -> <after>`).
- **Where**: `BundleGtfs.fs` 131–153; `GeoData/RouteRules.fs` 89–91.
- **Why**: STATIC_PIPELINE: "applies them last so they win over regional feed values".
- **Pinned by**: `Reviewed presentation overrides win over source display fields`.
- **Diagnostic**: `route_presentation_override`.
- **Proposed verdict**: replace with the presentation pass after the overlay (jdf-convert Q9),
  reading IČO and `cis:line` from model fields.
- **Constants**: the regex.

#### OVL-R100 Stops table

- **What**: used stops plus their parents (base parent, or the mapped place of a new post);
  base rows cloned with the zone rule (R101), correction name (R046), centroid (R091); used native
  places (`location_type 1`, centroid `G29`); new posts (`location_type 0`, name = parent's name,
  coordinates with `boarding_points`, `parent_station`, `wheelchair_boarding`, `platform_code`).
  Distinct by stop ID (base first), sorted.
- **Where**: `BundleGtfs.fs` `writeStops` 158–263.
- **Pinned by**: `Overlay enriches non-rail facts…` (posts referenced by calls).
- **Proposed verdict**: replace by `AddPlace`/`AddBoardingPoint`/`Set*` edits; pruning is the
  publisher's.
- **Why / Constants / Diagnostic**: – / – / –.

#### OVL-R101 IDS JMK stop zones

- **What**: with `stop_zones`: for source groups whose source ID is the literal `ids-jmk-gtfs`, the
  matched place (or first member's mapped place) gets the sorted distinct non-blank member
  `zone_id`s. `stops.txt.zone_id` := the single distinct value of {existing, hashed regional IDs}
  else `""` (unpublished, B5). `cz_stop_zones.txt` gets per used place and code a row
  `(place, overlay:ids-jmk-gtfs:zone:sha16(code), code, "", ids-jmk, ids-jmk-gtfs)`. The writer
  publishes these as place zones applied to every passenger call at the place that has neither an
  explicit call zone nor a route-stop zone (B6).
- **Where**: `BundleGtfs.fs` 166–190; `BundleExtensions.fs` 105–132; `Serving/PackageWriter.fs`
  `zoneInputs`; `Serving/RouteStopWriter.fs` 130–145.
- **Pinned by**: `Combined …` (`ids-jmk` call zone system).
- **Constants**: `ids-jmk-gtfs`, `ids-jmk`.
- **Diagnostic**: none.
- **Proposed verdict**: port as a profile hook `zones { system = "ids-jmk" }`, scoped to calls of
  trips bound to the source (Q10).

#### OVL-R102 Agencies table

- **What**: base agencies with no ID or a used ID, plus native agencies; sorted by ID.
- **Where**: `BundleGtfs.fs` `writeAgencies` 266–282.
- **Proposed verdict**: drop (publisher prunes).
- **Why / Pinned by / Constants / Diagnostic**: – / – / – / –.

#### OVL-R103 Calendars and feed info

- **What**: `calendar_dates.txt` with one `exception_type 1` row per active day of every interned
  service (sorted by ID); no `calendar.txt`. `feed_info`: base rows with start/end = window and
  `feed_version = regional-overlay-v1-gvd-<year>`.
- **Where**: `BundleGtfs.fs` `writeCalendar` 285–305.
- **Proposed verdict**: drop (publisher derives calendars from `DateSet`s).
- **Why / Pinned by / Constants / Diagnostic**: – / – / `regional-overlay-v1-gvd-` / –.

#### OVL-R104 Czech extension rows

- **What**: `cz_routes`: used base rows + used native routes (`cis_line_id` incl. `source:*`,
  `public_line_number` = source short name, `source_provenance` = source ID). `cz_trips`: each base
  row once per slice (trip ID replaced); additions with `cis_line_id` (blank for `source:*`),
  `source_trip_ids = src:trip;…`, `coverage_sources`. `cz_stops`: used base rows, native places
  (`asw_id` = distinct group-column values, `source_ids`), new posts (`post_id`, `asw_id`,
  `source_ids`).
- **Where**: `BundleExtensions.fs` 36–103.
- **Pinned by**: `Trip-set authority emits complete source pattern…` (no fabricated CIS trip).
- **Proposed verdict**: replace by model keys: the added trip carries the `cis:line` key of its
  route only; coverage markers are dropped unless the architect wants them published (Q9).
- **Why / Constants / Diagnostic**: – / – / –.

#### OVL-R105 Call zones per slice

- **What**: base `cz_trip_stop_zones` rows copied per slice with the slice ID; with resequencing
  each row is emitted for every output sequence of its base sequence (repeats share zones).
- **Where**: `BundleExtensions.fs` 133–152.
- **Proposed verdict**: port implicitly (zones live on calls; a copied call keeps them).
- **Why / Pinned by / Constants / Diagnostic**: "Repeated calls of a stop place share its zones" /
  – / – / –.

#### OVL-R106 Pathways and levels

- **What**: never imported; counted in `exclusions.csv` (`hard_exclusion`).
- **Where**: `Reports.fs` 436–445.
- **Pinned by**: `Overlay enriches non-rail facts…`.
- **Proposed verdict**: port as "not read".
- **Why / Constants / Diagnostic**: – / – / –.

### 3.13 Transfers

#### OVL-R107 Base transfers onto slices

- **What**: every base transfer row: trip-bound sides become every slice of that trip (a trip
  without slices drops the row); the cartesian product of from × to slices is emitted **without
  date intersection** (B15); a row survives iff its stop IDs are used (route IDs unchecked).
- **Where**: `BundleTransfers.fs` 48–72.
- **Proposed verdict**: replace: transfers on trips follow the split with date intersection.
- **Why / Pinned by / Constants / Diagnostic**: – / – / – / –.

#### OVL-R108 Source transfers

- **What**: with `transfers`: stop-only rows are not projected (provenance
  `stop_only_not_projected`: "Standard GTFS cannot express date-bounded stop-only validity");
  rows with one trip side → `transfer_reference_unresolved`; with both: outputs per side = slices
  of `transfers`-eligible bindings (compatible selection, shared dates) plus the addition; none →
  `transfer_reference_unresolved`; for each output pair with shared dates and both output stops
  used → row with source `transfer_type`, times, provenance `trip_specific_projected`.
- **Where**: `BundleTransfers.fs` 74–133.
- **Pinned by**: `Overlay enriches non-rail facts…` (wait 300 in `transfer.parquet`).
- **Diagnostic**: `transfer_reference_unresolved` (69 460 on golden).
- **Proposed verdict**: port (`AddTransfer`).
- **Why / Constants**: – / –.

#### OVL-R109 Transfer reconciliation

- **What**: group rows by the six selectors (U+001F), sorted by key; one distinct
  `(transfer_type, min, max)` → keep; differing types → drop
  (`transfer_conflict_quarantined`); else merge: `min_transfer_time` = max, `max_waiting_time` =
  min of parseable values (`transfer_date_variants_merged`). Written sorted by (from trip, to trip,
  from stop, to stop).
- **Where**: `Support.fs` `reconcileTransferRows` 94–131; `BundleTransfers.fs` 134–140.
- **Why**: "so no published wait is longer than every source promises".
- **Pinned by**: `Projected transfer revisions merge conservatively or are quarantined`.
- **Diagnostic**: the two codes.
- **Proposed verdict**: port.
- **Constants**: none.

### 3.14 Shapes (dormant in production)

#### OVL-R110 Source shapes

- **What**: only with `shapes`: points sorted by (shape, sequence); per selected shape: ≥ 2 points,
  strictly increasing sequences, coordinates in range, distances all-or-none and non-decreasing,
  else `shape_invalid`; missing → `shape_missing`; ID `overlay:shape:sha20(points)` where the
  hashed text is `lat,lon,dist` per point (`G29`, `;`-joined). Selected for eligible bindings and
  projections; call distances gate R080.
- **Where**: `Shapes.fs`; `Projection.fs` 143–155; `BundleGtfs.fs` 308–324.
- **Why**: BASE_PLAN 17: "regional feeds' shapes are not taken".
- **Pinned by**: `Shape spool preserves canonical IDs with interleaved geometries`; `Unselected
  malformed shapes do not grant diagnostics or abort the overlay`; `Overlay enriches non-rail
  facts…` (`selectedShapes = 2`).
- **Diagnostic**: `shape_missing`, `shape_invalid`, `shape_call_distance_invalid`.
- **Proposed verdict**: do not port now (Q14).
- **Constants**: none.

### 3.15 Serving package (keys and base semantics)

#### OVL-R111 Base semantics through `base_to_output_trips.csv`

- **What**: mapping rows `(base trip, output trip, first date, last date)` = enclosing range of
  each slice. The writer copies base `service_note`; re-targets `assignment` (location-scoped kept
  if the location survives), `travel_restriction`, `connection_claim` to every output trip of their
  trip (IDs `jdf:assignment-projection:…`, `restriction-projection`, `connection-projection`
  = `feedId` digest of the original ID and target trip, when the trip changed; target trip of a
  connection = first projection); sequences moved by `resequenced_calls.csv`; base rows win over
  regenerated rows with the same key.
- **Where**: `Reports.fs` 69–87; `Serving/PackageBaseRelations.fs` 34–129;
  `Serving/PackageWriter.fs` 260–290.
- **Pinned by**: `Overlay projects native base semantics onto replacement trips`; `Authoritative
  service deduplicates baseline variants and scopes semantic evidence`.
- **Proposed verdict**: replace: semantics attached to trips/calls in the model follow the trip
  split automatically; the "projection" IDs are ledgered.
- **Why / Constants / Diagnostic**: comment ("Project public base semantics through the explicit
  base-to-output mapping") / `jrutil-serving-v2` digest prefix / –.

#### OVL-R112 Source trip keys

- **What**: `source_to_output_trips.csv`: per binding × compatible slice × contiguous date run,
  and per addition × reference × run: `(source, original trip, base trip or "", output trip,
  from, to, method, edits, deltas, margin)`. Published as `source_key` trip rows: namespace =
  source ID minus `-gtfs` + `:gtfs_trip_id`; identifier = original trip; public = output trip;
  rows where identifier equals output ID skipped; `binding_method` from method:
  `companion_assertion*` → `companion_assertion`, `structural*` → `structural_match`,
  `authoritative_source_trip_set` kept, empty → `source_native`.
- **Where**: `Reports.fs` 146–170; `Serving/PackageKeyRelations.fs` 67–81;
  `Serving/PackageRows.fs` 79–89.
- **Why**: BASE_PLAN 6.1 (source bindings).
- **Pinned by**: integration tests via traces.
- **Proposed verdict**: port (`AddKey`, namespace from the profile, method enum).
- **Constants**: `-gtfs`, namespace suffixes.
- **Diagnostic**: none.

#### OVL-R113 IDS JMK line/course keys

- **What**: each operational row × each output date range of its source trip → `source_key`
  `ids-jmk:line_course`, identifier `compositeKey [line; course]`, method `operator_crosswalk`.
- **Where**: `Reports.fs` 171–191; `Serving/PackageKeyRelations.fs` 83–98.
- **Pinned by**: `Combined …`.
- **Proposed verdict**: port.
- **Why / Constants / Diagnostic**: BASE_PLAN 6.1 / – / –.

#### OVL-R114 Route and stop keys

- **What**: `source_to_output_routes.csv` (bindings: source route → **base trip's** route, B7;
  additions: → addition route) and `source_to_output_stops.csv` (accepted stops whose output stop
  is used: → output boarding point). Published as `<sys>:gtfs_route_id` / `<sys>:gtfs_stop_id`
  over the whole package range, method `source_native` if the output ID starts with `overlay:`,
  else `structural_match`; rows with identical IDs skipped.
- **Where**: `Reports.fs` 88–96, 192–207; `Serving/PackageKeyRelations.fs` 128–138.
- **Proposed verdict**: port (fix B7).
- **Why / Pinned by / Constants / Diagnostic**: – / – / – / –.

#### OVL-R115 Base keys re-targeted

- **What**: base `source_key` rows outside `cis:line, cis:line_trip, czptt:*`: trip rows onto
  each slice clipped to the slice's enclosing range; route/location rows kept if the entity
  survives. National keys are re-derived from `cz_*` (R104).
- **Where**: `Serving/PackageKeyRelations.fs` 163–190.
- **Pinned by**: `Overlay slicing carries regional base keys and calls`.
- **Proposed verdict**: replace (keys live on model entities and follow splits).
- **Why / Constants / Diagnostic**: – / – / –.

#### OVL-R116 Call keys

- **What**: `source_to_output_calls.csv` maps (source trip, source sequence) → (output trip,
  1-based output position) — folded runs map to their output call, kept repeats to their own
  positions. The writer resolves positions to sorted output sequences and emits a `call_key`
  (`<sys>:gtfs_trip_id`) where source sequence ≠ output sequence. Base `call_key` rows follow each
  output trip that still holds the (unshifted, B8) sequence.
- **Where**: `Reports.fs` 209–260; `Serving/PackageKeyRelations.fs` 202–251.
- **Pinned by**: `A trip with a call the base trip lacks binds to it and keeps the call`;
  `Overlay slicing carries regional base keys and calls`.
- **Proposed verdict**: port (`AddCallKey`).
- **Why / Constants / Diagnostic**: – / – / exception on a position out of range.

#### OVL-R117 Location and route metadata inheritance

- **What**: without JDF sidecars, `location` metadata (municipality, district, okres, nearby
  place, country, precision, coordinate source) is read from the base `location` relation by ID
  (posts inherit their parent's); `coordinate_source = overlay` only for IDs starting
  `overlay:` without base metadata; route `timetable_kind` from the base by route ID, else
  `regular`.
- **Where**: `Serving/PackageGtfsRelations.fs` 83–132, 167–177.
- **Pinned by**: `Stop metadata and district codes reach the base and survive the overlay`.
- **Proposed verdict**: replace (fields of model entities; coordinate source set by the edit).
- **Why / Constants / Diagnostic**: "A regional overlay keeps base location ids" / `overlay:` / –.

#### OVL-R118 Manifest

- **What**: the overlay builds `bundle_format obehy-jrutil-regional-gtfs-overlay`,
  `bundle_version 2`, `audit_date`, `base_snapshot_date`, `base_manifest_sha256`, `sources`,
  `policy_sha256`, `gvd`, `conversion`; the package writer keeps only `conversion` (→ `compiler`),
  `gvd` (→ `service_horizon`) and `sources` merged after the base manifest's sources (B3).
- **Where**: `BundleWriter.fs` 135–150; `Serving/PackageWriter.fs` 155–186.
- **Pinned by**: `Combined …` (three sources, bundle 3); `Overlay enriches non-rail facts…`.
- **Proposed verdict**: replace with the publisher's manifest; record overlay provenance (policy
  hashes, audit date) explicitly if wanted (Q9).
- **Why / Constants / Diagnostic**: – / – / –.

### 3.16 Reports and diagnostics

#### OVL-R119 Mapping and report files

- **What**: `mappings/`: `base_to_output_trips`, `source_to_output_stops`, `source_to_output_trips`,
  `operational_to_source_trips`, `source_to_output_routes`, `source_to_output_calls`,
  `resequenced_calls` (if any). `reports/`: `post_pruning`, `stop_group_matches` (columns incl.
  `target_stable_digest` = SHA-256 of the stable name key [+ `|lat,lon`], status
  `quarantined|corrected|native|matched`, `geodata_disposition`
  `pid_name_and_coordinates|pid_native|none|matched_identity` for every source),
  `stop_context_inference`, `trip_candidate_scores`, `ambiguous_trip_candidates`, `pattern_edits`,
  `diagnostics`, `source_trip_additions`, `exclusions`, `headsigns`, `snapshot_day_coverage`,
  `coverage`, `trip_coverage_populations`, `coverage_by_mode`, `coverage_by_tier`,
  `coverage_by_source`, `trip_coverage_populations_by_mode`, `unmatched_trip_reasons`.
- **Where**: `Reports.fs`.
- **Proposed verdict**: replace mappings by model keys; keep the review reports as diagnostics
  rows or `jrutil explain` output (Q9).
- **Why / Pinned by / Constants / Diagnostic**: – / integration tests / – / –.

#### OVL-R120 Diagnostic suppression

- **What**: `diagnostics.csv` omits: `stop_match_unresolved`/`stop_match_ambiguous` for groups
  later matched; `trip_validity_unresolved` for trips matched after availability;
  `trip_same_date_ambiguous` whose `trip|<first 8 chars of the message>` (the date, **parsed from
  the message**) was resolved; route/signature/validity/ambiguity/gap codes for fully covered
  trips.
- **Where**: `Reports.fs` 391–414.
- **Pinned by**: `Overlay enriches non-rail facts…` (no ambiguous `st1_` rows).
- **Proposed verdict**: replace: emit a diagnostic only once the decision is final (no parsing).
- **Why / Constants / Diagnostic**: – / 8 / –.

#### OVL-R121 Unmatched trip reasons

- **What**: per unmatched-type diagnostic of a source trip: disposition
  `source_native_addition|authoritative_projection|matched_on_other_dates|withheld`; plus active
  trips without accepted dates and without such a diagnostic: `trip_projection_unrepresented` or
  `trip_unclassified_unmatched`.
- **Where**: `Reports.fs` 331–390.
- **Pinned by**: `Trip-set authority emits complete source pattern…`.
- **Proposed verdict**: port as diagnostics.
- **Why / Constants / Diagnostic**: – / – / –.

#### OVL-R122 Coverage figures

- **What**: accepted dates per source trip = shared dates of bindings with slices + addition
  dates. `coverage.csv`: route, stop (matched groups / groups), trip, trip_date, call, call_date,
  shape, transfer (projected rows / source rows); ratio 1.0 when total 0, six decimals. Per day and
  mode `snapshot_day_coverage.csv` with scope `audit_day`, `future_unverified` (after the snapshot
  date) or `other_date`. Populations: raw_active, shared_date_candidate, pattern_compatible,
  available_target_candidate (shared − capacity gap), base_snapshot_gap,
  base_snapshot_capacity_gap; per mode and per source (`bound_to_base_trip`,
  `added_as_source_trip`, `unrepresented`; stop groups `matched_to_base_place`, `source_native`,
  `unresolved`).
- **Where**: `Reports.fs` 272–657.
- **Proposed verdict**: port a smaller set into the diagnostics summary.
- **Why / Pinned by / Constants / Diagnostic**: – / `Trip-set authority…` ("trip" in diagnostics)
  / – / –.

#### OVL-R123 `diagnostics.json`

- **What**: every `diagnostics.csv` row counted by code (severity `warning`), ≤ 10 examples per code
  sorted by (object, message); the eight coverage CSVs embedded under `coverage_populations`.
- **Where**: `Serving/PackageWriter.fs` `writeDiagnosticsSummary` 68–117.
- **Proposed verdict**: replace by the `jr-publish` diagnostics writer.
- **Why / Pinned by / Constants / Diagnostic**: – / – / 10 / –.

### 3.17 Determinism and runtime

#### OVL-R124 Determinism

- **What**: the overlay is single-threaded (only file hashing uses `Array.Parallel`, ordered).
  Outputs are sorted or follow input order; dictionaries are iterated only in insertion order
  (with removals only in `stopGroupMatches`, which is never iterated into output). Decisions do not
  depend on source binding order (sources sorted), but do depend on source row order where "first"
  is taken (representative stop, `members.[0]` for source identity, base trip order for R084 is ID
  order).
- **Where**: throughout.
- **Pinned by**: `Overlay enriches non-rail facts…` (two runs byte-identical, inputs unchanged);
  `Combined …` (binding order).
- **Proposed verdict**: port; tests for thread-count independence once matching is parallel.
- **Why / Constants / Diagnostic**: AGENTS rule 7 / – / –.

#### OVL-R125 Memory boundaries and scratch

- **What**: `NoInlining` method boundaries, a forced compacting GC after analysis, clearing of
  indexes, `MemoryReclaimGate` at 3 250 000 000 bytes in the external sort, row logs on disk.
- **Where**: `RegionalGtfsOverlay.fs` 13–18; `Runtime.fs`; `Scratch.fs`; `BundleWriter.fs` 114–121.
- **Proposed verdict**: drop (AGENTS rule 8).
- **Why / Pinned by / Constants / Diagnostic**: memory / storage tests / 3.25 GB, 64 MiB / –.

---

## 4. Identity

### 4.1 IDs and keys the overlay creates

`dateKey(d)`: bytes = `ceil(window_days / 8)` bytes, bit `i` (day `i` of the window) at byte
`i / 8`, bit `i % 8` (LSB first); `dateKey` = lower-case hex SHA-256 of those bytes. The byte
length depends on the GVD window (46 bytes for GVD 2026), so the same calendar hashes differently
in another window and differently from `jr_model::DateSet::to_bytes` (fixed 48 bytes).

| ID | Format | Hashed text | Kind |
|---|---|---|---|
| service | `overlay:service:` + `dateKey[0..16]` | packed dates | F# internal (encoding) |
| split trip | `<base trip ID>:overlay:` + `sha12(base \| dateKey \| selectionEncoding or "base")` | see below | F# internal |
| added trip | `overlay:<src>:trip:` + `sha20(src \| original trip_id \| dateKey)` | | source ID + F# encoding |
| post | `overlay:<src>:post:` + `sha16(original stop_id)` | | source identifier, hashed |
| shared post | `overlay:regional-all:post:` + `sha16(place ID ⟨US⟩ PLATFORM)` | | F# internal (`regional-all`, place ID) |
| stop place | registry `place_id` or `overlay:<src>:stop-place:` + `sha16(group key)` | group key (4.2) | **registry-pinned** |
| route | `overlay:<src>:route:` + `sha16(original route_id \| cis)` | cis may be `source:<src>:<route>` | source identifier + F# pseudo-line |
| agency | `overlay:<src>:agency:` + `sha16(original agency_id)` | | source identifier, hashed |
| shape | `overlay:shape:` + `sha20(p₁;p₂;…)`, `pᵢ = lat,lon,dist` (`G29`, empty if absent) | | content |
| zone | `overlay:ids-jmk-gtfs:zone:` + `sha16(code)` | | unpublished |
| projected semantics | `jdf:{assignment,restriction,connection}-projection:` + 32 hex of `sha256("jrutil-serving-v2\n" + kind + "\n" + canonicalFields)` | original ID + target trip | F# internal |
| combined group | `group:<src>:group:<x>[:<name>]`, `<src>:<parent>` | | F# strings, registry keys |
| pseudo-line | `source:<src>:<original route_id>` | | F# internal, parsed back |
| equivalence key | `cis:<line>` or `route:<id>` + `|` + call digest | | internal |
| key namespaces | `<source ID without -gtfs>:gtfs_trip_id`, `:gtfs_route_id`, `:gtfs_stop_id`, `ids-jmk:line_course`, `ids-jmk:gtfs_trip_line_course` (R131) | | published contract |

`selectionEncoding` = `|`-joined: output stop IDs `;`-joined; output shape ID or empty; distances
(invariant decimal text) `;`-joined; arrivals `;`-joined; departures `;`-joined; and, only if there
are repeats, repeats `;`-joined each as `targetOrdinal@stop@arrival@departure@distance` ("Selections
without repeated calls keep the encoding they always had"). `factKey` = SHA-256 of it. Note the
split trip ID hashes the encoding, the slice grouping uses the fact key.

### 4.2 Registry keys (`overlay_places.csv`)

The registry row `(source_id, group_key)` is looked up as `source_id ⟨US⟩ group_key`, and
`group_key` is the group ID of R039 computed on the **combined** input:

- a stop with `parent_station` → `<source ID>:<parent stop_id>` (IDS JMK: `ids-jmk-gtfs:U1051N3249`);
- a PID flat group → `group:<source ID>:group:<asw_node_id>:<normalizeName(stop_name) joined by '-'>`
  (`group:pid-gtfs:group:1040:pivovarská`); PID stops without `asw_node_id` fall back to the parent
  or the stop ID inside the same pattern;
- an IDS JMK stop without parent → `group:ids-jmk-gtfs:group:<stop_id>:<name tokens>`.

The key therefore embeds: the namespacing of R012, the `group:` prefixes of R016 and R039, the
choice of grouping column, `split_flat_groups_by_name`, and `normalizeName`'s .NET
`ToLowerInvariant` + `Char.IsLetterOrDigit` (UTF-16) semantics. The default `place_id` is
`sha16(group_key)`. Either the Rust key builder reproduces these strings exactly, or the 119 rows
are migrated (re-keyed) in the same change; the `place_id` values themselves are public and must
not change.

### 4.3 Places where an ID or string is parsed

| Where | What is parsed |
|---|---|
| `PackageReader.prepareCompilerView` 178–182 | `cis:line_trip` identifier split at the first `:` into line and trip |
| `RouteRules.jdfAgencyIco` | IČO from `jdf:agency:<IČO>:<n>` (regex, unescaped) |
| `BundleExtensions` 69, `Reports` 342, `PackageKeyRelations` 25–28 | `cisLineId.StartsWith "source:"` / `isCisLine` to detect the pseudo-line |
| `PackageGtfsRelations` 132, 218, 380; `PackageKeyRelations` 133 | `id.StartsWith "overlay:"` → coordinate source `overlay`, shape generation `source`, binding method `source_native` |
| `PackageRows.sourceNamespace` | strips `-gtfs` from the source ID to build key namespaces |
| `PackageRows.bindingMethod` | prefix of the method string (`companion_assertion…`, `structural…`) |
| `Reports` 401–402 | date = first 8 characters of a diagnostic message |
| `MultiSourcePreparation.combinedStopGroupId` | `group:` / `stop:` prefix of a group ID |
| `SourceAnalysis.stopPlaceRegistryCandidates` 720–725 | group ID prefix up to its last `:` |
| `Projection` 540, `Reports` 124–135, `SourceAnalysis` 382 | method strings (`trip_context_*`, `+gtfs_authoritative`, `source_native`) |
| `InputPreparation` 139 | ` [?]` suffix of a stop name |

---

## 5. Source profiles

| Behaviour | PID | IDS JMK | Generic | Implemented as |
|---|---|---|---|---|
| Adapter name | `pid-v1` | `ids-jmk-v1` | not accepted | code list (`MultiSourcePreparation.loadPolicy`) |
| Revision ordering of trip variants | 6 digits from `trip_id` | none (all equal) | none | code branch `adapter = pid-v1` (R015); profile `source_revision` ignored |
| Stop group key | `asw_node_id` → parent → stop | parent → stop | parent → stop | code branch (R016); profile `group_column` ignored |
| Post key | `asw_stop_id` | stop ID | stop ID | code branch (R017) |
| CIS line claim | `route_sub_agencies.txt` by (route, sub-agency) | none | none | policy `route_join` (R018) |
| Operational crosswalk | none | `api.txt` line/course, strict | none | code branch `adapter = ids-jmk-v1` (R019); Oběhy download check |
| Route match tiers | companion + structural | structural | per policy | policy (unioned) |
| Coordinate identity tier | not configured (gets 25/10 m by merge) | 25 m / 10 m | per policy | policy (max-merged) |
| Overrides | `pid-stop-overrides.csv` | none | per policy | policy |
| Authority modes | bus, tram, metro, trolleybus, ferry | bus, tram, trolleybus, ferry | per policy | policy → trip flags |
| Source-native modes | same five | ferry | per policy | policy → trip flags |
| Integrated-system silence | system 30001, excluded 199041/199042, max 20 | none | per policy | policy → `overlay_ids_authority.txt` |
| Feed window | `feed_info` | calendar extent (no `feed_info`) | either | code (R020) |
| Stop zones | none | `zone_id` → system `ids-jmk` | none | capability `stop_zones` + literal `ids-jmk-gtfs` and `ids-jmk` (R101) |
| Unpadded times (`4:32:00`) | – | yes | – | generic parsing |
| After-midnight fold | yes | yes | yes | code, generic (really a JDF-base rule) |
| Heavy rail excluded | yes | yes | yes | hard-coded class + policy `excluded_route_types` |
| Key namespace | `pid:` | `ids-jmk:` | `<id minus -gtfs>:` | code (`sourceNamespace`); namespaces must be in the serving schema's closed list |
| Report labels | `pid_name_and_coordinates`, `pid_native` | same (mislabelled) | same | literals in `Reports` 132–133 |
| Keep source calls | candidate: yes | candidate: yes | per policy | policy (any-merged) |
| Maximum unresolved groups per trip | – (gets 2 in candidate) | candidate 2 | per policy | policy (max-merged) |
| Snapshot skew | 7 days | 7 days | per policy | policy |

Assumptions of a road/JDF base (to lift for the foreign-rail extension): the base speaks CIS
(`cis:line` keys, `cis:line_trip` parsing, CIS licence lines for authority and presentation), JDF
detour routes, LinExt integrated systems, ` [?]` estimated names, JDF departure-day dating
(R056), JDF tariff kilometres in `shape_dist_traveled`, bus/trolleybus compatibility, coach types
200–209, `feed = "jdf"`, heavy rail hard-excluded (R013), `jdf:agency:` parsing (R099), and the
names matcher's Czech CIS conventions (`rozc`, synonyms). A rail overlay needs: train-number
claims instead of CIS lines, platform/track boarding points instead of posts, calls extended past
the border (adding calls before/after the base trip, which R068 does not allow: endpoints must be
equal), and the rail mode classes.

---

## 6. Known or probable bugs

- **B1 No one-to-one assignment and no time tolerance.** `pattern_nearest` accepts any base trip
  with the same pattern on that date, however far apart in time (R069 has no maximum), and
  several source trips may pick the same base trip (R072). The collision is only seen in R082,
  which quarantines it: 4 020 `overlay_fact_conflict` events on the golden run. A source trip
  whose nearest base trip was taken by another source trip is not re-assigned to its own twin.
- **B2 Merged policy ignores per-source settings.** The template is the alphabetically first
  source (R011, R022): PID runs with IDS JMK's coordinate-identity tier (25 m / 10 m), the maximum
  `maximum_unresolved_groups_per_trip` (candidate: 2 for PID too), `keep_source_calls` of any
  source, the union of capabilities and route tiers; `group_column`, `post_column` and
  `source_revision` of the profiles are ignored; profile `trip_match_tiers` order is ignored.
- **B3 Overlay manifest fields are discarded.** `BundleWriter` builds `audit_date`,
  `base_snapshot_date`, `base_manifest_sha256`, `policy_sha256`, `bundle_format`; the package
  writer keeps none of them (golden manifest has no such key).
- **B4 `coordinate_source` not updated.** A base place whose coordinates are replaced by a PID
  centroid (R091) keeps the base metadata's `coordinate_source` and precision (R117).
- **B5 `stops.txt zone_id` is dead output and loses data.** `setUniqueZone` blanks the zone when the
  base value and the regional value differ; the published `location` has no zone, and the hashed
  `overlay:ids-jmk-gtfs:zone:*` IDs are written nowhere public.
- **B6 IDS JMK zones leak to every trip.** Place zones apply to every call at the place lacking an
  explicit or route-stop zone, including PID and national trips that IDS JMK does not run (R101).
- **B7 Route keys point at the base route of moved slices.** `source_to_output_routes.csv` uses the
  bound base trip's route even when the slice moved to a derived route (R088), and includes
  bindings whose dates were all quarantined; the `gtfs_route_id` key may target a route without
  that service or a pruned route.
- **B8 Base `call_key` ignores resequencing.** `keyRelations.baseCalls` uses the base sequence,
  while base calls after a kept repeat moved to later sequences (`PackageBaseRelations` applies
  `resequenced_calls.csv` to semantics but not to keys).
- **B9 Repeated calls clone the base row.** A kept source call inherits the base call's JDF
  kilometre (`shape_dist_traveled` → `source_kilometre`), pickup/drop-off types and stop headsign
  (`BundleCalls.repeatedCalls`).
- **B10 Separator-less join key.** `writeRouteJoin` concatenates `route_id` and `sub_agency_id`
  with `""`, so `L1`+`23` and `L12`+`3` collide.
- **B11 Failed overrides fall through.** An override whose target is missing, ambiguous or in an
  unsupported namespace leaves the group to automatic matching instead of quarantining it (R040).
- **B12 Synonyms loaded from the compile-time source path.** `StopMatcher.synonymsFile =
  __SOURCE_DIRECTORY__ + "/StopMatcherSynonyms.txt"`; on a machine without the source tree the
  expansion vocabulary silently shrinks to five pairs and stop matching changes.
- **B13 After-midnight fold of a call arriving before midnight.** A first call arriving 23:59:50 and
  departing 24:00:10 becomes arrival `00:00:-10` (negative seconds formatted with `D2`); dates
  shifted past the window end vanish without a diagnostic.
- **B14 Stale place IDs after native places.** `nativeStopPlaces` does not refresh the source calls,
  so projections carry the namespaced source stop ID as `stopPlaceId` (addition dedup key and
  headsign destination lookup use it).
- **B15 Base transfers across disjoint slices.** R107 emits the cartesian product of the from- and
  to-trip slices without intersecting their dates, and keeps rows whose route IDs were pruned.
- **B16 Duplicated headsign provenance.** `fullHeadsign` logs a row per call per slice write and per
  trip, so `headsigns.csv` repeats rows.
- **B17 Stale audit scripts.** `audit_regional_overlay_references.py` reads `extensions/`,
  `serving/source_trip_map`, `source_call_map`, `source_trip_coverage`;
  `audit_duplicate_journeys.py` reads `serving/source_trip_map` with `trip_namespace
  operational_line_course`. None exists in serving v5 (keys are `source_key`/`call_key`), so both
  scripts fail on current packages. What they check: every GTFS/extension reference resolves; no
  IDS JMK output trip duplicates another output journey on a date (first/last minute ± 3, endpoints
  ≤ 200 m, ≥ 2 matched calls within 180 s and 150 m, coverage ≥ 0.75, max delta ≤ 120 s, unless
  `api.txt` gives both disjoint line/courses).
- **B18 Unknown capability tiers disable silently.** A `minimum_capability_tier` value that is not a
  tier makes `bindingEligible` false for every binding (R077); keys are not validated.
- **B19 Override validity is all-or-nothing.** An override valid for part of the window applies to
  the whole window (R040).
- **B20 Hard failures for data problems.** A malformed `api.txt` line, an `api.txt` missing one trip,
  a source-native route with a missing agency, or a trip referencing a missing route abort the
  whole overlay (both sources), against BASE_PLAN 15.3.
- **B21 Blank route label matches everything.** R050/R051 make every compatible-mode base route a
  candidate for a source route with an empty `route_short_name` (cost, false matches).
- **B22 `geodata_disposition` mislabels IDS JMK.** Report labels say `pid_*` for any source.
- **B23 Conflict on an owned line-date removes the trip entirely.** In R082 a quarantined conflict
  marks the claiming source trips represented, and in R086 the base trip loses that date because the
  line is authoritative and no selection resolved. Neither the national trip nor a source trip runs
  that day, although the diagnostic says "national value retained". PID owns its lines, so this hits
  PID schedule conflicts (part of the 4 020 events).


---

## 7. Documentation versus code

- **Overlay levels.** STATIC_PIPELINE "Overlay policy" and BASE_PLAN 12.1 specify, per source,
  mode, coverage scope and capability, an integer priority and one of `disabled`, `fill_missing`,
  `preferred`, `authoritative`. The code has a flat capability list per (merged) policy plus a
  minimum matching tier per capability (R077); no priorities, no `fill_missing`/`preferred`, no
  per-mode or per-scope settings. "Authority" in the code is something else: trip-set authority
  over CIS line-dates (R057–R064, R086).
- **"Omission is not deletion"** (STATIC_PIPELINE) and "a regional feed that omits a field makes no
  deletion claim" (BASE_PLAN 12) versus trip-set authority, which retires national trips the source
  does not run, and the integrated-system silence (R062), which withdraws whole lines.
- **"It does not replace entire trips merely because a matching trip exists"** (BASE_PLAN 12) versus
  `schedules`, call replacement with repeats, headsign and route switch, and authority additions.
- **First fixture "PID bus slice supplying exact posts only. National times, names and colours remain
  selected"** (STATIC_PIPELINE) versus production profiles enabling `schedules` and the four route
  display capabilities for every mode.
- **`Values.capabilityNames` comment**: "Stop names, headsigns, trip short names, calendars and
  agencies always stay with the base." Stop names change (R046), headsigns change for authority
  slices (R089), calendars are split and retired (R086, R087), agencies are added (R098).
- **Trips dedup by CIS line + CIS trip** (BASE_PLAN 12.2) versus structural matching; CIS trip
  numbers are never used, only CIS lines.
- **"Names, public line numbers and zero-padding may generate candidates but never establish a CIS
  identity on their own"** (BASE_PLAN 12.3) versus R057(2): a unique structural route by public
  line number gives IDS JMK trips a CIS line and line authority.
- **"If ambiguity remains, trip-specific replacement stays quarantined"** (BASE_PLAN 12.3) versus
  `trip_equivalent_tie_expanded`: one source trip enriches several base trips judged equivalent.
- **Matching order step 2 "an already reviewed source binding"** (BASE_PLAN 12.3) is not
  implemented (no binding memory besides `overlay_places.csv` for native stops).
- **`api.txt` as a candidate relation** (BASE_PLAN 6.1: "the same key can map to several rows";
  "quarantine what stays ambiguous") versus a strict all-or-nothing parser (R019).
- **"One broken upstream source must not destroy the nationwide feeds"; failing overlays are
  omitted and diagnosed** (BASE_PLAN 15.3) versus R007, R008, R019, R098 aborting the run; there
  is no last-known-good or per-source omission in JrUtil.
- **"Do not implement an implicit 'PID wins everything' rule"** (BASE_PLAN 12.1): the union of
  capabilities applies every capability to every source (B2).
- **BASE_PLAN 14.2 "coverage never limits the national stop universe"**: holds for posts, not for
  zones (B6).
- **BASE_PLAN 17 / STATIC_PIPELINE "Every shape is generated: the overlay policies leave the regional
  feeds' shapes capability off"**: true of the policies; the shapes code (R110) and tests remain.
- **README of `jrunify-ext-geodata`** says a native place's default ID changes on rename; true, and
  the registry pins the hash (2.4); the README does not say the key embeds normalised names.
- **`RegionalGtfsOverlay.compile` doc** "The sources are merged into one namespaced input and resolved
  in a single pass" — matches.
- **BundleWriter comment** "The production base is read in place until finalization; its serving
  payloads are never duplicated into compiler scratch" versus `prepareCompilerView`, which extracts
  and rewrites the base GTFS into scratch.

---

## 8. Duplicated logic

| Concept | Implementations |
|---|---|
| Name normalisation | `Names.normalizeName` and `Names.nameTokens` (overlay); `Support.resolveFullHeadsignDetailed` own tokeniser `[\p{L}\p{N}]+` and its own abbreviation table; `GeoData/StopMatcher` (Lucene synonyms, same file); `CzPtt/CzPttCoordinates.normalizedName` (FormD transliteration); JDF registry name keys (jdf-merge) |
| Abbreviation vocabularies | `Names.overlayExpansions`, `StopMatcherSynonyms.txt`, `resolveFullHeadsignDetailed.tokenMatches` |
| Distance | `Core/Geo.haversineMetres` (used by overlay and CZPTT); Python copy in `audit_duplicate_journeys.py` |
| Spatial index | `InputPreparation` 0.002° grid; other grids in posts/CZPTT code |
| Mode classifier | `GtfsFiles.modeClass` (overlay classes); `PackageBaseRelations.servingMode` (serving modes, different ranges: cable, `12`, defaults to bus); `JdfPostPolicyEvaluation.modeClass`; JDF transport-mode rules |
| Calendars / date sets | `DateSet.Dates` (packed, SHA-keyed), raw `bool[]` windows (authority, slicing), `System.Collections.BitArray` (Reports, claims), `DateUtils.DateBitmap`, `GtfsRows.parseCalendar` versus the JDF calendar code |
| Time parsing | `TripScoring.timeSeconds`, `matchingTimeNormalizer` (int64, throws differently), `PackageRows.seconds`, the formatter in `foldAfterMidnightTrips` |
| CSV reading | `GtfsFiles.csvFields/csvRows/csvValues` (overlay, also used by Serving), `Gtfs/GtfsParser.fs`, `PackageReader.writeCsv` + `GtfsFiles.writeCsvRow` |
| External sort / dedup | `Scratch.sortRowsBy`, `Serving/HashDedup` |
| Hashing helpers | `Hashing.sha256Text`, `Values.sha256Tree`, `DateSet` key, `Identity.digest` |
| Interval logic | `Reports.dateRanges` (runs), enclosing range for base mappings, `PackageKeyRelations` clipping |
| Route label normalisation | `Names.structurallyCompatibleRouteLabels`; `RouteRules` licence handling |
| Transfer selector key | `Support.TransferSelectorColumns` and `PackageReader` (same six columns, U+001F) |
| Headsign / name resolution | `Projection.canonicalDestinationName` versus JDF headsign rules |

---

## 9. Performance-relevant structure

Recorded timings (`work/overlay-golden/perf/*.json`, 8 jobs, golden subset, PID + IDS JMK):
overlay 181.1 s / 7.10 GB peak private (baseline), 251.9 s / 7.09 GB (candidate),
197.5 s / 7.36 GB (candidate2), 181.6 s / 6.92 GB (cleanup). For comparison on the same subset:
`fix` 60.9 s / 3.15 GB, `merge` 34.8 s / 0.89 GB, `bundle` 37.4 s / 0.90 GB. The overlay is the
most expensive stage by far.

What dominates, from the code (nothing measured here):

- **Text round trips.** Sources are read, rewritten and re-hashed in full (stage 5, 10); the base
  package is decoded from Parquet and `gtfs.zip`, rewritten as CSV (stage 12), and finally the
  overlay output is written as CSV and re-parsed by the package writer (stage 35). `csvRows` builds
  a `Dictionary<string,string>` per row for every table except `stop_times` hot paths.
- **Repeated scans** of `stop_times` (section 1: five source passes, four base passes in the overlay
  plus the writer's) and of small tables (`routes`, `trips`, `transfers` re-read in Reports).
- **Hashing.** Three SHA-256 signatures + one digest per candidate base trip and per source view,
  one SHA-256 per selection fact key, SHA-256 per interned date set, per ID; hashing of both source
  ZIPs and of the whole combined directory.
- **Per-date loops.** `matchSourceTrip` loops over all window days (~364) and re-filters every
  candidate list per day; slicing loops over all base trips × days with `Seq.mapi` and allocation;
  coverage reports loop over trips × days several times.
- **Candidate blow-up.** Blank labels make all compatible routes candidates (B21); without an exact
  pattern hit, every base trip of every candidate route is aligned (Levenshtein up to 3 edits per
  pair).
- **Quadratic spots.** `BundleGtfs.isUsedPlace` scans all of `outputStopForSource` per regional
  zone place; `Projection.compatibleSelections` over `Array.allPairs` per target date; F# `Map`
  updates per member in context inference and native places.
- **External sort** of all output calls through disk, and of shapes when enabled.
- **Memory**: national `BaseSignature`s, `CallValue` arrays with string fields, interned strings,
  and the 7 GB peak is managed with forced GCs and method boundaries.
- **Validation twice** of the base package.

The Rust plan (in-memory `Timetable`, dense handles, `DateSet` bitsets, no text hand-offs) removes
all of the first four items by construction.

---

## 10. Open questions for the architect

- **Q1 Trip assignment.** (a) Port F#: per date nearest candidate, no tolerance, collisions
  quarantined (B1). (b) Per (line, date) one-to-one assignment (min-cost on the rank tuple) with a
  policy maximum first-departure deviation and the availability pass subsumed. Recommendation: (b);
  ledger the difference category "assignment" and judge on the golden subset.
- **Q2 Policy merge.** (a) Reproduce the merged template (B2). (b) Each source profile governs its
  own stops, routes and trips. Recommendation: (b), ledgered.
- **Q3 Conflict on an owned line-date (B23).** (a) Port (trip vanishes). (b) Keep the national trip
  on that date. (c) Add the source trip. Recommendation: (b), matching the diagnostic's promise.
- **Q4 Edit vocabulary.** The overlay needs edits the vocabulary lacks: replace a trip's calls
  (times and inserted calls), `SetTripRoute`, `SetTripHeadsign`, `SetCallHeadsign`, a call-zone or
  place-zone claim, and trip splitting. Options: (a) one `ReplaceTripCalls { trip, calls }` plus
  `SetTripRoute` and `SetTripHeadsign`; split = `AddTrip` (copy) + `SetTripService`. (b) Fine-grained
  `SetCallTimes`/`InsertCall`. Recommendation: (a): fewer kinds, and precedence on whole call lists
  is what the overlay decides anyway.
- **Q5 Overlay IDs.** Service, split-trip and added-trip IDs embed the window-length date packing,
  `selectionEncoding` and `regional-all`. (a) Reproduce exactly. (b) Change once: services from
  `identity::service`, split trips `<base>:overlay:<hash of DateSet bytes>`, added trips
  `overlay:<src>:trip:<hash(src, trip, DateSet bytes)>`, posts unchanged
  (`overlay:<src>:post:sha16(stop_id)`), routes and agencies unchanged. Recommendation: (b); keep
  post, route, agency and stop-place IDs exact (they are keyed on source identifiers and the
  registry), compare trips by schedule.
- **Q6 Registry keys.** (a) Build `group_key` exactly as F# does (section 4.2) in
  `identity::overlay::place_group_key`. (b) Migrate the 119 rows to a cleaner key
  (`<source>:<asw_node_id or parent>[:<name>]`) in the same change. Recommendation: (a) now — the key
  is data the reviewers already hold; revisit with the registry tooling.
- **Q7 Authority windows.** Trip-set authority covers the dates the source's complete trips run; the
  integrated-system rule extends it to the whole feed window, also for lines the source runs on some
  days. (a) Port. (b) Restrict the extension to silent lines. Recommendation: (a) (it is what removes
  stale national trips), as named rules with the maximum and exclusions in the PID profile.
- **Q8 Base-family coupling.** The after-midnight fold, bus/trolleybus compatibility, detour
  preference and the estimated-stop correction are facts of a JDF base. (a) Keep them in the engine.
  (b) Move them to a "base profile" (road vs rail) the engine reads. Recommendation: (b), so the
  foreign-rail overlay reuses the engine.
- **Q9 Reports, mappings and markers.** Mappings become keys; which review reports stay
  (`stop_group_matches`, `unmatched_trip_reasons`, `headsigns`, coverage), and should coverage
  markers (`coverage_sources`) or the overlay provenance (policy hashes, audit date) be published?
  Recommendation: keep the review reports as diagnostics artifacts, publish policy hashes in the
  manifest, drop unpublished `cz_*` markers.
- **Q10 Zones.** (a) Port place-level IDS JMK zones for every call (B6). (b) Zones only on calls of
  trips bound to or added from IDS JMK. Recommendation: (b).
- **Q11 Failure policy.** (a) Port hard failures. (b) A data error in one source omits that source
  with a diagnostic and the build continues (BASE_PLAN 15.3); checksum and policy errors stay fatal.
  Recommendation: (b) for `api.txt`, agencies and references; (a) for checksums and policies.
- **Q12 Headsigns.** Authority slices take the source headsign without a capability. (a) Port as is.
  (b) A `headsigns` capability in the profile. Recommendation: (b).
- **Q13 Coordinate provenance (B4).** Recommendation: replaced coordinates get `CoordSource::Overlay`
  and exact precision (fix, ledgered).
- **Q14 Source shapes.** Off in production; code and tests exist. (a) Port. (b) Do not port; record
  R110 as dropped and revisit with the foreign-rail extension. Recommendation: (b).
- **Q15 Equivalent ties.** One source trip enriching several semantically identical base trips
  (`trip_equivalent_tie_expanded`) versus BASE_PLAN 12.3. Recommendation: port (they are national
  duplicates; R084 already dedups under authority) and amend BASE_PLAN.

---

## 11. Rules added by the rewrite

#### OVL-R130 Stated times that would run backwards are not taken

- **What**: a part of a base trip whose selection (R080) would give calls whose times run
  backwards (source times at aligned calls next to base times at the others, or further calls
  between them) keeps the base trip's times and takes no further calls; its boarding points and
  stop headsigns are still taken. Diagnostic `overlay_schedule_not_monotonic`.
- **Where**: `crates/jr-overlay/src/projection/batch.rs`.
- **Why**: the timetable's invariant (`Timetable::check`) forbids a trip's times running
  backwards; F# wrote such calls to `stop_times.txt` unchecked.
- **Pinned by**: none on the golden slice yet (counted in the projection report).
- **Constants**: none.
- **Diagnostic**: `overlay_schedule_not_monotonic`.

#### OVL-R131 A crosswalked trip ID keys every trip of its line/course

- **What**: each crosswalk row (R019) of a source × the R112 runs of every source trip with the
  same line and course → `source_key` `<sys>:gtfs_trip_line_course`, identifier the row's GTFS
  trip ID, public the output trip of the run, validity the run, method `operator_crosswalk`.
  A trip ID thus resolves, on any date, to the trip its line/course runs that day.
- **Where**: `crates/jr-overlay/src/projection/keys.rs`.
- **Why**: IDS JMK's realtime (KORDIS GTFS-RT) names a run by the trip ID of one of its calendar
  variants, not always the one running that day (Oběhy `docs/sources/ids-jmk.md`, IDSJMK-Q9):
  on 2026-11-24, 131 of 511 running buses and trolleybuses named a variant of another date. The
  crosswalk resolves by date in the static build, where `api.txt` is (BASE_PLAN section 6.1),
  so the realtime lookup stays literal.
- **Pinned by**: `ovl_r131_a_crosswalked_trip_id_keys_every_trip_of_its_line_and_course`.
- **Constants**: the namespace suffix `gtfs_trip_line_course` (serving 5.3).
- **Diagnostic**: none.
