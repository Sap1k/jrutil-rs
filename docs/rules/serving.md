# Serving: production package inventory (F# reference)

Specification inventory of how the F# JrUtil writes a `jrutil-production` package, for the
Rust `jr-publish` rewrite. Sources read: `jrutil/src/Serving/*.fs`, `Core/Hashing.fs`,
`Core/Geo.fs`, `CzPtt/CzPttPackage.fs`, `CzPtt/CzPttBundle.fs` (sidecars), the parts of
`CzPtt/CzPttToGtfs.fs`, `CzPttEntities.fs`, `CzPttTransfers.fs`, `CzPttAgencies.fs`,
`Gtfs/Gtfs.fs` that produce the values the writer copies, `jrutil.tests/ServingContractTests.fs`,
`docs/*.md`, `contracts/*.json`, `route-shapes/` and the Oběhy consumers
(`repo/src/obehy/release/{contract,load}.py`, `production_package.py`, `route_shapes.py`,
`STATIC_PIPELINE.md`). Ground truth: the CZPTT package
`artifacts/fetched/20261012T080259Z-8f416ec58c3a/czptt` ("the rail package") and the overlay
package `work/overlay-golden/out/cleanup/overlay` ("the overlay package").

File references are relative to `E:/Git/obehy/jrutil/jrutil/src/` unless stated. "GTFS
column X" means a column of the compiler's intermediate GTFS table, which the rewrite does not
have; for each such column the source fact is named.

Conventions used below:

- *nonblank(x)*: F# `nullableString` = null when `String.IsNullOrWhiteSpace(x)`, else x
  (whitespace-only strings become null, not only empty ones).
- *ordinal*: `String.CompareOrdinal` / UTF-16 code-unit order. F# generic `compare`/`Seq.sort`
  on strings is also ordinal.
- *feedId(feed, kind, fields)*: the generated-ID digest scheme of section 6.1.

---

## Architect's verdicts

**After the switch (2026-11-24):** the limitations and post-switch changes recorded below are
decided in `docs/roadmap.md`, which takes precedence where the two differ.

**These override the "Proposed verdict" lines below.** Where a rule is not listed, its proposed
verdict stands. `serving-v5.json` (5.2) is the authoritative contract; `production-v3.json` is
corrected to say 5.2.

| Question | Decision |
|---|---|
| 1 | Port: CZPTT trips have no direction, so a line's route stops hold both directions. Recorded as a limitation. |
| 2 | Fix: an estimated position is published `estimated` with `route_time` or `route_end`. `country_code` is filled for every CZPTT location. Both go in the parity ledger. |
| 3 | Drop the `czptt:pa` call keys. |
| 4 | IDS coverage beyond zones is not computed or published. |
| 5 | Service IDs are derived from the service's dates. |
| 6 | The serving `transfer` relation holds the source fact (a timed transfer between two trips at two boarding points). The stop-pair, type 2, zero-minute form MOTIS needs is an option of the GTFS projection. Parity ledger. |
| 7 | A missing agency URL is null in serving. |
| 8 | Implement the line-up check: companion output that does not give one offset per match point is discarded with a diagnostic. |
| 9, 18 | Decided with the road compiler (M2). |
| 10 | Decided with the overlay (M4). CZPTT keeps `PID`-style system IDs from the KADR abbreviation. |
| 11 | As above: 5.2. |
| 12 | Generated IDs use the rewrite's own content digest (`jr_model::identity`). They are compared by content. |
| 13 | `build_spec_sha256` is the SHA-256 of a canonical JSON document: compiler version, the hash of every policy and data file used, and each input's ID and digest. |
| 14 | Decided with the road compiler (M2). |
| 15 | The compiler's diagnostics are summarised in `diagnostics.json` (`counts_by_code`, `examples_by_code`). |
| 16 | The `sr70:track` identifier uses the subsidiary code. The boarding point's `public_code` is the subsidiary name when there is one, else the code. |
| 17 | As implemented in `jr-publish::validate`: extras are accepted only for a newer minor. |
| 19 | Port: a railway point's `timepoint` is "has a time". |
| 20 | Relations are written sorted by primary key. Parity is judged by key and content. |
| 21 | Yes: the overlay reads the base `Timetable` in memory. |

Further: the manifest gains `compiler` for every package. Identifiers are never parsed (section
6.5): `run_key`, `run_part`, the PA and TR identifiers, the train number and the coordinate
source are model fields. Rows are deduplicated by construction, not by a hash pass
(section 1.5).

---

## 1. Package layout and determinism

### 1.1 Files

```text
<output>/
├── gtfs.zip
├── serving/<19 relations>.parquet     (every relation, also when empty)
├── manifest.json
└── diagnostics.json
```

The inventory is closed: `Validation.inspect` rejects any other file (section 11). The
writer (`PackageWriter.writePackage`) refuses an existing output path
(`"Production package output already exists"`) and a feed other than `jdf`/`czptt`
(`"Unknown package feed …"`). `CzPttPackage.write` writes into a sibling temporary directory
`.<name>.<guid>.tmp` (scratch `.<name>.<guid>.czptt`) and `Directory.Move`s it into place only
after validation succeeded; on failure the partial output is logged and deleted by the caller's
`finally`. The overlay (`RegionalOverlay/BundleWriter.fs`) and JDF (`Jdf/JdfBundle.fs`) do the
same through their own wrappers.

Writer phase order (`PackageWriter.writePackage`), which matters because later phases rewrite
files written by earlier ones:

1. `prepare-serving-core`: in-memory projection of the GTFS tables (`gtfsRelations`).
2. If the compiler did not hand over native `trip_call`: `write-ordered-trip_call`,
   `write-ordered-shapes`, `write-ordered-trip` (CZPTT and overlay take this path; JDF hands
   over native `trip_call`, `trip`, `service_note`, `assignment` files).
3. `prepare-serving-keys` (`keyRelations`), `prepare-serving-semantics`
   (`semanticRelations`, JDF sidecars), CZPTT notes/features (`czpttRelations`), overlay base
   projection (`projectedBaseRelations`).
4. One pass over `Schema.relations` in declared order writing every remaining relation through
   primary-key deduplication (`dedup-<rel>`/`write-<rel>`), or moving/concatenating native files.
   `route_stop`, `route_stop_zone`, `call_zone` are skipped here.
5. `route-stop-order` (`RouteStopWriter.finalize`): writes `route_stop`, `route_stop_zone`,
   `call_zone`, and rewrites `trip_call` and `travel_restriction`.
6. `route-shapes` (optional, `RouteShapes.write`): rewrites `shape`, `shape_point`, `trip`,
   `trip_call`.
7. `project-gtfs` (`GtfsProjection.write`) from the finished Parquet files.
8. `write-diagnostics-summary`, `hash-production-payloads` (manifest), `validate-production-package`.

### 1.2 Parquet settings

| Item | Value | Where |
|---|---|---|
| Library | Parquet.Net 6.0.3 (`created_by = "Parquet.Net version 6.0.3 (build e2a1c4da…)"`) | real packages |
| Compression | Snappy, every column chunk | `ColumnWriter.Writer` |
| Custom key/value metadata | `obehy.schema_version` = `"5.2"`, `obehy.relation` = relation name, `jrutil.bundle_format` = `"jrutil-production"` | `ColumnWriter.Writer` (contract `parquet_metadata`) |
| Column order | exactly the `Schema.relations` field order (= `serving-v5.json` order) | `ColumnWriter.Append` checks count, type and nullability per index |
| Row-group maximum | 65,536 rows (contract `row_group_maximum`; validation rejects larger groups) | |
| Actual group sizes (not contractual) | generic relations: ≤65,536 rows and flushed at a 16 MiB estimated buffer; `trip_call`, `trip`, `route_stop`, zones, `travel_restriction`: 8,192 (rewritten by the route-stop pass); `shape_point`: 16,384 after the route-shapes phase, else 65,536; `shape`: 8,192 | `PackageRows.writeParquet`, `RouteStopWriter`, `RouteShapes` |
| Empty relation | file with schema, metadata, zero row groups | rail package `call_key`, `connection_claim`, `travel_restriction` |

Type mapping (`ColumnWriter.dataField`; manifest/contract type names in brackets):

| Contract | Parquet physical / logical | pyarrow sees |
|---|---|---|
| `string` | BYTE_ARRAY, UTF8 | `string` |
| `int16` | INT32 with INT(16) annotation | `int16` |
| `int32` | INT32 | `int32` |
| `int64` (declared in `Schema.FieldType`, used by no relation) | INT64 | — |
| `double` | DOUBLE | `double` |
| `bool` | BOOLEAN | `bool` |
| `date32` | INT32 DATE | `date32[day]` |

Nullability: `nullable = false` fields are REQUIRED, `true` fields OPTIONAL (pyarrow reports the
same flags as the contract on both real packages). `ColumnWriter` rejects a null in a required
text column; value-typed required columns cannot hold null by construction.

### 1.3 ZIP settings (`GtfsProjection.write`)

- Entries flat, one per table, sorted ordinal by file name (`agency.txt`, `calendar.txt`,
  `calendar_dates.txt`, `feed_info.txt`, `routes.txt`, `shapes.txt`, `stop_times.txt`,
  `stops.txt`, `transfers.txt`, `trips.txt` in the rail package).
- Deflate, `CompressionLevel.Fastest` (general-purpose flag `0x806`: UTF-8 names + fastest),
  entry timestamp `1980-01-01 00:00:00` (`DateTimeOffset(1980,1,1,0,0,0,TimeSpan.Zero)`).
  `production-v3.json` declares `zip_entry_timestamp`, `zip_entry_order: ordinal-name`.
- CSV: UTF-8 without BOM, LF line endings, header row, **every field double-quoted** including
  the header and empty values (`""`), embedded `"` doubled (`GtfsFiles.writeCsvRow`).

### 1.4 Row order

The contract says row order is unspecified; consumers must not rely on it. What F# actually
emits (useful only for parity debugging):

- `trip_call`: ordinal `trip_id`, then `sequence` ascending. `TripCallWriter.Append` throws
  `"Trip calls are not strictly ordered at …"` otherwise, so the compiler's `stop_times` must
  already be sorted that way. The route-stop and route-shapes rewrites keep the order.
- `shape`/`shape_point` (ordered path): ordinal `shape_id`, `shape_pt_sequence` strictly
  ascending (`"shapes.txt is not strictly ordered at …"`); route-shapes appends the new
  `motis` shapes after the existing ones in ordinal shape-id order, points in sequence order.
- `trip`: compiler `trips.txt` order, first occurrence kept (`RelationWriter.write`).
- Every other relation: generation order through `HashDedup.dedup`. While the estimated row
  bytes stay under the 64 MiB budget (`Scratch.defaultBufferBytes`; row estimate
  `48 + 32·fields + 2·chars`) the input order is kept; above it all rows are spilled into 256
  partitions by the top byte of `XxHash128(key text)` and emitted partition by partition. Large
  CZPTT relations (`assignment`, `source_key`, `service_note`, `service_exception`) are in
  partition order in the rail package.
- `route_stop`: route directions in ordinal `route_id`, then direction (null first, as −1),
  slots in sequence order. `route_stop_zone`: slot order, then `source_order`. `call_zone`:
  trips in first-seen order of `trip_call`, calls in sequence order, then `source_order`.
- `travel_restriction`: input order, a route-scoped row reaching several slots expanded in place
  in ascending slot index.

### 1.5 Deduplication and primary-key conflicts

- Generic relations (`PackageBaseRelations.deduplicated`): every row is serialised to text
  (`PackageRows.fieldText`: date `yyyy-MM-dd`, numbers invariant, null → `""`), keyed by the
  primary-key cells joined with U+001F. A repeated key with an identical serialised row is
  dropped; a repeated key with a different row throws
  `InvalidOperationException("Relation <rel> contains conflicting rows for primary key <key>")`
  and fails the build. Rows are parsed back (`parseField`), so **an empty string in a nullable
  text field becomes null** at this point.
- Overlay base rows (`preferBase`): base-package rows and regenerated rows of the same key are
  merged with *base first wins, silently* (no conflict error), keyed by
  `Identity.compositeKey(fieldText pk cells)`.
- Native compiler files plus generated rows of the same relation (JDF `trip`, `service_note`,
  `assignment`) are concatenated by `RelationWriter.concat` **without** deduplication across
  the two parts; a duplicate would only be caught by validation.
- `trip` (ordered path) dedups by `trip_id` with the same conflict rule (`RelationWriter.write`).
- `trip_call`: no dedup; strict ordering forbids duplicates.

---

## 2. Relations, column by column

Feed differences are marked **[czptt]**, **[jdf]**, **[overlay]** (the overlay package has
`feed = "jdf"` and a base package). "GTFS→serving" functions are in
`Serving/PackageGtfsRelations.gtfsRelations` unless noted.

### 2.1 `agency` (PK `agency_id`)

| Column | Derivation |
|---|---|
| `agency_id` | GTFS `agency_id`. [czptt] `czptt:agency:<EscapeDataString(RU code)>` (`CzPttEntities.agencyId`); one per responsible RU code of accepted journeys. |
| `name` | GTFS `agency_name` (verbatim, required). [czptt] RU name from the catalog (`CzPttAgencies`). |
| `url` | nonblank(GTFS `agency_url`). Note `Gtfs.fillStandardRequiredFields` replaces a missing URL by the placeholder `jrutil://invalid`, which therefore reaches serving (seen in the overlay package). [czptt] 38 of 42 rows are `https://portal.cisjr.cz/`. |
| `timezone` | GTFS `agency_timezone` (verbatim). [czptt] always `Europe/Prague`. |
| `language` | nonblank(`agency_lang`). [czptt] `cs`. |
| `phone`, `fare_url`, `email` | nonblank of `agency_phone`, `agency_fare_url`, `agency_email`. [czptt] always null. |

### 2.2 `location` (PK `location_id`)

Two producers: GTFS stops (`gtfsLocations`) and CZPTT railway points that are not GTFS stops
(`operationalLocations`), concatenated in that order.

From a GTFS stop:

