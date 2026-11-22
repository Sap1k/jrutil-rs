# JDF merge, stop identity, line versions, calendars and the international filter: rule inventory (F# reference)

Status: **proposal for review**. Nothing here is implemented yet. The inventory describes what the
F# JrUtil (`E:/Git/obehy/jrutil`) and its Oběhy caller do today; the verdicts are proposals for
the architect.

Sources read in full: `jrutil/src/Jdf/JdfMerger.fs`, `JdfStopReconciliation.fs`,
`JdfCalendar.fs`, `JdfInternationalFilter.fs`, `JdfTripStopSpool.fs`, `JdfFix.fs`;
`jrutil/src/GeoData/StopRegistry.fs`, `GeoData/StopMatcherSynonyms.txt`, `DateUtils.fs`,
`Holidays.fs`; `Jdf/JdfModel.fs`; the parts of `Jdf/JdfGtfsRules.fs` that define the policy type,
the trip-version key and the route grouping; `Utils.fs` (`MultiDict`); the `merge-jdf` and
`fix-jdf` commands in `jrutil-multitool/Commands/JdfCommands.fs`; the calendar/filter calls in
`Jdf/JdfBundle.fs`; tests `JdfMergerTests.fs`, `StopRegistryTests.fs`,
`InternationalRouteFilterTests.fs`, `HolidayTests.fs` and the calendar and version cases of
`JdfToGtfsTests.fs` and `PerformanceEquivalenceTests.fs`; `jrunify-ext-geodata/README.md`,
`registry.py` and the three registry CSVs (formats only); `repo/src/obehy/national_jdf.py`,
`repo/src/obehy/gvd.py`; `jrutil/README.md`, `repo/JDF_SEMANTICS.md`, `repo/BASE_PLAN.md` §6
and §8, the registry part of `repo/PROGRESS.md`.

Real-data peeks (bounded, read-only): every 8th of the 2,487 batches in
`work/overlay-golden/inputs/jdf/batches` for `VerzeJDF.txt`; the first 400 for repeated licence
numbers in `Linky.txt`; one fixed batch; the header rows of
`work/overlay-golden/out/baseline/merge.zip`. Facts learned from them are marked **(data)**.

Conventions used below:

- "version" = one `Linky.txt` row, keyed `(licence, distinction)` = `(Route.id,
  Route.idDistinction)`. "Source distinction" = the one in the input batch; "merged distinction" =
  the one the merger assigns.
- "spelling" = a stop's `(town, district, nearbyPlace)`; "display name" = `stopDisplayName` = the
  present components joined with `,` (no space), e.g. `Ústí nad Labem,Hlavní nádraží`.
- "precise" = a `Polzast.txt` row with precision `S` (`StopPrecise`); `E` (`Estimated`) never
  counts as a location for any gate in this area.
- "EPSG:3035 distance" = Euclidean distance after projecting WGS84 to ETRS89-extended LAEA Europe
  (`GeoData.Common.pointWgs84ToEtrs89Ex`); "equirectangular distance" =
  `StopRegistry.distanceMetres` (`north = Δlat·111320`, `east = Δlon·111320·cos(mean lat)`).
- `\u001f` is the ASCII unit separator used inside composite keys.
- "ordinal" = .NET ordinal / UTF-16 code-unit order; F# `compare` and `Seq.sort` on strings are
  ordinal.
- Proposed policy keys live in `policy/jdf.toml` (prefix `jdf.`); proposed data files under
  `data/names/` (shared with the rail side) and `data/jdf/`.
- Test names are `JdfMergerTests` method names unless another class is named.
- File references are relative to `E:/Git/obehy/jrutil/jrutil/src/` unless stated.

Where this area starts and stops:

- **Takes over from** the batch reader and fixer (another inventory): parsing, JDF 1.9/1.10 →
  1.11 conversion, geocoding with the stop matcher, `rejectImplausibleMatches`,
  `estimateMissingStopLocations`, `PolzastZdroj`, post-candidate evidence capture,
  `dropDegenerateBatch`. Input to this area is one *fixed* batch (section 2.1).
- **Hands over to** the GTFS/serving conversion (another inventory): route grouping
  (`jdf:route:…`), stop/trip ID rendering, the GTFS calendar representation, notices, zones,
  transfers, transport-mode rules (`applyTransportModeRules`, applied right after the filter).
  The trip-version key (R254) is computed there but is listed here because it is built only from
  facts this area produces.
- **Hands over to** the post estimator (another inventory): `JrutilRoutingDemands` (R252),
  `JrutilPostCandidateEvidence` after remapping (R236), `posts.csv` `est:<k>` ordinals (R211).

---

## Architect's verdicts

**These override the "Proposed verdict" lines below.** Where a rule is not listed, its proposed
verdict stands. Rule IDs in the Rust code are the IDs of this file. The bar is parity with what
F# publishes with the stop registry (production passes it; on the bounded set 12 of about 6,000
stops are unregistered, none ambiguous).