| Column | Derivation |
|---|---|
| `location_id` | GTFS `stop_id`. [czptt] station `czptt:stop:<Esc(country)>:<Esc(primary code)>`; boarding point `<station>:platform:<Esc(subsidiary code)>`, `<station>:unspecified` (no subsidiary in the source), or `<station>:platform:BUS` for rail-replacement (NAD) parts (`CzPttEntities.stationIdFor/stopId/busStopId`; `Esc` = `Uri.EscapeDataString`). |
| `kind` | `stop_place` if GTFS `location_type = "1"`; else `boarding_point` if `parent_station` non-empty; else `stop_place`. |
| `domain` | `heavy_rail` if feed = `czptt`, else `surface` (per package, not per row). |
| `parent_location_id` | nonblank(`parent_station`). |
| `name` | GTFS `stop_name` verbatim. [czptt] point name (catalog/SR70 name index, else the message's `PrimaryLocationName`); an estimated coordinate appends ` [?]` (`Gtfs.markApproximateStopName`, upstream). |
| `public_code` | nonblank(`platform_code`). [czptt] boarding point: `LocationSubsidiaryName` if non-blank else `LocationSubsidiaryCode`; `BUS` for NAD points; null for `:unspecified` and stations. |
| `description` | nonblank(`stop_desc`). [czptt] only NAD points: `"Pro přesné informace k nástupišti náhradní dopravy sledujte informace dopravce."` |
| `municipality_name`, `district_name`, `district_code`, `nearby_place`, `country_code` | nonblank of sidecar `source_stop_metadata` columns `town`, `district`, `okres`, `nearby_place`, `country` for this stop, else for its parent (a boarding point without its own metadata inherits the stop place's). [overlay] no sidecar: the base package's `location` row of the same id (`municipality_name … coordinate_source`). [czptt] no `source_stop_metadata` is written, so all five are null for GTFS stops (only operational points get `country_code`). |
| `coordinate_precision` | `missing` if not *positioned*; else `coordinatePrecision(metadata precision)` (`"estimated"`→`estimated`, `"missing"`→`missing`, anything else → `exact`); `exact` when there is no metadata. *Positioned*: both `stop_lat` and `stop_lon` parse **and not (lat = 0.0 and lon = 0.0)**; the (0,0) case exists because `fillStandardRequiredFields` writes 0/0 for missing coordinates. **[czptt] has no metadata, so an estimated CZPTT stop is published as `exact`** (SERVING-R004). |
| `coordinate_source` | null when not positioned. Else the metadata `coordinate_source` mapped by `coordinateSource` (SERVING-R005); if that is null: the `coordinate_source` of the `operational_points` sidecar row whose `"czptt:stop:" + source_location_id` equals this id or its parent, mapped the same way; else `overlay` when the id starts with `overlay:`; else null. [czptt] sidecar values are `sz_sr70`, `osm`, `estimated` → `sr70`, `osm`, `catalogue` (the last is wrong, SERVING-R059). Rail package: `sr70` 10,028, `osm` 174. |
| `longitude`, `latitude` | parsed `stop_lon`/`stop_lat` when positioned, else null. [czptt] SR70/OSM/estimate selection in `CzPttBundle.convert` (upstream). |
| `url` | nonblank(`stop_url`). |
| `timezone` | nonblank(`stop_timezone`). [czptt] `Europe/Prague` on stations only. |
| `wheelchair_boarding` | parsed int16 of `wheelchair_boarding`, null when blank. [czptt] always null. |

From a CZPTT railway point (sidecar `operational_points`, rows whose
`"czptt:stop:" + source_location_id` is not a GTFS `stop_id`):

| Column | Derivation |
|---|---|
| `location_id` | `"czptt:stop:" + source_location_id`, where `source_location_id = "<country>:<primary code>"` **without** URI escaping (CzPttBundle) — differs from `stationIdFor` when a code contains reserved characters (SERVING-R060). |
| `kind` | `operational_point`; `domain` `heavy_rail`; `parent_location_id`, `public_code`, `description`, `municipality_name`, `district_name`, `district_code`, `nearby_place`, `url`, `timezone`, `wheelchair_boarding` null. |
| `name` | `source_name`: the point name of the lexicographically smallest name among the PA calls at that point (`Seq.minBy name`). |
| `country_code` | nonblank(`country_code` = `CountryCodeIso`). |
| `coordinate_precision` | `exact` if both latitude and longitude present, else `missing`. Estimated coordinates are only selected for passenger points, so in practice none reach here. |
| `coordinate_source` | `coordinateSource(coordinate_source)` when positioned, else null. |
| `longitude`, `latitude` | sidecar values when positioned. |

The sidecar holds one row per (country, primary code) seen in any accepted PA, including points
whose GTFS stops were dropped (unresolved timed-only points), which then appear here as
operational points. Rail package: 753 operational points, 4 of them `missing`.

### 2.3 `route` (PK `route_id`)

| Column | Derivation |
|---|---|
| `route_id` | GTFS `route_id`. [czptt] `czptt:route:line:<line code>:<RU>:<route type>` when the part's line code maps to a catalog line valid on the PA start date, else `czptt:route:fallback:<RU>:<category>:<route type>:<train number or category>` (all components escaped). |
| `agency_id` | GTFS `agency_id` (required). |
| `mode` | `servingMode(int route_type)` (SERVING-R006). [czptt] `rail` (100–106) or `bus` (714 NAD). |
| `gtfs_route_type` | int of GTFS `route_type`. [czptt] the CZPTT category route type (100/102/103/105/106) or 714 for NAD. |
| `short_name` | nonblank(`route_short_name`). [czptt] distinct labels of the journey joined `/`: catalog line mark, or `<category> <train number>`, suffixed ` (NAD)` for NAD. |
| `long_name` | nonblank(`route_long_name`). [czptt] catalog line name (trailing spaces trimmed) when the journey has exactly one label with a mapped line. |
| `description` | nonblank(`route_desc`). [jdf] `Výlukový jízdní řád` for detour routes. |
| `url` | nonblank(`route_url`). |
| `color`, `text_color` | nonblank. [czptt] colour by route type (`106`→`1c1745`, `102`→`b91c1c`, `103`→`b45309`, `105`→`4c1d95`, else `475569`), text `ffffff` (`CzPttEntities.routeColor`). |
| `sort_order` | parsed int of `route_sort_order` or null. |
| `timetable_kind` | sidecar `source_route_metadata(gtfs_route_id, detour)`: `"True"` → `detour`, other non-empty → `regular`; [overlay] the base package's value for the same route id; default `regular` (always `regular` for CZPTT). |

### 2.4 `service_calendar` (PK `service_id`)

| Column | Derivation |
|---|---|
| `service_id` | GTFS `service_id`. Not generated by the writer. [czptt] `Gtfs.deduplicateCalendarWithPrefix "czptt:service"`: per-PA services `czptt:service:<Esc PA>:calendar:journey` (one per PA, dates from `PlannedCalendar.BitmapDays`) are grouped by identical content and renamed `czptt:service:exc:<i>`, `i` = 0-based group index in first-appearance order over the **ordinal-sorted original ids** (SERVING-R061). [overlay] `overlay:service:<hash>`. |
| `valid_from`, `valid_to` | GTFS `start_date`, `end_date`. |
| `weekday_mask` | sum of `1 << index` for `monday..sunday` (index 0..6) equal to `"1"`. |

Plus one row per service that appears only in `calendar_dates.txt`: `valid_from`/`valid_to` =
min/max of all its exception dates (added **and** removed), `weekday_mask = 0`. [czptt] every
service is of this kind (9,230 rows, all mask 0); [overlay] likewise (691).

### 2.5 `service_exception` (PK `service_id, service_date`)

`service_id`, `service_date` = GTFS `date`, `added` = (`exception_type = "1"`). [czptt] only
added dates (1,149,533 rows, all `true`).

### 2.6 `trip` (PK `trip_id`) — ordered path `writeOrderedTrips`; JDF native `TripWriter.write`

| Column | Derivation |
|---|---|
| `trip_id` | GTFS `trip_id`. [czptt] `czptt:trip:<Esc PA>:<journey.index + 1>`. |
| `route_id`, `service_id` | GTFS values (required). |
| `direction` | int16 of `direction_id` or null. [czptt] always null (`directionId = None`). |
| `headsign` | nonblank(`trip_headsign`). [czptt] display name of the PA's **last passenger call** (same headsign on every part). |
| `short_name` | nonblank(`trip_short_name`). [czptt] `<category of first call> <train number>` (train number: first `OperationalTrainNumber` of the part's calls, else the TR identifier core), or the category alone. |
| `block_key` | nonblank(`block_id`). [czptt] `czptt:block:<Esc PA>`; when consecutive parts of the PA alternate between rail and NAD, `czptt:block:<Esc PA>:mode:<modeBlockIndex+1>` (index increments at each rail↔NAD change). [jdf] JDF block. |
| `run_key` | [czptt] the `PA=` token of `cz_trips.txt.source_trip_ids` (the PA identifier verbatim, e.g. `Pa:0054:--KADR058625:01:2026`); null for trips without one (road). **Parsed** from a packed string (SERVING-R014). |
| `run_part` | [czptt] `Int16.Parse` of the trip id text after its last `:` (= `journey.index + 1`). **Parsed from the id** (SERVING-R014). Rail package: 1..6. |
| `wheelchair_accessible`, `bikes_allowed` | int16 of GTFS values or null. [czptt] from `projectedTripFeatures` (upstream, central notes 17/34/22/26–29/36). JDF native maps `bikesAllowed` `NoInformation/OneOrMore/NoBicycles` → 0/1/2. |
| `shape_id` | nonblank(`shape_id`); replaced by the route-shapes phase for trips it shapes. |

### 2.7 `trip_call` (PK `trip_id, sequence`) — see section 3 for CZPTT

Passenger calls, from GTFS `stop_times` (`writeOrderedTripCalls`; JDF native
`TripCallWriter.fromGtfs`, which is field-for-field identical — pinned by
`Native trip calls equal the existing GTFS projection for every field`):

| Column | Derivation |
|---|---|
| `trip_id`, `sequence` | `trip_id`, `stop_sequence`. |
| `location_id` | the stop's `parent_station` when non-empty, else the `stop_id` itself. |
| `boarding_point_id` | the `stop_id` when it has a parent, else null. |
| `passenger_service` | `true`. |
| `route_stop_id` | null when written (JDF native: the compiler route-stop key `routeStopKey`); replaced by the route-stop pass (section 7). |
| `scheduled_arrival`, `scheduled_departure` | `H*3600+M*60+S` of the GTFS time, null when blank. GTFS semantics (noon − 12 h); may exceed 86,400. [czptt] times already shifted upstream by whole days when a PA has negative times (`CzPttEntities.gtfsTimeShift`). |
| `pickup_type`, `dropoff_type` | int16 of the GTFS value, `0` when blank. |
| `timepoint` | `false` iff GTFS `timepoint = "0"`, else `true` (blank → true). |
| `stop_headsign` | nonblank(`stop_headsign`). |
| `shape_distance_traveled` | null; set only by the route-shapes phase. |
| `subsidiary_code`, `subsidiary_name`, `active_line_code` | null, except [czptt] (section 3.4). |
| `source_kilometre` | parsed GTFS `shape_dist_traveled` (JDF kilometre), null when blank. [czptt] always null. |

Non-passenger calls: CZPTT only, section 3.

### 2.8 `route_stop`, `route_stop_zone`, `call_zone`

Written only by the route-stop pass, section 7.

### 2.9 `shape` (PK `shape_id`) and `shape_point` (PK `shape_id, sequence`)

From GTFS `shapes.txt` (ordered path `writeOrderedShapes`):

- `shape.shape_id` = distinct `shape_id`; `generation_method` = `source` if the id starts with
  `overlay:`, else `compiler` (prefix test, SERVING-R034).
- `shape_point`: `sequence` = `shape_pt_sequence`, `longitude`/`latitude` parsed,
  `distance_traveled` = cumulative haversine metres from the first point
  (`Geo.haversineMetres`, sphere radius 6,371,000 m, rounded `Math.Round(x, 1)` banker's
  rounding). The GTFS `shape_dist_traveled` column is ignored.
- Native-JDF path: ordered shapes are not written; the unordered `gtfsRelations` rows are used,
  whose `distance_traveled` is **null** (contradicts the 5.2 rule "computed for every shape";
  harmless only while JDF has no shapes).
- Route-shapes rows: section 8 (`generation_method = motis`).

Rail package: 8,022 shapes, all `motis`; overlay package: none (phase not run on that subset).

### 2.10 `transfer` (PK `transfer_key`)

From GTFS `transfers.txt`:

| Column | Derivation |
|---|---|
| `transfer_key` | `feedId(feed, "transfer", [from_stop_id, to_stop_id, from_route_id, to_route_id, from_trip_id, to_trip_id])` with the raw GTFS cell values (empty string when absent) under the **GTFS column names** (SERVING-R035). |
| `from_location_id`, `to_location_id` | nonblank(`from_stop_id`/`to_stop_id`): a boarding-point id, not resolved to the stop place. |
| `from_route_id`, `to_route_id`, `from_trip_id`, `to_trip_id` | nonblank GTFS values. |
| `transfer_type` | int16 GTFS value. |
| `minimum_transfer_time` | parsed `min_transfer_time` or null. |
| `maximum_waiting_time` | parsed `max_waiting_time` or null (not published in `gtfs.zip`). |

[czptt] Sources (`CzPttTransfers.fs`, `CzPttToGtfs.fs`):
- Consecutive parts of one PA without a mode change: type `4` (in-seat), trips set, stops and
  min time null. 5,018 rows in the rail package.
- Consecutive parts with a rail↔NAD change at the same station: first a type `1`, min `0`
  between the last stop of one and the first stop of the next, with trips; cross-station NAD
  boundaries are omitted with a diagnostic.
- Cross-PA NAD connections (`crossPaTransfers`): type `1`, min `0`, stops and trips.
- Then **every type-1 transfer with both stops is rewritten** to a stop-pair rule:
  stops kept, routes and trips dropped, `transfer_type = 2`, `min_transfer_time = 0`, then
  distinct (`CzPttToGtfs.fs:157-180`, comment: MOTIS ignores trip specificity and applies its
  default transfer time unless a stop-pair minimum exists). 1,418 rows. This is the MOTIS
  1→2 rewrite; it happens **upstream of serving**, so serving `transfer` carries the
  MOTIS-adapted rule, not the source fact (SERVING-R036).

[jdf]/[overlay] compiler transfers with `max_waiting_time` (overlay package: type 1 with
waiting time 12,497 rows, type 2 with minimum 18,646).

### 2.11 `service_note` (PK `note_id`) — section 5.1

### 2.12 `assignment` (PK `assignment_id`) — section 5.2

### 2.13 `connection_claim` (PK `connection_id`) — section 5.3; empty for CZPTT.

### 2.14 `travel_restriction` (PK `restriction_id`) — section 5.4; empty for CZPTT.

### 2.15 `source_key`, `call_key` — section 4.

---

## 3. CZPTT trip parts and calls

### 3.1 What the CZPTT compiler hands over

`CzPttPackage.write` builds `CompilerOutput.Output` with `feed = "czptt"`, the GTFS tables
(after `deduplicateCalendarWithPrefix "czptt:service"` and `fillStandardRequiredFields`), the
`cz_*` tables (`cz_routes.txt`, `cz_trips.txt`, `cz_trip_stop_zones.txt`), and every
`*.parquet` in the scratch directory as a sidecar table:

| Sidecar | Columns | Read by the writer? |
|---|---|---|
| `operational_points` | `source_location_id` (`"<CC>:<code>"`), `country_code`, `primary_code`, `source_name`, `latitude`, `longitude`, `coordinate_source` (`sz_sr70`/`osm`/`estimated`), `coordinate_source_object_id`, `coordinate_match_method` | yes: locations, coordinate sources, `sr70` keys |
| `operational_calls` | `source_pa_id`, `source_sequence` (= source index + 1), `source_location_id`, `passenger_call`, `arrival_seconds`, `departure_seconds` (unshifted), `subsidiary_code`, `subsidiary_name`, `active_line_code` | yes: railway points and operational columns |
| `source_call_metadata` | `gtfs_trip_id`, `stop_sequence`, `source_pa_id`, `source_sequence` | yes, for `call_key`, but `stop_sequence` is written equal to `source_sequence`, so it never yields a row |
| `source_note_metadata` | `source_note_id`, `source_pa_id`, `note_kind`, `source_code`, `gtfs_trip_id` (one row per intersecting part, or one row with null), `label`, `raw_value`, `valid_from`, `valid_to` (NodaTime `LocalDate.ToString()` text, parsed back leniently by `PackageRows.date`: `yyyyMMdd` after removing `-`, else `DateTime.Parse` invariant), `resolved`, `from_sequence`, `to_sequence` | yes |
| `source_feature_metadata` | `source_feature_id`, `gtfs_trip_id`, `call_sequence`, `source_code`, `feature_kind`, `note_id`, `source_object_id` | yes |
| `source_ids_coverage_metadata`, `source_ids_coverage_trip_metadata` | IDS `CZIPTS`/`CZCalendarIPTS` coverage | **no** — never read by the package writer (open question 4) |

Sidecar Parquet files carry their own metadata (`obehy.schema_version = "1"`,
`obehy.bundle_format = "czptt-v1"`, …); they never leave scratch.

The compiler's diagnostics object (`accepted_pa_count`, `rejected_journeys`, `ids_diagnostics`,
…) is passed as `diagnostics` but has no `"diagnostics"` array, so **none of it reaches the
production `diagnostics.json`** (it is only copied into the optional diagnostic artifact).

The writer assumes CZPTT's `Sidecar` operational-point mode, in which GTFS `stop_times` hold
only passenger calls (`CzPttNormalize.selectedCalls`). In `Gtfs` mode non-passenger calls would
also be GTFS stop times and the writer would add them a second time with the same sequence,
which `TripCallWriter` rejects.

### 3.2 How a PA becomes trips

Upstream (`CzPttNormalize.journeys`, `CzPttToGtfs`): the PA's normalized calls are cut into
segments ("journeys") at operator, line and rail/NAD boundaries; each journey becomes one GTFS
trip. Consecutive journeys share their boundary call (same source index and times).

In serving:

- `trip_id = czptt:trip:<Esc PA>:<journey.index + 1>`.
- `run_key` = PA identifier verbatim; `run_part` = `journey.index + 1` (recovered by parsing,
  see 2.6). Contract: "run_part orders the trip parts of one train path (1, 2, …)".
- `block_key` as in 2.6.
- All parts of a PA share `service_id` (the PA calendar), `headsign`, TR key; each part has its
  own `route_id`, `short_name`, train number.

### 3.3 `trip_call.sequence` and railway points (`writeOrderedTripCalls`)

`sequence` is the PA's source sequence (1-based source index), for passenger calls (GTFS
`stop_sequence = call.sourceIndex + 1`) and railway points alike, so a part's sequences have
gaps where calls are absent and the parts of one PA share the number space. Rail-package
example (PA `--KADR058625`): part 1 = sequences 1–12 (9, 10, 11 railway points), part 2 =
12–17 (16 a railway point), part 3 = 17–18.

Per part, computed from the GTFS stop_times:
- `low`/`high` = min/max GTFS `stop_sequence` of the part;
- `pathLow`/`pathHigh` = min/max over all parts of the same PA;
- *rail* = `servingMode(route_type of the part's route) = "rail"`;
- *opens* = `low = pathLow`; *closes* = `high = pathHigh`.

A non-passenger point of the PA (`operational_calls` row with `passenger_call = false`) is added
to the part iff the part is *rail* and
`low < seq < high`, or (*opens* and `seq < low`), or (*closes* and `seq > high`).
A bus (NAD) part gets no railway points; leading/trailing points of a PA whose first/last part
is NAD are dropped; points that fall between two parts that do not share a boundary are dropped.
Passenger points (`passenger_call = true`) are never added from the sidecar (they are GTFS
stop times or were deliberately not exported).

Railway-point row:

| Column | Value |
|---|---|
| `location_id` | `"czptt:stop:" + source_location_id` |
| `passenger_service` | `false` |
| `boarding_point_id`, `route_stop_id`, `stop_headsign`, `shape_distance_traveled` (until route shapes), `source_kilometre` | null |
| `scheduled_arrival`, `scheduled_departure` | the sidecar's `arrival_seconds`/`departure_seconds` verbatim (null when the source gives none; the contract's "arrival = departure for a pass" is whatever the source has) |
| `pickup_type`, `dropoff_type` | `1` |
| `timepoint` | `arrival or departure present` (a passenger call uses CZPTT's inferred-timing flag instead; railway points have none) |
| `subsidiary_code`, `subsidiary_name`, `active_line_code` | sidecar values |

Note: the sidecar times are **not** shifted by `gtfsTimeShift`, while passenger times are; for a
PA with negative times the two would be inconsistent (no such PA in the rail package).

Contract check (`Validation.validateTripParts`): consecutive parts (by `run_part`) must have
last call of part n−1 and first call of part n with equal `sequence`, `location_id`,
`scheduled_arrival`, `scheduled_departure`; a `bus` part must have no non-passenger call.

Rail package: 1,125,269 calls, 522,842 non-passenger.

### 3.4 Passenger-call fields specific to CZPTT

- `subsidiary_code`, `subsidiary_name`, `active_line_code`: from the `operational_calls` row of
  the same PA and the same sequence (the call's `LocationSubsidiaryCode`, `LocationSubsidiaryName`,
  and the call's line code). `subsidiary_name` is null throughout the rail package.
- `pickup_type`: `1` if the call is not a passenger call or has activity `DisembarkOnly`;
  else `3` if it has the request-stop activity (`0030`); else `0`. `dropoff_type` likewise with
  `EmbarkOnly` (`CzPttEntities.stopTimes`). Rail package passenger calls: 0, 1, 3.
- `timepoint`: GTFS `Approximate` (→ `false`) when the timing was inferred or both times are
  missing, else `Exact`.
- `boarding_point_id`: the `:platform:<code>`, `:unspecified` or (NAD) `:platform:BUS` stop;
  `location_id` its station.
- Operational-point locations: section 2.2.

---

## 4. Keys

All rows are built in `PackageKeyRelations.keyRelations`. Validity dates use:
- *service bounds* (`serviceBounds`): per `service_id`, `[start_date, end_date]` of
  `calendar.txt`, widened by every `calendar_dates.txt` date (added or removed); services only
  in `calendar_dates` get `[min, max]` of their dates;
- *trip bounds*: service bounds of the trip's service;
- *route bounds*: union of trip bounds of the route's trips;
- *package bounds*: min of all service starts .. max of all service ends (`1970-01-01` twice
  when there are no services).

Rows whose namespace is not one of the 14 declared namespaces are dropped silently
(`isDeclared`) — a source without a declared namespace publishes no keys.

### 4.1 `source_key` rows

| # | Namespace | Entity | Identifier | `public_id` | Validity | `binding_method` | Feeds |
|---|---|---|---|---|---|---|---|
| 1 | `<system>:gtfs_trip_id` | trip | mapping `source_trip_id` (verbatim) | mapping `output_trip_id` | mapping `valid_from..valid_to` | `bindingMethod(method)` | overlay; rows with `source_trip_id = output_trip_id` skipped |
| 2 | `<system>:line_course` | trip | `compositeKey [operational_line_id; operational_trip_id]` (e.g. `L%2F1/C%202`, contract encoding "line '/' course") | each output trip of the mappings with the same (source, source trip, valid_from, valid_to) | the operational row's dates | `operator_crosswalk` | overlay, IDS JMK `api.txt` |
| 2a | `<system>:gtfs_trip_line_course` (5.3) | trip | a crosswalk row's source trip ID (verbatim) | each output trip of the mappings of every source trip with the row's line and course | that mapping's run | `operator_crosswalk` | overlay, IDS JMK `api.txt` (OVL-R131); czptt: a train trip ID keys the trains of its course number (OVL-R132) |
| 3 | `cis:line_trip` | trip | `PadLeft(cis_line_id, 6, '0') + ":" + Int64.Parse(cis_trip_id)` (leading zeros of the trip number dropped) | `cz_trips.trip_id` (if the trip exists) | trip bounds | `identity` | jdf; requires `cis_line_id` all ASCII digits, length 1..6, and non-empty trip number |
| 4 | `czptt:train_number` | trip | `cz_trips.train_number` verbatim (non-empty) | trip | trip bounds | `identity` | czptt (73,155) |
| 5 | `czptt:pa` / `czptt:tr` | trip | the text after `=` of `PA=`/`TR=` tokens of `cz_trips.source_trip_ids` (split on `|`, trimmed); i.e. PA / TR identifier verbatim (`Pa:0054:…`, `Tr:1154:…`) | every part of the PA | trip bounds of that part | `identity` | czptt (73,155 each) |
| 6 | `cis:line` | route | `PadLeft(cis_line_id, 6, '0')` | `cz_routes.route_id` (only routes that have trips) | route bounds | `identity` | jdf (CZPTT `cz_routes` has no CIS line) |
| 7 | `<system>:gtfs_route_id` | route | mapping `source_route_id` | `output_route_id` | package bounds | `source_native` if output id starts `overlay:`, else `structural_match` | overlay; equal ids skipped |
| 8 | `<system>:gtfs_stop_id` | location | mapping `source_stop_id` | `output_stop_id` | package bounds | as row 7 | overlay |
| 9 | `sr70` | location | `primary_code` of an `operational_points` row with `country_code = "CZ"` and `primary_code` exactly 5 ASCII digits | `"czptt:stop:" + source_location_id` — the stop place if the point is a GTFS station, else the `operational_point` location (rail package: 2,689 stop places, 741 operational points) | package bounds | `identity` | czptt (3,430) |
| 10 | `sr70:track` | location | `<sr70 code> + ":" + GTFS platform_code` of every GTFS stop whose `parent_station` is a row-9 location and whose `platform_code` is neither `""` nor `"BUS"` | that boarding point (`…:platform:<code>`) | package bounds | `identity` | czptt (3,088) |
| 11 | any non-national namespace from the base package's `source_key` | trip/route/location | base identifier | trip keys: each output trip cut from the base trip (`base_to_output_trips.csv`), validity clipped to `max(from)…min(to)` of key and cut, dropped if empty or the trip is not in the output; route/location keys: kept if the target is in the output | as described | base method | overlay |

`<system>` = source id with a trailing `-gtfs` removed (`pid-gtfs` → `pid`), `sourceNamespace`.
Rows 1–2 and 11 are the overlay's; national namespaces (`cis:line`, `cis:line_trip`,
`czptt:train_number`, `czptt:pa`, `czptt:tr`) are never carried from the base, always
re-derived (rows 3–6).

**The `sr70:track` track part** is the GTFS `platform_code`, i.e. CZPTT
`LocationSubsidiaryName` when non-blank, else `LocationSubsidiaryCode`
(`CzPttEntities.stops`). The contract says "CZPTT track designation"; in the rail package
`subsidiary_name` is always absent, so it is the subsidiary code (`1`, `2`, `1a`, …).
`:unspecified` boarding points (no platform code) and NAD `BUS` points get no track key.

The trip-key semantics (contract): a trip key applies on its trip's service dates within
`[valid_from, valid_to]`; a key may name several trips (CZPTT TR and train number name every
part; PA names every part).

`bindingMethod(method)`: empty → `source_native`; `authoritative_source_trip_set`,
`source_native`, `operator_crosswalk` verbatim; prefix `companion_assertion` →
`companion_assertion`; anything else (including prefix `structural`) → `structural_match`.

Identity rows (a public id resolving to itself) are never written. Rail package:
225,983 rows, all `identity`.

### 4.2 `call_key` rows

| # | Namespace | Identifier | `source_sequence` | `trip_id`, `sequence` | Feeds |
|---|---|---|---|---|---|
| 1 | `<system>:gtfs_trip_id` | source trip id | `source_call_ordinal` (text verbatim) | output trip; `sequence` = the output trip's sorted `stop_sequence` list at 1-based position `output_call_ordinal` (throws `"Call mapping for … names position …"` when out of range); row only when `source_call_ordinal ≠ string sequence`; only for mapped (source, source trip, output trip) triples | overlay |
| 2 | `czptt:pa` | `source_pa_id` | `source_sequence` | `gtfs_trip_id`, `stop_sequence` when `stop_sequence ≠ source_sequence` | czptt — **never fires**: the sidecar writes both equal, and serving keeps the PA sequence |
| 3 | base `call_key` rows | base | base | each distinct output trip cut from the base trip that still *holds* the sequence (within first..last call and, for trips with gaps, in its sequence set) | overlay |

Rail package: 0 rows. Overlay package: 1,391,759.

---

## 5. Typed semantics

### 5.1 `service_note`

[czptt] (`czpttRelations`), one row per distinct `source_note_id`:

| Column | Value |
|---|---|
| `note_id`, `source_object_id` | `czptt:note:<Esc PA>:<central|noncentral|calendar>:<index+1>` (index = position among that PA's parameters of the kind) |
| `kind` | `czptt_central_note` (`CZCentralPTTNote`), `czptt_local_note` (`CZNonCentralPTTNote`), `czptt_calendar_note` (`CZCalendarPTTNote`) |
| `label` | central: catalog central-note name for the code; local: field 5 (index 4) of the raw value; calendar: null |
| `text` | the raw parameter value verbatim (e.g. `44|CZ33624||CZ34434||0|`) |
| `valid_from`, `valid_to` | central/local: min/max of the referenced calendar's dates when it resolves; calendar note: fields 2 and 3 (`yyyyMMdd`) |
| `service_note_type` | central: the note code; calendar: field 1; local: null |

Rail package: 155,146 central, 1,709 local, 629 calendar.

[jdf] (`semanticRelations`, JDF native `NoteWriter.write`): sidecar `source_notice_metadata`:
`note_id = source_object_id = source_notice_id`, `kind` = `reservation`→`reservation_note`,
`service_note`→`timetable_note`, else verbatim (`route_information`), `label`, `text`,
`valid_from`, `valid_to`, `service_note_type` nonblank.

### 5.2 `assignment`

`assignmentRow` turns blank strings into null (`IsNullOrWhiteSpace`).

| Producer | `assignment_id` | `scope` | `kind` | Fields set |
|---|---|---|---|---|
| [czptt] note link, trip-less note row (`gtfs_trip_id` null: calendar notes, unresolved central/local notes, notes intersecting no part) | `feedId("czptt", "note-assignment", [note; scope; trip; from=""; to=""])` | `trip` | `note` | `trip_id` = **every** part of the PA (ordinal trip-id order), `note_id` |
| [czptt] note link with trip, no range (`from_sequence` or `to_sequence` empty) | same scheme | `trip` | `note` | `trip_id`, `note_id` |
| [czptt] note link with range `[low, high]` | same, with `from`/`to` = first/last sequence inside | `call_range` | `note` | `trip_id`, `call_sequence` = smallest, `call_sequence_to` = largest **actual `trip_call` sequence of that part** (passenger or railway point) within `[low, high]`; if none lies inside, falls back to `trip` scope |
| [czptt] feature | `feedId("czptt", "feature", [feature = source_feature_id])` | `call` if `call_sequence` set, else `trip` | sidecar `feature_kind` | `trip_id`, `call_sequence`, `source_code`, `note_id`, `source_object_id` |
| [jdf] note link (`NoteWriter.assignment`) | `feedId(feed, "note-assignment", [note; scope; route; trip])` | `trip` if a trip, else `route` | `note` | `route_id` (only when no trip), `trip_id`, `note_id` |
| [jdf] trip feature | `feedId(feed, "trip-feature", [trip; code; kind])` | `trip` | `feature_kind` | `trip_id`, `source_code`, `source_object_id` |
| [jdf] route feature (LinExt integrated systems) | `feedId(feed, "route-feature", [route; code; kind])` | `route` | `feature_kind` | `route_id`, `source_code`, `source_object_id` |
| [jdf] location feature | `feedId(feed, "location-feature", [location; code; kind])` | `location` | `feature_kind` | `location_id`, `source_code`, `source_object_id` |
| [overlay] base assignment projected onto an output trip ≠ base trip (or other service) | `feedId(feed, "assignment-projection", [assignment = base id; trip = output trip])` | base | base | base fields with `trip_id`, `service_id` replaced; `call_sequence(_to)` moved by `resequenced_calls.csv` |

Scope `service` and kind `calendar_designation` exist in the contract; JDF native code may emit
`calendar_designation` (not in the overlay subset). The two `note-assignment` variants use
different field sets, so the same kind name hashes different canonical strings.

CZPTT note ranges: `from_sequence`/`to_sequence` are `firstIndex+1`/`lastIndex+1` of the note's
endpoint calls, set only when both endpoints resolved and `first ≤ last`; `tripIds` = parts
having a call with source index in the range (`intersectingTrips`). Per part the range is
clipped to the part's own calls in serving (so a note over three parts yields three
`call_range` rows).

CZPTT features (`CzPttToGtfs.fs:341-407`):
- Central-note codes → kinds: `17` → `wheelchair_accessible_vehicle`, `wheelchair_booking_recommended`;
  `34` → `wheelchair_accessible_vehicle`, `wheelchair_booking_required`; `22` →
  `bicycle_transport`, `bicycle_carry_on`; `26` → `bicycle_transport`, `bicycle_storage`,
  `bicycle_reservation_available`; `27` → `…storage`, `bicycle_reservation_required`; `28` →
  `bicycle_transport`, `bicycle_carry_on`, `bicycle_reservation_available`; `29` →
  `…carry_on`, `bicycle_reservation_required`; `36` → `bicycle_transport_prohibited`.
  Only resolved ranges; NAD parts skipped; per part: one `trip`-scope row per kind when the
  range covers the part's first..last call, else one `call` row per covered call of the
  journey. `source_feature_id = <note id>:feature:<kind>:trip:<Esc trip>:<sequence|trip>`,
  `note_id = source_object_id = <note id>`.
- Request stops (activity `0030`): `on_request`, `call` scope, `source_code = "0030"`,
  `source_object_id = <PA>:sequence:<n>:activity:0030`,
  `source_feature_id = <that>:feature:on_request:trip:<Esc trip>`, no note.

Rail package: `call_range` 227,438, `call` 180,797, `trip` 141,258; never `route`, `service`,
`location`.

### 5.3 `connection_claim` ([jdf] sidecar `source_transfer_metadata`)

`connection_id = source_object_id = source_transfer_id`; `direction` `m` → `waits_for`,
`M` → `connects_to` (other values verbatim, then rejected by validation); `origin_trip_id`;
`origin_sequence` = the call of `(trip, source_route_stop_id)` from the JDF native transfer
sequences, or the first matching `source_call_metadata` row, **else `0`**;
`target_source_route_id`, `…stop_id`, `…post_id`, `…end_stop_id`, `…end_post_id`,
`wait_minutes`, `note` nonblank; `service_id`, `target_source_trip_id`,
`target_public_line`, `target_destination_text`, `target_route_id`, `target_trip_id`,
`target_location_id` null; `target_derivation` always `structured`; `resolution_status` always
`unresolved`. [overlay] base claims projected: new id
`feedId(feed, "connection-projection", [connection; trip])` when the origin trip changes;
`target_trip_id` → first projected trip or null. Empty for CZPTT.

### 5.4 `travel_restriction` ([jdf] sidecar `source_travel_restriction_metadata`)

`restriction_id = feedId(feed, "restriction", [scope; route; trip; route_stop; group])`;
`scope` verbatim (`route_stop`/`trip_call`); `route_id`, `trip_id` nonblank;
`source_route_stop_id` verbatim; `route_stop_id` = `routeStopKey(route, version, route stop)`
when a route is given, else null; `call_sequence`, `service_id` null; `group_code` verbatim;
`source_object_id = compositeKey [scope; route; trip; route_stop; group]`. The route-stop pass
then resolves it (section 7.6). [overlay] base rows projected with
`feedId(feed, "restriction-projection", [restriction; trip])`. Empty for CZPTT.

### 5.5 `transfer`

Section 2.10.

---

## 6. Generated identifiers

### 6.1 `Identity.feedId` (and its dead twin `bindingId`)

```
feedId(feed, kind, fields) = feed + ":" + kind + ":" + hex32
hex32 = lower-hex SHA-256( "jrutil-serving-v2\n" + kind + "\n" + canonical(fields) )[0..32]
canonical(fields) = fields stably sorted by name (ordinal),
                    each encodeComponent(name) + "=" + encodeComponent(value),
                    joined with "&"
encodeComponent(s): UTF-8 bytes; A–Z a–z 0–9 - . _ ~ kept, every other byte "%XX" (upper hex)
```

`:` is encoded as `%3A` here (unlike `compositeKey`). Null values are passed as `""`
(F# `string null`). `bindingId kind fields = "v1:" + kind + ":" + hex32` is unused.

Uses (field names exactly as passed):

| Kind | Fields | Relation |
|---|---|---|
| `transfer` | `from_stop_id`, `to_stop_id`, `from_route_id`, `to_route_id`, `from_trip_id`, `to_trip_id` (GTFS cells) | `transfer.transfer_key` |
| `note-assignment` (CZPTT) | `note`, `scope`, `trip`, `from`, `to` (`from`/`to` = decimal or `""`) | `assignment` |
| `note-assignment` (JDF) | `note`, `scope`, `route`, `trip` | `assignment` |
| `feature` | `feature` | `assignment` (CZPTT) |
| `trip-feature` / `route-feature` / `location-feature` | `trip`/`route`/`location`, `code`, `kind` | `assignment` (JDF) |
| `restriction` | `scope`, `route`, `trip`, `route_stop`, `group` | `travel_restriction` |
| `restriction-slot` | `assignment` (= restriction id), `route_stop` (= slot id); feed = text of the restriction id before its first `:` | `travel_restriction` |
| `assignment-projection`, `connection-projection`, `restriction-projection` | `assignment`/`connection`/`restriction` (base id), `trip` (output trip) | overlay |

### 6.2 Composite keys

`compositeKey(components) = join("/", encodeComponent(c).Replace("%3A", ":"))` — readable
colons, `/` and everything else escaped (`jdf:route:000645:1/10%2Fwest`).

- `route_stop_id = compositeKey [route_id; direction or "-"; location_id; visit]`, visit =
  1-based occurrence of the location in the merged slot list of that route direction
  (section 7). Example: `czptt:route:line:5007:3736:106/-/czptt:stop:CZ:54212/1`.
  Location ids containing `%` (CZPTT ids are `EscapeDataString`ed) would be re-escaped as `%25`.
- `routeStopKey(route, version, stop)` = `compositeKey [route; stop]` or
  `[route; version; stop]` — the JDF compiler's route-stop key, held transiently in
  `trip_call.route_stop_id` and in restriction rows until the route-stop pass; never published.
- `source_object_id` of JDF restrictions (5.4); `cis:line_trip`-like `line_course` identifier
  (4.1 row 2).

### 6.3 Shape and pattern IDs (`RouteShapes`)

- Pattern id = lower-hex SHA-256 of `class + ("\n" + lat + "," + lon + "," + (1|0))*` with
  coordinates formatted `0.0000000` (invariant) after `Math.Round(x, 7)`, first 20 hex chars.
- Shape id = `<feed>:shape:motis:` + first 20 hex chars of SHA-256 of
  `class + ("\n" + lat + "," + lon)*` over the companion's returned points (formatted
  `0.0000000`). Patterns with identical geometry share a shape.

### 6.4 IDs the writer does not generate but copies

Trip, route, stop, agency, service, block, note ids come from the compilers (2.x, 3.2).
CZPTT service ids `czptt:service:exc:<i>` depend on a group index over the sorted set of all
PA ids, so they change between runs whenever PAs are added or removed (SERVING-R061).

### 6.5 Places where an ID or packed string is parsed

The rewrite forbids all of these; each must become a field.

| # | Where | Parse | Information recovered |
|---|---|---|---|
| 1 | `PackageGtfsRelations.czpttParts` | trip id text after the last `:` → `Int16` | `run_part` |
| 2 | `czpttParts`, `PackageKeyRelations` row 5 | `cz_trips.source_trip_ids` split on `|`, tokens `PA=`/`TR=` | `run_key`, PA and TR keys |
| 3 | `RouteStopWriter` step 9 | restriction id text before the first `:` | feed prefix for `restriction-slot` ids |
| 4 | `writeOrderedShapes`, `gtfsRelations` | shape id starts with `overlay:` | `generation_method` `source` vs `compiler` |
| 5 | `gtfsRelations` | stop id starts with `overlay:` | `coordinate_source = overlay` |
| 6 | `PackageKeyRelations` rows 7–8 | output id starts with `overlay:` | `binding_method` `source_native` vs `structural_match` |
| 7 | `PackageRows.sourceNamespace` | source id ends with `-gtfs` | namespace system |
| 8 | `PackageRows.bindingMethod` | method string prefixes `companion_assertion`, `structural` | `binding_method` |
| 9 | `PackageRows.coordinateSource` | substrings `osm`, `route-time`, `route-end`, `sr70`, `gapfill`, literal `overlay` | `coordinate_source` |
| 10 | `PackageReader.prepareCompilerView` (overlay base view) | `cis:line_trip` identifier split at the first `:` | CIS line and trip number for the overlay |
| 11 | `CzPttBundle.convert` (upstream) | stop id ends with `:platform:BUS` | point identity of NAD boarding points |
| 12 | `writeOrderedTripCalls` | `"czptt:stop:" + "<CC>:<code>"` rebuilt from the sidecar instead of the id the stop was created with | railway point location id |
| 13 | key rows 1, 7, 8 | `source id == output id` | "identity binding, skip" |

---

## 7. Route-stop order (`RouteStopWriter.finalize`, `RouteStopOrder.merge`)

Runs over the finished `trip.parquet`/`trip_call.parquet` of every producer.

### 7.1 Grouping

1. Each trip belongs to the group `(route_id, direction)`; a null direction is its own group
   (`direction` null for every CZPTT trip, so **both travel directions of a CZPTT route merge
   into one slot list**, visible in the rail package as visit `/2` on the second direction).
2. Passenger calls only (`passenger_service = true`); locations interned. Per call:
   `time` = departure, else arrival, else unknown; `zones` = the call's zone set (7.5).
3. Per trip, calls sorted by sequence; the trip's *pattern* is its location sequence, keyed
   `compositeKey(location ids)`. Patterns are distinct per group; `weight` = number of trips;
   the pattern's representative trip is the **ordinally smallest trip id** running it.
   A call's trip unknown in `trip` throws `"trip_call references unknown trip …"`.
4. Pattern times: for the representative trip, fraction `(t − tmin)/(tmax − tmin)` per call,
   `tmin/tmax` over its known times; all NaN if fewer than 2 known times or `tmax ≤ tmin`;
   NaN for unknown times.

### 7.2 Merge (`RouteStopOrder.merge`)

Groups processed in ordinal `route_id`, then direction (null as −1). Within a group the
patterns (input sorted by key ordinal) are merged in order **weight descending, then length
descending, then key ordinal ascending**. For each pattern:

1. LCS of the current merged slot list and the pattern's locations (DP table from the end).
   Traceback from the start: take a match when the locations are equal and
   `T[i,j] = T[i+1,j+1] + 1`; otherwise advance the merged list when
   `T[i+1,j] ≥ T[i,j+1]` (prefer aligning to the earliest slot), else advance the pattern.
2. Aligned positions reuse the existing slot (adding the pattern's time to the slot's running
   mean); unaligned pattern positions become new slots.
3. Each gap between aligned pairs (and the tail) is filled by interleaving the existing slots
   of that gap with the new ones, keeping each side's order: a new slot goes before an existing
   one only if both times are known and the new time is **strictly** smaller.
4. Every repeated visit of a location is its own slot (loop lines keep both rows).

Output: slot locations in order and, per pattern, the slot of each position.

### 7.3 Numbering and IDs

`sequence` = 1-based slot position. `route_stop_id` as in 6.2; the visit counter counts
occurrences of the location in the merged list of the group. `route_stop` row:
`route_stop_id`, `route_id`, `direction` (group value), `sequence`, `location_id` (always the
stop place / location, never a boarding point).

### 7.4 `trip_call.route_stop_id`

Every passenger call gets the slot of its position in its trip's pattern (binary search of its
sequence among the trip's passenger-call sequences); non-passenger calls get null.

### 7.5 Zones

Zone facts (`PackageWriter.zoneInputs`), precedence per passenger call:
1. explicit call zones: `cz_trip_stop_zones.txt` rows (trip, stop_sequence) with non-blank
   `zone_code`; zone = (`zone_code`, nonblank `ids_system_id`); duplicates removed, source order
   kept. [czptt] CZPTT IDS zones (`zone_system = "PID"`); [overlay] base call zones and regional
   stop zones.
2. else zones of the call's incoming (compiler) route-stop key: [jdf] sidecar
   `source_route_stop_zone_metadata` sorted by (route, version, route stop id as text,
   int `zone_order`), system null.
3. else zones of its location: `cz_stop_zones.txt` rows with blank `route_id`
   (`stop_place_id` → zones).

A zone set is the ordered list; sets are interned by `code U+001F system` joined by U+001E, so
the same zones in another order are a different set. A slot is *unanimous* when every call at
it has the same non-empty set: then one `route_stop_zone` row per zone
(`source_order` = 0-based index). Otherwise every call at the slot with a non-empty set gets
`call_zone` rows (`trip_id`, `sequence`, `source_order`, `zone_code`, `zone_system`); calls
without zones get nothing. Contract: a call's zones are its slot's `route_stop_zone` rows if any,
else its `call_zone` rows.

### 7.6 Travel restrictions

For every incoming restriction row:
- with `trip_id`: if `call_sequence` set → the slot of that call; else if `route_stop_id` (the
  compiler key) set → the slot of the first call of that trip carrying that key; set
  `route_stop_id` = slot id and `call_sequence` = that call; unresolved → both null (other
  columns kept). **Trip-scoped JDF rows have no route, hence no key, hence never resolve**:
  all 457 overlay-package rows have null `route_stop_id` and `call_sequence` (contract says
  "a trip-scoped one points at its call").
- without trip and without key: kept as is.
- key already a slot id (base row carried by an overlay): kept.
- otherwise: slots reached by any call carrying that key: exactly one → that slot;
  several → one row per slot with id `feedId(<feed parsed from id>, "restriction-slot", …)`;
  none → `route_stop_id` null.

### 7.7 Diagnostics (`diagnostics.json` → `route_stop_order`)

`route_directions` (groups), `patterns` (sum of distinct patterns), `slots`,
`maximum_patterns_per_route_direction`, `route_stop_zones` (rows), `call_zones` (rows),
`unmatched_call_zone_calls` = explicit call-zone keys that matched no passenger call.
Rail package: 8,571 / 19,196 / 71,391 / 101 / 2,723 / 118,005 / 0.

---

## 8. Route-shapes phase (`RouteShapes`, optional)

Enabled by `--route-shapes` (`RouteShapeOptions { companion; graph; cache }`); runs after the
route-stop pass, before the GTFS projection. Never fails the build.

### 8.1 Scan

- Trips with a non-null `shape_id` are counted `already_shaped` and left alone.
- Mode class from `route.mode` (`shapeClass`): `bus`, `trolleybus` → `bus`; `tram`, `metro`,
  `rail` → `rail`; `water` → `ferry`; anything else (`cable`, `funicular`) → no class, counted
  in `not_routed.unsupported_mode[mode]`.
- A location is *exact* when `coordinate_precision = "exact"` and both coordinates present.
- Match points of a trip, all calls in sequence order (passenger and railway points): the
  boarding point's position if exact (Rust, SERVING-R031: or placed by the post estimator),
  else the location's if exact, else skipped (a skipped passenger call counts in
  `estimated_calls_skipped`). Coordinates `Math.Round(x, 7)`;
  `waypoint = not passenger_service`. Each point remembers its call's sequence.