**Order.** F# is deterministic for a given export because batches are merged in file-name order.
The rewrite keeps that as a stated rule, not an accident: *batches are taken in ordinal order of
their names, and where a rule says "first", it means first in that order*. A batch's name is a
source fact (the export's own identifier). An order-free clustering of stops and a total order
for versions (Q1, Q7, Q8) are possible later as deliberate, measured changes; they are not part
of the port.

| Q | Decision |
|---|---|
| Q1 | Port the incremental reconciliation (R217–R225) in batch-name order, including chaining. |
| Q2 | Fix B1: one quarantined stop per ambiguous identity (normalised identity and locality); later occurrences of the same identity join it instead of creating another. |
| Q3 | Port R223 as is (no distance cap). |
| Q4 | Port: retired rows are parsed and not treated specially (R209). |
| Q5 | Port the asymmetry of one-component suffixes (R219) as it is. Recorded as a limitation. |
| Q6 | The synonym file is ported verbatim, conflicting lines included: changing it moves stop identities. A load-time diagnostic reports alternatives that appear in two lines. |
| Q7 | Port R246 with both arms; the last tie-break is batch-name order, then position in the batch. |
| Q8 | Port the side-by-side units (R243, R244) with the day-class masks, in batch-name order. |
| Q9, Q10 | A version is never copied: its trips are masked with the dates it wins (R248). Trip IDs: `jdf:trip:<line>:<yymmdd>[:det]:<trip>`; a version sharing start and detour flag with an earlier-ending one appends eight hex characters of SHA-256 of `<yymmdd from>|<yymmdd to>` (the export date leaves the hash: fixes B8); side-by-side versions with identical published validity append `:p2`, `:p3` … in batch-name order. No `:pN` for date chunks. |
| Calendar questions | Port R255–R268 as written, undated odd/even weeks included; the holiday table and the Easter rule become data and a correct algorithm (the 2051 crash is fixed). |
| Filter questions | Port R269–R281, the two domestic-drop cases included, each dropped route and trip diagnosed. Country codes as the feed publishes them. |
| Smaller | Dates before the reference date are kept (R249). Agency identity as in R238. A post-evidence conflict is a diagnostic and the first row wins. Stops no published trip uses are neither published nor offered as registry candidates (Q20). Re-merge input and the by-ID strategy are dropped. |

Further:

- R205, R206, R250, R251, R253 (strategy switch, spool, merged header, extension files, output
  order) are dropped: there is no merged JDF.
- R228 is dropped: numbering is always registry plus provisional (R229, R230); no registry means
  an empty one.
- R230: the provisional number is computed from the identity of the first occurrence in
  batch-name order, as today, so numbers already reviewed through the candidates file stay.
- R231: the location of a merged stop is chosen as today (precise over estimated, then the
  preferred spelling's); the conflict diagnostic is kept.
- R249: validity is read into `DateSet`s over the GVD horizon after the winners are decided on
  the published validity.
- R252 (routing demands) belongs to the post estimator.

---

## 1. Data flow

**Stage 0 — Oběhy invocation (`national_jdf.py`).** `_plan` (606–616) picks
`reference_date = config.reference_date or prague_today()` and `gvd_year = config.gvd_year or
resolve_timetable_year("auto")` (`gvd.py`). `stage_nested_jdf_batches` (138–) unpacks the two
national exports into one directory as `drahy-<stem>.zip` and `vld-<stem>.zip`. `fix-jdf`
(`_fix_arguments`, 669–686) writes `work/fixed/<name>.zip`; `merge-jdf` (`_merge_arguments`,
689–709) gets `--strict --gvd-year --reference-date`, and `--stop-registry=<dir>
--stop-registry-candidates=<release>/stop-registry/stop-candidates.csv` when a registry is
configured; `jdf-to-bundle` (524–562) gets `--international-route-policy=regional-adjacent` and
the same `--gvd-year`. Rules R200–R201.

**Stage 1 — Command entry (`JdfCommands.mergeJdf`, 128–243).** Validates the GVD year and the
reference date (R200, R202), computes the GVD bounds, loads the registry (`StopRegistry.load`,
R207–R212), constructs `JdfMerger(MergeStopsByName, spillPath, workers, ?registry)` (R205, R206).
Batches are enumerated per input directory in argument order, each directory's batch paths sorted
ordinally (R203), parsed in parallel with ordered output, and handed to `merger.add` **one at a
time in that order** (R204).

**Stage 2 — Registry preload (`JdfMerger` constructor, 82–116).** Every `stops.csv` row becomes a
reconciler alias of its resolved number, with its reference coordinate as a precise location
(R210). This happens before any batch.

**Stage 3 — Per-batch merge (`JdfMerger.beginAdd`, 662–1057).** In this order inside one batch:
attribute references (R213) → stops (R214–R231, one source stop at a time in `Zastavky.txt`
order) → stop locations (R231) → post evidence (R236) → stop posts (R237) → agencies (R238) →
versions (R239, R240) → per-version relations (R241, R242) → trip stops (spool, R206).
Output of the stage: the growing merged collections listed below.

**Stage 4 — Overlap resolution (`resolveRouteOverlaps`, 543–547; `resolveLicenceOverlaps`,
445–541).** Per licence with more than one version, in merged-distinction order: day-class masks
(R243), side-by-side units (R244), per-date winner (R245–R247), cut/copy/delete (R248).

**Stage 5 — Validity bounding (`boundValidity`, 553–577).** Drop expired and next-GVD versions,
clamp the rest (R249); also records the reference date for the header (R250).

**Stage 6 — Write (`write`, 268–275; `batchWithTripStops`, 153–259).** Header row (R250), every
relation in collection order (R253), `JrutilRouteVersions` (R251), `JrutilRoutingDemands` (R252),
trip stops streamed from the spool with distinction rewriting (R206); then the registry
candidates CSV (R232–R234).

**Stage 7 — Calendar (`JdfCalendar.prepareGtfsCalendarWithWorkersAndProgress`, 131–276), run by
`jdf-to-bundle` on the merged batch (`JdfBundle.fs` 311–317).** Per trip: running days over its
version's validity (R255–R266); trips that never run are listed in `tripsToDelete` (R267); the
GTFS representation is chosen (R268).

**Stage 8 — International filter
(`JdfInternationalFilter.applyInternationalRoutePolicyInternal`, 53–327), run by `jdf-to-bundle`
right after Stage 7 (`JdfBundle.fs` 326–333) with the calendar's `tripsToDelete`.** Rules
R269–R281. `fix-jdf` also runs it per source batch, but only logs the decisions (R282).

### State the merger carries from batch to batch

| State | Content | Order-dependent? |
|---|---|---|
| `stops` (list), `stopIndexesById` | merged stop rows, index by merged ID | numbering (no registry), first spelling, list order |
| `StopReconciler` | every alias ever added: registry rows, then each matched/new source spelling, with location; exact, suffix and 75 m spatial indexes | yes: later batches match earlier batches' spellings |
| `stopLocationsByStop`, `stopLocationSourcesByStop` | best location per merged stop and its source | partly (R231) |
| `provisionalReasons`, `provisionalSpellings`, `unregisteredAliases` | registry-candidate bookkeeping | first spelling per key |
| `attributeRefs`, `attributeRefsByValue`, `lastAttributeRefId` | merged `Pevnykod` | numbering |
| `agenciesByIco` (MultiDict) | agencies per IČO with merged distinctions | numbering |
| `routesByLicNum` (MultiDict) | versions per licence with merged distinctions | numbering, list order |
| `batchDateByRoute`, `originalValidityByRoute` | per merged version: batch creation date, published validity | no |
| per-version MultiDicts (route integrations, route stops, trips, route info, notes, transfers, agency alternations, alternate names, reservations) | rows keyed by merged version | dictionary order |
| `tripGroups`, `lastTripGroupId` | merged `SpojSkup` | numbering |
| `stopPostsSet`, `postCandidateEvidence` | dedup keys | first wins / conflict |
| `TripStopSpool` | `Zasspoje.txt` text chunks per merged version | dictionary order, text rewrite |

What depends on batch order is collected in section 9.

---

## 2. Inputs

### 2.1 From one fixed batch (`JdfModel.JdfBatch`)

Only these fields drive decisions; everything else in the listed relations is copied.

| Relation (file) | Fields read | Used by |
|---|---|---|
| `VerzeJDF.txt` | `creationDate` ("datum výroby", `ddMMyyyy`, optional) | batch date of each version (R240, R246) |
| `Zastavky.txt` | `id`, `town`, `district`, `nearbyPlace`, `regionId` (okres code), `country`, 6 attribute IDs | stop identity (R214–R235), country for the filter (R270) |
| `Polzast.txt` (JrUtil extension) | `stopId`, `lat`, `lon` (decimal text, e.g. `"49.3971553531149"`), precision `S`/`E` | gates (R220, R221, R223), location merge (R231) |
| `PolzastZdroj.txt` (extension) | `stopId`, `source` (e.g. `external:MapaVDV`, `osm:node:…+osm:node:…`) **(data)** | copied with the chosen location (R231) |
| `Oznacniky.txt` | `stopId`, `stopPostId` | dedup (R237) |
| `Pevnykod.txt` | `attributeId`, `value`, `reserved1` | dedup (R213); day attributes (R243, R261–R264) |
| `Dopravci.txt` | every field (record equality) | dedup (R238) |
| `Linky.txt` | `id`, `idDistinction`, `agencyId`, `agencyDistinction`, `routeType`, `detour`, `grouped`, `timetableValidFrom`, `timetableValidTo` | versions (R239–R249), filter (R271) |
| `LinExt.txt` | route key | "integrated" (R276) |
| `Zaslinky.txt` | route key, `routeStopId`, `stopId`, 3 attribute IDs | remap; candidate routes (R271) |
| `Spoje.txt` | route key, `id`, 10 attribute IDs, `tripGroupId` | day-class mask (R243), calendar (R255–R264) |
| `SpojSkup.txt` | `id` | renumbering (R242) |
| `Zasspoje.txt` | route key, `tripId`, `stopId`, 3 attribute IDs, `kilometer`, `arrivalTime`, `departureTime` | remap; emitted calls, span, depth (R273–R275) |
| `Caskody.txt` | route key, `tripId`, `noteType` (`1`–`8` or blank), `dateFrom`, `dateTo` | mask (R243), calendar (R255–R260) |
| `Udaje.txt`, `Navaznosti.txt`, `Altdop.txt`, `Altlinky.txt`, `Mistenky.txt` | route key (`Altdop`: also agency key and attributes) | remap only (R241) |
| `JrutilPostCandidateEvidence.txt` | `stopId`, `observationId`, whole row | remap and conflict (R236) |
| `JrutilRouteVersions.txt` | all | only when the input is itself a merged batch (R240) |
| `JrutilRoutingDemands.txt` | — | ignored; regenerated (R252) |

`Navaznosti` stop fields (`transferStopId`, `transferStopPostId`, `transferEndStopId`,
`transferEndStopPostId`) are documented in `JdfModel.fs` as global stop-register numbers and are
**not** remapped. `Caskody.tripId`, `Navaznosti.tripId`, `Altdop.tripId`, `Mistenky.tripId` are
per-version trip numbers and are not remapped either.

**(data)** Every sampled source batch of one national export carries the same `VerzeJDF`
creation date, the export date (`20112026` in all 309 sampled batches; one batch had no readable
`VerzeJDF`). No batch among the first 400 has two `Linky.txt` rows with the same licence. Fixed
batches carry `Polzast` precision `S` and `E`; the merged sample has 6,190 locations, 16 of them
`E`. `Caskody` note types in the merged sample: `1` (9,587), `2` (19,046), `3` (521), `4`
(49,606), blank (576); no `5`–`8`.

### 2.2 Registry (`jrunify-ext-geodata/registry/`, `StopRegistry.load`)

All three files: UTF-8, optional BOM stripped, RFC 4180 quoting, header row required and must be
exactly the one below (`CsvFile.Parse(... ignoreErrors = false)`), otherwise the load fails.
`stops.csv` is required; `posts.csv` and `overlay_places.csv` are optional.

`stops.csv` — `id,town,district,nearby_place,okres,country,lat,lon,status,note`:

| Column | Parse | Meaning |
|---|---|---|
| `id` | trimmed `Int64`, `1 ≤ id < 1000000000` | the merged-JDF stop number `N` (`jdf:stop:N`) |
| `town` | trimmed, required | spelling component |
| `district`, `nearby_place` | trimmed, blank → absent | spelling components |
| `okres` | trimmed, blank → absent, upper-cased | `regionId` |
| `country` | trimmed, blank → absent, upper-cased | `country` |
| `lat`, `lon` | both blank → no reference; both set → invariant floats with `|lat| ≤ 90`, `|lon| ≤ 180`; one set → error | reference coordinate; tells same-identity stops apart |
| `status` | trimmed, `active` or `retired`, else error | parsed and **ignored** by the merger (R209) |
| `note` | free text; the first `merged_into:` followed by digits names the surviving number | R208 |

Semantics: rows sharing an `id` are aliases (spellings) of one stop; a row is never edited or
deleted; an `id` is never reused (README). `registry.py validate` additionally requires: file
sorted by `(int id, stop_key, lat, lon)`; for each `stop_key` (casefolded, whitespace-collapsed
town/district/nearby + upper okres + upper country) mapping to two or more ids, every id has a
reference and every pair is ≥ 75 m apart (`DISTINCT_STOP_METRES`). JrUtil checks none of these.
**(data)** 37,598 rows; 1,159 without reference; no `merged_into`; no `retired`.

`posts.csv` — `stop_id,post_key,lat,lon,status,note`: `stop_id` must be a registered id;
`post_key` is `est:<k>` (`k` a positive integer without leading zeros) or `post:<x>` (`x`
non-empty, no `:` or whitespace); `(stop_id, post_key)` unique; `est:` rows need a reference.
Consumed by the post estimator (R211). **(data)** 57,237 rows.

`overlay_places.csv` — `source_id,group_key,place_id,status,note`: all three keys required,
`(source_id, group_key)` unique. Consumed by the overlay. **(data)** 119 rows.

Candidates file written by `merge-jdf` (`stopCandidatesHeader`):
`provisional_id,town,district,nearby_place,okres,country,lat,lon,reason,alias_of_suggestion`
(R232–R234). `registry.py promote` reads it (header containing `provisional_id`), and for each
row whose `decision` column is `new` (or blank with `reason=new` under `--accept-new`) or
`alias`: `new` rows sharing a `provisional_id` get one number — an existing id when a registered
row has the same `stop_key` and (either side has no point, or they are < 75 m apart), else
`max(id)+1` in `(provisional_id, stop_key)` order; `alias` rows are added under `alias_of`, or
under `alias_of_suggestion` when it holds a single value (no `;`). A row whose `(stop_key, id)`
is already registered is skipped. The file is re-sorted. The candidates file has no `decision`
or `alias_of` column; the reviewer adds them.

### 2.3 Reference date and GVD year

- `--gvd-year`: integer `2000..9999` (R200). Bounds: `gvdBounds y = (secondSunday(Dec, y−1),
  secondSunday(Dec, y) − 1 day)`, e.g. 2026 → 2025-12-14..2026-12-12.
- `--reference-date`: ISO `YYYY-MM-DD`, must lie inside the bounds (R202). Used for dropping
  expired versions (R249) and as the merged header's creation date (R250).
- Oběhy defaults: Prague's today and the GVD active today (R201). `jdf-to-bundle` gets the same
  `--gvd-year` (package `service_horizon`; other inventory).

---

## 3. Rules

### Stage 0 — invocation

#### JDF-R200 — GVD bounds
- **What:** `gvdSecondSunday y` = `Dec 1 of y` + `((7 − isoDow(Dec 1)) mod 7) + 7` days
  (ISO Monday = 1 … Sunday = 7). `gvdBounds y = (gvdSecondSunday (y−1), gvdSecondSunday y − 1)`.
  `--gvd-year` must parse as an integer in `2000..9999`, else `invalidArg "--gvd-year"
  "Expected a four-digit year"`.
- **Where:** `DateUtils.gvdSecondSunday`, `gvdBounds` (83–90); `JdfCommands.mergeJdf` (133–140).
  Python twin `gvd.second_sunday`.
- **Why:** README ("The December GVD change is a hard cutover").
- **Pinned by:** `Validity is bounded to the current GVD from the reference date`, `Past starts
  are clamped to the GVD start` (both use `gvdBounds 2026`).
- **Constants:** month `12`, "second Sunday" → `jdf.validity.gvd_change = "second-sunday-of-december"`
  (or keep in code as the definition of a GVD; it is legislation, not a threshold); year range
  `2000..9999` (input validation).
- **Diagnostic:** fatal argument error.
- **Proposed verdict:** port. The horizon of every `DateSet` is these bounds.

#### JDF-R201 — Default reference date and GVD year (Oběhy)
- **What:** `reference_date = config.reference_date or prague_today()` (date in
  Europe/Prague); `gvd_year = config.gvd_year or automatic_timetable_year()` = `today.year + 1`
  if `today ≥ second_sunday(today.year)` else `today.year`. The two defaults are computed
  independently, so an explicit `gvd_year` with a defaulted reference date can fail R202.
- **Where:** `national_jdf._plan` (606–616); `gvd.py`.
- **Why:** `BuildConfig` comment ("None resolves to today in Europe/Prague and its GVD").
- **Pinned by:** none in JrUtil.
- **Constants:** zone `Europe/Prague`.
- **Diagnostic:** none.
- **Proposed verdict:** port into the build spec (`jrutil` CLI): the reference date and GVD year
  are explicit build inputs recorded in the manifest; no wall clock in the compiler.

#### JDF-R202 — Reference date inside the GVD
- **What:** `--reference-date` parsed with the ISO pattern (failure → "Expected YYYY-MM-DD");
  `reference < gvdStart || reference > gvdEnd` → `invalidArg "--reference-date" "The reference
  date is outside GVD {y} ({start}..{end})"`.
- **Where:** `JdfCommands.mergeJdf` (136–142).
- **Why:** unknown (consistency of R249).
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** fatal argument error.
- **Proposed verdict:** port as build-spec validation.

#### JDF-R203 — Batch order
- **What:** for each `<JDF-in-dir>` argument in command-line order, `Jdf.findJdfBatchPaths dir |>
  Seq.sort` (ordinal on full paths); batches are parsed in parallel but merged strictly in that
  order. Oběhy passes one directory, so the order is the ordinal order of `drahy-<stem>.zip`
  then `vld-<stem>.zip` (e.g. `drahy-1`, `drahy-10`, `drahy-100`, …).
- **Where:** `JdfCommands.mergeJdf` (184–203); `national_jdf.stage_nested_jdf_batches`.
- **Why:** README (stable relative-path order); determinism.
- **Pinned by:** none directly; order sensitivity is pinned *against* by `Registered stops keep
  their numbers whatever the batch order`, `Canonical name is independent of input order`,
  `Trip version keys name the published schedule in any merge order`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** replace: batches are an unordered set; every rule below that depends on
  order must be made order-independent (section 9). Keep a canonical batch order (by source
  name, ordinal) only as a last-resort tie-break where the architect accepts one.

#### JDF-R204 — Unreadable batch
- **What:** a batch that fails to parse is logged (`Error while processing {Batch}`) and skipped,
  unless `--strict`, which re-raises. An exception inside `merger.add` always aborts. Oběhy
  passes `--strict`.
- **Where:** `JdfCommands.mergeJdf` (203–219).
- **Why:** README ("Pass `--strict` … when a malformed input batch must fail").
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** log; fatal under `--strict`.
- **Proposed verdict:** replace: a broken input file is an error (AGENTS conventions); a
  malformed entity is a reader diagnostic. No non-strict mode.

#### JDF-R205 — Stop merge strategy
- **What:** `MergeStopsByName` (production) or `MergeStopsById` (stops with equal source IDs are
  one stop, nothing else is reconciled; used only by tests). A registry with `MergeStopsById` →
  `invalidArg`. `--stop-registry-candidates` without `--stop-registry` → `invalidArg`.
- **Where:** `JdfMerger` constructor (26–30, 85–86); `beginAdd` (697–705); `mergeJdf` (152–168).
- **Why:** type comments ("convenient if you know all batches have consistent IDs").
- **Pinned by:** `By-ID strategy does not reconcile distinct IDs`; the version tests use
  `MergeStopsById`.
- **Constants:** none.
- **Diagnostic:** argument errors.
- **Proposed verdict:** drop `MergeStopsById` (no production use; CIS stop IDs are per batch).
  Tests of version resolution need no stop strategy in the rewrite.

#### JDF-R206 — Trip-stop spool and disk preflight
- **What:** `merge-jdf` always spools `Zasspoje` rows to `.{out}.trip-stops.{guid}.tmp` beside
  the output; requires ≥ 1 GiB free (`spill_preflight`). Rows are serialized per batch in
  parallel partitions of ≥ 2048 rows, appended as chunks per merged version key; a deleted
  version drops its chunk list; a copied version shares the source's chunk list; on write, chunks
  are emitted in spool-dictionary order, and a chunk whose stored distinction differs from its key
  is rewritten by replacing every occurrence of the text `","{old}";\r\n` with `","{new}";\r\n`
  (the last field of a `Zasspoje` record); no replacement in a chunk → failure.
- **Where:** `JdfTripStopSpool.fs`; `JdfMerger.copyRoute`/`deleteRoute`/`write`; `mergeJdf`
  (147–183).
- **Why:** memory (national `Zasspoje` is ~140 MB of text in the golden subset **(data)**).
- **Pinned by:** `Spill-backed output is byte-identical across interleaved rows and route splits`.
- **Constants:** `1 GiB`, `2048`, `1 MiB` buffers.
- **Diagnostic:** progress events only.
- **Proposed verdict:** drop (AGENTS rule 8). Trips own their calls in memory; a cut is a
  `DateSet` mask (R248), so nothing is copied or rewritten.

### Stage 1 — registry load

#### JDF-R207 — `stops.csv` parsing and validation
- **What:** as in section 2.2. Errors (`invalidArg "stopRegistry" "{file}:{line}: …"`): header
  mismatch; `id` not an integer in `1..999999999`; blank `town`; status not `active`/`retired`;
  lat/lon half-set or out of range; bad `merged_into` (R208). Ids are **not** checked for
  uniqueness or sorting; nothing checks that same-identity rows are 75 m apart.
- **Where:** `StopRegistry.readTable` (94–102), `parseStops` (104–154).
- **Why:** README of `jrunify-ext-geodata` (append-only, reviewed).
- **Pinned by:** `StopRegistryTests.Registry files load with aliases merges and posts`,
  `StopRegistryTests.Invalid registries are rejected`.
- **Constants:** `provisionalBase = 1000000000` → `jdf.stops.provisional_base`; header text
  (contract with `registry.py`).
- **Diagnostic:** fatal.
- **Proposed verdict:** port the reader to `jr-formats` (facts only) and the validation to the
  compiler; also enforce `registry.py validate`'s checks *in the compiler's own identity terms*
  (R214), since those are the ones that decide (see B12).

#### JDF-R208 — `merged_into` resolution
- **What:** in `note`, the first occurrence of `merged_into:` followed by the longest run of
  digits; empty run, unparsable, or equal to the row's own id → error; target not a registered
  id → error; target that has any row with `merged_into` → error ("Chains are not followed").
  `resolvedId = mergedInto ?? id`; every use of a registry row uses `resolvedId`.
  `registry.py` uses the regex `merged_into:([1-9][0-9]*)` (first match).
- **Where:** `parseStops` (116–153); `RegistryStop.resolvedId`; `JdfMerger` constructor (88).
- **Why:** README ("A `merged_into:<id>` note marks a duplicate; JrUtil uses the target number").
- **Pinned by:** `Merged registry numbers resolve to the surviving number`.
- **Constants:** marker `merged_into:`.
- **Diagnostic:** fatal on bad notes.
- **Proposed verdict:** port; make `merged_into` a column-level fact in the reader rather than
  parsing free text, if the registry format may change (open question Q17).

#### JDF-R209 — Retired rows still match
- **What:** `status = retired` is parsed into `retired = true` and never read by the merger;
  retired rows are preloaded and matched like active ones.
- **Where:** `parseStops` (138); `JdfMerger` constructor (87–113).
- **Why:** unknown. README says a retired stop "is no longer served"; keeping it matchable does
  keep its number if it comes back.
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port the behaviour (a returning stop keeps its number), with a
  diagnostic `jdf_stop_retired_matched` when a retired identity is matched. Unsure; see Q4.

#### JDF-R210 — Registry preload into the reconciler
- **What:** before the first batch, for every row in file order: `stop = {id = resolvedId; town;
  district; nearbyPlace; regionId; country}`; `registryIds += resolvedId`;
  `registryIdentities += (resolvedId, stopIdentityKey stop)` (R214);
  `registryReferences.TryAdd(resolvedId, reference)` (**first** row with a reference per number
  wins); `AddAlias(resolvedId, stop, reference as a precise location)` (R225 — so a second row
  with the same number and the same lower-cased trimmed raw spelling collapses into the first
  alias, only filling its missing okres/country and a missing location);
  `registryStopsByTown[town.Trim().ToLowerInvariant()] += stop` (for R234).
- **Where:** `JdfMerger` constructor (82–116).
- **Why:** comment: "Registered identities are known before any batch, so they resolve the same
  way whatever the merge order. Reference coordinates apply the usual 75 m gate to precise
  locations."
- **Pinned by:** `Registered stops keep their numbers whatever the batch order`, `Same-named
  registered stops are split by reference coordinates`.
- **Constants:** none beyond R220.
- **Diagnostic:** log `Loaded stop registry {Sha256}: {Rows} rows, {Ids} numbers`.
- **Proposed verdict:** port.

#### JDF-R211 — `posts.csv` and `overlay_places.csv` loading
- **What:** validation as in section 2.2 (`parsePosts` 166–185, `isPostKey` 156–164,
  `parseOverlayPlaces` 187–202). The merger does not use them. `assignInferredPostOrdinals`
  (249–284: registered `est:` posts reused within `inferredPostMatchMetres = 25.0` m
  equirectangular, closest pairs first, ties by ordinal then location ID ordinal; the rest take
  `max(registered est ordinal) + 1, …` in location-ID ordinal order) belongs to the post
  estimator.
- **Where:** `StopRegistry.fs`.
- **Why:** README.
- **Pinned by:** `StopRegistryTests.Inferred posts keep registered ordinals despite new locations`,
  `StopRegistryTests.A registered post is reused by its nearest location only`.
- **Constants:** `25.0` → `posts.registry.inferred_post_match_m` (post estimator's policy).
- **Diagnostic:** fatal on invalid rows.
- **Proposed verdict:** port the reader; the rules are the post-estimator and overlay
  inventories'.

#### JDF-R212 — Registry digest
- **What:** `sha256` = SHA-256 (lower-case hex, via `Hashing.sha256Text`) of the text
  `stops={d1};posts={d2};overlay_places={d3}`, each `d` the SHA-256 of the file's bytes, `""` for
  an absent optional file. Logged only.
- **Where:** `StopRegistry.load` (206–221).
- **Why:** comment ("for provenance logging").
- **Pinned by:** `StopRegistryTests.Registry files load with aliases merges and posts` (length 64).
- **Constants:** format string.
- **Diagnostic:** log.
- **Proposed verdict:** replace by the stage-cache input hash and a manifest entry (Oběhy already
  records per-file hashes).

### Stage 2 — attribute references

#### JDF-R213 — `Pevnykod` deduplication and renumbering
- **What:** per batch, for each attribute ref in file order: if `(value, reserved1)` already has a
  merged ID (merged set as it was *before this batch*, copied into
  `existingAttributeRefsByValue`), map to it; otherwise `lastAttributeRefId += 1`, add the ref
  under the new ID and map to it. (Two refs in the same batch with an equal key therefore both get
  new IDs; the second overwrites the global index entry.) Every attribute array of the batch is
  remapped through the map: stops (6), route stops (3), trips (10), trip stops (3), agency
  alternations (6). An attribute ID absent from the batch's `Pevnykod` throws
  (`KeyNotFoundException`), aborting the merge. Arrays are structurally interned (memory only).
- **Where:** `beginAdd` (663–683, 965–981).
- **Why:** JDF attribute numbers are local to a batch.
- **Pinned by:** `Expanded CIS stop name replaces abbreviated alias` (merged attributes `{1, 2}`).
- **Constants:** none.
- **Diagnostic:** none (exception on unknown ID).
- **Proposed verdict:** replace: attributes are typed facts on the entity (the model carries the
  `Attribute` values, not numbers); `Pevnykod` numbering exists only in a JDF projection. An
  unknown attribute ID is a reader diagnostic and the attribute is dropped.

### Stage 3 — stop reconciliation

#### JDF-R214 — Stop name normalisation
- **What:** `tokenize s`: replace U+00A0 by space; `ToLowerInvariant`; replace `n.l.` and then
  `n. l.` by `nad labem` (substring, anywhere); replace every `.` by `. `. Split into tokens of
  characters that are `Char.IsLetterOrDigit` or `.`; for each raw token: if the synonym table
  (R215) has it, emit the canonical form, else emit it with trailing dots removed; drop empty
  tokens. `normalizeStopName stop`: components = `[town; district?; nearbyPlace?]`, each tokenized
  and joined with one space, empty components dropped; `tokens` = components split on spaces;
  `flattened` = tokens joined with spaces. Derived keys:
  - `stopIdentityKey` = components joined with `\u001f` (the exact-match identity);
  - `rawAliasKey` = lower-cased trimmed raw `town`, `district ?? ""`, `nearbyPlace ?? ""` joined
    with `\u001f` (alias dedup, R225);
  - `localityKey` = upper-cased trimmed `regionId ?? ""` and `country ?? ""` joined with `\u001f`;
  - `spellingKey` = display name + `\u001f` + `localityKey` (candidate bookkeeping).
- **Where:** `JdfStopReconciliation.tokenize` (71–94), `normalizeStopName` (96–108),
  `componentKey`, `rawAliasKey` (113–118), `stopIdentityKey` (165–166); `JdfMerger.localityKey`,
  `spellingKey` (76–81).
- **Why:** abbreviations in CIS spellings (`Ústí n.L.,hl.nádr.` vs `Ústí nad Labem,Hlavní nádraží`).
- **Pinned by:** `Expanded CIS stop name replaces abbreviated alias` (exact after normalisation),
  `Canonical name is independent of input order`.
- **Constants:** `n.l.`, `n. l.` → `nad labem` → data file `data/names/stop_expansions.txt`
  (or merged into the synonym file).
- **Diagnostic:** none.
- **Proposed verdict:** port into `jr-geo`'s one name normaliser (AGENTS rule 5), with the
  expansion pairs as data. The identity key format is internal.

#### JDF-R215 — Synonym table
- **What:** `GeoData/StopMatcherSynonyms.txt`, read once: per line, text before `#`, trimmed,
  lower-cased; blank → skipped; comma-separated alternatives, each trimmed, blanks dropped;
  canonical = the longest alternative, ties broken by smallest ordinal; every alternative (the
  canonical included) maps to the canonical; **later lines overwrite earlier ones**. Lookup is
  per raw token (with its dots). Consequences of the current file (59 lines): `h.` → `horách`
  (line `h., horách` overrides `hor., h., horní`); `l.` → `lázně` (overrides `l., labem`); `n.`,
  `nad` → `nisou`; `d.`, `dol.`, `der` → `dolní`; `vel.`, `velký`, `velké` → `velká`; `sv.`,
  `svatý`, `svatá`, `svaté` → `svatého`; `nádr.`, `st.`, `nádraží`, `stanice` → `stanoviště`;
  `i.`, `in` → `im`; `obú` → `obecní úřad` and `měú` → `městský úřad` (canonical with a space;
  it becomes two tokens; the multi-word alternatives themselves can never match a single token).
  Full line list (alternatives as written): `nám., náměstí` · `hl., hlavní` · `rozc., rozcestí`
  · `odb., odbočka` · `žel., železniční` · `aut., autobusové` · `nádr., st., nádraží, stanice,
  stanoviště` · `zast., zastávka` · `rest., restaurace` · `sídl., sídliště` · `nem., nemocnice`
  · `háj., hájenka, hájovna` · `obú, obecní úřad` · `měú, městský úřad` · `nákup., nákupní` ·
  `stř., středisko` · `pož., požární` · `zbroj, zbrojnice` · `ul., ulice` · `hor., h., horní` ·
  `dol., d., dolní, der` · `vel., velký, velká, velké` · `n., nad, nisou` · `p., pod` · `l.,
  labem` · `orl., orlicí, orlických` · `vlt., vltavou` · `svit., svitavou` · `jiz., jizerských,
  jizerou` · `met., metují` · `cidl., cidlinou` · `bez., bezd., bezdězem` · `jev., jevišovkou` ·
  `mor., moravě, moravou` · `des., desnou` · `bystř., bystřicí` · `sáz., sázavou` · `dřev.,
  dřevnicí` · `h., horách` · `zdrav., zdravotnická` · `heřm., heřmanice` · `l., lázně` · `sv.,
  svatý, svatá, svaté, svatého, svaté` · `a., an` · `i., in, im` · `w., wald`.
- **Where:** `JdfStopReconciliation.synonymMap` (50–69). The same file feeds the fix-jdf Lucene
  matcher (`StopMatcher.makeAnalyzer`) and the overlay name matcher (`RegionalOverlay/Names.fs`)
  with different parsers.
- **Why:** file comment ("used for matching different versions of stop names together").
- **Pinned by:** `Expanded CIS stop name replaces abbreviated alias`, `Hierarchical CIS name
  replaces shortened MHD form` (`rozc.` = `rozcestí`).
- **Constants:** the whole file → `data/names/stop_synonyms.txt` (shared by every consumer).
- **Diagnostic:** none; propose a load-time warning `names_synonym_conflict` when one alternative
  appears in two lines.
- **Proposed verdict:** port as data. Whether to fix the conflicting lines (`h.`, `l.`, `n.`
  ↔ `nisou`, `der`) is Q6: any change moves stop merges and provisional numbers.

#### JDF-R216 — Locality compatibility
- **What:** `compatibleLocality a b` = `compatibleOptional a.country b.country &&
  compatibleOptional a.regionId b.regionId`, where `compatibleOptional` is true when either value
  is absent, else ordinal case-insensitive equality (untrimmed). An incompatible alias is never a
  candidate of any kind.
- **Where:** `compatibleOptional`, `compatibleLocality` (120–127); `matchAlias` (255).
- **Why:** same names in different okresy are different stops.
- **Pinned by:** `Nearby unrelated names and conflicting regions remain separate`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### JDF-R217 — Candidate retrieval
- **What:** for an incoming stop with normalized components `C` (length `n`) and optional precise
  location projected to EPSG:3035 `(x, y)`, the candidate aliases are the union of:
  1. `exactIndex[key(C)]` (aliases whose full identity equals `C`);
  2. if `n ≥ 2`: `suffixIndex[key(C)]` (aliases one of whose indexed suffixes equals `C`);
  3. for `offset = 1 … n−1`: `exactIndex[key(C[offset..])]` (aliases whose full identity equals a
     proper suffix of `C`, including a one-component suffix);
  4. if precise: every alias in the 3×3 grid cells around `cell(x, y) = (⌊x/75⌋, ⌊y/75⌋)`.
  An alias's suffixes are indexed (`suffixIndex`) only for `offset = 1 … m−1` where the suffix has
  ≥ 2 components and ≥ 2 tokens (the token condition is implied by the first). Each alias's grid
  cell comes from its precise location only.
- **Where:** `nameCandidateIds` (218–228), `nearbyCandidateIds` (230–245), `gridCell` (210–211),
  `AddAlias` (373–381).
- **Why:** class comment ("Name candidates use dictionaries; fuzzy candidates are limited to
  adjacent projected 75 m grid cells").
- **Pinned by:** `Spatial candidate comparisons stay bounded for dispersed stops`, `Dense local
  clusters do not trigger global comparisons`, `Fuzzy candidates do not depend on global token
  rarity`.
- **Constants:** cell size `75.0` m (= R220 threshold) → `jdf.stops.match_distance_m`.
- **Diagnostic:** none.
- **Proposed verdict:** port as an index (`jr-geo` spatial grid); the retrieval is an
  optimisation and must not change the result versus comparing all aliases (see R219 for the one
  place where it does).

#### JDF-R218 — Exact match
- **What:** an alias is an `Exact` candidate when its normalized component array equals the
  incoming one (element-wise), the localities are compatible (R216) and R220's distance gate
  passes. Similarities are computed for logging only.
- **Where:** `exactOrSuffix` (247–251), `matchAlias` (253–270).
- **Why:** stop identity in CIS is the spelling.
- **Pinned by:** `Expanded CIS stop name replaces abbreviated alias`, `Exact candidate wins over
  an older suffix candidate`.
- **Constants:** none.
- **Diagnostic:** statistics `exact` (R235).
- **Proposed verdict:** port.

#### JDF-R219 — Suffix match
- **What:** an alias is a `Suffix` candidate when one component array is a proper suffix of the
  other (`shorter.Length < longer.Length` and the last `shorter.Length` components are equal),
  localities compatible, R220 passes. Because of R217's indexes, an incoming stop **with one
  component** `[Y]` never retrieves an alias `[X; Y]` by name (one-component suffixes are not
  indexed and the suffix index is not consulted for `n = 1`); it can still find it through the
  spatial grid when both are precise and within the 3×3 cells. The reverse (incoming `[X; Y]`,
  alias `[Y]`) is always retrieved. So whether `Dětkovice` and `Ludmírov,Dětkovice` merge when one
  lacks a precise location depends on which batch came first.
- **Where:** `isSuffix` (129–134), `exactOrSuffix`, `matchAlias`; index asymmetry in
  `nameCandidateIds` and `AddAlias`.
- **Why:** CIS hierarchical names vs shortened urban forms (test `Hierarchical CIS name replaces
  shortened MHD form`: `Dolní Jiřetín,rozc.` vs `Horní Jiřetín,Dolní Jiřetín,rozcestí`). The
  one-component restriction: unknown (probably to stop a bare town name from attaching to every
  stop in it).
- **Pinned by:** `Hierarchical CIS name replaces shortened MHD form`, `Missing coordinates merge
  only a unique suffix locality`, `Ambiguous suffix candidates remain separate`.
- **Constants:** minimum indexed suffix length `2` components → `jdf.stops.min_suffix_components`.
- **Diagnostic:** statistics `suffix`; log `Merged stop {AliasName} into {CanonicalName}
  ({MatchKind}, Levenshtein …, Dice …, distance …)` when display names differ.
- **Proposed verdict:** port with a symmetric definition (B3, Q5): either one-component suffixes
  match in both directions or in neither.

#### JDF-R220 — 75 m distance gate
- **What:** when both the incoming stop and the alias have a precise location, `d` = EPSG:3035
  distance; an `Exact` or `Suffix` candidate with `d > 75` is rejected. When either side has no
  precise location, exact and suffix candidates are accepted **at any distance**. A registry
  alias's location is its reference coordinate (R210).
- **Where:** `matchAlias` (257–262); `distanceThreshold = 75.0` (196).
- **Why:** README ("Reference coordinates get the usual 75 m check … which is how registered stops
  with the same name are told apart"); registry README ("75 m, the distance at which JrUtil stops
  merging same-named stops").
- **Pinned by:** `Compatible names beyond 75 metres remain separate`, `A later precise location
  updates an existing alias`, `Same-named registered stops are split by reference coordinates`.
- **Constants:** `75.0` → `jdf.stops.match_distance_m = 75` (shared with `registry.py`'s
  `DISTINCT_STOP_METRES`).
- **Diagnostic:** none.
- **Proposed verdict:** port, with the distance from `jr_model::geo`. Note the bypass through
  location-less aliases (B2).

#### JDF-R221 — Fuzzy match
- **What:** when neither exact nor suffix applies: both sides precise with `d ≤ 75`, both
  normalized names have ≥ 2 tokens, `levenshteinSimilarity(flattened) ≥ 0.90` and
  `tokenDice(tokens) ≥ 0.80` → `Fuzzy` candidate. `levenshteinSimilarity a b` = 1.0 if equal, 0.0
  if either is empty, else `1 − dist/max(len a, len b)` (UTF-16 code units, unit costs).
  `tokenDice` = `2·|A∩B| / (|A|+|B|)` over token *sets* (1.0 when both empty).
- **Where:** `levenshteinSimilarity` (144–161), `tokenDice` (136–142), `matchAlias` (271–289).
- **Why:** minor spelling differences of the same stop (`stare` / `staré`).
- **Pinned by:** `Balanced fuzzy match accepts a minor spelling difference nearby`, `Nearby
  unrelated names and conflicting regions remain separate`, `Fuzzy candidates do not depend on
  global token rarity`.
- **Constants:** `0.90` → `jdf.stops.fuzzy_min_levenshtein`; `0.80` →
  `jdf.stops.fuzzy_min_token_dice`; `2` tokens → `jdf.stops.fuzzy_min_tokens`; `75` (R220).
- **Diagnostic:** statistics `fuzzy`; the merge log line.
- **Proposed verdict:** port.

#### JDF-R222 — Selecting among candidates; ambiguity
- **What:** all candidates are grouped by merged stop ID; per stop the best alias is the minimum
  by `(kind rank Exact 0 / Suffix 1 / Fuzzy 2, distance ?? 0.0)` (stable sort over candidate
  aliases in ascending alias-ID order). Across stops only the strongest kind present is kept.
  Exactly one stop left → match (`Choice1Of2`); none → no match; more than one → ambiguous
  (`Choice2Of2 candidates`, statistics `ambiguous`). No distance tie-break between stops.
- **Where:** `FindMatch` (291–324).
- **Why:** README / BASE_PLAN ("ambiguity is quarantined").
- **Pinned by:** `Ambiguous suffix candidates remain separate`, `Exact candidate wins over an
  older suffix candidate`.
- **Constants:** kind ranks.
- **Diagnostic:** log `Ambiguous stop reconciliation for {StopName}; candidates: {Candidates}`
  (each `"{display name or 'registered N'} [{kind}; distance={d}]"`).
- **Proposed verdict:** port; diagnostic `jdf_stop_ambiguous` with the candidates as fields.

#### JDF-R223 — Registry tie-break by nearest reference
- **What:** only when R222 is ambiguous: if the incoming location is precise **and every**
  candidate stop ID has a registry reference, pick the candidate with the smallest
  equirectangular distance to its reference, ties by smaller stop ID. Otherwise the stop stays
  ambiguous. No maximum distance (a candidate may have qualified through a different, location-less
  alias of the same number, R220).
- **Where:** `JdfMerger.nearestRegisteredCandidate` (579–592), `beginAdd` (707–713).
- **Why:** comment: "Several registered stops share a name: take the one whose reference
  coordinates are nearest to a precise incoming location."
- **Pinned by:** `Same-named registered stops are split by reference coordinates`.
- **Constants:** none (distance function differs from R220's; section 8).
- **Diagnostic:** none.
- **Proposed verdict:** port, using one distance function; cap at `jdf.stops.match_distance_m`
  (unsure, Q3).

#### JDF-R224 — No match or ambiguous: a new stop (quarantine)
- **What:** a new merged stop is created from the incoming row (attributes remapped) with a new
  number (R228 or R230); `provisionalReasons[id] = "ambiguous"` if R222 returned candidates, else
  `"new"` (registry runs only). The incoming spelling becomes an alias of the new number (R225).
  A later batch's stop with the same spelling therefore sees the registered candidates **plus**
  this new stop and is ambiguous again, so it creates yet another stop (B1).
- **Where:** `beginAdd` (754–784).
- **Why:** BASE_PLAN §6 ("ambiguity is quarantined").
- **Pinned by:** `Ambiguous suffix candidates remain separate` (3 stops),
  `Same-named registered stops are split by reference coordinates` (`unlocated` → provisional,
  reason `ambiguous`).
- **Constants:** reason texts `new`, `ambiguous` (candidates-file contract).
- **Diagnostic:** `jdf_stop_quarantined` (ambiguous), `jdf_stop_provisional` (new).
- **Proposed verdict:** replace: one quarantined stop per ambiguous identity (normalized identity
  + locality), shared by every occurrence, computed after all batches are known (section 9, Q2).

#### JDF-R225 — Alias accumulation
- **What:** after a match (to an existing or registered number) and after creating a new stop, the
  incoming source spelling is added as an alias of that number with its location
  (`AddAlias`). Aliases are keyed by `(number, rawAliasKey)`: an existing alias with the same key
  is updated (missing okres/country filled from the incoming; location replaced only when it had
  none, or had a non-precise one and the incoming is precise; spatial index moved accordingly);
  otherwise a new alias with a fresh alias ID is indexed in the exact, suffix and spatial indexes.
  All later matching sees these aliases, so suffix and fuzzy matches chain: a spelling that
  matches only an earlier batch's spelling joins its stop.
- **Where:** `StopReconciler.AddAlias` (326–381); `beginAdd` (740, 783).
- **Why:** comment on the reconciler ("Incremental, indexed reconciliation").
- **Pinned by:** `A later precise location updates an existing alias`, `Exact candidate wins
  over an older suffix candidate`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** replace with an order-independent clustering over all spellings of all
  batches (section 9, Q1). Chaining is either allowed explicitly (transitive closure, with a
  diagnostic) or forbidden (matching only against registry rows and exact-identity groups).

#### JDF-R226 — Canonical spelling of a merged stop
- **What:** when an incoming spelling joins an existing merged stop, `rank s` =
  `(−redundantAdjacent, componentCount, −dotCount, displayLength)` over the non-blank components,
  where `redundantAdjacent` = number of adjacent component pairs equal after trim (ordinal
  ignore-case), `dotCount` = `.` characters in the display name, `displayLength` = UTF-16 length
  of the display name. The incoming spelling replaces the current one when its rank is greater
  (tuple order), or equal and its display name is ordinally smaller. The merged row takes the
  preferred spelling's `town`/`district`/`nearbyPlace`; `regionId` and `country` = preferred's
  if present else the other's; attributes per R227. For a registered number the first row is the
  first incoming spelling, not the registry spelling.
- **Where:** `isCanonicalNamePreferred` (397–418); `beginAdd` (716–732).
- **Why:** test names: expanded CIS names beat abbreviations; clean beats redundant; hierarchical
  beats shortened.
- **Pinned by:** `Expanded CIS stop name replaces abbreviated alias`, `Clean alias wins over a
  redundant three-component alias`, `Hierarchical CIS name replaces shortened MHD form`,
  `Canonical name is independent of input order`.
- **Constants:** rank tuple.
- **Diagnostic:** merge log line.
- **Proposed verdict:** port (it is a total order, so order-independent); compute it once over
  the final cluster.

#### JDF-R227 — Stop attributes union
- **What:** merged stop attributes = distinct union of current and incoming merged attribute IDs,
  sorted ascending by **merged attribute-ref ID**, first 6 kept; more than 6 → warning `Merged
  stop {StopId} has {AttributeCount} attributes; JDF can retain only six`.
- **Where:** `mergeAttributes` (139–151).
- **Why:** JDF `Zastavky` has six attribute columns.
- **Pinned by:** `Expanded CIS stop name replaces abbreviated alias`.
- **Constants:** `6`.
- **Diagnostic:** warning log.
- **Proposed verdict:** replace: the model's stop place carries the set of typed facilities; the
  6-slot limit belongs to a JDF projection only (none is published today).

#### JDF-R228 — Stop numbering without a registry
- **What:** `lastStopId += 1` for each new merged stop, in merge order (1, 2, 3, …).
- **Where:** `beginAdd` (769–773).
- **Why:** README ("numbers stops with a counter in merge order, so `jdf:stop:N` changes").
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** drop. The rewrite always numbers by registry + provisional hash (R229,
  R230); a run without a registry is an empty registry.

#### JDF-R229 — Registered numbers
- **What:** a match to a registered alias gives the merged stop the registered (resolved) number.
  The first such match in the merge creates the merged row (`stops.Add({incoming with id})`);
  later ones update it (R226).
- **Where:** `beginAdd` (733–738).
- **Why:** BASE_PLAN §6, §8.
- **Pinned by:** `Registered stops keep their numbers whatever the batch order`, `Merged registry
  numbers resolve to the surviving number`.
- **Constants:** none.
- **Diagnostic:** summary counts (R235).
- **Proposed verdict:** port.

#### JDF-R230 — Provisional numbers
- **What:** identity = `stopIdentityKey(incoming) + "\u001f" + localityKey(incoming)` of the
  **source stop that created the merged stop** (not the canonical spelling). `digest =
  SHA-256(UTF-8(identity))`; `value = (first 8 bytes as little-endian UInt64) mod 1000000000`;
  candidate `k` = `1000000000 + (value + k) mod 1000000000` for `k = 0, 1, …`; the first candidate
  not already a merged stop number (`stopIndexesById.ContainsKey`) is taken; `k` reaching
  `1000000000` → `invalidOp "Provisional stop ID range is exhausted"`.
- **Where:** `StopRegistry.provisionalStopId` (226–233); `beginAdd` (774–778).
- **Why:** comment: "It depends only on the identity, so it is the same whatever order batches are
  merged in; a collision probes upwards."
- **Pinned by:** `Unregistered stops are listed with deterministic provisional numbers`,
  `Provisional numbers probe past collisions`, `Registered stops keep their numbers whatever the
  batch order`.
- **Constants:** `1000000000` base and span → `jdf.stops.provisional_base`,
  `jdf.stops.provisional_span`; hash = SHA-256 LE u64 (identity contract).
- **Diagnostic:** `jdf_stop_provisional` with reason.
- **Proposed verdict:** port the formula (it is published and reviewed through the candidates
  file), but feed it the *cluster's* canonical identity and probe in sorted identity order, so
  that both the hash input and collisions are order-independent (B4).

#### JDF-R231 — Stop location merge
- **What:** for each `Polzast` row of the batch (file order), mapped to its merged stop:
  1. if the merged stop has a location, both are precise, they differ, and EPSG:3035 distance
     `> 1000` m → warning `Stop location for {StopId} in new batch is too far from existing
     location` (nothing else happens);
  2. the incoming row replaces the stored one when there is none, or it is more precise (`S` over
     `E`), or equal precision and the incoming spelling won R226 in this batch
     (`preferredIncomingLocationSources`);
  3. on replacement, the stored source becomes the batch's `PolzastZdroj` source for that stop, or
     is removed if the batch has none.
  A later duplicate `Polzast` row for one source stop overwrites an earlier one before this step
  (last wins).
- **Where:** `beginAdd` (685–689, 785–815); `locationDistanceThresh = 1000` (137).
- **Why:** comment ("Either this is a new location or a precision upgrade").
- **Pinned by:** none directly (`A later precise location updates an existing alias` covers the
  alias, not the stored location).
- **Constants:** `1000` m → `jdf.stops.location_conflict_warn_m`; precision ranks `S`=0, `E`=1.
- **Diagnostic:** warning log → `jdf_stop_location_conflict` with both positions.
- **Proposed verdict:** replace: coordinates are the `coordinates` pass's business; the compiler
  carries every source location fact per spelling, and the pass chooses (precise over estimated,
  then the canonical spelling's, then a deterministic order). Keep the conflict diagnostic.

#### JDF-R232 — Registry candidates: provisional stops
- **What:** registry runs only. Every spelling that reaches a provisional number is recorded:
  when the number is created (R224) and whenever another spelling joins it, keyed by
  `spellingKey` (first stop per key wins). Rows: for each provisional number in ascending order,
  for each of its spellings in ordinal `spellingKey` order: `[number; town; district; nearby;
  okres; country; lat; lon; reason; suggestion]`, where `lat`/`lon` are the merged stop's stored
  location only if precise, formatted `0.000000` (invariant), else empty; `reason` =
  `provisionalReasons[number]`; suggestion per R234. Spellings of provisional stops whose versions
  were all dropped later (R248, R249) are still listed.
- **Where:** `recordStopIdentity` (594–603), `stopRegistryCandidates` (608–649).
- **Why:** README ("unregistered stops and new spellings of registered ones").
- **Pinned by:** `Unregistered stops are listed with deterministic provisional numbers` (exact row
  `[provisional; "Bílina"; "lázně"; ""; "UL"; "CZ"; "50.548000"; "13.775200"; "new"; ""]`),
  `Same-named registered stops are split by reference coordinates` (reason `ambiguous`).
- **Constants:** header and reason texts (contract with `registry.py`); coordinate format.
- **Diagnostic:** the CSV itself; log `Wrote {Count} stop registry candidates to {Path}`.
- **Proposed verdict:** port (the file is a contract with `registry.py promote`); list only stops
  that survive into the published timetable (Q20).

#### JDF-R233 — Registry candidates: new spellings of registered stops
- **What:** a spelling that joined a registered number and whose `(number, stopIdentityKey)` is
  not among the registry's identities (R210) is recorded once per `spellingKey` (first number
  wins). So only suffix, fuzzy and different-locality matches are offered; spellings differing
  from a registry row only by normalisation (e.g. `nám.` vs `náměstí`) are not. Rows, after the
  provisional ones, in ordinal `spellingKey` order: `[""; town; district; nearby; okres; country;
  lat; lon; "alias"; number]`.
- **Where:** `recordStopIdentity` (600–603), `stopRegistryCandidates` (642–649).
- **Why:** comment ("spellings of registered stops that matched only by suffix, fuzzy name or
  locality").
- **Pinned by:** `A new spelling of a registered stop is offered as an alias`, `Merged registry
  numbers resolve to the surviving number` (no candidate).
- **Constants:** `alias`.
- **Diagnostic:** CSV.
- **Proposed verdict:** port.

#### JDF-R234 — Registry candidates: alias suggestions
- **What:** for a provisional spelling: registered stops with the same lower-cased trimmed raw
  `town`, locality-compatible (R216), `levenshteinSimilarity(flattened normalized names) ≥ 0.85`;
  distinct numbers sorted ascending, **first three by number** (not by similarity), joined with
  `;`; empty when none or the town is unknown.
- **Where:** `stopRegistryCandidates.suggestions` (616–628).
- **Why:** reviewer convenience; `promote` uses a single suggestion as the alias target.
- **Pinned by:** none.
- **Constants:** `0.85` → `jdf.stops.registry_suggestion_min_similarity`; `3` →
  `jdf.stops.registry_suggestion_max`.
- **Diagnostic:** CSV.
- **Proposed verdict:** port, ranking by similarity then number (B16).

#### JDF-R235 — Reconciliation statistics
- **What:** counters `exact`, `suffix`, `fuzzy`, `ambiguous`, `candidateComparisons`,
  `fuzzyComparisons`; registry summary `{Registered}` (merged stops `< 1000000000`),
  `{Provisional}` (count of provisional numbers), `{Ambiguous}` (reason `ambiguous`),
  `{Aliases}` (unregistered spellings). Logs only.
- **Where:** `logStopMergeSummary` (282–294), `logStopRegistrySummary` (651–660).
- **Why:** BASE_PLAN §8 report list.
- **Pinned by:** several tests read `stopMergeStatistics`.
- **Constants:** none.
- **Diagnostic:** logs.
- **Proposed verdict:** replace by `diagnostics.json` counts per code (BASE_PLAN §8 also asks for
  "Large coordinate shifts against the registry", which nothing computes today; propose
  `jdf_stop_registry_shift` with a policy threshold).

### Stage 4 — other shared entities

#### JDF-R236 — Post evidence remap and conflict
- **What:** each `JrutilPostCandidateEvidence` row gets its merged stop ID; key `(stopId,
  observationId)`; an existing different row with the key → `invalidArg "batch" "Conflicting
  merged post observation {stop}/{observation}"` (aborts the merge); an equal row → skipped.
  Output sorted by `(stopId, candidateId, observationId)`.
- **Where:** `beginAdd` (817–825); `batchWithTripStops` (243–246).
- **Why:** unknown.
- **Pinned by:** `Post observations remap to canonical stops deterministically`.
- **Constants:** none.
- **Diagnostic:** fatal.
- **Proposed verdict:** port the remap (evidence attaches to the merged place); make a conflict a
  diagnostic that keeps the first by sorted key (Q16). Content belongs to the post estimator.

#### JDF-R237 — Stop posts deduplication
- **What:** an `Oznacniky` row is added unless `(merged stopId, stopPostId)` is already present;
  the first batch's row (name, description, post name) wins.
- **Where:** `beginAdd` (826–833).
- **Why:** unknown.
- **Pinned by:** `Expanded CIS stop name replaces abbreviated alias` (two posts of one stop kept).
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port with a diagnostic `jdf_post_attribute_conflict` when the duplicate's
  fields differ; whether post numbers of different carriers at one merged stop are the same post
  is unknown (B15).

#### JDF-R238 — Agency deduplication
- **What:** for each `Dopravci` row in file order: find an agency with the same IČO (`id`) whose
  record equals the row in **every** field except `idDistinction`; found → map to it; not found →
  new merged distinction = `max(existing distinctions for the IČO) + 1` (1 when none), added
  immediately, so a later identical row in the same batch maps to it. All `(agencyId,
  agencyDistinction)` references (versions, `Altdop`) are remapped.
- **Where:** `beginAdd` (835–874).
- **Why:** comment ("We need to add it right away, in case the input has multiple agencies of the
  same ID").
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** replace: an operator is identified by IČO; differing contact fields are
  facts with a source batch, chosen by a named rule (e.g. most frequent, then sorted), with a
  diagnostic `jdf_agency_variant`. Unsure whether serving relies on agency distinctions (Q15).

#### JDF-R239 — Version renumbering
- **What:** each `Linky` row of the batch gets merged distinction = (number of versions of its
  licence already merged) + 1, computed for the whole batch **before** any is added. Two versions
  of one licence in one batch therefore get the **same** merged distinction, and their per-version
  rows are merged under one key (B5). All per-version relations are re-keyed through the
  resulting `routeIdMap`.
- **Where:** `beginAdd` (882–912).
- **Why:** comment ("We don't resolve validity overlaps here").
- **Pinned by:** every version test (one version per batch).
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** replace: a version's identity is `(source batch, licence, source
  distinction)` plus its published facts (R240); no merged distinction exists in the model.

#### JDF-R240 — Source version facts
- **What:** per merged version: `sourceCreationDate` = the batch's `VerzeJDF` creation date (may
  be absent); `originalValidity` = the published `(timetableValidFrom, timetableValidTo)`. If the
  input batch carries a `JrutilRouteVersions` row for the source key (re-merging a merged batch),
  those values are taken from it instead.
- **Where:** `beginAdd` (891–908).
- **Why:** model comment ("the published identity of each merged route version, which the
  merge-assigned distinction does not keep").
- **Pinned by:** `Trip version keys name the published schedule in any merge order`, `Versions
  starting on one day get a hashed trip version key`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port as fields on the version entity; drop the re-merge path (no
  intermediate JDF in the rewrite).

#### JDF-R241 — Per-version relation remapping
- **What:** `LinExt`, `Zaslinky` (also `stopId` → merged, attributes remapped), `Spoje`
  (attributes remapped), `Zasspoje` (`stopId`, attributes), `Udaje`, `Caskody`, `Navaznosti`,
  `Altdop` (agency remapped, attributes), `Altlinky`, `Mistenky` are re-keyed to the merged
  version and stored per version (`MultiDict.Replace`). `Zasspoje.stopPostId` and the
  `Navaznosti` stop fields are not remapped.
- **Where:** `beginAdd` (914–1055).
- **Why:** plain re-keying.
- **Pinned by:** `Spill-backed output is byte-identical across interleaved rows and route splits`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** replace: handles in the model; no re-keying.

#### JDF-R242 — Trip groups
- **What:** every `SpojSkup` row gets `lastTripGroupId += 1` in merge order. A trip keeps a
  (remapped) `tripGroupId` only if its version has `grouped = true`; then a trip without a group
  ID throws (`Option.get`). Groups of non-grouped versions are still emitted.
- **Where:** `beginAdd` (936–963).
- **Why:** unknown.
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** none (exception).
- **Proposed verdict:** port as a fact on the trip (group name/description), dropping the
  numbering; a grouped version's trip without a group is a reader diagnostic. Unsure whether
  anything downstream uses trip groups.

### Stage 5 — line versions

#### JDF-R243 — Day-class mask of a version
- **What:** 8-bit mask; bits 0–6 = Monday–Sunday on ordinary days, bit 7 = a public holiday. Per
  trip, from its attributes (merged `Pevnykod`): `X` (`WeekdayService`) → `0b0001_1111`; `+`
  (`HolidaySundayService`) → `0b1100_0000`; a digit `d` in `1..7` (`DayOfWeekService d`) →
  `bit(d−1) | 0b1000_0000`; other attributes, and digits outside `1..7`, contribute nothing. A
  trip with no contributing attribute, or with any `Caskody` row of type `2` (jede také) or `3`
  (jede jen), has mask `0xFF`. Version mask = OR over its trips (0 for a version without trips).
- **Where:** `dayClassMask` (402–432).
- **Why:** comment: "Conservative: a trip without day codes, or with dated 'runs also/only'
  notes, can run on any day, so two masks are disjoint only when the versions provably never run
  on the same date."
- **Pinned by:** `Weekday and weekend versions with one validity are both kept`, `Versions with
  one validity and shared days keep only one`.
- **Constants:** bit layout.
- **Diagnostic:** none.
- **Proposed verdict:** replace: the version's running `DateSet` (union of its trips' calendars
  from R255–R264) is exact, so "provably disjoint" is `a ∩ b = ∅`. See R244.

#### JDF-R244 — Side-by-side versions (units)
- **What:** versions of one licence, in merged-distinction order, are grouped first-fit into
  units: a version joins the first unit whose head has the same current `timetableValidFrom`,
  `timetableValidTo`, `detour` and batch date, and whose accumulated mask is disjoint from the
  version's mask; otherwise it starts a new unit. Only a unit's head takes part in R245–R247; all
  members get the unit's won dates. First-fit makes the result order-dependent (B7: with `X`,
  `6`, `+` versions of one validity, which weekend version survives depends on their order).
- **Where:** `resolveLicenceOverlaps` (471–490).
- **Why:** comment: "Versions with the same validity, batch date and detour flag whose trips run
  on disjoint day classes (a weekday-only and a weekend-only timetable published side by side)
  are kept together instead of one deleting the other."
- **Pinned by:** `Weekday and weekend versions with one validity are both kept`, `Versions with
  one validity and shared days keep only one`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** replace: with `DateSet`s, resolve per date only among versions that
  actually run on that date (a version keeps every date on which it is the winner among the
  versions running then); side-by-side publications then need no special case (Q8).

#### JDF-R245 — Detour priority
- **What:** `detourPriority(r, o)` = `r.detour && not (r.origTo = o.origTo && r.origFrom <
  o.origFrom)` (published validity, R240). An open-ended detour that ends with a later-starting
  version has no priority over it.
- **Where:** `resolveLicenceOverlaps` (447–456).
- **Why:** comment: "CIS publishes many detour timetables open-ended, ending with the regular
  versions. Such a detour only lasts until the next version starts, so it has no priority over a
  variant that starts later and was originally published with the same end."
- **Pinned by:** `Later versions supersede an open-ended older detour`, `A cut open-ended detour
  does not delay later versions`, `A bounded detour keeps priority over a later regular version`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### JDF-R246 — Batch-date priority
- **What:** `hasPriority(r, o)` = `(dp(r,o) && (not dp(o,r) || date(o) < date(r))) || (not
  dp(o,r) && date(o) < date(r))`, with `dp` = R245 and `date` = batch creation date (option
  order: absent < any date). I.e. a detour with priority beats a non-priority version; between
  two priority detours or two non-priority versions the newer batch wins; a non-priority version
  never beats a priority detour. **(data)** All batches of one export carry the export date, so
  in production this reduces to `dp(r,o) && not dp(o,r)`.
- **Where:** `resolveLicenceOverlaps` (457–461).
- **Why:** comment ("the version with priority wins (a newer batch, or a detour …)").
- **Pinned by:** the three detour tests (equal batch dates); the newer-batch arm only by
  `Versions starting on one day get a hashed trip version key` (2026-01-02 vs 2026-01-01).
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port the detour arm; drop the batch-date arm or keep it as dead weight
  (Q7): within one export it never fires.

#### JDF-R247 — Per-date winner
- **What:** boundaries = distinct sorted `{head.validFrom, head.validTo + 1}` over units (current
  validity). For each interval `[b_i, b_{i+1} − 1]`, the candidates are units whose head covers
  `b_i`; the winner is a left fold in unit order: start with the first candidate, replace the
  current best by the next candidate when `beats(next, best)`. `beats(r, o)`: `hasPriority(r,o)`
  → true; `hasPriority(o,r)` → false; later `timetableValidFrom` wins; then earlier
  `timetableValidTo`; then smaller merged distinction (= earlier batch). Won intervals of a unit
  are coalesced when contiguous. `beats` is not transitive in general (B6), so the fold result can
  depend on unit order, contrary to the comment.
- **Where:** `resolveLicenceOverlaps` (462–469, 492–517).
- **Why:** comment: "on each date the version with priority wins …; otherwise the one starting
  later, then the one ending sooner, then the first added. … Deciding every date against the
  published input ranges keeps the result independent of the order of comparisons."
- **Pinned by:** `A year-long special-day version does not hide later regular versions`, the
  three detour tests, `Trip version keys name the published schedule in any merge order`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** replace with a set-algebra resolver over `DateSet`s: per date, the winner
  is the maximum under a *total* order (priority class, then later start, then earlier end, then
  a content-derived key instead of batch order); each version's trips are masked with the dates
  it wins. This is the "one interval resolver" of AGENTS rule 5 for road.

#### JDF-R248 — Cutting, copying and deleting versions
- **What:** per unit member: no won interval → the version and every per-version relation are
  deleted (`Route … is superseded on every date, removing it`); otherwise the version's validity
  becomes its first won interval, and each further interval becomes a **copy** of the version with
  merged distinction `max(distinctions of the licence) + 1`, carrying a copy of every per-version
  relation (route integrations, route stops, trips, trip stops, route info, notes, transfers,
  agency alternations, alternate names, reservations) and the same source facts (R240). Trips,
  notes and attributes are not otherwise changed; a cut version's notes keep their dates.
- **Where:** `resolveLicenceOverlaps` (519–541); `copyRoute` (316–400); `deleteRoute`
  (296–314).
- **Why:** comment ("Each version keeps the dates it wins, split into contiguous ranges").
- **Pinned by:** `Later versions supersede an open-ended older detour`, `Trip version keys name
  the published schedule in any merge order` (`:p2` chunk).
- **Constants:** none.
- **Diagnostic:** debug logs `… version {Dist} keeps {Ranges}`.
- **Proposed verdict:** replace: masking the trips' `DateSet`s with the won dates; no copies. The
  trip identity of later chunks (`:pN`, R254) is Q9. Diagnostic `jdf_version_superseded`
  (entirely) and `jdf_version_cut` (partly), with the version identity and the dates kept.

#### JDF-R249 — Validity bounding
- **What:** after R247/R248, for each version: `validTo < reference || validTo < gvdStart` →
  deleted (`expired`); else `validFrom > gvdEnd` → deleted (`next-GVD`); else if `validFrom <
  gvdStart || validTo > gvdEnd` → clamped to `[max(validFrom, gvdStart), min(validTo, gvdEnd)]`.
  Dates before the reference date inside the GVD are kept. "Open-ended" validity is a far-future
  `validTo` (e.g. `31122099` in the test) and is clamped like any other.
- **Where:** `boundValidity` (549–577).
- **Why:** comment: "The GVD change is a hard cutover, so validity past its end (often open-ended
  on international lines) is never real. Run after overlap resolution, which needs every
  version's published validity."
- **Pinned by:** `Validity is bounded to the current GVD from the reference date`, `Past starts
  are clamped to the GVD start`.
- **Constants:** none (inputs R200, R202).
- **Diagnostic:** log `Bounded route validity to {GvdStart}..{GvdEnd} as of {Reference}: dropped
  {Expired} expired and {NextGvd} next-GVD versions, clamped {Clamped}` → codes
  `jdf_version_expired`, `jdf_version_next_gvd`, `jdf_version_clamped`.
- **Proposed verdict:** replace: versions' validity is read into `DateSet`s over the GVD horizon
  (dates outside are counted by `DateSet::from_dates`); a version whose `DateSet` is empty or ends
  before the reference date is dropped with the diagnostic. Order relative to R247 matters:
  overlap resolution must still see the published validity (R247 uses it).

#### JDF-R250 — Merged batch header
- **What:** `VerzeJDF` = `{version = "1.11"; duNum = None; region = None; batchId = None;
  creationDate = referenceDate ?? today (wall clock); generator = "JrUtil JdfMerger"}`. The
  reference date is set by `boundValidity`; without it (tests) the wall clock is used.
- **Where:** `batchWithTripStops` (208–215); `boundValidity` (554).
- **Why:** comment: "The merged batch is dated by the build's reference date, not the wall clock,
  so identical inputs produce identical merged JDF bytes."
- **Pinned by:** none.
- **Constants:** `"1.11"`, `"JrUtil JdfMerger"`.
- **Diagnostic:** none.
- **Proposed verdict:** drop (no merged JDF). The reference date is a manifest field; the
  `filtered-jdf` projection (ARCHITECTURE) decides its own header.

### Stage 6 — output facts and derived identity

#### JDF-R251 — `JrutilRouteVersions`
- **What:** one row per surviving merged version (copies included): `routeId`,
  `routeDistinction` (merged), `originalValidFrom`, `originalValidTo`, `sourceCreationDate`;
  sorted by `(routeId, routeDistinction)`. Written as `JrutilRouteVersions.txt`
  (**(data)** e.g. `"100100","1","30112026","31082027","20112026"`).
- **Where:** `batchWithTripStops` (248–258); `Jdf.fs` writer (328).
- **Why:** model comment (R240).
- **Pinned by:** `Trip version keys name the published schedule in any merge order` (keys equal
  after write/re-read).
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** replace by model fields on the version (section 5).

#### JDF-R252 — Routing demands (handed to the post estimator)
- **What:** computed at write time from merged route stops of every surviving version
  (consecutive pairs in `routeStopId` order where either stop has post-candidate evidence →
  `("both", "local", prev, next)`) and from every evidence stop with a stored location
  (`("both", "terminal", stop, None)`); demands with no located endpoint dropped; `demandId` =
  lower-case hex SHA-256 of `"{mode}|{previous}|{next}|{searchClass}"` (F# option text); distinct
  by ID; sorted.
- **Where:** `batchWithTripStops` (162–206).
- **Why:** model comment ("Compact merge output consumed by the Oběhy Osmium orchestration").
- **Pinned by:** none here.
- **Constants:** see the post-estimator inventory.
- **Diagnostic:** none.
- **Proposed verdict:** not inventoried here; the post estimator derives it from the model.

#### JDF-R253 — Output order
- **What:** stops and stop posts in merge order; agencies, versions and every per-version
  relation in `MultiDict` (`Dictionary`) enumeration order, which is insertion order only while
  nothing was removed (.NET reuses freed slots), with version lists reordered by the
  `Remove`/`Add` pairs of R248 and R249; trip stops in spool-dictionary order (R206); attribute
  refs in merged order; locations in dictionary order; `JrutilRouteVersions`, evidence and demands
  sorted.
- **Where:** `batchWithTripStops` (216–247); `JdfTripStopSpool.WriteTo`.
- **Why:** none (incidental).
- **Pinned by:** `Spill-backed output is byte-identical …` (memory vs spill only).
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** drop (AGENTS rule 7: sorted or source order).

#### JDF-R254 — Trip-version key (identity built from this area's facts)
- **What:** for each licence, versions grouped by `(origFrom, detour)`; `baseKey` =
  `esc(licence) + ":" + yyMMdd(origFrom) + (":det" if detour)`, `esc` = `Uri.EscapeDataString`.
  Within a group, versions grouped by `source = (origFrom, origTo, created)` and sorted by
  `(origTo, created)` (absent created first); the first source group gets `sourceKey = baseKey`,
  the others `baseKey + ":" + hex8(SHA-256(UTF-8("{yyMMdd origFrom}|{yyMMdd origTo}|{yyMMdd
  created or ''}")))` (first 4 bytes, lower-case). Within a source group the members (chunks of
  R248 **and** side-by-side unit members of R244, which share all three facts) are sorted by
  `(timetableValidFrom, merged distinction)` and keyed `sourceKey`, `sourceKey:p2`, `:p3`, …; a
  repeated key → `invalidOp`. Trip ID = `jdf:trip:{key}:{trip number}`. A version without a
  `JrutilRouteVersions` row uses its current validity and the batch date.
- **Where:** `JdfGtfsRules.buildTripVersionKeys` (341–377), `jdfTripId` (388–389);
  `inheritTripVersionKeys` keeps parent keys for filtered batches.
- **Why:** README ("Trip ids name the published schedule rather than the merge-assigned
  distinction, so they stay the same between runs"); BASE_PLAN §6 ("trip IDs are stable for one
  timetable version").
- **Pinned by:** `Trip version keys name the published schedule in any merge order`, `Versions
  starting on one day get a hashed trip version key`.
- **Constants:** date format `yyMMdd`; `:det`; `:p`; hash length 8 hex.
- **Diagnostic:** fatal on repeated key.
- **Proposed verdict:** port the scheme through `jr_model::identity`, but (a) drop `created` from
  the hash (it is the export date and changes every build, B8) or replace it with a published
  fact, and (b) give side-by-side versions a content-derived discriminator instead of `:pN` by
  merge order (B8, Q10).

### Stage 7 — calendars (`JdfCalendar`, run on the merged batch)

The running days of a trip are computed for every date `d` of its version's (cut and clamped)
validity by a **first-match ladder**, R256–R264 in that order. R255 prepares the notes. The
ladder runs over the version's current validity, so after R248/R249 it runs per chunk.

#### JDF-R255 — Note preprocessing
- **What:** the trip's `Caskody` rows with a type (blank type → ignored by the calendar); per row
  `from = dateFrom ?? version.timetableValidFrom` (current, i.e. cut/clamped, start) and `to =
  dateTo ?? from`. A row covers `d` when `from ≤ d ≤ to`. So a row without dates covers only the
  version's first day, and a row with only `dateFrom` covers that single day.
- **Where:** `tripDateBitmap` (33–45).
- **Why:** unknown (single-date forms of types 2/3/4 are standard JDF; the default start is not).
- **Pinned by:** `JdfToGtfsTests.Shared inactive schedules delete every source trip` (a `4` row
  spanning the validity).
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port the single-date rule; replace the "no dates → first validity day"
  default by the JDF 1.11 meaning of each type (B9, Q11), with a diagnostic
  `jdf_note_without_dates` for types that require dates.

#### JDF-R256 — Type 3 "jede jen" (runs only)
- **What:** ladder step 1: a type-3 row covers `d` → runs. Step 2: the trip has any type-3 row
  (not covering `d`) → does not run. Day attributes are ignored for such trips.
- **Where:** `tripDateBitmap` (69–70).
- **Why:** JDF `Caskody` type 3.
- **Pinned by:** none.
- **Constants:** type code `3`.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### JDF-R257 — Type 4 "nejede" (does not run)
- **What:** step 3: a type-4 row covers `d` → does not run.
- **Where:** `tripDateBitmap` (71).
- **Why:** JDF type 4.
- **Pinned by:** `JdfToGtfsTests.Shared inactive schedules delete every source trip`.
- **Constants:** `4`.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### JDF-R258 — Type 2 "jede také" (runs also)
- **What:** step 4: a type-2 row covers `d` → runs (after type 4, so "nejede" beats "jede také"
  on the same date).
- **Where:** `tripDateBitmap` (72).
- **Why:** JDF type 2.
- **Pinned by:** none.
- **Constants:** `2`.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### JDF-R259 — Types 5–8: odd and even weeks
- **What:** step 5: a type-5 or type-7 row covers `d` and the ISO-8601 week number of `d` is even
  → does not run. Step 6: a type-6 or type-8 row covers `d` and the ISO week number is odd → does
  not run. Otherwise the ladder continues (these rows only exclude). Types 5/6 ("jede v
  lichých/sudých týdnech", normally undated) are treated exactly like 7/8 ("… od – do"), so with
  R255 an undated type-5/6 row restricts only the version's first day (B9).
- **Where:** `tripDateBitmap` (73–78).
- **Why:** JDF types 5–8.
- **Pinned by:** none.
- **Constants:** `5`–`8`; week rule `ISO` (`WeekYearRules.Iso`).
- **Diagnostic:** none.
- **Proposed verdict:** port 7/8; fix 5/6 to apply over the whole validity (Q11). **(data)** no
  5–8 rows in the merged sample, so the fix changes nothing measured.

#### JDF-R260 — Type 1 "jede" (runs in)
- **What:** step 7: the trip has any type-1 row and none covers `d` → does not run. A covering
  type-1 row does not make the trip run; the day attributes (steps 8–11) still decide.
- **Where:** `tripDateBitmap` (79–80).
- **Why:** JDF type 1 (the period in which the trip runs according to its day codes).
- **Pinned by:** none.
- **Constants:** `1`.
- **Diagnostic:** none.
- **Proposed verdict:** port. Whether a type-1 row with only `dateFrom` means one day or "from
  this date on" is Q11.

#### JDF-R261 — `+` (Sundays and holidays)
- **What:** step 8: attribute `+` and (`d` is a holiday per R265 or a Sunday) → runs.
- **Where:** `tripDateBitmap` (81–83).
- **Why:** JDF `Pevnykod` `+`.
- **Pinned by:** none.
- **Constants:** `+`.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### JDF-R262 — `X` (working days)
- **What:** step 9: attribute `X`, `d` not a holiday, not Saturday, not Sunday → runs.
- **Where:** `tripDateBitmap` (84–87).
- **Why:** JDF `X`.
- **Pinned by:** none.
- **Constants:** `X`.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### JDF-R263 — Day of week `1`–`7`
- **What:** step 10: attribute digit equal to the ISO day of week of `d` (Monday 1 … Sunday 7) →
  runs, **including on holidays** (a Monday-only trip runs on Easter Monday). A digit outside
  `1..7` matches no date.
- **Where:** `tripDateBitmap` (88–90); `Attribute.CsvParse` (any all-digit text is a day).
- **Why:** JDF `1`–`7`; the holiday behaviour: unknown.
- **Pinned by:** none.
- **Constants:** `1`–`7`.
- **Diagnostic:** none; propose `jdf_day_code_invalid` for digits outside `1..7` (reader).
- **Proposed verdict:** port; confirm the holiday semantics against JDF 1.11 (Q11).

#### JDF-R264 — No day attribute
- **What:** step 11: runs iff the trip has none of `X`, `+`, `1`–`7` (any digit counts, so a trip
  whose only day code is `8` never runs).
- **Where:** `tripDateBitmap` (49–55, 91).
- **Why:** a trip without day codes runs daily.
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### JDF-R265 — Czech public holidays
- **What:** for every year from `validFrom.Year` to `validTo.Year`: 1 Jan, 1 May, 8 May, 5 Jul,
  6 Jul, 28 Sep, 28 Oct, 17 Nov, 24 Dec, 25 Dec, 26 Dec, Good Friday (Easter − 2) and Easter
  Monday (Easter + 1); filtered to the validity. Good Friday is applied for every year (it is a
  holiday since 2016).
- **Where:** `Holidays.czechHolidays`, `czechHolidayDates` (27–54).
- **Why:** Czech law (zákon 245/2000 Sb.).
- **Pinned by:** `HolidayTests.Czech holidays for 2020`.
- **Constants:** the date list → `data/calendar/cz_public_holidays.toml` (fixed dates plus
  Easter offsets −2, +1).
- **Diagnostic:** none.
- **Proposed verdict:** port as data.

#### JDF-R266 — Easter date
- **What:** the "New Scientist" Gregorian algorithm returns `(day p, month n)`. Holidays are then
  computed as "days since March" `= p + (n − 3)·31`, ±offset, and converted back with
  `day = x mod 31`, `month = x / 31 + 3`, which yields day 0 when `x` is a multiple of 31: Easter
  on 2 April crashes on Good Friday (`LocalDate(y, 4, 0)`), Easter on 30 March crashes on Easter
  Monday. First affected years: 2051, 2056 (2 April) and 2059, 2070, 2081, 2092 (30 March).
- **Where:** `easterSundayDate` (8–24), `czechHolidays` (28–31, 44–45).
- **Why:** code comment ("I'm pretty sure this only works by accident").
- **Pinned by:** `HolidayTests.Easter Sunday for 2020`.
- **Constants:** algorithm.
- **Diagnostic:** none.
- **Proposed verdict:** replace with a correct computus and date arithmetic (B10); harmless before
  2051.

#### JDF-R267 — Trips that never run
- **What:** a trip whose ladder yields no running date over its validity is put in
  `tripsToDelete` (by GTFS trip ID). The international filter treats it as inactive (R272) and
  keeps it; the GTFS conversion omits it and its stop times.
- **Where:** `prepareGtfsCalendarWithWorkersAndProgress` (193–194, 258–262); `JdfToGtfs`
  (`getCzTrips`, 597–603, 761, 824, 831).
- **Why:** a trip without service dates is not publishable.
- **Pinned by:** `JdfToGtfsTests.Shared inactive schedules delete every source trip`.
- **Constants:** none.
- **Diagnostic:** none → propose `jdf_trip_never_runs`.
- **Proposed verdict:** port: an empty `DateSet` after version resolution and bounding drops the
  trip with the diagnostic.

#### JDF-R268 — GTFS calendar representation (handed over)
- **What:** weekday flags from `X` → Mon–Fri, `+` → Sun, `d` → day `d`, none → all seven;
  calendar over the version validity; if the number of days where bitmap and weekday calendar
  differ exceeds the number of running days, the trip is published as added exceptions only,
  else as calendar plus differing exceptions. Equal schedules share one service,
  `jdf:service:{weekday bitmap or "exc"}:{ordinal}` in ordinal trip-ID order.
- **Where:** `prepareGtfsCalendarWithWorkersAndProgress` (150–218), `gtfsCalendarBitmap`,
  `materializeUniqueCalendars` (109–126).
- **Why:** comment ("Pick the most efficient representation").
- **Pinned by:** `JdfToGtfsTests.Shared calendars are identical across workers`,
  `PerformanceEquivalenceTests.Calendar deduplication matches the previous implementation`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** drop from the compiler: `jr-publish` derives masks and service IDs from
  `DateSet`s (serving verdict 5: service IDs from the dates).

### Stage 8 — the international route filter (run on the merged batch)

#### JDF-R269 — Policies
- **What:** `--international-route-policy`: `null`, `""`, `keep-all` → `KeepAll` (the batch is
  returned unchanged, no decisions); `regional-adjacent` → `RegionalAdjacent` (R270–R281); any
  other text → `invalidArg`. Oběhy passes `regional-adjacent` to `fix-jdf` and `jdf-to-bundle`.
- **Where:** `JdfGtfsRules.parseInternationalRoutePolicy`, `internationalRoutePolicyName`
  (112–119); `JdfInternationalFilter` (59).
- **Why:** unknown in code; the reasons (`regional_adjacent`, …) describe the intent: keep
  regional cross-border service near the border, drop long-distance international lines.
- **Pinned by:** `InternationalRouteFilterTests.Keep-all preserves backward-compatible
  conversion`.
- **Constants:** policy names → `jdf.international.policy = "regional-adjacent"`.
- **Diagnostic:** none.
- **Proposed verdict:** port `regional-adjacent` as the only behaviour, switchable off by policy
  (`keep-all` exists for backward compatibility only; unsure whether anything uses it).

#### JDF-R270 — Stop country
- **What:** per merged stop: non-blank `country` → canonical: trimmed, upper-cased, `AT`/`A` →
  `A`, `DE`/`D` → `D`, others as they are (`CZ`, `PL`, `SK`, `UA`, …); blank country with an
  `okres` (`regionId`) → `CZ`; otherwise unknown.
- **Where:** `canonicalCountry` (34–38); `stopCountries` (73–83).
- **Why:** comment ("Unknown stop countries deliberately remain candidates for inspection").
- **Pinned by:** `Long neighboring and non-neighboring routes are rejected` (`AT`),
  `Short neighboring route is retained even when mislabeled city` (`DE`).
- **Constants:** alias table `AT→A`, `DE→D` → `data/jdf/country_aliases.toml`.
- **Diagnostic:** none.
- **Proposed verdict:** port; the country is a stop-place fact normalised once (ISO alpha-2
  would be cleaner: Q12).

#### JDF-R271 — Candidate routes
- **What:** a version needs classification when its `routeType` is `M`, `N` or `P` (declared
  international) or any of its `Zaslinky` stops has a country other than `CZ` (unknown
  included). Other versions are kept with all trips and reason `domestic`.
- **Where:** `routeIsDeclaredInternational` (40–45); `potentiallyInternationalRoutes` (86–96).
- **Why:** comment ("Only these routes need expensive trip-call country classification").
- **Pinned by:** `Short neighboring route is retained even when mislabeled city` (type `A`
  classified because of its foreign stop).
- **Constants:** type codes `M`, `N`, `P`.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### JDF-R272 — Only active trips are classified
- **What:** trips in `tripsToDelete` (R267) are not classified, do not contribute calls or
  countries to their route's decision, and are kept by the filter (later dropped by the GTFS
  conversion).
- **Where:** `activeTripKeys` (61–72); `tripAllowed` (257).
- **Why:** test name.
- **Pinned by:** `InternationalRouteFilterTests.Foreign calls on a trip without retained service
  dates are ignored`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port (trips with an empty `DateSet` are already gone in the rewrite).

#### JDF-R273 — Emitted calls
- **What:** a call counts when its `departureTime` is a time, or `departureTime` is absent and
  `arrivalTime` is present. `|` (passing), `<` (not passing) as departure, or no times at all → the
  call is ignored for countries, span and depth.
- **Where:** `callIsEmitted` (28–32); `callsByTrip` (106–121).
- **Why:** a passed foreign stop is not service abroad.
- **Pinned by:** `InternationalRouteFilterTests.Passing foreign stop does not make a domestic
  route international`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port, using the compiler's one "passenger call" predicate (section 8).

#### JDF-R274 — Trip classification
- **What:** over the trip's emitted calls (countries per R270), first match:
  1. no foreign country: all known and `CZ` present → `DomesticTrip`; otherwise (an unknown
     country, or no emitted call at all) → rejected `unknown_stop_country`;
  2. no `CZ` call → `ForeignOnlyTrip`;
  3. any unknown country → rejected `unknown_stop_country`;
  4. foreign countries not a subset of `{A, D, PL, SK}` → rejected `non_adjacent_country`;
  5. any emitted call without `kilometer` → rejected `missing_timetable_kilometres`;
  6. `span > spanLimit` → rejected `trip_span_exceeds_limit` (with span and depth);
  7. `depth > depthLimit` → rejected `foreign_depth_exceeds_limit`;
  8. else `QualifyingCrossBorderTrip(span, depth)`.
- **Where:** `classifyTrip` (132–169).
- **Why:** reason names; tests.
- **Pinned by:** `Short neighboring route is retained even when mislabeled city`, `Integrated route
  uses relaxed envelope`, `Long neighboring and non-neighboring routes are rejected`,
  `Foreign-only and missing-kilometre trips fail closed`.
- **Constants:** adjacent set → `jdf.international.adjacent_countries = ["A", "D", "PL", "SK"]`
  (after R270 aliases); reason texts.
- **Diagnostic:** per-trip reason (counted in the route decision).
- **Proposed verdict:** port.

#### JDF-R275 — Span and depth
- **What:** over emitted calls (all with kilometres by then): `span = max(km) − min(km)`;
  `depth = max over foreign calls f of (min over Czech calls c of |km(f) − km(c)|)`. Kilometres are
  the `Zasspoje` timetable kilometre (decimal).
- **Where:** `classifyTrip` (150–163).
- **Why:** distance travelled abroad measured along the timetable.
- **Pinned by:** the same tests as R274 (synthetic `span`/`depth`).
- **Constants:** none.
- **Diagnostic:** maxima in the route decision (R277).
- **Proposed verdict:** port.

#### JDF-R276 — Thresholds and the integrated exception
- **What:** a version is "integrated" when it has at least one `LinExt` row. Limits: integrated
  `span ≤ 200` km and `depth ≤ 80` km; otherwise `span ≤ 120` km and `depth ≤ 60` km (strict `>`
  rejects).
- **Where:** `integratedRoutes` (126–129); `classifyTrip` (164).
- **Why:** test name ("Integrated route uses relaxed envelope").
- **Pinned by:** `Integrated route uses relaxed envelope`, `Long neighboring and non-neighboring
  routes are rejected` (133 km / 84 km rejected).
- **Constants:** `120`, `60` → `jdf.international.max_span_km = 120`,
  `jdf.international.max_foreign_depth_km = 60`; `200`, `80` →
  `jdf.international.integrated_max_span_km = 200`,
  `jdf.international.integrated_max_foreign_depth_km = 80`.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### JDF-R277 — Route decision
- **What:** for a candidate version: `countries` = distinct sorted known countries of its active
  trips' emitted calls; `isPotentiallyInternational` = candidate and (foreign countries non-empty
  or declared international). `keep` = not potentially international, or ≥ 1 qualifying trip.
  Reason, first match: not potentially international → `domestic`; ≥ 1 qualifying →
  `regional_adjacent`; no foreign country → `international_metadata_without_foreign_geography`;
  foreign-only trips and no rejected trip → `foreign_only_trip`; exactly one distinct rejection
  reason → that reason; else `no_qualifying_cross_border_trip`. Counts recorded: retained domestic
  trips (non-candidates: all active trips), qualifying, rejected, foreign-only; maximum span and
  depth over qualifying trips; `integrated`. A declared-international version whose emitted calls
  are all Czech is therefore **dropped with all its domestic trips** (B13).
- **Where:** `classify` (171–222).
- **Why:** reason names.
- **Pinned by:** `Short neighboring route is retained even when mislabeled city`,
  `Foreign-only and missing-kilometre trips fail closed`, `001398 regional trip survives while its
  foreign-only continuation is pruned`, `Passing foreign stop does not make a domestic route
  international`.
- **Constants:** reason texts (published in diagnostics).
- **Diagnostic:** decision record (R281).
- **Proposed verdict:** port; Q12 on the declared-international-without-foreign-calls case.

#### JDF-R278 — Trip retention in kept routes
- **What:** a trip survives when its version is kept and (it is inactive, or its version is not a
  candidate, or its disposition is `DomesticTrip` or `QualifyingCrossBorderTrip`). Rejected and
  foreign-only trips of kept versions are dropped — including trips rejected only for
  `unknown_stop_country` on a version whose decision is `domestic` (a candidate only because of an
  unknown-country route stop).
- **Where:** `tripAllowed` (255–263).
- **Why:** test name ("regional trip survives while its foreign-only continuation is pruned").
- **Pinned by:** `001398 regional trip survives while its foreign-only continuation is pruned`.
- **Constants:** none.
- **Diagnostic:** none per trip → propose `jdf_international_trip_dropped` with the reason.
- **Proposed verdict:** port with the diagnostic; Q12 for the unknown-country case.

#### JDF-R279 — Pruning of dependents
- **What:** kept: versions with `keep`; their route stops, route info, `LinExt`, `Altlinky`,
  `JrutilRouteVersions`; surviving trips and their trip stops (**all** of a qualifying trip's
  calls, foreign ones included), notes, transfers, reservations, agency alternations; trip groups
  used by surviving trips; stops referenced by a kept route stop, with their posts, locations,
  location sources, post evidence; routing demands whose both endpoints (when present) are kept
  stops; agencies referenced by a kept version or a kept agency alternation. Everything else is
  removed. Under `keep-all` nothing is pruned, so stops of versions dropped by R248/R249 survive.
- **Where:** `applyInternationalRoutePolicyInternal` (263–325).
- **Why:** test names ("no dangling rows").
- **Pinned by:** `Rejected route dependents are pruned`, `Rejected route bundle has diagnostics
  manifest metadata and no dangling rows`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** replace: removing a trip or route is an edit; unreferenced places are not
  published by `jr-publish` regardless of policy (Q20).

#### JDF-R280 — `RegionalInternational` route type
- **What:** a kept version with ≥ 1 qualifying trip gets `routeType = RegionalInternational` (a
  value never parsed from JDF).
- **Where:** (291–298); `JdfModel.RouteType` comment.
- **Why:** comment ("Effective classification produced after per-trip international filtering").
- **Pinned by:** `001398 regional trip survives while its foreign-only continuation is pruned`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** replace by a separate model fact (`cross_border = regional`) next to the
  source route type, instead of overwriting it.

#### JDF-R281 — Decisions record (handed over)
- **What:** one `InternationalRouteDecision` per classified version (all versions under
  `regional-adjacent`): `routeId`, merged distinction, `keep`, `reason`, `countries`, maxima,
  `integrated`, counts. Logged as `International route policy {Policy}: retained {N} and dropped
  {M} cross-border route distinctions` (versions with any non-`CZ` country); written to the
  bundle manifest (`international_route_filter`) and to diagnostics
  (`filtered_international_route`, source object `jdf:route:{licence}:{merged distinction}`).
- **Where:** (213–222, 243–247, 349–357); `JdfBundleManifest.fs` (211–250).
- **Why:** provenance.
- **Pinned by:** `Rejected route bundle has diagnostics manifest metadata and no dangling rows`.
- **Constants:** texts.
- **Diagnostic:** as described.
- **Proposed verdict:** port as diagnostics `jdf_international_route_dropped` keyed by the version
  identity (not the merged distinction, section 9).

#### JDF-R282 — The filter in `fix-jdf`
- **What:** `fixBatch` runs the filter on each source batch and keeps only its decisions for a
  summary log; the written batch is **not** filtered.
- **Where:** `JdfFix.fixBatch` (45–47, 92–94); `JdfCommands.fixJdf` (103–118).
- **Why:** comment ("Route classification can change after batches with the same distinction are
  merged. Always geocode a non-degenerate source batch.").
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** log.
- **Proposed verdict:** drop (there is no fix stage).

---

## 4. Identity

### 4.1 Stop numbers (`jdf:stop:<N>`)

| Case | Number | Stable across builds? |
|---|---|---|
| Registry run, matches a registered alias (R229) | `resolvedId` (`merged_into` target or `id`), `1..999999999` | yes, while the spelling (or a chained alias, B4) matches |
| Registry run, no match or ambiguous (R230) | `1000000000 + ((LE-u64 of SHA-256(identity)) + k) mod 1000000000`, identity = `"{c1}\u001f{c2}[\u001f{c3}]\u001f{OKRES}\u001f{COUNTRY}"` with `c` the normalized components (R214) of the **first** spelling that created the stop | only if the same spelling creates it first and no collision ordering changes |
| No registry (R228) | counter `1, 2, …` in merge order | no |

Registry contract (what must stay stable): a registered number is never reused or renumbered;
aliases are added, never edited; `merged_into` redirects; reference coordinates split same
identities ≥ 75 m apart; the candidates CSV format (R232–R234) is what `registry.py promote`
consumes; `provisional_id` in that file is the key that groups spellings into one new number.
BASE_PLAN §6: "stop, post and rail-location IDs are stable across builds".

The *identity* that decides matching is three different things today (section 7, item 5):
JrUtil's normalized component key + compatible locality; `registry.py`'s casefolded raw key with
exact okres and country; BASE_PLAN's `(town, district, nearby place, okres, country)`.

### 4.2 Version identity

- Merged distinction (`Route.idDistinction` after merge): `count + 1` per batch (R239), `max + 1`
  for copies (R248). **Order-dependent.** It is embedded downstream in route-stop keys
  (`<route>/<distinction>/<route stop>`, `JdfBundle.fs` 483, README), in the
  `(line, distinction)` identity of zones, notices, transfers and diagnostics (README), and in the
  filter diagnostic object ID `jdf:route:{licence}:{distinction}` (R281).
- Published version identity: `(licence, originalValidFrom, originalValidTo, sourceCreationDate,
  detour)` (R240) — what the trip-version key uses.
- Trip-version key: R254, `<esc licence>:<yyMMdd>[:det][:<hash8>][:pN]`; trip ID
  `jdf:trip:<key>:<trip>`. BASE_PLAN §6: "trip IDs are stable for one timetable version".

### 4.3 Other numbers this area assigns (all order-dependent, none published as IDs by this area)

Attribute-ref IDs (R213), agency distinctions (R238; the GTFS agency ID may embed them — other
inventory), trip-group IDs (R242), reconciler alias IDs (internal), service IDs (R268, publish).

---

## 5. Output facts

What the merged batch carries beyond standard JDF 1.11:

| Fact | F# carrier | What the model must carry instead |
|---|---|---|
| Published version identity | `JrutilRouteVersions.txt` (R251) | per version: licence, source distinction, source batch, `published_valid_from/to`, `source_creation_date`, `detour`; and per trip: the version handle and the won `DateSet` |
| Stop locations and precision | `Polzast.txt` + `PolzastZdroj.txt` (fix-jdf extensions, merged by R231) | per spelling occurrence: location, precision (`precise`/`estimated`), source; the coordinates pass chooses |
| Post evidence | `JrutilPostCandidateEvidence.txt` (R236) | evidence attached to the merged place (post-estimator facts) |
| Routing demands | `JrutilRoutingDemands.txt` (R252) | nothing; derived by the post estimator |
| Merged header | `VerzeJDF` (R250) | `reference_date`, `gvd_year` in the build spec and manifest |
| Registry candidates | `--stop-registry-candidates` CSV (R232–R234) | per place: `registered` vs `provisional`, provisional reason (`new`/`ambiguous`), all source spellings with their localities and match kind, the registered number a spelling joined |
| Stop provisional flag, okres | `source_stop_metadata.parquet` (`okres`, `stop_id_provisional`; README) | the same, as place fields |
| International decisions | filter result → manifest, diagnostics (R281) | diagnostics keyed by version identity; `cross_border` fact on kept versions (R280) |
| Effective route type | `RegionalInternational` (R280) | a separate fact; the source route type stays |
| Trips that never run | `tripsToDelete` (R267) | nothing (empty `DateSet` → trip dropped with diagnostic) |

The merged batch also loses information the model should keep: which batch each spelling, agency
row, post row and version came from (needed for order-free tie-breaks and for `explain`), the
match kind of each spelling, and the published validity of cut chunks (kept only in
`JrutilRouteVersions`).

---

## 6. Known or probable bugs

- **B1 — Ambiguous stops multiply across batches** (R224). A spelling that is ambiguous between two
  registered same-name stops and has no precise location becomes provisional stop P; the same
  spelling in a later batch now has candidates {A, B, P} (or {B, P} when its precise location
  rejects A) → still ambiguous, and R223 does not apply because P has no registry reference → a
  second new stop, provisional number `hash + 1` (R230's probe, since the identity is the same).
  Without a registry the same happens with the counter. Not pinned (every registry test merges a
  single batch). Evidence: `beginAdd` 754–784, `nearestRegisteredCandidate` 585–586.
- **B2 — A location-less alias bypasses every distance gate** (R220, R223). Exact/suffix
  candidates are accepted at any distance when either side lacks a precise location, and the per-stop
  best alias picks the one with `distance ?? 0.0`. Once any spelling of a registered stop joins it
  without a precise location, a far-away incoming same-name stop matches that alias. So "split by
  reference coordinates" holds only until such an alias exists, and depends on batch order.
- **B3 — Asymmetric suffix retrieval** (R219): `[X; Y]` finds an earlier `[Y]`, but `[Y]` does not
  find an earlier `[X; Y]` unless both are precise and close. Batch order decides whether
  `Dětkovice` and `Ludmírov,Dětkovice` merge when one has no precise location.
- **B4 — Chaining and first-spelling hashing** (R225, R230): suffix and fuzzy matches against
  earlier batches' spellings make cluster membership order-dependent, and the provisional number is
  hashed from whichever spelling created the stop, so a different batch order can give a different
  provisional number for the same place. The comment's claim "the same whatever order batches are
  merged in" holds only for exact identities.
- **B5 — Two versions of one licence in one batch share a merged distinction** (R239): `Array.map`
  computes `count + 1` for all rows before adding; both versions, and all their trips, notes and
  trip stops, end up under one key (trip numbers may collide), and `deleteRoute` removes both.
  Latent: no batch among the first 400 sampled has a repeated licence **(data)**.
- **B6 — The per-date winner relation can be cyclic** (R247). Example on one date: D (detour,
  published `f1..X`), R (regular, `f2..X`, `f2 > f1`), R' (regular, `f3..Y`, `f3 > f2`, `Y ≠ X`),
  equal batch dates: R beats D (D has no detour priority over R, R starts later), D beats R' (D has
  priority, ends differ), R' beats R (starts later). The fold's result depends on unit order (=
  batch order), contrary to the comment.
- **B7 — Side-by-side grouping is first-fit** (R244). Versions `X` (mask `0x1F`), `6` (`0xA0`),
  `+` (`0xC0`) with one validity: in order X, 6, + the unit is {X, 6} and `+` is a separate unit
  that loses the tie-break (higher distinction) and is deleted; in order +, X, 6 the unit is {+, X}
  and `6` is deleted. Which weekend timetable survives depends on batch order.
- **B8 — Trip-version keys are not stable across builds** (R254). (a) The hash of a second
  same-start version includes `sourceCreationDate`, which is the export date in every batch
  **(data)**, so those trips get new IDs every day. (b) Side-by-side unit members share
  `(origFrom, origTo, created)` and are keyed `:p1`/`:p2` by merged distinction, i.e. by batch
  order; the README says `:pN` marks date chunks of one version.
- **B9 — Undated notes cover one day** (R255, R259). `dateTo` defaults to `dateFrom`, which
  defaults to the version's *current* start: types 5/6 (odd/even weeks, normally undated) restrict
  only the first day of validity; any undated typed note moves with cutting and clamping. Latent in
  the sample (no 5–8 rows **(data)**).
- **B10 — Easter-derived holidays crash** for Easter on 2 April or 30 March (R266), first in
  2051/2059.
- **B11 — Batch-date priority is inert** (R246): all batches of one export share the creation date
  **(data)**, so "a newer batch wins" never fires and identical-validity conflicts are settled by
  batch file-name order (merged distinction).
- **B12 — Registry identities are validated in the wrong terms** (R207). `registry.py validate`
  checks the 75 m separation per casefolded raw key with exact okres/country; JrUtil matches on
  synonym-normalized components with locality compatible when absent. Two rows with different ids
  whose spellings differ only by abbreviation (`nám.` / `náměstí`), or one with blank okres, are
  the same identity to JrUtil and are never checked; without references they are permanently
  ambiguous.
- **B13 — The international filter drops domestic service in two cases** (R277, R278): a
  version declared `M`/`N`/`P` whose emitted calls are all Czech is dropped entirely
  (`international_metadata_without_foreign_geography`); on a version kept as `domestic`, trips
  through a stop of unknown country are dropped (`unknown_stop_country`).
- **B14 — Registry candidates and stops of dropped versions** (R232, R279): candidates are taken
  from every merged spelling, including stops used only by versions later superseded, expired or
  next-GVD; those stops stay in the merged `Zastavky` and are pruned only by the
  `regional-adjacent` filter.
- **B15 — Post rows deduplicated by number only** (R237): two batches' `Oznacniky` rows with the
  same post number at one merged stop are one post, first batch's names win, silently.
- **B16 — Alias suggestions are the three smallest ids**, not the three most similar (R234).
- **B17 — Stop attribute truncation keeps the six lowest merged attribute IDs** (R227), which
  depend on first-seen order of `Pevnykod` values.
- **B18 — Wall-clock header date** when `boundValidity` is not called (R250) — tests only.
- **B19 — Day codes outside `1..7`** (R243, R263, R264): the mask treats such a trip as running
  every day, the calendar as never running.
- **B20 — Two distance functions in one decision** (R220 EPSG:3035 vs R223 equirectangular), and
  R223 has no distance cap.
- **B21 — One conflicting post observation aborts the national merge** (R236).
- **B22 — Unknown attribute IDs abort** (R213; also `Jdf.parseAttributes` uses `Array.find` in the
  calendar).
- **B23 — Retired registry rows match** (R209); probably intended, undocumented.

---

## 7. Documentation vs. code

1. **Quarantine.** JrUtil README: "without a precise location such a stop is quarantined";
   BASE_PLAN §6/§8 and PROGRESS §2: "ambiguity is quarantined, not merged". Code: each ambiguous
   occurrence becomes a provisional stop, and repeats create further stops (B1); a precise location
   also leads to quarantine whenever one candidate is not a registered stop with a reference (R223).
2. **Order independence of version resolution.** `resolveLicenceOverlaps` comment: "Deciding every
   date against the published input ranges keeps the result independent of the order of
   comparisons". Not when `beats` is cyclic (B6), not for unit grouping (B7), and the final tie-break
   is "the first added" (batch order).
3. **"A newer batch" wins.** Same comment; inert in production (B11).
4. **Registry order independence.** README: "A stop that matches a registered identity takes its
   number whatever the batch order"; PROGRESS §2 accept criterion "identical IDs across batch
   orders". True for exact identities; chaining and location-less aliases break it (B2, B4).
5. **What a stop identity is.** BASE_PLAN §8: "registered stop by `(town, district, nearby place,
   okres, country)`". Code: normalized components with synonyms; okres and country only as
   compatibility when both present; `registry.py` uses yet another key (B12).
6. **75 m rule in the registry README** ("Such stops must be at least 75 m apart") is checked by
   `registry.py` on raw spellings only (B12).
7. **Retirement.** Registry README: "A stop that is no longer served gets `status=retired`".
   JrUtil ignores the status (R209).
8. **Trip-key chunks.** README: "When the merger splits one version into date chunks, the later
   chunks append `:p2`, `:p3`". Side-by-side versions also get `:pN` (B8b). README: hash "from their
   published validity and batch date" — the batch date is the export date (B8a), so the key is not
   "the same between runs" for such versions.
9. **Provisional numbers.** README: "hashed from its normalized name, okres and country, so it is
   stable before review too" — hashed from the first spelling seen (B4).
10. **BASE_PLAN §8 report.** Lists "Large coordinate shifts against the registry" and "A sudden
    increase in provisional stops should block publication"; JrUtil computes neither (R231 compares
    batches with each other at 1000 m and only logs; no gate found in this area).
11. **JDF_SEMANTICS.** "`X`, `+`, `1`-`7` and typed `Caskody` calendar rules — Compiled into GTFS
    calendars; Calendar meaning is retained". Types 5/6 without dates are not (B9).
12. **Service IDs (probable).** README: "Deduplicated GTFS service patterns use the derived
    `gtfs:service:<weekday-bitmap>:<ordinal>` namespace". The JDF path names them
    `jdf:service:{bitmap|exc}:{ordinal}` (R268); `gtfs:service` is the generic
    `Gtfs.deduplicateCalendar`. Whether the serving writer renames them is the serving inventory's.
13. **Merge-order stop numbers.** README (registry section) describes the counter as the
    no-registry behaviour; Oběhy's `BuildConfig` comment says the same. Consistent, noted only
    because the rewrite drops it (R228).

---

## 8. Duplicated logic

Within this area and its neighbours:

1. **Distance** (keep one, `jr_model::geo`):

   | Copy | Formula | Used for |
   |---|---|---|
   | `StopReconciler` distance, `JdfMerger.locationDistance` | EPSG:3035 projected Euclidean | 75 m gate, 1000 m warning, grid cells |
   | `StopRegistry.distanceMetres` | equirectangular, `111320` m/degree | R223, `est:` posts (25 m) |
   | `registry.py coordinate_distance_metres` | same equirectangular | registry validation, promote |
   | CZPTT `Geo.haversineMetres` (czptt.md §8 item 17) | haversine | rail coordinates |

2. **Name normalisation**: `JdfStopReconciliation.tokenize` (synonyms per token, `n.l.`
   expansion); `StopMatcher` Lucene analyzer (fix-jdf; same synonym file, Solr parser with
   expansion); `RegionalOverlay/Names.fs` (same file, own parser plus overlay expansions);
   `registry.py normalize_component` (whitespace + casefold only); CZPTT `normalizedName`,
   `railwayNameCore`, `prepareName` (czptt.md §8 item 9). One normaliser in `jr-geo` with the
   synonym file as data.
3. **Stop identity keys**: `stopIdentityKey`, `rawAliasKey`, `spellingKey`, `localityKey`,
   provisional identity (R230), `registry.py stop_key`. One identity definition, used by the
   compiler and by the registry validator.
4. **Similarity**: `levenshteinSimilarity` here (and reused for suggestions with another
   threshold), the Lucene scorer in fix-jdf, CZPTT fuzzy scores. One similarity function.
5. **Day attributes interpreted three times**: `tripDateBitmap` (exact ladder), `dayClassMask`
   (conservative mask), GTFS weekday flags (`servicedDays`); they disagree on codes outside `1..7`
   (B19) and on notes. With `DateSet`s only the ladder remains; masks are a publish concern.
6. **GVD bounds**: `DateUtils.gvdBounds`/`gvdSecondSunday` and `gvd.py second_sunday`.
7. **Calendar construction**: against czptt.md §8 item 6 — CZPTT bitmaps and JDF ladders must both
   build `DateSet`s through one constructor over the GVD horizon; version resolution (R247) and
   CZPTT cancellation (CZPTT-R013) are both set algebra.
8. **"Is this a passenger call"**: `JdfInternationalFilter.callIsEmitted` and the GTFS stop-time
   emission in `JdfToGtfs` (probable; the latter is the GTFS inventory's) — one predicate.
9. **First-wins vs ranked attributes across sources**: JDF canonical spelling uses a total ranking
   (R226, order-independent), while JDF agencies (R238), posts (R237), locations at equal precision
   (R231) and CZPTT stop names (czptt.md B28) use first-wins. One "choose among source values"
   rule shape with an explicit order.
10. **Per-version relation copy/delete/remap**: twelve near-identical blocks in `copyRoute`,
    `deleteRoute` and `beginAdd`; disappears with handles.
11. **Registry CSV**: F# `StopRegistry.readTable`/`writeCsv` and `registry.py read_table`/
    `write_table`, with different validation (B12) and different `merged_into` parsing (R208).
12. **Interval resolution**: AGENTS rule 5 asks for one interval resolver; R247 is the road
    resolver, CZPTT-R071 the rail endpoint resolver — different concepts, keep both, name them
    distinctly.

---

## 9. What depends on order or on spools today

Batch order = ordinal order of `drahy-*.zip` then `vld-*.zip` (R203).

| Depends on batch order | Rules | Redesign |
|---|---|---|
| Stop numbers without a registry | R228 | dropped |
| Cluster membership through suffix/fuzzy chaining against earlier spellings | R219, R225 | collect all spellings first; cluster deterministically (registry rows first, then exact identities, then suffix/fuzzy edges between groups, sorted) |
| Asymmetric one-component suffix retrieval | R217, R219 | symmetric rule |
| Repeated ambiguous stops; provisional probing | R224, R230 | one quarantine stop per ambiguous identity; probe in sorted identity order |
| Provisional hash input (first spelling) | R230 | hash the cluster's canonical identity (or the sorted minimum identity) |
| Location at equal precision; source of the location | R231 | chosen by the coordinates pass with an explicit order |
| `regionId`/`country` fill-in of the merged stop | R226 | choose by rule over all spellings |
| Attribute-ref numbering; stop attribute truncation | R213, R227 | typed attributes, no numbers |
| Agency distinctions | R238 | operator by IČO, variants chosen by rule |
| Merged version distinctions, and everything keyed by them downstream (route-stop keys `<route>/<distinction>/<rs>`, zone/notice/transfer/diagnostic identities, `jdf:route:{licence}:{distinction}`) | R239, R248, R281 | key by published version identity |
| Unit grouping (first-fit) | R244 | per-date resolution over actual running dates |
| Fold order of a cyclic winner relation; last tie-break "first added" | R247 | a total order with a content-derived last key |
| `:pN` among side-by-side versions | R254 | content-derived discriminator |
| Stop post and post-evidence first-wins | R236, R237 | explicit rule |
| Registry candidates: first spelling per key, first number per alias spelling | R232, R233 | all spellings, sorted |
| Trip-group numbering | R242 | facts, no numbers |

Dictionary enumeration order (not batch order, but just as arbitrary): every relation's output
order (R253), including the trip-stop spool's route dictionary; the route list per licence after
the `Remove`/`Add` of R248/R249. Replace with sorted output (AGENTS rule 7).

The spool (R206): trip stops are written as JDF text per batch and re-keyed on output by a textual
search-and-replace of the distinction suffix `","{old}";\r\n`; a copied version shares the source's
chunks and is rewritten on the fly. Nothing of this survives: trips own their calls, cutting masks
dates (R248), and there is no intermediate text.

Order-insensitive today (keep it so): canonical spelling choice (R226), registered numbers for
exact identities (R229), per-trip calendars (R255–R268, parallel chunks with a stable result),
the international filter (R269–R281, parallel per route, results indexed by route), post evidence
sorting (R236).

---

## 10. Open questions for the architect

- **Q1 — Stop clustering model.** Replace incremental alias accumulation (R225) with a two-phase,
  order-free clustering over all batches' spellings? Should suffix/fuzzy matches be transitive
  (A~B, B~C ⇒ A~C), or may a spelling join only a registry identity or an exact-identity group?
- **Q2 — Quarantine shape.** One quarantined stop per ambiguous identity (all occurrences share it,
  one provisional number) instead of one per occurrence (B1)? Or keep the occurrences on the
  registered stops' boarding points as "unknown which"? AGENTS says "quarantined with a
  diagnostic"; what does a quarantined stop publish as?
- **Q3 — Distance gates.** Should a spelling without a precise location still match a registered
  identity at any distance (B2)? Should registry references be authoritative (a precise location
  beyond 75 m of every reference never joins it)? Cap R223 at 75 m?
- **Q4 — Retired registry rows.** Keep matching them (number survives a comeback) with a
  diagnostic, or exclude them?
- **Q5 — One-component suffix matches.** Allow in both directions, or in neither (B3)?
- **Q6 — Synonym file errors.** `n.`/`nad` → `nisou`, `l.` → `lázně`, `h.` → `horách`, `der` →
  `dolní`, `vel.`/`velké` → `velká`. Fix now (stop merges and provisional numbers move; registry
  exact identities may become ambiguous) or keep for parity and fix later with a registry review?
- **Q7 — Batch-date priority.** Drop it (inert, B11) or keep it for future sources with real batch
  dates? What replaces "first added" as the last tie-break: source batch name (deterministic but
  arbitrary), a content hash, or a diagnostic plus quarantine of the licence?
- **Q8 — Side-by-side versions.** Replace units (R243, R244) by per-date resolution among versions
  that run on that date? This changes behaviour when a newer all-days version and an older
  weekend version overlap (today the newer deletes the older on every date, weekends included).
- **Q9 — Chunks.** With `DateSet` masking a cut version is one set of trips with fewer dates. Keep
  publishing later chunks as separate trips with `:pN` IDs (requires splitting a trip's `DateSet`
  into contiguous runs at publish time), or publish one trip per source version (trip IDs of `:pN`
  chunks change once)?
- **Q10 — Trip-version key.** Remove `created` from the hash (B8a; IDs of hashed versions change
  once), and what discriminates side-by-side versions (B8b)?
- **Q11 — JDF 1.11 calendar semantics to confirm against the specification:** undated types 5/6
  (whole validity?), type 1 with only `dateFrom`, whether `1`–`7` trips run on holidays, whether
  any typed note may legitimately lack dates. Who checks the spec text?
- **Q12 — International filter edge cases:** drop declared-international versions with only Czech
  calls (B13)? Drop trips through unknown-country stops on otherwise domestic versions? Normalise
  countries to ISO alpha-2 (`AT`, `DE`) in the model and keep `A`/`D` only as reader aliases?
- **Q13 — Past dates.** Validity before the reference date inside the GVD is kept today. Keep
  (horizon = whole GVD), or mask dates before the reference date?
- **Q14 — Multiple versions of one licence in a batch** (B5): treat as distinct versions keyed by
  source distinction (proposed), or reject the batch?
- **Q15 — Agencies.** Is the operator identity IČO alone? Does serving need the per-variant agency
  rows (R238), or one row per IČO with a chosen variant?
- **Q16 — Post-evidence conflicts** (B21): diagnostic and keep-first-by-key, or fatal as today?
- **Q17 — Registry format.** Keep `merged_into:` in the free-text `note` (R208) or add a column?
  Should the compiler enforce `registry.py validate`'s rules in its own identity terms (B12) and
  fail the build on a violation?
- **Q18 — Holidays as data.** Holiday table in `data/calendar/` (law changes, e.g. Good Friday
  since 2016) or in code?
- **Q19 — Re-merging merged input** (R240's `JrutilRouteVersions` input path) and `MergeStopsById`
  (R205): drop both?
- **Q20 — Unreferenced places and their candidates.** Prune stops used by no published trip
  regardless of the filter policy, and list registry candidates only for published stops (B14)?

---

## 11. Rules added by the rewrite

#### JDF-R283 — Attribute numbers without a `Pevnykod` row
- **What:** an attribute slot (`Zastavky`, `Zaslinky`, `Spoje`, `Zasspoje`, `Altdop`) whose number
  has no `Pevnykod.txt` row is ignored: it contributes no fixed code (in particular no day
  attribute to the calendar, R261–R264).
- **Why:** F# resolved attribute numbers with `Array.find` and aborted the batch (and with it the
  run) on a dangling number (B22). A batch is not rejected for one bad reference.
- **Constants:** none.
- **Diagnostic:** `jdf_attribute_dangling` (warning), once per batch and number, with the count
  of slots.

#### JDF-R284 — Stops and agencies no surviving version uses
- **What:** after version resolution and bounding (R247–R249), and again after the international
  filter (R279), merged stops referenced by no surviving version's route stops or calls, and
  agencies referenced by no surviving version or agency alternation, are dropped; references are
  renumbered. Registry candidates (R232, R233) are offered only for stops that remain.
- **Why:** verdict Q20: stops no published trip uses are neither published nor offered for review
  (F# kept them unless the `regional-adjacent` filter ran, B14).
- **Constants:** none.
- **Diagnostic:** `jdf_unused_dropped` (info), one per pruning with the counts.

#### JDF-R285 — Rows with references the batch cannot resolve
- **What:** a row of a batch relation that names a version (licence and distinction), a trip, a
  stop or an agency the batch does not have, or a second version or trip with a key already taken
  in the batch, is dropped.
- **Why:** F# looked such references up with `.[key]` / `Map.find` and aborted the whole merge
  (B22-like); the verdicts make unknown references a diagnostic that drops the row.
- **Constants:** none.
- **Diagnostic:** `jdf_reference_unknown` (warning), once per batch, relation and missing kind,
  with the count.