- Patterns = distinct (class, points) by pattern id (6.3); a trip with zero match points still
  gets a pattern (id of the class alone).

### 8.2 Companion input and command line

`patterns.csv` in a temporary directory, LF, UTF-8 without BOM, header
`pattern_id,class,seq,lat,lon,waypoint`, patterns in ordinal id order, one row per match point,
`seq` 0-based, coordinates `0.0000000`, waypoint `1`/`0`. The companion validates the header,
the order, contiguous `seq`, coordinate range and `waypoint ∈ {0,1}`, but **ignores the
waypoint flag** (`route-shapes/src/patterns.cc`).

```
<companion> route --graph <graph> --patterns <work>/patterns.csv --out <work>/out [--cache <cache>]
```

stdout/stderr logged line by line. Non-zero exit or a start failure → every pattern unshaped
with reason `companion_failed`. With zero patterns the companion is not run.

### 8.3 Companion output (`route-shapes/README.md`, `src/run.cc`)

Sorted by pattern; cold/warm runs and any thread count byte-identical:
- `pattern_status.csv`: `pattern_id,class,status,points,routed_m,beeline_m`; status
  `routed`, `partial_beeline`, `failed`, `unsupported`, `too_few_points`.
- `shapes.csv`: `pattern_id,seq,lat,lon,dist_m` (Douglas–Peucker per span, default 2 m,
  1e-7°, consecutive duplicates dropped, every match point stays a vertex).
- `offsets.csv`: `pattern_id,match_seq,point_index,dist_m`.
- `diagnostics.json`, `manifest.json` (tool, format, osr commit, graph key, simplify tolerance,
  patterns FNV-1a).

The companion's cache (`--cache`) is keyed by graph key, osr commit and tolerance (README);
the F# writer has no cache of its own.

### 8.4 Mapping into the relations

- A pattern is accepted when it has a status row and **at least one shape point**, whatever its
  status (best effort; pinned by `Route shapes are best effort: a shape away from a stop is
  still taken`). There is no check that the offsets line up with the match points (the contract
  says shapes are kept "unless its output does not line up with the pattern"; the code has no
  such check, and a shorter offsets list would throw).
- Unaccepted patterns: reason `companion_failed`, the companion status, or `missing` (no status
  row) → `unshaped_patterns[reason]`.
- `shape`: new rows appended after the existing ones in ordinal shape-id order,
  `generation_method = motis`.
- `shape_point`: per new shape, `sequence` = 0-based point index, `longitude`, `latitude`,
  `distance_traveled` = companion `dist_m`.
- `trip.shape_id` set for every trip whose pattern was accepted.
- `trip_call.shape_distance_traveled` for those trips: the `dist_m` of `offsets.csv` row *k*
  of the pattern (rows taken in file order; `point_index` ignored), where *k* is the index of
  the call's sequence among the trip's match-point sequences; calls that are no match point
  get null. Trips not shaped keep null.

### 8.5 Diagnostics (`route_shapes` section)

`trips`, `already_shaped`, `routed_trips` (trips with a pattern, routed or not), `patterns`,
`not_routed.unsupported_mode{mode: n}`, `estimated_calls_skipped`, `shaped_patterns`,
`shapes`, `shaped_trips`, `unshaped_patterns{reason: n}`, `companion` (the companion's
`manifest.json` verbatim, when present). Absent when the phase is not run. Oběhy copies the
section into `release.json` (`route_shapes.summaries`).

---

## 9. GTFS projection (`GtfsProjection.tables`)

Cell formatting: text verbatim (null → empty); integers invariant; doubles `ToString("R")`
(`0.0` → `0`); booleans `1`/`0`; dates `yyyyMMdd`; times
`[-]HH:MM:SS` from seconds (hours zero-padded to at least 2 digits, negative values get `-`).

| File | Columns ← source | Filter / transformation |
|---|---|---|
| `agency.txt` | `agency_id, agency_name, agency_url, agency_timezone, agency_lang, agency_phone, agency_fare_url, agency_email` ← `agency_id, name, url, timezone, language, phone, fare_url, email` | — |
| `stops.txt` | `stop_id, stop_name, stop_desc, stop_lat, stop_lon, stop_url, location_type, parent_station, stop_timezone, wheelchair_boarding, platform_code` ← `location_id, name, description, latitude, longitude, url, kind, parent_location_id, timezone, wheelchair_boarding, public_code` | `operational_point` rows dropped; `location_type` = `1` for a `stop_place` that is some row's `parent_location_id`, else `0` (recomputed, SERVING-R009) |
| `routes.txt` | `route_id, agency_id, route_short_name, route_long_name, route_desc, route_type, route_url, route_color, route_text_color, route_sort_order` ← `route_id, agency_id, short_name, long_name, description, gtfs_route_type, url, color, text_color, sort_order` | — |
| `trips.txt` | `route_id, service_id, trip_id, trip_headsign, trip_short_name, direction_id, block_id, shape_id, wheelchair_accessible, bikes_allowed` ← `route_id, service_id, trip_id, headsign, short_name, direction, block_key, shape_id, wheelchair_accessible, bikes_allowed` | — |
| `stop_times.txt` | `trip_id, arrival_time, departure_time, stop_id, stop_sequence, stop_headsign, pickup_type, drop_off_type, shape_dist_traveled, timepoint` ← `trip_id, scheduled_arrival, scheduled_departure, boarding_point_id ?? location_id, sequence, stop_headsign, pickup_type, dropoff_type, shape_distance_traveled, timepoint` | `passenger_service = true` only; `shape_dist_traveled` is metres along the generated shape |
| `calendar.txt` | `service_id, monday..sunday, start_date, end_date` ← `service_id`, bit *i* of `weekday_mask` (bit 0 Monday), `valid_from, valid_to` | every service, including mask-0 rows |
| `calendar_dates.txt` | `service_id, date, exception_type` ← `service_id, service_date`, `added ? 1 : 2` | — |
| `shapes.txt` | `shape_id, shape_pt_lat, shape_pt_lon, shape_pt_sequence, shape_dist_traveled` ← `shape_point` | only when `shape_point` has rows |
| `transfers.txt` | `from_stop_id, to_stop_id, from_route_id, to_route_id, from_trip_id, to_trip_id, transfer_type, min_transfer_time` ← `transfer` | only when `transfer` has rows; `maximum_waiting_time` deliberately not published (validation rejects a `max_waiting_time` column) |
| `feed_info.txt` | compiler's GTFS `feed_info.txt` table passed through verbatim | [czptt] `feed_publisher_name = "Oběhy project (via JrUtil)"`, `feed_publisher_url = "https://obehy.cz"`, `feed_lang = "cs"`, `feed_start_date` = min PA calendar start, `feed_end_date` = max PA calendar end (start when no end), `feed_version = "czptt:" + max CzpttCreation as yyyyMMdd'T'HHmmss`, `feed_contact_email = "admin@obehy.cz"` (`CzPttAgencies.feedInfo`, `Gtfs.obehyFeedInfo`) |

Deliberately not published: `run_key`, `run_part`, railway points, `subsidiary_*`,
`active_line_code`, `source_kilometre`, zones (`stops.zone_id` is no longer written), location
metadata, `coordinate_*`, `timetable_kind`, `route.mode`, notes, assignments, claims,
restrictions, keys, `maximum_waiting_time`, `shape.generation_method`.

The MOTIS transfer rewrite (type 1 → stop-pair type 2, min 0) is not in the projection: it is
upstream in `CzPttToGtfs.fs`, so serving `transfer` already holds the rewritten rows (2.10).

---

## 10. `manifest.json` and `diagnostics.json`

Both are written with `System.Text.Json`, 2-space indentation, a trailing `\n`, UTF-8 without
BOM, default encoder (non-ASCII and `+` escaped as `\uXXXX` with upper-case hex, e.g.
`\u002B00:00`).

### 10.1 `manifest.json` (`PackageWriter.writeManifest`)

Keys in this order (optional ones only when present):

```jsonc
{
  "bundle_format": "jrutil-production",
  "bundle_version": 3,
  "serving_schema_version": "5.2",
  "diagnostics_schema_version": 1,
  "identity_contract": "jrutil-identity-v1",
  "feed_version": "sha256:<hex>",
  "build_spec_sha256": "<hex>",
  "compiler": { … },            // optional: compiler manifest "conversion" verbatim
  "contract_valid": true,
  "publication_eligible": true,
  "calibration": false,
  "service_horizon": { … },     // optional: compiler manifest "gvd" verbatim
  "sources": [ { … } ],         // optional (validation requires it)
  "namespaces": [ { "name": "cis:line", "entity_kind": "route" }, … ],
  "relations": [ {
      "name": "agency", "path": "serving/agency.parquet",
      "schema": [ { "name": "agency_id", "type": "string", "nullable": false, "enum": "…"? }, … ],
      "primary_key": [ … ],
      "foreign_keys": [ { "fields": [ … ], "relation": "…", "target_fields": [ … ] } ],
      "row_count": 42 }, … ],
  "files": [ { "path": "diagnostics.json", "size_bytes": 970, "sha256": "<hex>" }, … ]
}
```

- `feed_version` = `"sha256:"` + lower-hex SHA-256 of the `files` entries (sorted ordinal by
  path, every file of the package except `manifest.json`) rendered `path + "=" + sha256` and
  joined with `\n` (no trailing newline).
- `build_spec_sha256` = SHA-256 of the compiler manifest's **raw JSON text**
  (`JsonElement.GetRawText()`). For CZPTT that text is the .NET serialisation (indented) of
  `{"schema_version":1,"source_format":"czptt","source":{"source_id":"national-czptt",
  "payload_kind":"file"|"directory","payload_sha256":<input digest>}}`; the input digest is the
  file's SHA-256, or for a directory the SHA-256 of sorted `relative/path=sha256` lines joined
  with `\n` (`CzPttPackage.inputDigest`).
- `contract_valid` is hard-coded `true` (the package is validated before activation).
- `publication_eligible` = compiler manifest `publishable` (default `true`);
  `calibration` = compiler manifest `calibration` (default `false`).
- `service_horizon` = compiler manifest `gvd` verbatim (JDF/overlay
  `{"year":2026,"start_date":"20251214","end_date":"20261212"}`; absent for CZPTT).
- `compiler` = compiler manifest `conversion` verbatim (overlay `{"tool":"jrutil","version":…}`;
  absent for CZPTT although the contract says the manifest records the converter version).
- `sources`: base package manifest sources first (overlay), then the compiler's; each from
  `sources[]`, else `source`, else `source_snapshot`; deduplicated by `source_id` (first wins);
  objects verbatim. CZPTT: one object `{source_id, payload_kind, payload_sha256}`.
- `namespaces`, `relations`: `Schema` order; `schema` types as in 1.2, `enum` only for coded
  fields; `row_count` = rows written.
- `files`: every payload, ordinal by path, with byte size and lower-hex SHA-256.

What the Python consumers read:
- `production_package.read_manifest`: `bundle_format == "jrutil-production"`,
  `bundle_version == 3`, major of `serving_schema_version` == 5, `contract_valid is True`,
  `publication_eligible is True` (when publishing), presence of `gtfs.zip`,
  `diagnostics.json`, `serving/`.
- `release/load.py`: `relations[].name/path/schema[].name/type/nullable/primary_key/row_count`
  (schema checked against the vendored contract with minor tolerance, `contract.MINOR_FIELDS`
  maps `trip_call.source_kilometre` to minor 2), `files[].path/size_bytes/sha256` (size and
  SHA-256 of every relation file checked), `serving_schema_version`, `feed_version` (stored in
  `control.package`), the whole manifest stored as JSONB. Row counts after COPY are compared
  with `row_count`.
- `route_shapes.summaries`: `diagnostics.json` → `route_shapes`.
- Nobody reads `service_horizon`, `namespaces`, `calibration`, `build_spec_sha256`,
  `compiler`, `identity_contract`, `diagnostics_schema_version`, `sources` (except via the
  stored JSONB), nor Parquet key/value metadata.

### 10.2 `diagnostics.json` (`writeDiagnosticsSummary`)

```jsonc
{
  "schema_version": 1,
  "counts_by_code": [ { "severity": "warning", "code": "…", "count": 67 }, … ],
  "examples_by_code": { "<code>": [ { "severity": "…", "source_object_id": "…", "message": "…" } ] },
  "coverage_populations": { "<report name>": [ { "<csv column>": "<text>" }, … ] },
  "route_stop_order": { … section 7.7 … },
  "route_shapes": { … section 8.5 … }      // optional
}
```

- Events: the compiler diagnostics object's `"diagnostics"` array items (`severity` default
  `warning`, `code` default `unknown`, `source_object_id`, `message` default `""`) plus every
  row of the compiler report `diagnostics.csv` (severity `warning`).
- `counts_by_code`: count per (severity, code), sorted by code then severity (ordinal).
- `examples_by_code`: per code (ordinal), up to 10 events sorted by
  (`source_object_id`, `message`).
- `coverage_populations`: the non-empty report tables among `coverage.csv`,
  `coverage_by_mode.csv`, `coverage_by_tier.csv`, `coverage_by_source.csv`,
  `trip_coverage_populations.csv`, `trip_coverage_populations_by_mode.csv`,
  `snapshot_day_coverage.csv`, `exclusions.csv`, named without extension, rows as objects of
  text cells (overlay only).
- Validation caps the file at 1 MiB. Rail package: empty counts and examples (CZPTT
  diagnostics are dropped, 3.1).

The optional `--diagnostics-out` artifact (`writeDiagnosticArtifact`, separate from the package)
has its own `manifest.json` (`diagnostics_format: "jrutil-diagnostics"`, `traces_included`,
`evidence_references`, `files`).

---

## 11. Validation (`Validation.inspect`, raised by `validatePackage`)

`validatePackage` throws `ArgumentException("Invalid JrUtil production package:\n…")` listing
all errors. Steps, in order (FK, value, trip-part and GTFS checks run only if steps 1–17 found
no error):

1. Directory exists, else `Package directory does not exist: <root>`.
2. Every expected file present: `Missing declared production file: <path>`
   (expected = `gtfs.zip`, `manifest.json`, `diagnostics.json`, `serving/<rel>.parquet` × 19).
3. No other file: `Undeclared production file: <path>`.
4. `manifest.json`: `bundle_format` = `jrutil-production`, else `manifest.json: bundle_format must be jrutil-production`.
5. `bundle_version` = 3 (number), else `… must be 3`.
6. `serving_schema_version` a string `major.minor` with major 5, else `manifest.json: serving_schema_version must be 5.<minor>`.
7. `diagnostics_schema_version` = 1.
8. `identity_contract` = `jrutil-identity-v1`.
9. `files` array present, else `manifest.json: files inventory is missing`.
10. Every payload (expected files minus `manifest.json`) inventoried: `manifest.json: payload is not inventoried: <path>`.
11. No extra inventory entry: `manifest.json: undeclared inventory entry: <path>`.
12. For each inventoried present payload: size equal (`<path>: size does not match manifest`), SHA-256 equal (`<path>: SHA-256 does not match manifest`).
13. `relations` array present (`manifest.json: relations are missing`); every schema relation declared (`manifest.json: relation is not declared: <name>`); no unknown relation (`manifest.json: unknown relation declaration: <name>` — not minor-tolerant).
14. Per relation Parquet file: field count (`expected N fields, found M`); per field name, CLR type (date may read as `DateTime`) and nullability (`field <name> type/nullability mismatch`).
15. Parquet metadata `obehy.schema_version` major 5 (`serving schema metadata mismatch`), `obehy.relation` = name (`relation metadata mismatch`).
16. Per row group: ≤ 65,536 rows (`row group exceeds the 65,536-row validation budget`); no null primary-key cell (`null primary key at row <n>`, stops PK checking for the file); PK uniqueness by 64-bit XxHash of type-tagged key tokens (`s`+text, `d`+`yyyy-MM-dd`, `f`+`R` double, `v`+invariant) sorted and scanned (`<n> duplicate primary keys`); total rows = manifest `row_count` (`manifest row count X, physical row count Y`).
17. `sources`: non-empty array, each with `payload_sha256` matching `^[0-9a-f]{64}$` and not 64 zeros (`manifest.json: source <id> has no payload_sha256 digest`); missing → `manifest.json: sources are missing`.
18. Foreign keys: for every declared FK, every row whose FK cells are all non-null must hash-match a target key (64-bit XxHash of tokens, one target at a time): `serving/<rel>.parquet: <n> rows have (<fields>) missing from <target>, e.g. <values>`.
19. Coded values: every non-null coded cell in its enumeration (int16 as decimal text): `… rows have <field> outside <enum>, e.g. <v>`.
20. Feed prefix: every relation with a single text PK (`agency`, `location`, `route`, `service_calendar`, `trip`, `route_stop`, `shape`, `transfer`, `service_note`, `assignment`, `connection_claim`, `travel_restriction`): PK starts with `jdf:`, `overlay:` or `czptt:` (`… have <key> without a feed prefix`).
21. Key encodings: `source_key` rows of a declared namespace must have `entity_kind` = the namespace's kind and `identifier` matching its regex; `call_key` rows the regex only (`… do not match their namespace's entity kind or identifier encoding`). Regexes: `cis:line` `^[0-9]{6}$`; `cis:line_trip` `^[0-9]{6}:[1-9][0-9]*$`; `czptt:train_number` `^[0-9]+$`; `ids-jmk:line_course` `^[^/]+/[^/]+$`; `sr70` `^[0-9]{5}$`; `sr70:track` `^[0-9]{5}:.+$`; all others `^.+$`.
22. Trip parts (only trips with `run_key`): no non-passenger call on a trip whose route `mode = "bus"` (`<n> non-passenger calls belong to bus trip parts`); consecutive `run_part`s share one boundary call (equal `sequence`, `location_id`, `scheduled_arrival`, `scheduled_departure` between last call of n−1 and first of n): `serving/trip.parquet: <n> trip parts do not share one boundary call with the previous part, e.g. <trip>`.
23. GTFS projection (order-independent multiset fingerprint: count, sum and xor of XxHash64 of U+001F-joined cells):
    `stop_times.txt` = passenger calls `(trip, sequence, boarding ?? location, arrival, departure, pickup, dropoff)` (GTFS blank pickup/dropoff read as `0`, times parsed back to seconds);
    `trips.txt` = `(trip_id, route_id, service_id)`; `calendar.txt` = `(service_id, valid_from, valid_to, weekday_mask)`;
    `calendar_dates.txt` = `(service_id, date, added ? 1 : 2)`. Error `gtfs.zip/<file>: <n> rows are not the projection of <m> serving rows`.
24. ZIP: entries flat (`gtfs.zip: entries must be flat GTFS table names`), ordinal order (`gtfs.zip: entries are not in deterministic ordinal order`), `transfers.txt` header without `max_waiting_time` (`gtfs.zip/transfers.txt: max_waiting_time is not a standard GTFS column`).
25. `diagnostics.json` ≤ 1 MiB (`diagnostics.json: bounded production summary exceeds 1 MiB`).
26. Package ≤ 2 GiB (`production package exceeds the 2 GiB acceptance limit`).

Not validated: row content of `agency`, `routes`, `stops`, `shapes`, `transfers` GTFS files;
`route_stop` coverage of passenger calls; zones' unanimity rule; `feed_version`; JSON shape of
`diagnostics.json`. `compareByteIdentical` validates both packages and compares the file sets and
every file's size and SHA-256.

---

## 12. Comparison (`Comparison.comparePackages`, reference only)

- Files: `file` differences for paths in only one package.
- `manifest.json` and `diagnostics.json` compared per top-level key as canonical JSON (object
  keys sorted ordinal, arrays in order), ignoring `files` and `feed_version` in the manifest
  (`kind = manifest`/`diagnostics`, subject = key).
- Each serving relation present on both sides: rows matched by a SHA-256-derived 128-bit hash of
  the type-tagged primary-key cells (null `\0`, text `s…`, other `v…`); counts removed / added /
  changed, duplicate keys reported, up to 5 samples each, changed samples list
  `field: left -> right` (`kind = relation`). `column <glob>` expectations drop matching
  columns from key and value comparison in every relation.
- GTFS tables: multisets of rows, each canonicalised as non-empty `column=value` cells sorted
  ordinal (column order and empty-vs-absent ignored); tables in only one ZIP reported
  (`kind = gtfs`). Legacy `extensions/*.txt` compared the same way (`kind = extension`).
- Expectation file lines: `<kind> <glob>` (`*`, `?`), `#` comments; a difference whose kind
  matches and whose subject matches the glob is expected; `isEquivalent` = no unexpected
  differences.

---

## 13. Rules

Verdicts: `port` (keep behaviour), `replace` (keep intent, clean form given), `drop` (nothing
depends on it). "Pinned by" names tests in `jrutil.tests/ServingContractTests.fs`.

**SERVING-R001 Generated-ID digest scheme**
- What: `feedId` (6.1): `<feed>:<kind>:` + first 32 hex of SHA-256 of
  `"jrutil-serving-v2\n" + kind + "\n" + canonical(fields)`.
- Where: `Serving/Identity.fs`.
- Why: stable, feed-prefixed, opaque ids independent of row order.
- Pinned by: none directly (indirectly every package test).
- Constants: `jrutil-serving-v2`, 32 hex chars, unreserved set `A-Za-z0-9-._~`.
- Verdict: `port` in `jr_model::identity` for the kinds the rewrite still emits (ids of
  `assignment`, `transfer`, … are PKs only; nobody joins on them across releases, so a clean
  scheme would also be acceptable — decide once, see open question 12).

**SERVING-R002 Composite keys keep colons**
- What: `compositeKey` escapes like `encodeComponent` but restores `%3A` to `:`; `/` is the
  only delimiter.
- Where: `Identity.compositeKey`.
- Why: readable ids; GTFS/JDF ids contain colons (comment in code).
- Pinned by: `Composite public keys preserve identifier colons and escape delimiters`.
- Constants: `/`, `%3A`.
- Verdict: `port` (route-stop ids are public FK targets in Oběhy).

**SERVING-R003 Route-stop slot id**
- What: `<route>/<direction or "-">/<location>/<visit>`; visit = occurrence count in the merged list.
- Where: `RouteStopWriter.finalize` step 4.
- Why: readable, contract-specified (`PRODUCTION_CONTRACT.md` "Route stops and zones").
- Pinned by: `Route stops are ordered slots with readable identifiers and call zones`,
  `Trips without a direction get their own route stop order`.
- Constants: `-`.
- Verdict: `port`.

**SERVING-R004 Coordinate precision**
- What: `missing` when either coordinate missing or both exactly 0.0; else the metadata precision
  (`estimated`/`missing`/otherwise `exact`), `exact` when no metadata; CZPTT GTFS stops have no
  metadata, so estimated CZPTT stops are `exact`.
- Where: `PackageGtfsRelations.gtfsRelations`, `PackageRows.coordinatePrecision`.
- Why: (0,0) because `fillStandardRequiredFields` fills missing coordinates with 0; the CZPTT
  gap is unknown (likely an oversight: `source_stop_metadata` is a JDF sidecar).
- Pinned by: none.
- Constants: `0.0, 0.0`.
- Verdict: `replace`: precision is a typed model field set by the compiler/pass that placed the
  coordinate (`Exact`/`Estimated`/`Missing`); no (0,0) sentinel. Ledger the CZPTT fix.

**SERVING-R005 Coordinate source by substring**
- What: provenance text → `osm` (prefix `osm`), `route_time` (contains `route-time`),
  `route_end` (contains `route-end`), `sr70` (contains `sr70`, case-insensitive), `gapfill`
  (contains `gapfill`, case-insensitive), `overlay` (exact), else `catalogue`; null for blank.
  Fallbacks: CZPTT point source by id or parent; `overlay` for `overlay:` ids.
- Where: `PackageRows.coordinateSource`, `gtfsRelations`.
- Why: unknown (adapter provenance strings were free text).
- Pinned by: `CZPTT trip parts share boundary calls and carry railway points only on rail`
  (`sz_sr70` → `sr70`).
- Constants: the substrings above.
- Verdict: `replace` with a typed `CoordinateSource` enum set where the coordinate is chosen.

**SERVING-R006 Route mode classification**
- What: `servingMode`: 0, 900–906 tram; 1, 400–405 metro; 2, 100–117 rail; 4, 1000–1021 water;
  5, 6, 7, 1400, 1701, 1300–1307 cable; 11, 800 trolleybus; everything else bus (200–209
  coaches, 700s, 714 NAD). `funicular` is never produced (GTFS 7 and 1400 are funiculars but map
  to `cable`).
- Where: `PackageBaseRelations.servingMode`.
- Why: "Ranges follow the overlay's mode classes" (comment).
- Pinned by: `Serving modes follow basic and extended GTFS route types`.
- Constants: the ranges above.
- Verdict: `replace`: one mode classifier in the model (rule 5); `route.mode` is a model field and
  `gtfs_route_type` derived from it plus the source's extended type; decide whether 7/1400 become
  `funicular`.

**SERVING-R007 Default timetable kind**
- What: `regular` unless the JDF sidecar says detour or the overlay base says otherwise.
- Where: `gtfsRelations`.
- Why: contract field is non-null (OUTPUT_MIGRATION's "null for non-JDF" is outdated).
- Pinned by: none.
- Constants: `regular`, `detour`, `"True"`.
- Verdict: `port` as a typed route field defaulting to `Regular`.

**SERVING-R008 Location kind and metadata inheritance**
- What: kind from GTFS `location_type`/`parent_station`; a boarding point without metadata
  inherits its stop place's `municipality … coordinate_precision/source`.
- Where: `gtfsRelations`.
- Why: unknown (JDF metadata keyed by stop place).
- Pinned by: none.
- Constants: —.
- Verdict: `replace`: kind is a model fact; inheritance, if wanted, is a model pass, not a writer rule.

**SERVING-R009 GTFS `location_type` recomputation**
- What: in `stops.txt`, a `stop_place` gets `location_type = 1` only if some location names it
  as parent; otherwise 0.
- Where: `GtfsProjection.tables`.
- Why: "A stop place that groups boarding points is a station" (comment): GTFS forbids
  location_type 1 stops in stop_times.
- Pinned by: none.
- Constants: —.
- Verdict: `port` in `jr-publish`'s GTFS projection.

**SERVING-R010 Exception-only services**
- What: a service only in `calendar_dates` gets a calendar row with mask 0 and min/max exception
  date (added and removed).
- Where: `gtfsRelations` (`exceptionCalendars`).
- Why: contract: every service has exactly one calendar row.
- Pinned by: `GTFS calendars are the projection of the calendar relations`.
- Constants: mask 0.
- Verdict: `replace`: derive calendar + exceptions from the model's `DateSet` in `jr-publish`
  (choose mask/range to minimise exceptions or keep mask 0; contract allows both).

**SERVING-R011 Railway points on trip parts**
- What: section 3.3 inclusion rule (rail parts only; interior points; leading points on the
  opening part; trailing points on the closing part).
- Where: `PackageGtfsRelations.writeOrderedTripCalls`.
- Why: contract "Calls and trip parts": the bus does not pass the railway points.
- Pinned by: `CZPTT trip parts share boundary calls and carry railway points only on rail`.
- Constants: mode `rail`.
- Verdict: `port`, but as a CZPTT compiler decision producing the model's calls, not a writer
  join of two tables by sequence.

**SERVING-R012 Non-passenger call fields**
- What: pickup/dropoff 1, no boarding point/route stop, `timepoint` = any time present, times
  verbatim (unshifted).
- Where: `writeOrderedTripCalls`.
- Why: contract `trip_call` description.
- Pinned by: same test (pickup 1, empty route stop).
- Constants: `1s`.
- Verdict: `replace`: same values, but times from the same shifted service-time base as
  passenger calls.

**SERVING-R013 Operational columns on passenger calls**
- What: `subsidiary_code/name`, `active_line_code` copied from the PA call with the same sequence.
- Where: `writeOrderedTripCalls`.
- Why: contract "The CZPTT subsidiary location and active line code are trip_call columns".
- Pinned by: none.
- Constants: —.
- Verdict: `port` as call fields in the model.

**SERVING-R014 `run_key`/`run_part` recovered by parsing**
- What: run key from the `PA=` token of a packed string; part from the trip id's last `:` segment.
- Where: `PackageGtfsRelations.czpttParts`.
- Why: artefact of the GTFS detour.
- Pinned by: `CZPTT trip parts share …` (`run_key`/`run_part` values).
- Constants: `PA=`, `|`, `:`.
- Verdict: `replace`: `Trip { run: Option<RunRef { pa, part }> }` set by the CZPTT compiler.

**SERVING-R015 Trip-key validity**
- What: `[valid_from, valid_to]` of trip keys = the trip's service bounds (calendar range widened
  by every exception date, including removed ones).
- Where: `PackageKeyRelations.serviceBounds`.
- Why: contract: a trip key applies on its trip's dates within its validity; bounds are only a
  cheap prefilter.
- Pinned by: none.
- Constants: —.
- Verdict: `replace`: first..last date of the trip's `DateSet` (removed dates should not widen).

**SERVING-R016 SR70 point and track keys**
- What: `sr70` = CZ points with a 5-digit primary code; target the stop place or operational
  point; `sr70:track` = code + `:` + GTFS platform code (subsidiary name if non-blank, else code),
  excluding empty and `BUS`; validity = package bounds; method `identity`.
- Where: `PackageKeyRelations` (`railKeys`).
- Why: contract 5.1: realtime sources name points and tracks by SR70.
- Pinned by: none.
- Constants: `"CZ"`, 5 digits, `"BUS"`.
- Verdict: `replace`: emit from typed point identity `(country, primary_code)` and the
  boarding point's subsidiary **code** (not the display name); NAD points are already a distinct
  kind in the model, no `"BUS"` string test.

**SERVING-R017 CIS line-trip encoding**
- What: line `PadLeft(6,'0')`, trip `Int64.Parse` (leading zeros dropped), only for lines of 1–6
  ASCII digits.
- Where: `PackageKeyRelations`.
- Why: contract namespace encoding `582492:143`.
- Pinned by: `Source keys use source namespaces and call keys only differing sequences`,
  `Validation rejects coded values, unprefixed ids and malformed keys`.
- Constants: 6.
- Verdict: `port` (JDF).

**SERVING-R018 CZPTT call keys**
- What: `czptt:pa` call keys only where GTFS and PA sequence differ — never, by construction.
- Where: `PackageKeyRelations` (`pathCalls`), `CzPttBundle` (`source_call_metadata`).
- Why: contract: call keys only where sequences differ.
- Pinned by: none.
- Constants: —.
- Verdict: `drop` (serving keeps the PA sequence; nothing reads these rows; the rail package has 0).

**SERVING-R019 No identity rows**
- What: key rows whose source id equals the public id are skipped.
- Where: `PackageKeyRelations` rows 1, 7, 8.
- Why: contract: a public ID resolves to itself without a row.
- Pinned by: `Source keys use source namespaces …`.
- Constants: —.
- Verdict: `port`.

**SERVING-R020 Undeclared namespaces are dropped**
- What: rows in namespaces outside the 14 declared ones are silently removed.
- Where: `PackageKeyRelations.isDeclared`.
- Why: contract: a regional source without a declared namespace publishes no keys.
- Pinned by: none.
- Constants: the namespace list.
- Verdict: `replace`: make the namespace a closed enum in the model so undeclared keys cannot be
  constructed; report a diagnostic instead of silent drop.

**SERVING-R021 Binding-method mapping**
- What: section 4.1 `bindingMethod`.
- Where: `PackageRows.bindingMethod`.
- Why: overlay matcher method strings predate the enum.
- Pinned by: `Source keys use source namespaces …` (`structural_trip_evidence+full_signature` → `structural_match`).
- Constants: prefixes `companion_assertion`, `structural`.
- Verdict: `replace` with a typed binding method from the overlay pass.

**SERVING-R022 Namespace from source id**
- What: `pid-gtfs` → `pid:`; any id ending `-gtfs` loses the suffix.
- Where: `PackageRows.sourceNamespace`.
- Why: source ids are `<system>-gtfs`.
- Pinned by: `Source keys use source namespaces …`.
- Constants: `-gtfs`.
- Verdict: `replace`: the source descriptor names its key system.

**SERVING-R023 CZPTT note links**
- What: trip-less notes link to every part (`trip`); unresolved ranges → `trip`; resolved →
  `call_range` clipped to the part's actual calls (railway points included), `trip` if empty.
- Where: `PackageSemanticRelations.czpttRelations`.
- Why: contract "Typed semantics": a note whose range cannot be resolved links to every part.
- Pinned by: none.
- Constants: —.
- Verdict: `port` (as model `Assignment` edits from the CZPTT compiler).

**SERVING-R024 CZPTT feature table**
- What: central-note code → kinds table (5.2), whole-trip vs per-call, NAD parts skipped,
  request stop `0030` → `on_request`.
- Where: `CzPtt/CzPttToGtfs.fs:341-407`.
- Why: unknown beyond the CZPTT code list.
- Pinned by: none in the serving tests.
- Constants: codes 17, 22, 26, 27, 28, 29, 34, 36, `0030`.
- Verdict: `port`, table as a data file (`data/`), per AGENTS rule 4.

**SERVING-R025 JDF connection claims**
- What: `m`/`M` → `waits_for`/`connects_to`; origin sequence fallback `0`; derivation always
  `structured`; status always `unresolved`.
- Where: `PackageSemanticRelations.semanticRelations`.
- Why: contract `connection_claim` keeps every Navaznosti row.
- Pinned by: `Overlay projects native base semantics onto replacement trips`.
- Constants: `0`.
- Verdict: `replace`: an unresolvable origin call is a diagnostic and the claim is quarantined,
  never sequence 0.

**SERVING-R026 Restriction slot resolution**
- What: section 7.6, including `restriction-slot` ids with a feed parsed from the id, and
  trip-scoped rows that never resolve their call.
- Where: `RouteStopWriter.finalize` step 9.
- Why: contract: route-scoped → one row per reached slot; trip-scoped → its call.
- Pinned by: `Overlay projects native base semantics onto replacement trips`.
- Constants: —.
- Verdict: `replace`: resolve restrictions on the model (trip-scoped to the call carrying the
  source route stop, route-scoped to slots); feed from the package, not the id.

**SERVING-R027 Zone precedence and unanimity**
- What: section 7.5.
- Where: `PackageWriter.zoneInputs`, `RouteStopWriter.finalize`.
- Why: contract "Route stops and zones".
- Pinned by: `Route stops are ordered slots …`, `Zones fall back to calls where trips at a slot disagree`.
- Constants: —.
- Verdict: `port` the publication rule (slot vs call); the precedence of zone sources belongs in
  the model (a call's zones are a model fact).

**SERVING-R028 Route-stop merge algorithm**
- What: section 7.2 (weight, length, key order; LCS earliest-slot tie; strict-earlier insertion).
- Where: `RouteStopOrder.merge`.
- Why: printed line timetable order; order-independent of input (comment).
- Pinned by: `Route stops are ordered slots …`, `Trips without a direction get their own route stop order`.
- Constants: —.
- Verdict: `port` exactly (slot ids and sequences are public).

**SERVING-R029 Pattern times from the smallest trip**
- What: a pattern's relative times come from its ordinally smallest trip id.
- Where: `RouteStopWriter.finalize` step 3.
- Why: "so input order never changes the result" (comment).
- Pinned by: none.
- Constants: —.
- Verdict: `port`.

**SERVING-R030 Shape classes**
- What: bus/trolleybus → `bus`; tram/metro/rail → `rail`; water → `ferry`; else unsupported.
- Where: `RouteShapes.shapeClass`.
- Why: companion classes (osr profiles).
- Pinned by: `The route-shapes phase writes companion shapes …` (class `bus`).
- Constants: class names.
- Verdict: `port` as a mapping from the one mode enum.

**SERVING-R031 Match-point selection**
- What: exact boarding point, else exact location, else skip; railway points included as
  waypoints; 7-decimal banker's rounding.
- Where: `RouteShapes.scan`.
- Why: contract "Shapes".
- Pinned by: `The route-shapes phase writes …` (patterns.csv text).
- Constants: 7 decimals, `0.0000000`.
- Verdict: `port` (round half away from zero vs to even only matters for parity on exact ties;
  pick one and ledger).

**SERVING-R032 Best-effort shape acceptance**
- What: any pattern with ≥1 returned point is accepted; no line-up check; shapes deduplicated by
  geometry hash.
- Where: `RouteShapes.write`.
- Why: contract/STATIC_PIPELINE: best effort, never fails the build.
- Pinned by: `Route shapes are best effort: a shape away from a stop is still taken`,
  `A failing route-shapes companion leaves a valid unshaped package`.
- Constants: 20 hex chars.
- Verdict: `replace`: same acceptance, plus the missing check that offsets count = match points
  (reason `misaligned`), as the contract text already claims.

**SERVING-R033 Shape offsets on calls**
- What: offsets taken in file order, `point_index` ignored; non-match calls null.
- Where: `RouteShapes.assignTrips`.
- Why: contract 5.2 metres from first point.
- Pinned by: `The route-shapes phase writes companion shapes with metre offsets before projection`.
- Constants: —.
- Verdict: `port` (index by `match_seq`, not file order).

**SERVING-R034 Compiler shape distances and generation**
- What: haversine cumulative metres, `Math.Round(·, 1)`; `source` for `overlay:` ids else `compiler`.
- Where: `writeOrderedShapes`, `gtfsRelations`.
- Why: "feed distances have no common unit" (comment).
- Pinned by: none.
- Constants: radius 6,371,000 m, 0.1 m.
- Verdict: `replace`: generation method is a model field; distance via `jr_model::geo`.

**SERVING-R035 Transfer key**
- What: `feedId(feed, "transfer", GTFS selector columns)`.
- Where: `gtfsRelations`.
- Why: transfers have no source id.
- Pinned by: `In-seat transfers between trips carry no locations`.
- Constants: field names `from_stop_id` … `to_trip_id`.
- Verdict: `port` (with model field names instead of GTFS names if ids may change).

**SERVING-R036 MOTIS transfer rewrite**
- What: CZPTT type-1 transfers with stops become stop-pair type 2, min 0, no trips.
- Where: `CzPtt/CzPttToGtfs.fs:157-180` (upstream of the writer).
- Why: MOTIS ignores trip-specific transfers and applies its default minimum unless a stop-pair
  minimum exists (code comment).
- Pinned by: none.
- Constants: `2`, `0`.
- Verdict: `replace`: keep the source fact (timed NAD↔rail connection between two trips) in the
  model/serving `transfer`, apply the MOTIS adaptation only in the GTFS projection — but that
  changes serving rows; architect decision (open question 6).

**SERVING-R037 GTFS time format**
- What: `[-]HH:MM:SS`, at least two hour digits.
- Where: `GtfsProjection.time`.
- Why: GTFS.
- Pinned by: `Validation requires gtfs.zip to project the relations …`.
- Constants: —.
- Verdict: `port`.

**SERVING-R038 GTFS projection omissions**
- What: `max_waiting_time`, zones, operational points and every non-standard field not published.
- Where: `GtfsProjection`, `Validation.validateZip`.
- Why: contract "gtfs.zip contains only standard GTFS".
- Pinned by: `Production package has closed validated inventory and deterministic bytes`,
  `Compiler view restores transfer waiting times from serving relations`.
- Constants: —.
- Verdict: `port`.

**SERVING-R039 `feed_info.txt` pass-through**
- What: the compiler's feed info verbatim; CZPTT values in section 9.
- Where: `PackageWriter` → `GtfsProjection.write`; `CzPttAgencies.feedInfo`.
- Why: contract: build metadata passed through.
- Pinned by: none.
- Constants: publisher strings, `czptt:` + creation time.
- Verdict: `port` (values as build metadata in the timetable, strings in `data/`/policy).

**SERVING-R040 `feed_version` and `build_spec_sha256`**
- What: feed version hashes the sorted `path=sha256` list; build spec hashes the compiler
  manifest's raw .NET JSON text.
- Where: `PackageWriter.writeManifest`.
- Why: payload identity; build reproducibility.
- Pinned by: `Production package has closed validated inventory and deterministic bytes`.
- Constants: `sha256:`.
- Verdict: `port` `feed_version`; `replace` `build_spec_sha256` with a hash of a canonical build
  spec the rewrite defines (nobody reads it).

**SERVING-R041 Contract validity and eligibility flags**
- What: `contract_valid` hard-coded true; `publication_eligible` = `publishable` (default true);
  `calibration` default false.
- Where: `writeManifest`.
- Why: contract: recorded independently, so calibration packages stay ineligible.
- Pinned by: none.
- Constants: defaults.
- Verdict: `port`.

**SERVING-R042 Source list merge**
- What: base sources first, dedup by `source_id`, verbatim objects.
- Where: `writeManifest`.
- Why: overlay packages inherit base provenance.
- Pinned by: `Overlay projects native base semantics onto replacement trips` (source ids).
- Constants: —.
- Verdict: `port` with a typed source record.

**SERVING-R043 Dedup and conflicts**
- What: identical rows collapse, differing rows with one key fail the build; spill changes order.
- Where: `HashDedup.dedup`.
- Why: compilers may emit the same fact twice.
- Pinned by: `Hash dedup collapses duplicates, rejects conflicts and cleans up after spilling`.
- Constants: 64 MiB, 256 partitions.
- Verdict: `replace`: the model guarantees unique keys (`jr_model::apply`); the writer asserts and
  errors, never dedups; output sorted.

**SERVING-R044 Base rows win silently**
- What: overlay base projections override regenerated rows with the same key, no conflict.
- Where: `PackageWriter.preferBase`.
- Why: unknown.
- Pinned by: none.
- Constants: —.
- Verdict: `replace` with the precedence table of `jr_model::apply` (recorded, not silent).

**SERVING-R045 Blank text becomes null**
- What: whitespace-only text values become null in nullable fields (`IsNullOrWhiteSpace`);
  empty strings become null after the text dedup round trip.
- Where: `PackageRows.nullableString`, `AssignmentWriter`, `deduplicated`.
- Why: GTFS CSV cannot distinguish null from empty.
- Pinned by: `In-seat transfers between trips carry no locations`.
- Constants: —.
- Verdict: `replace`: `Option<String>` in the model; no empty strings stored.

**SERVING-R046 GTFS placeholders leak into serving**
- What: `agency.url = jrutil://invalid` and (0,0) coordinates from `fillStandardRequiredFields`.
- Where: `Gtfs/Gtfs.fs:117`.
- Why: "some GTFS-consuming software actually needs them filled" (comment) — GTFS only.
- Pinned by: none.
- Constants: `jrutil://invalid`, `0m`.
- Verdict: `drop` from serving (url null); GTFS projection may fill placeholders if MOTIS needs them.

**SERVING-R047 Diagnostics summary**
- What: section 10.2 (counts, 10 examples per code, coverage tables); CZPTT compiler diagnostics
  dropped.
- Where: `PackageWriter.writeDiagnosticsSummary`.
- Why: bounded production summary (OUTPUT_AUDIT).
- Pinned by: route-shapes tests (section presence).
- Constants: 10 examples, 1 MiB.
- Verdict: `replace`: one diagnostic stream from all stages (rule IDs as codes), CZPTT included.

**SERVING-R048 Acceptance limits**
- What: ≤65,536 rows per group, diagnostics ≤1 MiB, package ≤2 GiB.
- Where: `Validation`.
- Why: consumer memory budget; bounded summary.
- Pinned by: `Validation rejects duplicate keys across row groups with matching hashes`.
- Constants: 65,536; 1 MiB; 2 GiB.
- Verdict: `port`.

**SERVING-R049 CZPTT location metadata gaps**
- What: CZPTT stop places have no `country_code` (operational points do); stations get
  `timezone = Europe/Prague`; boarding points inherit the station's coordinate source.
- Where: `gtfsRelations`, `CzPttEntities.stops`.
- Why: unknown (no CZPTT stop metadata sidecar).
- Pinned by: none.
- Constants: `Europe/Prague`.
- Verdict: `replace`: fill `country_code` for every CZPTT location from the point identity.

**SERVING-R050 CZPTT pickup/drop-off and timepoint**
- What: section 3.4.
- Where: `CzPttEntities.stopTimes`.
- Why: CZPTT activities.
- Pinned by: none.
- Constants: activity codes `DisembarkOnly`, `EmbarkOnly`, `RequestStop` (`0030`).
- Verdict: `port` (CZPTT compiler).

**SERVING-R051 CZPTT direction**
- What: CZPTT trips have null `direction`, so all trips of a route share one slot list.
- Where: `CzPttEntities.trip`, `RouteStopWriter`.
- Why: CZPTT has no direction concept; consequence on route stops probably unintended.
- Pinned by: `Trips without a direction get their own route stop order`.
- Constants: —.
- Verdict: architect decision (open question 1).

**SERVING-R052 Block key mode suffix**
- What: `:mode:<n>` suffix when a PA alternates rail/NAD.
- Where: `CzPttEntities.trip`.
- Why: unknown (likely to keep a GTFS block from mixing vehicles).
- Pinned by: none.
- Constants: `:mode:`.
- Verdict: `port` (CZPTT compiler).

**SERVING-R053 Passenger timepoint and default pickup**
- What: GTFS timepoint `"0"` → false, blank → true; blank pickup/drop-off → 0.
- Where: `writeOrderedTripCalls`, `TripCallWriter.fromGtfs`.
- Why: GTFS defaults.
- Pinned by: `Native trip calls equal the existing GTFS projection for every field`.
- Constants: —.
- Verdict: `replace`: typed call fields without GTFS defaults.

**SERVING-R054 Overlay inheritance from the base package**
- What: location metadata and `timetable_kind` read from the base package by id; base keys
  re-targeted and clipped; base call keys kept where the output trip still holds the call;
  base semantics projected.
- Where: `gtfsRelations`, `PackageKeyRelations` (5), `PackageBaseRelations.projectedBaseRelations`.
- Why: "A regional overlay is a semantic projection of its production base package" (comment).
- Pinned by: `Overlay slicing carries regional base keys and calls`,
  `Carried base call keys require exact target sequence membership`,
  `Overlay projects native base semantics onto replacement trips`.
- Constants: —.
- Verdict: `replace`: the overlay pass works on the base `Timetable`, so these facts travel in
  the model; no package re-reading.

**SERVING-R055 Overlay call keys by position**
- What: overlay call mappings count 1-based positions in the output trip, translated to that
  trip's sequence.
- Where: `PackageKeyRelations` (`regionalCalls`).
- Why: "trips cut from the base package" keep sequence gaps (comment).
- Pinned by: `Regional call keys resolve output positions to trip sequences`.
- Constants: —.
- Verdict: `replace`: the overlay records the source call sequence on the call.

**SERVING-R056 Operational-point coordinate source "estimated"**
- What: sidecar source `estimated` maps to `catalogue`.
- Where: `PackageRows.coordinateSource`.
- Why: none (fall-through).
- Pinned by: none.
- Constants: —.
- Verdict: `replace` (typed source; estimated points are `estimated` precision with source
  `route_time`/`route_end` per method).

**SERVING-R057 Exception-only CZPTT calendars**
- What: CZPTT emits only calendar exceptions (all added), so every service has mask 0.
- Where: `CzPttEntities.calendarExceptions`, `Gtfs.deduplicateCalendarWithPrefix`.
- Why: CZPTT bitmaps.
- Pinned by: none.
- Constants: —.
- Verdict: covered by SERVING-R010 `replace`.

**SERVING-R058 Feed prefix on single-key relations**
- What: validation requires `jdf:`, `overlay:` or `czptt:` on every single-text-PK relation.
- Where: `Validation.validateValues`.
- Why: contract `ids`: both packages share tables.
- Pinned by: `Validation rejects coded values, unprefixed ids and malformed keys`.
- Constants: the three prefixes.
- Verdict: `port`.

**SERVING-R059 CZPTT estimated stops published as exact**
- What: consequence of R004/R005 for CZPTT GTFS stops: precision `exact`, source `catalogue`
  (name still gets ` [?]`).
- Where: `gtfsRelations`.
- Why: none.
- Pinned by: none (rail package has no estimated stop).
- Constants: —.
- Verdict: `replace` (fix, ledger).

**SERVING-R060 Two constructions of the CZPTT point id**
- What: GTFS stops use `czptt:stop:` + escaped `country:code`; railway points and `sr70` targets
  use `czptt:stop:` + raw `country:code` from the sidecar.
- Where: `CzPttEntities.stationIdFor`, `PackageGtfsRelations`, `PackageKeyRelations`.
- Why: none.
- Pinned by: none.
- Constants: `czptt:stop:`.
- Verdict: `replace`: one id builder in `jr_model::identity`.

**SERVING-R061 CZPTT service ids depend on the PA set**
- What: `czptt:service:exc:<i>` with `i` = group index over sorted PA ids.
- Where: `Gtfs.deduplicateCalendarWithPrefix`.
- Why: merging identical calendars; readability ("bitmap string in the ID").
- Pinned by: none.
- Constants: `exc`.
- Verdict: `replace`: content-derived service id (digest of the `DateSet`) so ids are stable
  across runs; nothing outside the package keys on `service_id`.

**SERVING-R062 Sequence ordering contract of the writers**
- What: `trip_call` and `shape_point` writers require ordinal-sorted input and throw otherwise.
- Where: `TripCallWriter.Append`, `writeOrderedShapes`.
- Why: streaming writers.
- Pinned by: `Bounded compression preserves deterministic trip call bytes`.
- Constants: —.
- Verdict: `drop` the requirement as an input contract; the writer sorts (AGENTS rule 7).

---

## 14. Duplicated logic and detour artefacts

### 14.1 Concepts implemented more than once

| Concept | Implementations |
|---|---|
| ID escaping | `Identity.encodeComponent` (bytes, upper hex), `Identity.compositeKey` (same, colons restored), `Uri.EscapeDataString` (`CzPttNormalize.idComponent`), raw concatenation (`czptt:stop:` + sidecar id) |
| Trip projection | `writeOrderedTrips` (GTFS rows), `TripWriter.write` (typed JDF trips); a test asserts equality |
| Call projection | `writeOrderedTripCalls`, `TripCallWriter.fromGtfs` (+ test asserting equality) |
| Note links | `NoteWriter.assignment` (JDF), `semanticRelations.noteLinks` (same function via sidecar), `czpttRelations.link` (different field set for the same kind) |
| Assignment rows | `AssignmentWriter.write` (typed), `PackageSemanticRelations.assignmentRow` (dictionary) |
| Shape generation classification | `gtfsRelations.generation`, `writeOrderedShapes.generation` (same `overlay:` test), `RouteShapes` (`motis`) |
| Service bounds | `serviceBounds`, `exceptionCalendars` bounds |
| GTFS time parsing | `PackageRows.seconds`, `Validation.gtfsSeconds`; formatting in `GtfsProjection.time`; NodaTime `Period` in `TripCallWriter.fromGtfs` |
| Parquet readers | `PackageReader.readTextRows`, `ColumnReader.groups`, `Validation.keyColumn`, `Comparison.readColumn`, `CompilerOutput.parquetFileTable` |
| Cell text formatting | `PackageRows.fieldText`, `GtfsProjection.cells`, `Validation.cell`, `Validation.keyToken`, `Comparison.showCell`/`cellToken` |
| Mode classification | `servingMode` (route types), `RouteShapes.shapeClass` (modes), CZPTT route types upstream |
| Source manifest reading | `PackageRows.sourceManifest`/`combinedSourceManifest` (unused by the writer path), `writeManifest.addSources` |
| Trip-part rule | enforced by construction in `writeOrderedTripCalls`, re-checked in `Validation.validateTripParts` |
| Route-stop identity | compiler `routeStopKey` (JDF, transitional) and published slot id |
| PK dedup | `HashDedup` (writer), `preferBase` (overlay), `Validation` duplicate scan, `Comparison` duplicate scan |
| Writer budgets | `RelationWriter`, `ColumnWriter`, `TripCallWriter`, `writeParquet`, `HashDedup` each have their own byte estimates |

### 14.2 Behaviour that exists only because of the GTFS/sidecar detour

- Every value round-trips through CSV text: `nullableString`, `int16`/`number` parsing, blank → 0
  pickup defaults, timepoint `"0"`, (0,0) coordinates, `jrutil://invalid`.
- `run_key`/`run_part`, PA/TR keys parsed from `cz_trips.source_trip_ids` and the trip id.
- Railway points joined back to trip parts by sequence from a second table; operational columns
  copied onto passenger calls by the same join; sidecar times unshifted.
- `location.kind` and GTFS `location_type` recomputed from GTFS columns.
- Exception-only calendar rows reconstructed from `calendar_dates`.
- Location metadata, coordinate precision and source from a JDF sidecar keyed by GTFS stop id,
  with parent fallback; substring classification of provenance text.
- `overlay:` prefix tests for generation method, coordinate source, binding method.
- `source_call_metadata` and the dead `czptt:pa` call-key path.
- `transfer_key` hashed over GTFS column names; MOTIS 1→2 rewrite stored in serving.
- The text dedup round trip and its empty → null side effect.
- `trip_call` rewritten up to three times on disk (ordered write, route-stop pass, route shapes),
  `trip` twice; `travel_restriction` rewritten by the route-stop pass.
- `PackageReader.prepareCompilerView` rebuilds `cz_routes.txt`, `cz_stops.txt`,
  `cz_trips.txt` (parsing `cis:line_trip`), `cz_trip_stop_zones.txt` (expanding slot zones back
  to calls), restores `max_waiting_time` and `shape_dist_traveled` into GTFS so the overlay
  compiler can read the base package.
- IDS coverage sidecars written but never read.
- `diagnostics` hand-off whose CZPTT content is ignored.
- The JDF native/generic split (native files concatenated, `NativeCallArtifacts.transferSequences`
  as a side channel for connection origins).

---

## 15. Open questions for the architect

1. **CZPTT route stops merge both directions.** `direction` is null for every CZPTT trip, so
   opposite trains of a line route share one slot list (`/2` visits in the rail package). Keep,
   derive a direction (e.g. from line catalog or call order), or group CZPTT route stops per
   (route, ordered endpoint pair)?
2. **Estimated CZPTT coordinates** would be published `exact`/`catalogue` (SERVING-R059). Fix and
   ledger, and should `country_code` be filled for every CZPTT location (SERVING-R049)?
3. **`czptt:pa` call keys**: drop the path entirely (SERVING-R018)?
4. **IDS coverage** (`CZIPTS`, `CZCalendarIPTS`) is computed but not published (only the raw
   central "IDS" note survives). Should the rewrite publish it (as assignments or zones), or
   stop computing it?
5. **CZPTT service ids** are unstable across runs (SERVING-R061). Switch to content-derived ids
   at the CZPTT switch?
6. **MOTIS transfer rewrite** (SERVING-R036): keep serving `transfer` byte-compatible with F#
   (rewritten rows), or store the source fact and adapt only `transfers.txt`? The second changes
   the serving relation and what Oběhy loads.
7. **`agency.url = jrutil://invalid`** in serving (SERVING-R046): publish null instead?
8. **Shape line-up check**: the contract claims misaligned companion output is discarded; the code
   does not check. Implement (SERVING-R032) and update parity ledger?
9. **Trip-scoped JDF travel restrictions** never point at their call, contrary to the contract
   (overlay package: 457/457 unresolved). Fix in the rewrite (needs the restriction's source route
   stop resolved to the trip's call)?
10. **`zone_system` vocabulary** differs: CZPTT `PID`, overlay `ids-jmk`, fixtures `pid`. Define a
    closed list or a single normalisation?
11. **Contract versions disagree**: `production-v3.json` says `serving_schema_version: "5.0"`,
    `STATIC_PIPELINE.md` says 5.0, `Schema.fs`, `serving-v5.json` and both packages say `5.2`.
    Which file is authoritative for the Rust writer, and should `production-v3.json` be bumped?
12. **Generated-id scheme**: keep `jrutil-serving-v2` digests byte-identical for parity (assignment,
    transfer, restriction ids), or define the rewrite's own (they are PKs only)?
13. **`build_spec_sha256`** hashes .NET JSON text; nobody reads it. Define a canonical build spec?
14. **`funicular`**: GTFS 7/1400 map to `cable`; should the one mode classifier emit `funicular`?
15. **CZPTT diagnostics** never reach `diagnostics.json`. Should the rewrite publish CZPTT counts
    (rejected journeys, unresolved notes, coordinate methods) in the bounded summary?
16. **`sr70:track`** uses the subsidiary *name* when present. Should the identifier be the
    subsidiary code only (the contract says "track designation")?
17. **Validation minor-tolerance**: F# rejects unknown relations in the manifest, consumers ignore
    them. Should the Rust validator accept a newer minor's extra relations/fields?
18. **Connection claims with unresolvable origin** get `origin_sequence = 0` (SERVING-R025). Make
    it a quarantine?
19. **Non-passenger timepoint**: railway points use "has a time" while passenger calls use CZPTT's
    inferred-timing flag. Carry the inferred flag on railway points too?
20. **Row order**: F# order is generation/partition order; the rewrite will emit sorted rows. The
    contract allows it; confirm parity is compared by key only (`compare --expect`).
21. **Overlay and JDF**: the JDF native writers and the overlay's base re-reading
    (`prepareCompilerView`) disappear in the rewrite; confirm the overlay pass reads the base
    `Timetable` in memory and that base keys/semantics are model facts (SERVING-R054).
