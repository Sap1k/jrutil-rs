# CZPTT rule inventory (F# reference)

Status: **proposal for review**. Nothing here is implemented yet. The inventory describes what the
F# JrUtil (`E:/Git/obehy/jrutil`) and the Python message flattener do today; the verdicts are
proposals for the architect.

Sources read in full: `jrutil/src/CzPtt/*.fs`, `jrutil.tests/CzPttToGtfsTests.fs`, the parts of
`Gtfs/Gtfs.fs`, `DateUtils.fs`, `UnionCodec.fs`, `Utils.fs`, `Core/Geo.fs` that CZPTT calls,
`ext-services-cs/CzPttXml/CzPttXml.cs`, `repo/NATIONAL_CZPTT.md`,
`repo/src/obehy/national_czptt.py` (`flatten_messages`, `_message`, `_local_name`, plus the
post-build verifiers that parse CZPTT IDs), and the serving finalizer code that parses CZPTT IDs
back (`Serving/PackageGtfsRelations.fs`, `PackageKeyRelations.fs`, `PackageRows.fs`,
`PackageSemanticRelations.fs`).

Conventions used below:

- "PA" = the `PlannedTransportIdentifiers` with `ObjectType=PA`; "TR" likewise.
- `esc(x)` = .NET `Uri.EscapeDataString(x)` (RFC 3986: keeps `A–Z a–z 0–9 - . _ ~`, everything
  else UTF-8 percent-encoded with upper-case hex).
- "source index" = 0-based position of a `CZPTTLocation` in the message; "sequence" = index + 1.
- "calls" without qualification = the normalized calls of the whole PA (all locations, in source
  order), as opposed to "selected calls" (GTFS candidates) and "journey calls".
- Proposed policy keys live in `policy/czptt.toml` (prefix `czptt.`); proposed data files in
  `data/czptt/` or `data/names/`.
- Test names are the F# `CzPttToGtfsTests` method names unless another class is named.

---

## Architect's verdicts

**After the switch (2026-11-24):** the limitations and post-switch changes recorded below are
decided in `docs/roadmap.md`, which takes precedence where the two differ.

**These override the "Proposed verdict" lines below.** Where a rule is not listed here, its
proposed verdict stands. Rule IDs in the Rust code are the IDs of this file; new rules continue
from `CZPTT-R116`. Answers that came from data were checked on the golden subset (2,916
timetable and 6,691 cancellation messages, 67,521 locations).

### What the data says

- `Timing/Time` is always `HH:MM:SS.fffffff+hh:mm`; `StartDateTime`, `CZPTTCreation` and
  `CZPTTCancelation` are always naive. Day offsets are `0` or `1` (no negative offset in the
  subset).
- The occurrence fields of `CZCentralPTTNote`, `CZNonCentralPTTNote` and `CZIPTS` are always
  blank. Endpoint codes are country-prefixed (`CZ53179`, `AT01003`). 512 of 17,952 note
  endpoints name a point that is not in the PA; one names a point that occurs twice.
- `LocationSubsidiaryTypeCode` is always `1`; no subsidiary is coded `BUS`; `LocationSubsidiaryName`
  never occurs.
- Activities seen: `0001 0002 0027 0028 0029 0030 0031 0032 0033 0040 CZ02 CZ03 CZ04 CZ13`.
  `0001` with `CZ13`: 3 calls. `0030` without `0001`: 4 calls.
- Root parameters F# never read: `CZReroute`, `CZOriginalCalendarBitmap`,
  `CZOriginalCalendarStartDate`, `CZOriginalCalendarEndDate`, `CZTrainName` (330 messages).
  The reader keeps every parameter; nothing uses these yet.
- `SR70.csv` is headerless `code6,name,lat,lon`.

### Open questions

| Q | Decision |
|---|---|
| Q1 | Keep `Pa:…` / `Tr:…`. They are published keys and part of every trip ID. The model holds the identifier as fields and renders this form in one place. |
| Q2 | The calendar is the calendar of day offset 0. A PA with a negative time is moved as a whole: times forward by whole days, its dates back by the same days (fixes B3). The horizon is widened by a day at the start for this. |
| Q3 | One interval resolver (R071): a code with a two-letter prefix matches country and code, a bare code matches the code; a blank occurrence means the first match in the path, for a start and an end alike, and only when that first match of the end lies before the start, the first match at or after the start; an explicit occurrence `n` counts the matches from zero, as F# read it (`1` is the second visit). The first verdict read it 1-based on the belief that no real message has one; the national feed has 21, on trains that reverse at their terminus, and only the zero-based reading lets their notes reach the last stop. |
| Q4 | The offset in `Time` is read and ignored (local wall time). Naive timestamps are Europe/Prague. |
| Q5 | Four columns, no header. A row with an unusable coordinate poisons its code (port R019). |
| Q6 | Keep the `public_transport` branch (port R022); the contract text is corrected at the switch. |
| Q7 | Port: only `0001` decides passenger service. `CZ13` with `0001` stays a passenger call and gets a diagnostic (`czptt_unpublished_stop`). |
| Q8 | Port R044 as is. |
| Q9 | Port: one catalog version per PA, at its first validity date. |
| Q10 | Unused fields stay in the note's raw text only. |
| Q11 | Port R096 as a named rule: ranked ambiguity is accepted and diagnosed. |
| Q12 | Port the 300 m northward placeholder. |
| Q13 | Notes of rejected PAs are not published (fixes B10). |
| Q14 | R083 is a pass (`nad_connections`), not part of the compiler. Transfers are trip-to-trip facts in the model and in the serving `transfer` relation; the stop-pair form MOTIS needs is a GTFS projection option. |
| Q15 | There is no mode. Trips carry their railway points as the serving contract defines (serving §3.3); GTFS publishes passenger calls only. `gtfs` mode and R102 go. |
| Q16 | The compiler reads the source snapshot. `derived/messages.zip` goes (drop R008, R009). |
| Q17 | Diagnostics only. |
| Q18 | An unresolved passenger point is published without a position (`missing`), never at 0,0, with a diagnostic, and makes the package ineligible for publication. |

### Rules decided differently from their proposal

| Rule | Verdict |
|---|---|
| R003, R007, R012 | One identity, the PA. Two different payloads (raw bytes) for one PA among timetable messages are an input error that stops the build, as today. |
| R015 | A cancellation reaching outside the message's validity applies its overlap and is diagnosed (fixes B8). A bitmap whose length does not fit its interval drops that message with a diagnostic. |
| R017 | The reader reports what the catalogue says; the defaults live in the rules that use them. |
| R025 | Candidates are ordered by numeric node ID. |
| R028 | Port as a compile rule: a call with one time gets it on both sides. Raw one-sided times are not carried. |
| R030 | Activity codes are text; an unknown code is a diagnostic. |
| R037 | Port the repair. The repaired values are the call's times; the diagnostic records the values before and after. |
| R042 | A whole run of subsidiary duplicates collapses (fixes B5). |
| R046 | A journey is one part; the `parts` list goes. |
| R047 | Port, with a diagnostic. Part numbers keep their gap, so trip IDs do not move. |
| R051 | Replace as proposed: one diagnostic per moved boundary inside the passenger window. |
| R053 | First in processing order wins, as today, with a diagnostic when source names disagree. |
| R055 | The NAD boarding point keeps `…:platform:BUS`. A real subsidiary coded `BUS` would share it; none exists, and one appearing is diagnosed. |
| R057 | Port. A train number taken from the TR core is diagnosed and gets a `czptt:train_number` key only when it is all digits. |
| R066 | Replace: move the dates with the times (Q2). |
| R076 | Notes only for accepted PAs. |
| R078 | `on_request` only on passenger calls (fixes B34). |
| R082 | An in-seat transfer only between parts that share their boundary call; otherwise none, with a diagnostic (fixes B11). |
| R084 | Replace (Q14). |
| R085 | Port with the one resolver. Zones still apply on every date of the trip (B17 stands and is recorded as a limitation). |
| R087 | Drop: `stops.zone_id` is not published. |
| R088, R105–R112 | Drop: the facts are model fields; there are no sidecars and no `cz_*` tables. |
| R094–R101 | Port as the `rail_coordinates` pass. It reads the timetable's runs, so railway points inside a replacement-bus part are not evidence. |
| R103 | The model carries the precision; ` [?]` is appended when publishing. |
| R104 | Replace (Q18). |
| R110, R111 | Drop: IDS coverage beyond zones is not computed. |
| R113 | Replace: service IDs are derived from the dates. |
| R114 | Drop. |
| R115 | Replace: coded diagnostics, summarised in `diagnostics.json`. |

### Known F# bugs, fixed here and expected in the parity ledger

B1, B3, B5, B8, B10, B11, B14, B19 (moot: no sidecar), B20, B21, B32, B34. Left as they are
for now and recorded as limitations: B16, B17, B24, B27, B28, B29, B30.

---

## 1. Data flow

**Stage 0 — Message flattening (Python, `national_czptt.flatten_messages`).**
Input: the `sources/` snapshot records (annual ZIP `annual/JRYYYY.zip`, gzip change objects
`changes/YYYY-MM/*.xml.zip`). Output: `derived/messages.zip`, one XML message per entry named
`000000000.xml`, `000000001.xml`, … in application order, exact duplicates removed, conflicting
timetable identities rejected. The F# compiler relies entirely on this order for cancellation
semantics. Rules R001–R008.

**Stage 1 — Package entry (`CzPttPackage.write`).**
Input: options (catalog snapshot, operational-point mode, SR70 path, OSM PBF path, route-shapes
options), input path (file or directory), output path (must not exist). Creates private scratch
and staging directories next to the output, calls `CzPttBundle.convert` with `SpillBacked`
storage, then builds the package (Stage 9). The KADR catalog is loaded by the CLI
(`ConvertCommands.czpttToBundle` → `CzPttModel.loadCatalogSnapshot`, R017).

**Stage 2 — Parse (`CzPtt.parseAll`, `parseFromReader`).**
Input: path. Output: lazy sequence of `(name, CzpttMessage)` where `CzpttMessage` is
`Timetable of CzpttcisMessage | Cancellation of CzCanceledPttMessage`. Rules R009–R010.

**Stage 3 — Merge (`CzPttMerger.ProcessAll`, `Add`, `Cancel`).**
Input: the message sequence in order. Output: surviving timetable messages keyed by PA id (with
calendars reduced by cancellations), the set of all PA ids seen, unknown cancellation targets.
Rules R011–R016.

**Stage 4 — Prepare identities (`CzPttBundle.convert`, lines 141–148).**
Surviving messages sorted by PA id (ordinal); `cancelledPaIds` = seen − surviving. Rule R016.

**Stage 5 — Load coordinate sources (`CzPttCoordinates.loadSr70Coordinates`,
`loadOsmCandidates`).**
Output: SR70 index (coordinates by `("CZ", code)`, names by `("CZ", code)`, conflicting codes,
invalid codes) and OSM candidate nodes. Rules R018–R025.

**Stage 6 — Convert to a raw feed (`CzPttToGtfs.convert`).**
Input: catalog, options, SR70 point names, surviving messages. Per message, in PA-id order:
`normalize` (calls with carried RU/category/line) → `forceMonotonicFlaggedTimes` → reject if no
passenger call → reject on chronology → `projectPassengerServiceBoundaries` → `selectedCalls` →
reject if < 2 → `segments` → `journeys` → NAD passenger filter → `reconcileJourneyEdgeTimes` →
sidecar-mode diagnostic → accepted `(message, calls, selected, journeys)`. Then, over all
accepted messages: routes, trips, stop times, in-PA and cross-PA transfers, transfer rewrite,
calendar dates, agencies, `cz_routes`, notes (central, non-central, calendar), features (note and
request-stop), IDS trip-stop zones, `stops.zone_id`, operational calls, stops, feed info.
Output: `ConversionResult` (feed, operational calls, rejected journeys, accepted PA ids,
approximations, boundary adjustments, IDS diagnostics, notes, features). Rules R026–R088.

**Stage 7 — Coordinates (`CzPttBundle.convert`, lines 158–831).**
Input: raw result, SR70 index, OSM candidates, surviving messages. Builds stop→point maps, name
evidence and OSM indexes; SR70 authority; OSM selection with corridor veto and ambiguity
ranking; two re-evaluation passes; route estimates for passenger points; coordinate diagnostics;
drops unresolved timing-only stops; writes lat/lon and the ` [?]` suffix. Output: final
`ConversionResult` with `mergeDiagnostics`, `cancelledPaIds`, `coordinateDiagnostics`.
Rules R089–R104.

**Stage 8 — Sidecars (`CzPttBundle.convert`, lines 832–1152).**
Writes seven Parquet tables into the scratch directory. Rules R105–R112.

**Stage 9 — Package (`CzPttPackage.write`, lines 58–105; `Gtfs.deduplicateCalendarWithPrefix`,
`Gtfs.fillStandardRequiredFields`, `Gtfs.feedTables`; `PackageWriter.writePackage`).**
Calendar deduplication renames service IDs, required GTFS fields are filled, standard and Czech
tables are handed to the package writer with the sidecars, a diagnostics JSON and a manifest.
The serving finalizer then parses several CZPTT IDs back (section 4). Rules R113–R115.

---

## 2. Source facts

### 2.1 CZPTT XML (bindings `CzPttXml.cs`, XmlSerializer)

Read (element paths as in the bindings; root elements have no namespace):

| Path | Used for |
|---|---|
| `CZPTTCISMessage/Identifiers/PlannedTransportIdentifiers/ObjectType` (`TR`/`PA`) | selects PA and TR identifiers |
| `…/PlannedTransportIdentifiers/Company`, `Core`, `Variant`, `TimetableYear` | PA/TR id strings (R011); TR `Company` = initial RU (R033); TR `Core` = train-number fallback (R057) |
| `CZPTTCISMessage/CZPTTCreation` (xs:dateTime) | `feed_info.feed_version` only (R081); Python ordering (R002) |
| `CZPTTCISMessage/CZPTTInformation/PlannedCalendar/BitmapDays` (`[10]+`) | service dates, cancellations, overlaps, service-day counts |
| `…/PlannedCalendar/ValidityPeriod/StartDateTime` | bitmap start date; catalog validity date (R056, R085) |
| `…/PlannedCalendar/ValidityPeriod/EndDateTime` (optional) | merger bitmap interval (R013), `feed_end_date` (R081); ignored elsewhere |
| `…/CZPTTInformation/CZPTTLocation/Location/CountryCodeISO` | point identity |
| `…/Location/LocationPrimaryCode` (`[0-9]{5}`) | point identity; note/IDS endpoint matching |
| `…/Location/PrimaryLocationName` | stop name fallback (R053) |
| `…/Location/LocationSubsidiaryIdentification` (presence) | child stop kind; collapse rule (R042) |
| `…/LocationSubsidiaryIdentification/LocationSubsidiaryCode` (text) | child stop id; `platform_code` fallback; sidecar `subsidiary_code` |
| `…/LocationSubsidiaryIdentification/LocationSubsidiaryName` | `platform_code`; sidecar `subsidiary_name` |
| `…/CZPTTLocation/TimingAtLocation/Timing[@TimingQualifierCode='ALA'\|'ALD']/Time` | local time (offset suffix dropped, R027) |
| `…/Timing/Offset` | day offset |
| `…/CZPTTLocation/ResponsibleRU` | operator (carried forward, R033) |
| `…/CZPTTLocation/CommercialTrafficType` | category (R034) |
| `…/CZPTTLocation/TrafficType` | category fallback (R034) |
| `…/CZPTTLocation/OperationalTrainNumber` | train number (R057) |
| `…/CZPTTLocation/TrainActivity/TrainActivityType` | activities (R029, R030, R067, R078, R083) |
| `…/CZPTTLocation/NetworkSpecificParameter/Name`, `Value` (repeated, zipped) | location parameters (R031) |
| `CZPTTCISMessage/NetworkSpecificParameter/Name`, `Value` | root parameters (R031) |
| `CZCanceledPTTMessage/PlannedTransportIdentifiers/*` | cancellation target PA (R013) |
| `CZCanceledPTTMessage/PlannedCalendar/BitmapDays`, `ValidityPeriod/*` | cancelled days (R013) |

Network-specific parameter names read (exact, ordinal):

| Name | Level | Use |
|---|---|---|
| `CZPassengerServiceNumber` | location and root | KADR line code (R032) |
| `CZAlternativeTransport` | location | NAD flag, value `"1"` (R036) |
| `CZInconsistentTime` | location | time repair, value `"1"` (R037). The root-level value is **ignored** |
| `CZCentralPTTNote` | root | central notes (R073) |
| `CZNonCentralPTTNote` | root | local notes (R074) |
| `CZCalendarPTTNote` | root | note calendars (R072, R075) |
| `CZIPTS` | root | IDS coverage/zones (R085, R110, R111) |
| `CZCalendarIPTS` | root | IDS coverage sidecar only (R110, R111); never interpreted |

Declared in the bindings but ignored by F#: `CZPTTLocation/ResponsibleIM`,
`CZPTTLocation/TrainType` (enum `0|1|2`), `TimingAtLocation/DwellTime`,
`LocationSubsidiaryIdentification/AllocationCompany`,
`LocationSubsidiaryCode/@LocationSubsidiaryTypeCode`, `CZCanceledPTTMessage/CZPTTCancelation`
(read by Python only), the UTC-offset part of `Timing/Time`. Location-level note parameters,
location-level `CZIPTS`, and every other parameter name are ignored. Not in the bindings at all
(silently skipped by XmlSerializer): `RelatedPlannedTransportIdentifiers` (read by Python, R003).

Parsed but semantically ignored activity codes: `0026`, `0027`, `0031`, `0032`, `0033`, `0035`,
`0040`, `0043`, `0044`, `CZ01`, `CZ02`, `CZ03`, `CZ04`, `CZ05`, `0002`, `CZ13`. Used: `0001`
(passenger), `0028` (embark only), `0029` (disembark only), `0030` (request stop).

Strictness inherited from XmlSerializer (a failure aborts the whole run, R010): unknown
`ObjectType`, `TimingQualifierCode` other than `ALA`/`ALD`, `TrainType` outside `0|1|2`,
malformed `xs:dateTime`.

### 2.2 KADR catalog snapshot (`catalog.json`, produced by Python `snapshot_kadr`)

`CzPttModel.loadCatalogSnapshot` reads JSON arrays (missing array → empty; entries without
`code` dropped; dates ISO `yyyy-MM-dd`, unparsable → absent):

| Array | Fields read | Defaults | Used? |
|---|---|---|---|
| `lines` | `code`, `mark`, `name`, `valid_from`, `valid_to` | `mark`→`code`, `name`→`""` | all used |
| `companies` | `code`, `name`, `url` | `name`→`"Unknown {code}"` | all used |
| `ids` | `code`, `abbreviation`, `name`, `note`, `valid_from`, `valid_to` | `abbreviation`→`code`, `name`→`""` | `name` **unused** |
| `train_types` | `code`, `abbreviation` | `abbreviation`→`code` | used |
| `commercial_train_types` | `code`, `abbreviation` | `abbreviation`→`code` | used |
| `central_notes` | `code`, `name`, `text`, `valid_from`, `valid_to` | `name`→`code` | only `name`; `text`, validity **unused** |

`schema_version` is not read.

### 2.3 SR70 CSV (`--sr70`)

No header row (every row is data). Columns by position: `[0]` code (first 5 characters used,
rows with a shorter `[0]` dropped), `[1]` name, `[len-2]` latitude, `[len-1]` longitude (only when
the row has ≥ 4 columns). Every other column is ignored. Read with `File.ReadAllText` (UTF-8,
BOM-detecting) and `FSharp.Data.CsvFile` (comma, quotes).

### 2.4 OSM PBF (`--osm-pbf`)

Only `Node` objects with id, latitude and longitude. Tags read: `railway`, `public_transport`,
`ref:EU:PLC`, `addr:country`, `is_in:country_code`, `name`, `official_name`, `alt_name`,
`short_name`, `loc_name`, `old_name`, `uic_name`, `name:cs`, `name:de`, `name:pl`, `name:sk`.
Ways, relations, every other tag (explicitly `uic_ref`, `railway:ref`) ignored.

---

## 3. Rules

### Stage 0 — message flattening (Python)

#### CZPTT-R001 — Message root and timestamp are mandatory
- **What:** Each payload is parsed as XML. Root local name must be `CZPTTCISMessage` or
  `CZCanceledPTTMessage`, else the build fails (`Unsupported CZPTT root …`). The direct child
  `CZPTTCreation` (CIS) or `CZPTTCancelation` (cancel), stripped, must be non-empty, else fail.
  Malformed XML fails.
- **Where:** `national_czptt.py` `_message` (619–661).
- **Why:** the timestamp drives ordering (R005).
- **Pinned by:** none in F#.
- **Constants:** root names; timestamp element names.
- **Diagnostic:** `PipelineError` (build failure).
- **Proposed verdict:** port (into the reader as an input error, R005 needs the field).

#### CZPTT-R002 — Event time normalisation
- **What:** timestamp parsed ISO-8601 with `Z`→`+00:00`; a naive value is interpreted in
  `Europe/Prague`; the result is converted to UTC for ordering. Invalid → build failure.
- **Where:** `_message` (634–642).
- **Why:** unknown (SŽ timestamps may lack an offset).
- **Pinned by:** none.
- **Constants:** `Europe/Prague` → `czptt.ordering.naive_timestamp_zone = "Europe/Prague"`.
- **Diagnostic:** `PipelineError`.
- **Proposed verdict:** port.

#### CZPTT-R003 — Flattening identity
- **What:** identity = `"|".join(sorted(ids))` over **every** element named
  `PlannedTransportIdentifiers` or `RelatedPlannedTransportIdentifiers` anywhere in the document
  that has all five children; each id = `ObjectType:Company:Core:Variant:TimetableYear` (text,
  stripped, e.g. `PA:54:000000001234:00:2026`). Empty identity → failure.
- **Where:** `_message` (643–660).
- **Why:** unknown; NATIONAL_CZPTT.md says the conflict check is on the PA identity.
- **Pinned by:** none.
- **Constants:** element names.
- **Diagnostic:** `PipelineError` when empty.
- **Proposed verdict:** replace: one identity concept (PA identity, R011) for both dedup/conflict
  and merge; see section 8.

#### CZPTT-R004 — Source enumeration order
- **What:** records sorted by `relative_path`; for `annual_zip`, members sorted by file name,
  only non-directory names ending `.xml` (case-folded), source path `"{rel}//{member}"`; other
  records are single gzip payloads (source path = `rel`).
- **Where:** `flatten_messages` (689–703).
- **Why:** determinism.
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** `Malformed CZPTT gzip: …` failure.
- **Proposed verdict:** port (reader enumerates; compiler orders).

#### CZPTT-R005 — Application order
- **What:** messages whose source path starts with `annual/` first, in R004 order; then all
  others sorted by `(event_time_utc, 0 if CZPTTCISMessage else 1, source_path)`.
- **Where:** `flatten_messages` (705–714).
- **Why:** NATIONAL_CZPTT.md: "a cancellation published later in the same month is applied after
  its timetable instead of accidentally resurrecting the timetable"; additions precede
  cancellations only on equal timestamps.
- **Pinned by:** none.
- **Constants:** `annual/` prefix; the tie order.
- **Diagnostic:** none.
- **Proposed verdict:** port (the Rust compiler absorbs this; it is the single most important
  ordering requirement for cancellation correctness).

#### CZPTT-R006 — Exact duplicate removal
- **What:** in application order, a payload whose SHA-256 of raw bytes was already seen is
  dropped (first kept). Applies to cancellations too.
- **Where:** `flatten_messages` (715–721).
- **Why:** NATIONAL_CZPTT.md "Exact duplicate payloads are removed."
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** none (silent).
- **Proposed verdict:** port (raw-byte hash).

#### CZPTT-R007 — Conflicting timetable identity fails (Python level)
- **What:** among kept `CZPTTCISMessage`s, two different payloads with the same R003 identity →
  build failure `Conflicting CZPTT timetable messages share identity …`.
- **Where:** `flatten_messages` (722–727).
- **Why:** NATIONAL_CZPTT.md.
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** `PipelineError`.
- **Proposed verdict:** replace (merge with R012 on one identity definition).

#### CZPTT-R008 — Deterministic flattened ZIP
- **What:** entries `{index:09d}.xml`, date 1980-01-01 00:00:00, Deflate level 9, external attr
  `0o100644 << 16`, create_system 3.
- **Where:** `flatten_messages` (730–747).
- **Why:** reproducible digests in the run manifest.
- **Pinned by:** none.
- **Constants:** as listed.
- **Diagnostic:** none.
- **Proposed verdict:** drop if the Rust compiler reads `sources/` directly (an intermediate ZIP
  is an artifact of the Python/F# split). Keep only if the run manifest's flattened-message
  digest must survive.

### Stage 2 — parse (F#)

#### CZPTT-R009 — Input discovery by file name
- **What:** path ending (case-insensitive) `.xml.zip` or `.xml.gz` → decompressed as **gzip**;
  extension `.xml` → one file; `.zip` → every entry with extension `.xml`, in ZIP entry order,
  named `"{path}//{entry}"`; a directory → `Directory.EnumerateFiles` then
  `EnumerateDirectories`, recursively, in **file-system order (unsorted)**; anything else ignored.
- **Where:** `CzPtt.parseAll` (84–107).
- **Why:** code comment: "GZip is not ZIP, but tell that to SŽ".
- **Pinned by:** `Dense passenger corridor wins independent of input order` (directory input;
  only shows output does not depend on order for that case).
- **Constants:** extensions.
- **Diagnostic:** none.
- **Proposed verdict:** replace: `jr-formats` takes the explicit record list from the caller
  (AGENTS: it never discovers inputs) and gzip/zip detection by magic, not by name. Directory
  order nondeterminism must not survive (see B9).

#### CZPTT-R010 — Message type dispatch
- **What:** deserialize as `CZPTTCISMessage`; on any exception, deserialize as
  `CZCanceledPTTMessage`; if both fail, throw (`Failed deserializing CZPTT XML message …`), which
  aborts the run (parsing is lazy and outside the merger's try block).
- **Where:** `CzPtt.parseFromReader` (58–68).
- **Why:** code comment: XmlSerializer cannot try both types on a non-seekable reader.
- **Pinned by:** indirectly every test.
- **Constants:** none.
- **Diagnostic:** exception.
- **Proposed verdict:** replace: dispatch on the root element name; a malformed message is a
  diagnostic, a broken file an error (AGENTS working conventions).

### Stage 3 — merge

#### CZPTT-R011 — PA identity string
- **What:** `identifierStr` = `sprintf "%O:%s:%s:%s:%s" ObjectType Company Core Variant
  TimetableYear`. `%O` prints the **C# enum member name**, so the result is `Pa:…`/`Tr:…`
  (e.g. `Pa:54:000000001234:00:2026`), not the XML text `PA`. The PA is the first identifier with
  `ObjectType=PA` (`Seq.find`/`Array.find`; absence throws).
- **Where:** `CzPtt.identifierStr` (134–136), `timetableIdentifier`; `CzPttMerger.getPaidStr`;
  `CzPttNormalize.paId`; `CzPttBundle.paId`.
- **Why:** unknown (artifact of `%O`).
- **Pinned by:** `ServingContractTests.CZPTT trip parts share boundary calls…` (fixture uses
  `Pa%3A1`), `Spill-backed CZPTT merger replays messages…`.
- **Constants:** format.
- **Diagnostic:** none.
- **Proposed verdict:** port the identity; the casing is an open question (it is published as the
  `czptt:pa` / `czptt:tr` source keys and embedded in trip IDs). See Q1.

#### CZPTT-R012 — Duplicate and conflicting PA
- **What:** on `Add`, every PA id is recorded in `SourcePaIds`. If the PA is already present:
  SHA-256 of the **re-serialized** XML (XmlSerializer output, UTF-8) equal → ignored (log
  `Ignoring byte-equivalent duplicate message`); different → `CzPttInvalidException
  "Conflicting timetable messages share PA identity {paid}"`, which `ProcessAll` re-raises (run
  aborts). Note a partially cancelled message has a rewritten calendar, so a later byte-identical
  copy of its original would now conflict.
- **Where:** `CzPttMerger.Add` (83–105).
- **Why:** NATIONAL_CZPTT.md (conflicting PA identities fail the build).
- **Pinned by:** none directly.
- **Constants:** none.
- **Diagnostic:** log / fatal exception.
- **Proposed verdict:** port the conflict rule; replace the fingerprint with the raw-byte hash
  from R006 (re-serialization is an XmlSerializer artifact).

#### CZPTT-R013 — Cancellation semantics
- **What:** target = PA of `CZCanceledPTTMessage/PlannedTransportIdentifiers`. If present:
  message bitmap over `[Start, End ?? Start]`; cancel bitmap over its own interval; cancel bitmap
  is extended to the message interval with `false` ("no change"), inverted, ANDed with the
  message bitmap. Empty result → message removed. Otherwise the message's `PlannedCalendar` is
  replaced with `[Start..End]` of the message interval and the new bitmap. A later `Add` of a
  removed PA re-adds it.
- **Where:** `CzPttMerger.Cancel` (107–139); `CzPtt.parseCalendar`, `serializeCalendar`;
  `DateUtils.DateBitmap` (`And`, `Not`, `ExtendTo`).
- **Why:** code comment: "false in cancelBitmap means 'no change'".
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** none on success.
- **Proposed verdict:** port, on `DateSet` (one calendar type).

#### CZPTT-R014 — Unknown cancellation targets
- **What:** a cancellation whose PA is not present increments a per-PA counter. After all
  messages: warning log `{Count} cancellations targeted {DistinctCount} unknown PA identities`;
  `mergeDiagnostics` = `"unknown cancellation target {pa} ({count} messages)"`, sorted.
- **Where:** `CzPttMerger.Cancel`, `ProcessAll`; `CzPttBundle.convert` (823–828).
- **Why:** NATIONAL_CZPTT.md lists unknown cancellation targets in diagnostics.
- **Pinned by:** none.
- **Constants:** message text.
- **Diagnostic:** `diagnostics.json` → `merge_diagnostics`.
- **Proposed verdict:** port (one diagnostic code, PA as field).

#### CZPTT-R015 — Per-message failures are swallowed
- **What:** in `ProcessAll`, any exception other than `CzPttInvalidException` is logged (`Error
  while merging {file}`) and the message is skipped. This covers: a timetable without a PA
  identifier; a calendar whose `BitmapDays` length ≠ interval length (`DateBitmap` ctor throws);
  a cancellation whose interval is not inside the message interval (`ExtendTo`'s `assert` is
  compiled out in Release, so the index goes out of range and throws).
- **Where:** `CzPttMerger.ProcessAll` (146–159).
- **Why:** unknown.
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** log only (not in `diagnostics.json`).
- **Proposed verdict:** replace: each case becomes a named diagnostic with the entity
  quarantined; a cancellation partly outside the validity should apply its overlap (see B8).

#### CZPTT-R016 — Cancelled PA ids
- **What:** `cancelledPaIds` = (all PA ids ever added) − (PA ids of surviving messages), sorted.
  Surviving messages are sorted by PA id (ordinal) before conversion.
- **Where:** `CzPttBundle.convert` (142–147).
- **Why:** diagnostics.
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** `diagnostics.json` → `cancelled_pa_ids`.
- **Proposed verdict:** port.

Storage policy (`MemoryBacked`/`SpillBacked`, spill file `.{name}.czptt.{guid}.tmp`) is pinned by
`Spill-backed CZPTT merger replays messages and removes its spool` and `Memory and spill-backed
CZPTT sidecars are byte-identical`. **Proposed verdict:** drop (AGENTS rule 8: no spill files).

### Stage 5 — catalog and coordinate sources

#### CZPTT-R017 — Catalog snapshot defaults
- **What:** see 2.2. Entries without `code` are dropped; a missing/unparsable `valid_from` or
  `valid_to` becomes "unbounded" on that side; duplicate codes are kept (later lookups take the
  first, R034, or the latest valid, R056/R085).
- **Where:** `CzPttModel.loadCatalogSnapshot`, `tryString`, `tryDate` (232–318).
- **Why:** "old snapshots remain readable" (test name).
- **Pinned by:** `Catalog note list is additive and old snapshots remain readable`.
- **Constants:** default `"Unknown {code}"`; `""`.
- **Diagnostic:** none.
- **Proposed verdict:** replace: the reader reports facts (absent stays absent); defaults move to
  the rules that use them; a malformed date is a diagnostic, not an open bound.

#### CZPTT-R018 — SR70 row parsing
- **What:** per row: `[0]` shorter than 5 chars → row dropped. `code = [0][0..5]` (a 6-digit SR70
  code loses its check digit, so `533398` and `533399` both become `53339`). `name = [1].Trim()`,
  absent if blank or `"-"`. Coordinates: only when ≥ 4 columns: `[len-2]`, `[len-1]` parsed
  invariant float; valid iff `-90 ≤ lat ≤ 90` and `-180 ≤ lon ≤ 180`; otherwise the row has no
  coordinate.
- **Where:** `CzPttCoordinates.loadSr70Coordinates` (30–67).
- **Why:** SR70 is the authoritative Czech point list (NATIONAL_CZPTT.md).
- **Pinned by:** `SR70 names override Czech source names throughout bundle output`,
  `Invalid and conflicting SR70 identities fall back to exact OSM PLC`.
- **Constants:** code length `5`; `"-"` placeholder → `czptt.sr70.code_length = 5`,
  `czptt.sr70.name_placeholder = "-"`.
- **Diagnostic:** none here (see R019/R020).
- **Proposed verdict:** port; the column layout must be confirmed (Q5).

#### CZPTT-R019 — SR70 invalid code poisons the code
- **What:** a code with **any** row lacking a valid coordinate (fewer than 4 columns, unparsable
  or out of range) is "invalid" and gets **no** coordinate even if other rows for the code are
  valid. Invalid codes used by a `CZ` point are listed as `invalidSr70Identities`
  (`"CZ:{code}"`).
- **Where:** `loadSr70Coordinates` (77–91); `CzPttBundle.convert` (743–748).
- **Why:** NATIONAL_CZPTT.md: OSM is considered for "absent, invalid, or internally conflicting
  SR70".
- **Pinned by:** `Invalid and conflicting SR70 identities fall back to exact OSM PLC`.
- **Constants:** none.
- **Diagnostic:** `coordinate_diagnostics.invalidSr70Identities`.
- **Proposed verdict:** port, but say explicitly whether one bad row should poison a code with a
  valid row (unknown intent; Python's verifier skips bad rows instead, section 8).

#### CZPTT-R020 — SR70 conflicting coordinates
- **What:** a code whose valid rows carry more than one distinct `(lat, lon)` pair (exact float
  equality) gets no coordinate; used `CZ` codes are listed in `conflictingSr70Codes` (bare code).
- **Where:** `loadSr70Coordinates` (69–76, 103–108); `CzPttBundle.convert` (681–688).
- **Why:** NATIONAL_CZPTT.md.
- **Pinned by:** `Invalid and conflicting SR70 identities fall back to exact OSM PLC`.
- **Constants:** none (equality is exact).
- **Diagnostic:** `coordinate_diagnostics.conflictingSr70Codes`.
- **Proposed verdict:** port.

#### CZPTT-R021 — SR70 point names
- **What:** for each 5-digit code, the set of distinct non-absent names; exactly one → name for
  `("CZ", code)`; zero or several → none. Independent of coordinate validity. Applies only to
  country `CZ`.
- **Where:** `loadSr70Coordinates` (93–102); passed as `pointNames` to `CzPttToGtfs.convert`.
- **Why:** test name: SR70 names override Czech source names.
- **Pinned by:** `SR70 names override Czech source names throughout bundle output`,
  `Provided point names apply to stops and headsigns`.
- **Constants:** none.
- **Diagnostic:** none (ambiguous names are silent).
- **Proposed verdict:** port; ambiguity should get a diagnostic.

#### CZPTT-R022 — OSM candidate eligibility
- **What:** a node is a candidate iff it has id, latitude, longitude and (`railway` ∈
  {`station`, `halt`, `stop`} **or** `public_transport` ∈ {`station`, `stop_position`}). Tag values
  are compared after null/blank filtering, case-sensitive.
- **Where:** `loadOsmCandidates` (305–325).
- **Why:** NATIONAL_CZPTT.md (but it excludes public-transport stop positions, section 7).
- **Pinned by:** `OSM nodes resolve PLC and name matches`.
- **Constants:** tag value lists → `data/czptt/osm_railway_location_tags.csv`.
- **Diagnostic:** none.
- **Proposed verdict:** port with the `public_transport` branch decided (Q6).

#### CZPTT-R023 — OSM PLC normalisation
- **What:** `ref:EU:PLC` with all whitespace removed and upper-cased; kept only if it matches
  `^[A-Z]{2}[0-9]{5}$`.
- **Where:** `loadOsmCandidates` (327–334).
- **Why:** PLC = country + 5-digit primary code.
- **Pinned by:** `OSM nodes resolve PLC and name matches`.
- **Constants:** regex.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### CZPTT-R024 — OSM candidate country
- **What:** country = first two letters of the PLC; else `addr:country`; else
  `is_in:country_code`; tag values trimmed, upper-cased and mapped `AUT→AT`, `CZE→CZ`, `DEU→DE`,
  `POL→PL`, `SVK→SK` (others kept). Used only as a ranking hint (R096).
- **Where:** `loadOsmCandidates` (335–342); `normalizedCountryCode` (296–303).
- **Why:** NATIONAL_CZPTT.md: "Country tags are ranking hints, not eligibility gates."
- **Pinned by:** `OSM name matching rejects wrong country ambiguity and corridor breaks`.
- **Constants:** alpha-3 map → `data/names/country_alpha3.csv`.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### CZPTT-R025 — OSM candidate names and identity
- **What:** names = values of the 11 name tags (2.4) in that order, trimmed, non-blank, distinct
  (`alt_name` etc. are not split on `;`). Object id `osm:node:{id}`. Candidates sorted by object
  id (ordinal string order, so `osm:node:10` < `osm:node:9`).
- **Where:** `loadOsmCandidates` (343–371).
- **Why:** unknown beyond matching recall.
- **Pinned by:** `Unknown-country OSM names allow railway suffix and fuzzy matching`.
- **Constants:** tag list → `data/czptt/osm_name_tags.txt`.
- **Diagnostic:** none.
- **Proposed verdict:** port (sort by numeric node id is cleaner; decide).

### Stage 6 — normalization, selection, journeys

#### CZPTT-R026 — Message processing order
- **What:** surviving messages are processed in ordinal order of PA id; all later "first wins"
  choices (stop names, route attributes) depend on it.
- **Where:** `CzPttToGtfs.convert` (37); `CzPttBundle.convert` (143–144).
- **Why:** determinism.
- **Pinned by:** `Dense passenger corridor wins independent of input order`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### CZPTT-R027 — Time parsing
- **What:** `Time` is split at the first `'+'` and the left part parsed with `TimeSpan.Parse`
  (invariant); seconds = `TotalSeconds` (fraction truncated) + `int(Offset) × 86400`. The UTC
  offset is discarded (local wall time kept). A `'-'` offset or `Z` is not stripped and would make
  `TimeSpan.Parse` throw, aborting the run. For each qualifier the **first** `Timing` with that
  qualifier is used.
- **Where:** `CzPttNormalize.timingSeconds`, `findRawTime` (51–66).
- **Why:** unknown.
- **Pinned by:** `Passenger activity variants and times over 24 hours are preserved`,
  `Negative source-day offsets shift only GTFS times`.
- **Constants:** `86400`.
- **Diagnostic:** none (exception on malformed values).
- **Proposed verdict:** port; malformed values become a diagnostic and the PA is rejected.

#### CZPTT-R028 — Arrival/departure mirroring
- **What:** `arrival = ALA ?? ALD`, `departure = ALD ?? ALA`. Consequently every normalized call
  has both times or neither; the raw "only one side given" fact is lost from this point on
  (including in the `operational_calls` sidecar).
- **Where:** `CzPttNormalize.locationTimes` (68–71).
- **Why:** unknown.
- **Pinned by:** indirectly (`Untimed GTFS calls are explicitly approximate`).
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** replace: the model carries raw `arrival`/`departure` options; the GTFS
  publisher fills the missing side.

#### CZPTT-R029 — Passenger call
- **What:** a location is a passenger call iff its activities contain `0001`.
- **Where:** `CzPtt.isPublicLocation` (144–145).
- **Why:** NATIONAL_CZPTT.md ("activity-0001 call").
- **Pinned by:** `PA without activity 0001 is omitted completely`, `Operational point mode
  controls only internal GTFS timing points`.
- **Constants:** `0001` → `data/czptt/train_activities.csv` (code, meaning, role).
- **Diagnostic:** none.
- **Proposed verdict:** port; `CZ13` ("unpublished stop") alongside `0001` is unhandled (Q7).

#### CZPTT-R030 — Closed activity code list
- **What:** activity codes are parsed into a closed union (20 codes, 2.1). An unknown code makes
  `parseUnion` throw `No union case for value "…"`, aborting the run. Parsing is lazy and
  `Seq.contains` stops at the first match, so whether an unknown code throws depends on its
  position and on which activity is being looked for.
- **Where:** `CzPtt.TrainActivity`, `locationActivities`; `UnionCodec.getUnionParser`.
- **Why:** unknown.
- **Pinned by:** none.
- **Constants:** code list.
- **Diagnostic:** exception.
- **Proposed verdict:** replace: the reader keeps codes as strings; unknown codes produce a
  diagnostic and are otherwise ignored.

#### CZPTT-R031 — Network-specific parameter pairing
- **What:** for each `NetworkSpecificParameter`, `Name[i]` is paired with `Value[i]` for
  `i < min(len(Name), len(Value))`; extra names or values are dropped. Name match is ordinal and
  exact. "Last value wins" where a single value is needed (`lastParameter`, root line uses
  `Seq.tryLast`). Values are not trimmed except the line code.
- **Where:** `CzPttNormalize.parameterPairs`, `parameterValues`, `lastParameter` (21–38); copy in
  `CzPttBundle.parameterPairs` (86–94).
- **Why:** unknown.
- **Pinned by:** `Repeated root and location network parameters survive parsing`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port (one implementation, section 8).

#### CZPTT-R032 — Active line code
- **What:** if **any** location of the PA has at least one `CZPassengerServiceNumber` value
  (even blank), each location's line = its own last value, trimmed, blank → none; **no carry
  forward** (a location without the parameter has no line). Otherwise every location gets the
  root-level last value, trimmed, blank → none.
- **Where:** `CzPttNormalize.normalize` (114–143).
- **Why:** NATIONAL_CZPTT.md: location values "describe the onward section … an absent or blank
  value clears the active line".
- **Pinned by:** `Location line applicability clears in both directions`, `Line change creates
  linked trips without changing train number`.
- **Constants:** parameter name.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### CZPTT-R033 — Responsible RU carry-forward
- **What:** current RU starts as the TR identifier's `Company`; each location's non-null
  `ResponsibleRU` replaces it (an empty string also replaces it); carried forward otherwise.
- **Where:** `normalize` (134, 144–145).
- **Why:** unknown.
- **Pinned by:** `Agency display names omit spaced dash qualifiers`, `Line and operator changes in
  one interstop span share a boundary`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port (treat empty as absent).

#### CZPTT-R034 — Category resolution
- **What:** per location: non-blank `CommercialTrafficType` → abbreviation of the **first**
  catalog `commercial_train_types` entry with that code, or the raw code if none; else non-blank
  `TrafficType` → same with `train_types`; else keep the carried category. Initial category
  `"Vlak"`.
- **Where:** `catalogAbbreviation`, `rawCategory` (77–91); `normalize` (135, 146–147).
- **Why:** unknown.
- **Pinned by:** `Line-less NAD keeps the train designation as its route label` (`84`→`Os`),
  `Line change creates linked trips…` (`Vlak 01234`), `Category changes split linked trips…`.
- **Constants:** `"Vlak"` → `czptt.category.default_label = "Vlak"`.
- **Diagnostic:** none (unknown codes pass through silently).
- **Proposed verdict:** port; unknown codes deserve a diagnostic.

#### CZPTT-R035 — Route type by category
- **What:** category ∈ {`IC`,`EC`,`RJ`,`rj`,`LE`,`SC`,`AEx`} → `102`; {`R`,`Ex`,`Rx`} → `103`;
  {`Os`,`Sp`,`TL`,`TLX`,`LET`} → `106`; {`NJ`,`EN`,`ES`} → `105`; else `100`. Case-sensitive
  (`RJ` and `rj` both listed).
- **Where:** `CzPttNormalize` (93–109).
- **Why:** NATIONAL_CZPTT.md palette by passenger-facing service class.
- **Pinned by:** `Rail categories receive the selected colors`.
- **Constants:** → `data/czptt/category_classes.csv` (category, route_type, colour).
- **Diagnostic:** none.
- **Proposed verdict:** port (one mode classifier, AGENTS rule 5).

#### CZPTT-R036 — Alternative transport (NAD) flag
- **What:** a location is NAD iff its last `CZAlternativeTransport` value is exactly `"1"`; no
  carry forward.
- **Where:** `normalize` (129–133).
- **Why:** NATIONAL_CZPTT.md (rail replacement).
- **Pinned by:** `Alternative transport segments use bus routes and timed transfers` and the other
  NAD tests.
- **Constants:** parameter name, `"1"`.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### CZPTT-R037 — CZInconsistentTime repair
- **What:** for each call (in order) whose location has any `CZInconsistentTime` value `"1"`:
  `left` = nearest earlier call with a time (looking at already-repaired calls);
  `right` = nearest later call that is **not** flagged and has a time (original values). If both
  exist: `L = left.dep ?? left.arr`, `R = right.arr ?? right.dep`. If `R ≥ L`:
  `inferred = L + (R − L) × (idx − left.idx) / (right.idx − left.idx)` (int64, truncating);
  `arrival` = own `arr ?? dep` if in `[L, R]`, else `inferred`; `departure` = own `dep ?? arr` if
  in `[arrival, R]`, else `arrival`. If changed, both times are replaced. If `R < L`, or either
  neighbour is missing, the call is left unchanged (chronology may then reject the PA, R039).
  Root-level `CZInconsistentTime` is ignored. Repaired values replace the call's times
  everywhere downstream, including `operational_calls`, and the stop time is `timepoint=1`.
- **Where:** `CzPttNormalize.inconsistentTime`, `forceMonotonicFlaggedTimes` (301–351).
- **Why:** PROGRESS.md "inconsistent-time correction (R10)"; test names.
- **Pinned by:** `CZInconsistentTime root parameter accepts monotonic source times`, `Flagged
  backward passenger times are bounded by surrounding calls`, `Flagged departure is raised to a
  valid arrival`, `Flagged call with backward surrounding anchors is rejected`.
- **Constants:** parameter name, `"1"`.
- **Diagnostic:** `"{pa}: corrected CZInconsistentTime at source sequence {n} from arrival={…},
  departure={…} to arrival={…}, departure={…}"` (F# option formatting) →
  `diagnostics.json` `sidecar_boundary_approximations`.
- **Proposed verdict:** port the repair; keep raw times in the model next to the repaired ones
  (see B19); give the diagnostic its own code.

#### CZPTT-R038 — Reject PA without passenger call
- **What:** no call with `0001` → PA rejected, reason `"no activity 0001 passenger call"`; no
  operational calls, trips, sidecar rows (but notes, R076).
- **Where:** `CzPttToGtfs.convert` (42–49).
- **Why:** NATIONAL_CZPTT.md.
- **Pinned by:** `PA without activity 0001 is omitted completely`.
- **Constants:** reason string.
- **Diagnostic:** `rejected_journeys` entry `{paId, reason, sequence=null, previousSeconds=null,
  currentSeconds=null}`.
- **Proposed verdict:** port.

#### CZPTT-R039 — Reject PA on chronology
- **What:** iterate calls in source order (all calls, including outside the passenger window);
  first violation wins: (a) both times and `departure < arrival` → `"departure precedes
  arrival"` (`previousSeconds=arrival`, `currentSeconds=departure`); (b) otherwise
  `first = arr ?? dep` < previous event → `"event precedes previous event"`; previous event is
  updated to `dep ?? arr` (untimed calls keep the previous). `sequence` = source sequence.
  Equal times are allowed.
- **Where:** `CzPttNormalize.chronologyError` (353–381).
- **Why:** NATIONAL_CZPTT.md.
- **Pinned by:** `Backward chronology rejects the complete PA`, `Flagged call with backward
  surrounding anchors is rejected`.
- **Constants:** reason strings.
- **Diagnostic:** `rejected_journeys`.
- **Proposed verdict:** port.

#### CZPTT-R040 — Service boundary projection
- **What:** service key = `(lineCode, routeType, category)` (RU is **not** in the key). For each
  source index `i ≥ 1` where the key differs from `i−1`: if call `i` is a passenger call, the
  change applies at `i`. Otherwise `next` = first passenger index > `i`; if none → the change is
  ignored (adjustment `"ignored-after-final-passenger-call"`, `appliedSequence=null`); else
  `prev` = last passenger index < `i` (or −1), candidates = RU transition indices `t` (RU of `t`
  ≠ RU of `t−1`) with `prev < t ≤ next`, ordered by `(|t − i|, t)`; the first candidate, or
  `next` if none, is the applied index. If applied ≠ `i`, an adjustment with reason
  `"coalesced-with-operator-boundary"` (applied index is an RU transition) or
  `"deferred-to-next-passenger-call"`. When several changes land on the same applied index, the
  one with the largest source index wins. The projected key then carries forward from each
  applied index; initial key = key of call 0. Only `selected` calls (R041) use the projected
  keys; `operational_calls` keep raw line codes. PAs with < 2 calls are unchanged.
- **Where:** `CzPttNormalize.projectPassengerServiceBoundaries` (387–472).
- **Why:** NATIONAL_CZPTT.md (a line change at an internal point moves to a passenger call);
  test names.
- **Pinned by:** `Internal line changes move to the next passenger call`, `Line and operator
  changes in one interstop span share a boundary`, `Line expiry before the terminus is deferred to
  the terminus`.
- **Constants:** reason strings.
- **Diagnostic:** `boundaryAdjustments` → `diagnostics.json` `line_boundary_adjustments`
  (`{paId, sourceSequence, appliedSequence, reason}`), only for accepted PAs.
- **Proposed verdict:** port.

#### CZPTT-R041 — Operational-point mode selection
- **What:** `Gtfs` mode: selected = every call with source index in `[firstPassenger,
  lastPassenger]`. `Sidecar` mode: selected = passenger calls only. Calls before the first or after
  the last passenger call are never selected. CLI default is `gtfs`; Python passes `sidecar` by
  default.
- **Where:** `CzPttNormalize.selectedCalls` (474–490); `ConvertCommands.czpttToBundle`.
- **Why:** NATIONAL_CZPTT.md "Operational timing points".
- **Pinned by:** `Operational point mode controls only internal GTFS timing points`,
  `Agency-only internal boundaries stay exact or are diagnosed`.
- **Constants:** mode → `czptt.operational_points = "sidecar"` (decide the single default).
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### CZPTT-R042 — Collapse consecutive subsidiary duplicates
- **What:** while scanning selected calls, a call is merged into the previously kept call iff:
  kept.sourceIndex + 1 = call.sourceIndex, both have `LocationSubsidiaryIdentification`, same
  `CountryCodeISO`, same `LocationPrimaryCode`, and equal projected line, RU, routeType, category,
  NAD flag. Merge: `passenger = a ∨ b`; `arrival = kept.arr ?? call.arr`; `departure = call.dep ??
  kept.dep`; location (subsidiary, activities), sourceIndex and everything else from the kept
  call. Because the kept call keeps its own sourceIndex, a run of three only collapses its first
  two.
- **Where:** `selectedCalls` (491–519).
- **Why:** NATIONAL_CZPTT.md "Consecutive subsidiary-only duplicates are collapsed in GTFS while
  their raw rows remain in Parquet."
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port the intent; fix the run-of-three case (B5).

#### CZPTT-R043 — Reject PA with fewer than two selected calls
- **What:** after selection, `< 2` calls → rejected `"fewer than two selected GTFS calls"`;
  boundary adjustments of that PA are discarded.
- **Where:** `CzPttToGtfs.convert` (56–65).
- **Why:** a GTFS trip needs two stop times.
- **Pinned by:** `PA with one selected GTFS call is rejected completely`.
- **Constants:** `2`; reason string.
- **Diagnostic:** `rejected_journeys`.
- **Proposed verdict:** port.

#### CZPTT-R044 — Segmentation
- **What:** segment key = `(lineCode, RU, routeType, category, NAD)` of selected calls. For
  `i` in `1 .. n−1`: if key(i) ≠ current key **and `i < n−1`**, close the segment
  `calls[start..i]` (inclusive: the boundary call belongs to both segments), `start = i`, key =
  key(i). The final segment `calls[start..]` is always added. Consequences: a key change at the
  **last** call is ignored (the terminus takes the previous key); the `Length > 1` check is dead
  code (segments always have ≥ 2 calls). Segment index = 0-based order of creation.
- **Where:** `CzPttNormalize.segments` (521–559).
- **Why:** NATIONAL_CZPTT.md "Route/operator/mode changes split one PA into linked trips sharing
  a PA block; category changes do the same".
- **Pinned by:** `Category changes split linked trips with the real terminus`, `Line change
  creates linked trips…`, `Location line applicability clears in both directions`, `Agency-only
  internal boundaries stay exact or are diagnosed`, `Line expiry before the terminus is deferred
  to the terminus`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port; confirm the terminal-call rule is intended (Q8).

#### CZPTT-R045 — NAD route type
- **What:** a NAD segment's route type is `714` regardless of category.
- **Where:** `segments` (542, 555).
- **Why:** GTFS extended type 714 = rail replacement bus.
- **Pinned by:** `Line-less NAD keeps the train designation as its route label`, `Alternative
  transport segments use bus routes and timed transfers`.
- **Constants:** `714` → `data/czptt/category_classes.csv` (NAD row).
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### CZPTT-R046 — Journeys and mode blocks
- **What:** one journey per segment (`parts` always holds exactly that one segment);
  `modeBlockIndex` starts at 0 and increments whenever a segment's NAD flag differs from the
  previous segment's.
- **Where:** `CzPttNormalize.journeys` (561–577).
- **Why:** block IDs per mode (R064).
- **Pinned by:** `Alternative transport segments use bus routes and timed transfers` (3 blocks).
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** replace: drop the one-element `parts` array (legacy from multi-part
  journeys; R058/R060 iterate it needlessly).

#### CZPTT-R047 — NAD journeys keep passenger calls only
- **What:** for a NAD journey, calls are filtered to passenger calls; fewer than 2 → the journey
  is dropped silently (its index is skipped, so trip numbers have gaps). Non-NAD journeys keep
  their operational calls (in `Gtfs` mode).
- **Where:** `CzPttToGtfs.convert` (69–79).
- **Why:** NATIONAL_CZPTT.md (NAD runs between public endpoints; test name).
- **Pinned by:** `NAD runs without two passenger calls are omitted`, `Alternative transport
  segments use bus routes and timed transfers` (operational NAD point `93001` absent).
- **Constants:** `2`.
- **Diagnostic:** none.
- **Proposed verdict:** port; add a diagnostic.

#### CZPTT-R048 — Edge timing copied across a shared point
- **What:** for each adjacent pair of retained journeys, if the left journey's last call and the
  right journey's first call have the same `(country, primary code)`: if exactly one side is
  timed, the untimed side copies `arrival = arr ?? dep`, `departure = dep ?? arr` from the timed
  side and is marked inferred.
- **Where:** `reconcileJourneyEdgeTimes` (626–643); `copyTiming`, `hasCompleteTiming`,
  `pointIdentity`.
- **Why:** NATIONAL_CZPTT.md "GTFS-only split edges without source timing first inherit a timed
  counterpart at the same primary location".
- **Pinned by:** `Untimed passenger boundary copies a timed same-station counterpart`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### CZPTT-R049 — Edge timing interpolation
- **What:** for each retained journey, an untimed first or last call is interpolated from the
  **whole PA's** calls: `left` = nearest earlier call (by source index) with a time, `right` =
  nearest later; if both, `span > 0` and `R ≥ L` (`L = left.dep ?? arr`, `R = right.arr ?? dep`):
  `t = L + (R − L) × (idx − left.idx) / span` (int64 truncating), arrival = departure = `t`,
  inferred. Intermediate untimed calls are not interpolated.
- **Where:** `interpolateTiming` (594–624); `reconcileJourneyEdgeTimes` (645–656).
- **Why:** NATIONAL_CZPTT.md "sequence-linear interpolation only when bracketed by known PA
  times".
- **Pinned by:** `Untimed split edges are interpolated only in GTFS`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port (one interpolation helper shared with R037, section 8).

#### CZPTT-R050 — Omit journey with an untimed edge
- **What:** after R048–R049, a journey whose first or last call still has no time is dropped.
  Other journeys of the PA survive; transfers are then computed over the survivors (R082).
- **Where:** `reconcileJourneyEdgeTimes` (655–669).
- **Why:** NATIONAL_CZPTT.md "A generated journey whose edge remains untimed is omitted and
  diagnosed."
- **Pinned by:** `Journey with an uninferable edge is omitted and diagnosed`.
- **Constants:** none.
- **Diagnostic:** `"{pa}: omitted generated journey {index+1} because GTFS edge timing could not
  be inferred at source sequence {s1[,s2]}"` → `sidecar_boundary_approximations`.
- **Proposed verdict:** port.

#### CZPTT-R051 — Sidecar-mode operator boundary diagnostic
- **What:** in `Sidecar` mode only, over the **whole PA's** calls: the first index `i` where RU(i)
  ≠ RU(i−1) and call `i` is not a passenger call yields one diagnostic (later ones are not
  reported; positions outside the passenger window are reported too).
- **Where:** `CzPttToGtfs.convert` (83–96).
- **Why:** NATIONAL_CZPTT.md (approximation is diagnosed).
- **Pinned by:** `Agency-only internal boundaries stay exact or are diagnosed`.
- **Constants:** message text.
- **Diagnostic:** `"{pa}: operator boundary moved from source sequence {i+1} to the following
  passenger call"` → `sidecar_boundary_approximations`.
- **Proposed verdict:** replace: one diagnostic per moved boundary, only for boundaries inside
  the passenger window (B14).

### Stage 6 — entities

#### CZPTT-R052 — Stop hierarchy
- **What:** every `(country, primary code)` used by a journey call yields a station
  (`location_type=1`) `czptt:stop:{esc(country)}:{esc(code)}`. Child (`location_type=0`,
  `parent_station` = station): NAD journey call → `{station}:platform:BUS`; call without
  subsidiary → `{station}:unspecified`; else `{station}:platform:{esc(LocationSubsidiaryCode)}`.
  `stop_times` always reference a child.
- **Where:** `CzPttEntities.stationIdFor`, `stopId`, `busStopId`, `journeyStopId`, `stops`.
- **Why:** NATIONAL_CZPTT.md ("same colon-separated convention as the JDF feed").
- **Pinned by:** `Stops and generated entities use the CZPTT namespace and JDF hierarchy`,
  `Alternative transport segments use bus routes and timed transfers`.
- **Constants:** `unspecified`, `platform`, `BUS`.
- **Diagnostic:** none.
- **Proposed verdict:** port via `jr_model::identity`; note the `BUS` collision (section 4).

#### CZPTT-R053 — Stop names
- **What:** name = SR70 name for `("CZ", code)` (R021) if present, else `PrimaryLocationName`,
  else `""`. Parent and children share the name. When the same stop id is produced from several
  calls with different source names, the first in processing order (PA id, journey, call) wins.
  Stations get `stop_timezone = "Europe/Prague"`; children none.
- **Where:** `CzPttEntities.stopName`, `stops` (47–104).
- **Why:** NATIONAL_CZPTT.md (but it says the source station name, section 7).
- **Pinned by:** `Platform name stays unchanged and friendly code is platform_code`, `Provided
  point names apply to stops and headsigns`, `SR70 names override Czech source names throughout
  bundle output`, `Headsign remains the terminus when an intermediate point has the same name`,
  `Fallback route naming does not alter stop municipalities`, `Missing Nazev20 gets a compact
  route label without changing stop text`.
- **Constants:** `Europe/Prague` → `czptt.timezone = "Europe/Prague"`.
- **Diagnostic:** none.
- **Proposed verdict:** port; replace "first wins" with a deterministic named choice plus a
  diagnostic when source names disagree.

#### CZPTT-R054 — platform_code and the NAD boarding point
- **What:** child `platform_code` = `LocationSubsidiaryName` if non-blank, else
  `LocationSubsidiaryCode`; `unspecified` children have none. NAD child: `platform_code = "BUS"`,
  `stop_desc = "Pro přesné informace k nástupišti náhradní dopravy sledujte informace dopravce."`.
- **Where:** `stops` (85–100); `busStopDescription`.
- **Why:** NATIONAL_CZPTT.md (friendly name is the passenger-facing code; the synthetic BUS point
  bounds stop-pair transfers to NAD).
- **Pinned by:** `Platform name stays unchanged and friendly code is platform_code`,
  `Alternative transport segments use bus routes and timed transfers`.
- **Constants:** `"BUS"`, the description → `data/czptt/strings.toml`
  (`nad_boarding_point.platform_code`, `nad_boarding_point.description`).
- **Diagnostic:** none.
- **Proposed verdict:** port.
- **Changed 2026-11-26:** tracks publish no `platform_code` (policy `tracks.public_code = false`).
  CZPTT subsidiaries are operational track markings, not the platforms passengers are shown, so
  publishing them misled. SŽ assigns platforms statically from the next timetable (GVD 2027);
  platform codes return once tracks are mapped to them. Tracks stay boarding points and keep
  their `sr70:track` keys; the NAD `BUS` point is unchanged.

#### CZPTT-R055 — Stop deduplication and order
- **What:** rows are stably sorted with description-bearing rows first, deduplicated by id (first
  wins), then sorted by id. The description-first sort makes the synthetic BUS row win when a real
  subsidiary is literally coded `BUS` (same id).
- **Where:** `stops` (101–104).
- **Why:** unknown (inferred: the `BUS` collision).
- **Pinned by:** `Untimed split edges are interpolated only in GTFS` (uses subsidiary `BUS`).
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** replace: make the synthetic id collision-free in `jr_model::identity`
  and drop the tie-break.

#### CZPTT-R056 — KADR line for a date
- **What:** among catalog lines with `code` = the journey's line code and validity containing the
  PA's `StartDateTime` date (absent bound = open), the one with the greatest `valid_from` (absent
  sorts first) wins; none → unmapped. The same line version applies to every service date of the
  PA.
- **Where:** `CzPttEntities.lineForDate` (106–113); date from `CzPttToGtfs.convert`.
- **Why:** NATIONAL_CZPTT.md ("When no dated KADR line exists…").
- **Pinned by:** `Unknown catalog lines use the train designation fallback`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port; decide per-date versions (Q9).

#### CZPTT-R057 — Train number
- **What:** first non-null `OperationalTrainNumber` among the given calls (journey calls for
  trips/routes, all PA calls nowhere); else the TR identifier's `Core` (12 characters, may be
  non-numeric). Raw string, leading zeros kept (`01234`).
- **Where:** `CzPttEntities.trainNumberForCalls` (121–126).
- **Why:** unknown.
- **Pinned by:** `Fallback routes use train designations and retain raw train numbers`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port the first rule; drop the TR-core fallback or diagnose it (B23).

#### CZPTT-R058 — Route label
- **What:** mapped line → `mark`; else `"{category} {trainNumber}"`, or `category` if no number.
  NAD journeys append `" (NAD)"`. Labels are made distinct in order and joined with `"/"` (always
  one label in practice, R046).
- **Where:** `CzPttEntities.labelsForJourney` (128–152); `route` (256–264).
- **Why:** NATIONAL_CZPTT.md ("Friendly KADR Znacka values such as S1 … are route short
  names").
- **Pinned by:** `Fallback routes use train designations…` (`RJ 01234`), `Line change creates
  linked trips…` (`S1`, `S12`), `Alternative transport segments…` (`S1 (NAD)`), `Line-less NAD
  keeps the train designation as its route label` (`Os 363784 (NAD)`), `Unknown catalog lines use
  the train designation fallback`, `Missing Nazev20 gets a compact route label…`, `Fallback route
  naming does not alter stop municipalities`.
- **Constants:** `" (NAD)"`, `"/"` → `data/czptt/strings.toml`.
- **Diagnostic:** none.
- **Proposed verdict:** port (drop the `/`-join vestige).

#### CZPTT-R059 — Route identity
- **What:** mapped line → `czptt:route:line:{esc(line)}:{esc(RU)}:{esc(routeType)}`; else
  `czptt:route:fallback:{esc(RU)}:{esc(category)}:{esc(routeType)}:{esc(trainNumber ??
  category)}`. Fallback routes are therefore one per train number.
- **Where:** `CzPttEntities.routeId` (154–164).
- **Why:** NATIONAL_CZPTT.md (but its grouping rule differs, section 7).
- **Pinned by:** `Fallback routes use train designations…` (one route for two PAs with the same
  number).
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port via `jr_model::identity`.

#### CZPTT-R060 — Route attributes and dedup
- **What:** `route_long_name` = mapped line `name.TrimEnd()` (only trailing whitespace) when the
  label set has exactly one label and it is mapped; else none. `agency_id` = journey RU agency.
  Routes are deduplicated by id keeping the **first** (processing order) and sorted by id; a later
  PA with the same route id but a different line version contributes nothing.
- **Where:** `CzPttEntities.route` (249–272); `CzPttToGtfs.convert` (101–111).
- **Why:** test name (right-trimmed).
- **Pinned by:** `Mapped route names are right-trimmed and feed contact is serialized`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port; diagnose disagreeing attributes under one id.

#### CZPTT-R061 — Route colours
- **What:** by route type: `106`→`1c1745`, `102`→`b91c1c`, `103`→`b45309`, `105`→`4c1d95`, any
  other (`100`, `714`) → `475569`; `route_text_color = ffffff`.
- **Where:** `CzPttEntities.routeColor` (239–245), `route`.
- **Why:** NATIONAL_CZPTT.md palette.
- **Pinned by:** `Rail categories receive the selected colors`, `Category changes split linked
  trips with the real terminus`.
- **Constants:** → `data/czptt/category_classes.csv`.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### CZPTT-R062 — Trip short name
- **What:** `"{category of the journey's first call} {trainNumber}"`, or the category alone.
  (The category is the projected one, R040.)
- **Where:** `CzPttEntities.tripShortName` (175–180).
- **Why:** NATIONAL_CZPTT.md ("Os 3456").
- **Pinned by:** `Fallback routes use train designations…`, `Category changes split linked
  trips…`, `Line change creates linked trips…` (`Vlak 01234`), `Alternative transport segments…`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### CZPTT-R063 — Trip headsign
- **What:** every trip of a PA has the headsign of the PA's **last passenger call** (whole PA,
  not the segment): its stop name (R053) if non-blank, else `"{country} {code}"`. Not marked with
  ` [?]`.
- **Where:** `CzPttEntities.publicEndpoints`, `pointDisplayName`, `trip`.
- **Why:** NATIONAL_CZPTT.md "Every segment keeps the complete PA's final passenger call as its
  headsign."
- **Pinned by:** `Category changes split linked trips with the real terminus`, `Line change
  creates linked trips…`, `Provided point names apply to stops and headsigns`, `Headsign remains
  the terminus when an intermediate point has the same name`, `SR70 names override Czech source
  names…`.
- **Constants:** fallback format.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### CZPTT-R064 — Block identity
- **What:** if any two adjacent retained journeys of the PA differ in NAD flag, every trip gets
  `czptt:block:{esc(pa)}:mode:{modeBlockIndex+1}`; otherwise `czptt:block:{esc(pa)}`.
- **Where:** `CzPttEntities.blockId`, `trip` (286–289); `CzPttToGtfs.convert` (119–123).
- **Why:** unknown (inferred: rail and replacement bus must not form one GTFS block).
- **Pinned by:** `Alternative transport segments use bus routes and timed transfers`, `Category
  changes split linked trips…`, `Line change creates linked trips…`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### CZPTT-R065 — Service calendar
- **What:** one service per PA, `czptt:service:{esc(pa)}:calendar:journey`; one
  `calendar_dates` row (`exception_type=1`) per `'1'` at bitmap index `i`, date =
  `StartDateTime` date + `i` (`EndDateTime` ignored). No `calendar.txt`.
- **Where:** `CzPttEntities.serviceId`, `calendarExceptions` (169–193).
- **Why:** unknown.
- **Pinned by:** `PA with one selected GTFS call is rejected completely` (no rows for rejected).
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port as a `DateSet`; service ids are renamed later (R113).

#### CZPTT-R066 — GTFS time shift for negative offsets
- **What:** per PA, `min` over all calls' arrival/departure (whole PA, after R037). If `min < 0`,
  every GTFS time of the PA is shifted by `ceil(−min / 86400) × 86400`; the calendar is **not**
  shifted. Operational sidecar times stay unshifted.
- **Where:** `CzPttEntities.gtfsTimeShift` (195–203), `stopTimes`.
- **Why:** NATIONAL_CZPTT.md "Negative source-day offsets are shifted by whole days only for
  GTFS".
- **Pinned by:** `Negative source-day offsets shift only GTFS times`.
- **Constants:** `86400`.
- **Diagnostic:** none.
- **Proposed verdict:** port the need, but the service date must move back by the same days
  (probable bug B3; Q2).

#### CZPTT-R067 — Pickup and drop-off types
- **What:** non-passenger call → pickup `1` and drop-off `1`. Passenger: pickup `1` if `0029`
  (disembark only), drop-off `1` if `0028` (embark only); otherwise `3` if `0030` (request stop),
  else `0`. Activities are those of the (kept) location (R042).
- **Where:** `CzPttEntities.stopTimes` (208–229).
- **Why:** NATIONAL_CZPTT.md ("Embark-only and disembark-only restrictions still take precedence
  on the forbidden side").
- **Pinned by:** `Passenger activity variants and times over 24 hours are preserved`,
  `Operational point mode controls only internal GTFS timing points`.
- **Constants:** activity codes → `data/czptt/train_activities.csv`.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### CZPTT-R068 — timepoint
- **What:** inferred (R048/R049) → `0`; timed → `1`; untimed → `0` with empty times.
- **Where:** `stopTimes` (231–235).
- **Why:** NATIONAL_CZPTT.md.
- **Pinned by:** `Untimed GTFS calls are explicitly approximate`, `Untimed split edges are
  interpolated only in GTFS`, `Flagged backward passenger times…` (repaired = `1`).
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### CZPTT-R069 — stop_sequence is the source sequence
- **What:** `stop_sequence = source index + 1` (gaps where calls are not selected; boundary calls
  share a sequence across the two trips).
- **Where:** `stopTimes` (222).
- **Why:** PA sequences are the trip-part sequences (ServingContractTests comment).
- **Pinned by:** `Alternative transport segments…` (features name PA sequences),
  `ServingContractTests.CZPTT trip parts share boundary calls…`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### CZPTT-R070 — Wheelchair and bicycle trip flags
- **What:** NAD journeys: none. Otherwise a code is "complete" for the journey if some central
  note (R073) has that code, `coversAllDates` and a resolved interval `first ≤ last` with `first ≤
  journey.first.sourceIndex` and `last ≥ journey.last.sourceIndex`. `wheelchair_accessible = 1`
  if `17` or `34` complete (never `2`). Bikes: positive = any of `22, 26, 27, 28, 29` complete;
  prohibited = `36` complete; positive only → `1`; prohibited only → `2`; both or neither → empty.
- **Where:** `CzPttNormalize.projectedTripFeatures`, `noteCoversJourney` (277–299);
  `CzPttEntities.trip`.
- **Why:** NATIONAL_CZPTT.md "GTFS trip flags are deliberately conservative"; code 17 also
  permits a lift-equipped vehicle.
- **Pinned by:** `Central bike notes retain distinctions and project only complete claims`,
  `Partial repeated-location and calendar bike notes remain unknown`, `Wheelchair notes conflicts
  and malformed notes are conservative`, `Alternative transport segments…`.
- **Constants:** code sets → `data/czptt/central_note_features.csv` (code, kinds, gtfs flag).
- **Diagnostic:** conflict → R079.
- **Proposed verdict:** port.

#### CZPTT-R071 — Note interval endpoint resolution
- **What:** `compact(x)` = `x.Trim().ToUpperInvariant()`, then if length > 2 and the first two
  characters are letters, drop them (so `CZ57076` → `57076`). Matches = PA calls (all, by source
  order) whose compact primary code equals the compact note code — **country ignored**.
  Occurrence: blank → `0`; a non-negative integer (digits only) → that value; else unresolved.
  Result = `matches[occurrence]` (**0-based**), unresolved if out of range.
- **Where:** `CzPttNormalize.compactLocationCode`, `noteOccurrence`, `resolveNoteEndpoint`
  (173–193).
- **Why:** unknown.
- **Pinned by:** `Partial repeated-location and calendar bike notes remain unknown` (occurrence
  `1` = second Praha).
- **Constants:** none.
- **Diagnostic:** via R073/R074.
- **Proposed verdict:** replace with the single interval resolver (section 8, Q3).

#### CZPTT-R072 — Note calendar resolution
- **What:** each root `CZCalendarPTTNote` value split on `'|'`; ≥ 4 fields required; `f1` =
  start date `yyyyMMdd`; dates = start + `i` for each `'1'` in `f3`; `f4` (non-blank) = next
  calendar id; rows grouped by `f0`. Resolving id `c`: unknown or already visited → unresolved;
  own dates = union of its rows; distinct next ids: > 1 → unresolved; 0 → own; 1 → own ∪
  resolve(next) (unresolved propagates). `f2` (end date) is ignored for dates.
- **Where:** `noteCalendarDates`, `resolvedCalendarDates` (207–249).
- **Why:** unknown.
- **Pinned by:** `Partial repeated-location and calendar bike notes remain unknown`.
- **Constants:** date format `yyyyMMdd`.
- **Diagnostic:** via R073/R074.
- **Proposed verdict:** port.

#### CZPTT-R073 — Central notes
- **What:** each root `CZCentralPTTNote` value, index = position among those values: fields on
  `'|'`; `code = f0` (or `""`); if ≥ 5 fields, `first = resolve(f1, f2)`, `last = resolve(f3,
  f4)` (R071); `f5` unused; `calendarId = f6` if non-blank. `coversAllDates` = no calendar id, or
  the resolved calendar contains every active date of the PA (bitmap from `StartDateTime`).
  `resolved = first ≤ last`. Label = catalog `central_notes.name` of the first entry with that
  code (validity ignored). `validFrom/validTo` = min/max of the resolved calendar if non-empty.
  `firstSequence/lastSequence` = resolved indices + 1. Kind `czptt_central_note`.
- **Where:** `CzPttNormalize.centralNoteSpecs` (251–275); `CzPttToGtfs.convert` (233–281).
- **Why:** NATIONAL_CZPTT.md (lossless notes).
- **Pinned by:** `Wheelchair notes conflicts and malformed notes are conservative` (`36|missing`
  unresolved), `Central bike notes…`, `Alternative transport segments…` (raw value kept).
- **Constants:** kind string.
- **Diagnostic:** `"{pa}: unresolved CZCentralPTTNote interval {raw}"`; `"{pa}: unresolved
  CZCentralPTTNote calendar {raw}"` (calendar id given, not covering, unresolvable) →
  `ids_diagnostics`.
- **Proposed verdict:** port; field meanings for `f5` unknown (Q10).

#### CZPTT-R074 — Non-central (local) notes
- **What:** each root `CZNonCentralPTTNote`: if ≥ 4 fields, `first = resolve(f0, f1)`, `last =
  resolve(f2, f3)`; label = `f4` if non-blank; `f5..f8` unused; `calendarId = f9` if non-blank.
  `code` none, kind `czptt_local_note`, bounds as R073.
- **Where:** `CzPttToGtfs.convert` (282–320).
- **Why:** NATIONAL_CZPTT.md.
- **Pinned by:** `CZPTT bundle writer emits all operational Parquet sidecars`.
- **Constants:** kind string.
- **Diagnostic:** `"{pa}: unresolved CZNonCentralPTTNote interval {raw}"`, `"{pa}: unresolved
  CZNonCentralPTTNote calendar {raw}"` → `ids_diagnostics`.
- **Proposed verdict:** port (Q10 for `f5..f8`).

#### CZPTT-R075 — Calendar notes
- **What:** each root `CZCalendarPTTNote`: `code = f0`, `validFrom = f1`, `validTo = f2`
  (`yyyyMMdd`); `resolved` = ≥ 4 fields and both dates parse; no trips, no sequences. Kind
  `czptt_calendar_note`.
- **Where:** `CzPttToGtfs.convert` (321–339).
- **Why:** NATIONAL_CZPTT.md (note calendars are kept losslessly).
- **Pinned by:** none directly.
- **Constants:** kind string.
- **Diagnostic:** `"{pa}: malformed CZCalendarPTTNote {raw}"` → `ids_diagnostics`.
- **Proposed verdict:** port.

#### CZPTT-R076 — Note scope and trip projection
- **What:** notes are produced for **every message that reached normalization**, including PAs
  later rejected (R038/R039/R043); those get empty trip lists. Note id
  `czptt:note:{esc(pa)}:{central|noncentral|calendar}:{index+1}`. Trip ids = retained journeys
  (NAD included) with any call whose source index is in `[first, last]`.
- **Where:** `CzPttToGtfs.convert` (28–41, 233–252).
- **Why:** unknown.
- **Pinned by:** `Wheelchair notes conflicts and malformed notes are conservative` (one note).
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port the projection; restrict notes to accepted PAs (B10).

#### CZPTT-R077 — Note features
- **What:** for accepted PAs, each central note with a mapped code and a resolved interval, for
  each **non-NAD** retained journey with at least one call in the interval: if the interval covers
  the whole journey, one trip-scoped feature per kind; otherwise one call-scoped feature per kind
  per covered call (call sequence = source sequence). Calendar coverage is **not** required.
  Mapping: `17` → `wheelchair_accessible_vehicle`, `wheelchair_booking_recommended`; `34` →
  `wheelchair_accessible_vehicle`, `wheelchair_booking_required`; `22` → `bicycle_transport`,
  `bicycle_carry_on`; `26` → `bicycle_transport`, `bicycle_storage`,
  `bicycle_reservation_available`; `27` → `bicycle_transport`, `bicycle_storage`,
  `bicycle_reservation_required`; `28` → `bicycle_transport`, `bicycle_carry_on`,
  `bicycle_reservation_available`; `29` → `bicycle_transport`, `bicycle_carry_on`,
  `bicycle_reservation_required`; `36` → `bicycle_transport_prohibited`.
- **Where:** `CzPttToGtfs.convert` (341–391).
- **Why:** NATIONAL_CZPTT.md (distinctions retained; not projected onto NAD).
- **Pinned by:** `Central bike notes retain distinctions and project only complete claims`,
  `Partial repeated-location and calendar bike notes remain unknown`, `Alternative transport
  segments…`.
- **Constants:** → `data/czptt/central_note_features.csv`.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### CZPTT-R078 — Request-stop feature
- **What:** every retained journey call (NAD included, passenger or not) with activity `0030`
  yields kind `on_request`, source code `0030`, call sequence = source sequence, no note.
- **Where:** `CzPttToGtfs.convert` (392–407).
- **Why:** NATIONAL_CZPTT.md ("Activity 0030 … also becomes a call-scoped on_request feature").
- **Pinned by:** `Passenger activity variants and times over 24 hours are preserved`,
  `Alternative transport segments…`.
- **Constants:** `0030`, `on_request`.
- **Diagnostic:** none.
- **Proposed verdict:** port (restrict to passenger calls: decide).

#### CZPTT-R079 — Conflicting bicycle claims diagnostic
- **What:** when R070 finds both positive and prohibited complete bicycle claims for a journey.
- **Where:** `CzPttToGtfs.convert` (409–416).
- **Why:** NATIONAL_CZPTT.md (contradictory claims stay unknown and are diagnosed).
- **Pinned by:** `Wheelchair notes conflicts and malformed notes are conservative`.
- **Constants:** none.
- **Diagnostic:** `"{pa}: conflicting whole-trip bicycle notes for {tripId}"` → `ids_diagnostics`.
- **Proposed verdict:** port.

#### CZPTT-R080 — Agencies
- **What:** one agency per distinct RU of retained journeys, sorted. `agency_id =
  czptt:agency:{esc(RU)}`. Name = catalog company name (first by code) → trimmed, cut at the first
  `" - "` when its index > 0 and the part before is non-blank (part right-trimmed); no company →
  `"Unknown {code}"`. URL = company url trimmed, non-blank; prefixed `https://` unless it matches
  `^[a-z][a-z0-9+.-]*://` (case-insensitive); kept only if it parses as an absolute `http`/`https`
  URI; else `https://portal.cisjr.cz/`. `agency_timezone = Europe/Prague`, `agency_lang = cs`.
- **Where:** `CzPttAgencies.publicAgencyName`, `agency` (24–60); `CzPttToGtfs.convert` (183–189).
- **Why:** NATIONAL_CZPTT.md ("Public agency display names omit catalog qualifiers beginning with
  a spaced dash"; "schemeless KADR operator URLs are normalized").
- **Pinned by:** `Agency display names omit spaced dash qualifiers`, `Negative source-day offsets
  shift only GTFS times` (default and schemeless URL).
- **Constants:** `" - "`, `https://`, `https://portal.cisjr.cz/`, `Europe/Prague`, `cs` →
  `czptt.agency.default_url`, `czptt.agency.lang`, `czptt.timezone`; separator →
  `czptt.agency.qualifier_separator = " - "`.
- **Diagnostic:** none.
- **Proposed verdict:** port; unknown RU deserves a diagnostic.

#### CZPTT-R081 — Feed info
- **What:** only if ≥ 1 accepted PA: publisher `Oběhy project (via JrUtil)`, url
  `https://obehy.cz`, lang `cs`, contact `admin@obehy.cz`; start = min `StartDateTime` date of
  accepted PAs; end = max of (`EndDateTime` date, else `StartDateTime` date); version =
  `czptt:` + max `CZPTTCreation` formatted `yyyyMMdd'T'HHmmss` (invariant). `CZPTTCreation` is a
  .NET `DateTime` from XmlSerializer, so a value with an offset is converted to the machine's
  local zone first.
- **Where:** `CzPttAgencies.feedInfo` (107–136); `Gtfs.obehyFeedInfo`.
- **Why:** unknown.
- **Pinned by:** `Mapped route names are right-trimmed and feed contact is serialized`.
- **Constants:** publisher strings → shared `policy/publish.toml` (not CZPTT-specific).
- **Diagnostic:** none.
- **Proposed verdict:** port (version from a timezone-explicit timestamp; dates from the model's
  `DateSet` rather than raw `EndDateTime`).

#### CZPTT-R082 — In-PA transfers
- **What:** for each adjacent pair of a PA's retained journeys: same NAD flag → `transfer_type=4`,
  trip ids, no stops, no minimum. Different flag → if the left's last call and the right's first
  call have the same `(country, primary code)`: `transfer_type=1`, `min_transfer_time=0`, from
  stop = left last child (BUS if NAD), to stop = right first child, trip ids (later rewritten,
  R084); else omitted with a diagnostic. "Adjacent" is over retained journeys, so a dropped NAD
  journey (R047) or omitted journey (R050) links journeys that do not share a stop.
- **Where:** `CzPttTransfers.transfers` (22–67).
- **Why:** NATIONAL_CZPTT.md (linked trips type 4; no invented walking transfer across stations).
- **Pinned by:** `Category changes split linked trips…`, `Line change creates linked trips…`,
  `Alternative transport segments…`, `Untimed passenger boundary copies a timed same-station
  counterpart`, `Cross-station NAD boundaries do not emit walking transfers`, `NAD runs without two
  passenger calls are omitted` (type 4 across the dropped NAD).
- **Constants:** transfer types `4`, `1`; `0` s.
- **Diagnostic:** `"{pa}: omitted cross-station NAD transfer {fromTrip} ({fromIdentity}) ->
  {toTrip} ({toIdentity})"` (identity = `esc(country):esc(code)`) →
  `sidecar_boundary_approximations`.
- **Proposed verdict:** port; type 4 only when the boundary stop is shared (B11).

#### CZPTT-R083 — Cross-PA NAD transfers
- **What:** stations eligible = station of the first passenger call of a NAD journey that is the
  PA's first retained journey, and of the last passenger call of a NAD journey that is the PA's
  last. Candidates = passenger calls of non-NAD journeys (all PAs) at eligible stations, indexed by
  `(station, lineCode)` and `(station, int64(trainNumber))` (non-numeric numbers not indexed).
  For each NAD journey with passenger calls:
  - **before** (only if it is the PA's first journey and its first passenger call is not `0029`):
    `t = first.dep ?? arr`; candidate time = `arr ?? dep`; `gap = t − candidateTime`; allowed iff
    the candidate call is not its journey's first call and not `0028`.
  - **after** (only if it is the PA's last journey and its last passenger call is not `0028`):
    `t = last.arr ?? dep`; candidate time = `dep ?? arr`; `gap = candidateTime − t`; allowed iff
    the candidate call is not its journey's last call and not `0029`.
  - eligible = candidates of another PA, allowed, timed, `0 ≤ gap ≤ 600`, and calendars overlap
    (some date active in both bitmaps). Number candidates (only if the NAD train number ≥ 300000,
    looked up as `number − 300000`) are used if any is eligible; otherwise line candidates
    (`(station, NAD call's lineCode)`).
  - closest selection: per candidate trip keep the minimum gap; sort by `(gap, tripId)`; keep a
    candidate unless another trip with `otherGap ≤ gap` has an overlapping calendar with it
    (equal gaps with overlapping calendars eliminate each other).
  - each kept candidate → `transfer_type=1`, `min_transfer_time=0`, stops and trips (rewritten by
    R084). Results distinct. Times of different PAs are compared directly in seconds (their day
    frames are not aligned).
- **Where:** `CzPttTransfers.calendarsOverlap`, `trainNumberValue`, `crossPaTransfers`
  (75–293).
- **Why:** test names (NAD bus published as a separate PA connects to the continuing train);
  NAD numbers are the train number + 300000.
- **Pinned by:** `Separate NAD trip matches a continuing train by passenger line`, `Separate NAD
  trip matches by the 300000 train-number prefix`, `Disjoint calendar variants each connect to the
  same NAD trip`, `Separate NAD trip connects onward to a train by passenger line`.
- **Constants:** `600` → `czptt.transfers.nad_max_gap_s = 600`; `300000` →
  `czptt.transfers.nad_train_number_offset = 300000`.
- **Diagnostic:** none.
- **Proposed verdict:** port (this is evidence-based inference: it belongs in a pass with its own
  rule IDs, not in the compiler core).

#### CZPTT-R084 — Transfer rewrite for MOTIS
- **What:** every transfer with `transfer_type=1` and both stops becomes `transfer_type=2`,
  `min_transfer_time=0`, stops kept, **trip ids removed**. Then distinct, sorted by
  `(fromTripId, toTripId, fromStopId, toStopId)` (absent first).
- **Where:** `CzPttToGtfs.convert` (155–180).
- **Why:** code comment: MOTIS applies its default transfer time unless transfers.txt has a
  stop-pair minimum and ignores trip specificity; the synthetic BUS point keeps it NAD-only.
- **Pinned by:** `Alternative transport segments…`, `Separate NAD trip matches…` (×2),
  `Disjoint calendar variants…`, `Separate NAD trip connects onward…`.
- **Constants:** types `1` → `2`, `0` s.
- **Diagnostic:** none.
- **Proposed verdict:** replace: the model keeps the precise trip-to-trip fact; the consumer-
  specific stop-pair form is a `jr-publish` decision.

#### CZPTT-R085 — IDS zone memberships (CZIPTS)
- **What:** for each accepted PA, each root `CZIPTS` value: < 5 fields → diagnostic, skipped.
  Else `code = f0`; endpoints via `resolveIntervalEndpoint(f1, parseOrdinal f2)` and `(f3, f4)`:
  code normalised by dropping the first two characters only if length > 2 and both are letters
  (no trim, no case change), matched exactly against `LocationPrimaryCode` of all PA calls
  (country ignored); occurrence blank → only if exactly one match; integer `n` with `1 ≤ n ≤
  matches` → `matches[n−1]` (**1-based**); non-integer → `-1` → unresolved. Catalog record =
  catalog `ids` with that code whose validity contains the PA's `StartDateTime` date, greatest
  `valid_from`. If both endpoints, `first ≤ last` and a record: zone code = `note` matched by
  `(?i)\bpásmo\s+(.+?)\s*$` (group 1 trimmed); no match → no rows and **no diagnostic** (coverage
  only). Otherwise one row per (call in `[first, last]` that is in a retained journey, generated
  trip containing it): `tripId`, `stopSequence = idx+1`, `zoneId = czptt:ids:{code}`, `zoneCode`,
  `idsSystemId` (R086), provenance `czptt:CZIPTS:{code}:{first+1}-{last+1}:calendar={f5 or
  "journey"}`. The calendar `f5` does **not** restrict the membership. Rows distinct, sorted by
  `(tripId, stopSequence, zoneId)`. Any unresolved endpoint or missing/invalid catalog record →
  diagnostic.
- **Where:** `CzPttAgencies.validIdsRecord`, `fareZone`, `resolveIntervalEndpoint`,
  `parseOrdinal` (62–105); `CzPttToGtfs.convert` (417–475).
- **Why:** NATIONAL_CZPTT.md ("Entries whose catalog note explicitly identifies a fare band (for
  example `PID pásmo P`) additionally produce … zone memberships").
- **Pinned by:** `Explicit IDS fare band creates trip-stop zones`, `Overlapping IDS bands remain
  lossless and do not set stops zone_id`.
- **Constants:** regex → `czptt.ids.fare_band_pattern = "(?i)\\bpásmo\\s+(.+?)\\s*$"`;
  `"journey"` default calendar label.
- **Diagnostic:** `"{pa}: malformed CZIPTS value {v}"`, `"{pa}: unresolved CZIPTS interval {v}"`
  → `ids_diagnostics`.
- **Proposed verdict:** port the intent with the single interval resolver and calendar-aware
  membership (B17, section 8).

#### CZPTT-R086 — IDS system id
- **What:** `abbreviation.Split('_')`: more than one part → part 0; else the whole abbreviation
  (`PID_PrahaP` → `PID`).
- **Where:** `CzPttAgencies.catalogIdsSystemId` (80–83); copy `CzPttBundle.idsSystem` (112–117).
- **Why:** unknown (KADR abbreviation convention).
- **Pinned by:** `Explicit IDS fare band creates trip-stop zones`.
- **Constants:** `'_'`.
- **Diagnostic:** none.
- **Proposed verdict:** port (one implementation).

#### CZPTT-R087 — stops.zone_id only when unanimous
- **What:** for each child stop id used by stop times: every `(trip, sequence)` call at that stop
  must have exactly one zone and all those zones must be equal → `zone_id` = that zone; otherwise
  none. Stations never get a zone. Computed before coordinate-based drops (R102).
- **Where:** `CzPttToGtfs.convert` (476–505, 536–539).
- **Why:** NATIONAL_CZPTT.md.
- **Pinned by:** `Explicit IDS fare band creates trip-stop zones`, `Overlapping IDS bands remain
  lossless and do not set stops zone_id`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port (as a `jr-publish` projection of model zone facts).

#### CZPTT-R088 — Czech extension tables
- **What:** `cz_routes`: `publicLineNumber` = route short name iff some journey of that route has
  a mapped KADR line, `cisLineId` none, provenance `czptt`. `cz_trips`: `trainNumber` (R057, raw),
  `sourceTripIds = "PA={paId}|TR={trId}"`, `coverageSources = "czptt"`, CIS ids none.
  `cz_trip_stop_zones` = R085 rows.
- **Where:** `CzPttToGtfs.convert` (190–215, 552–568).
- **Why:** NATIONAL_CZPTT.md (PA/TR reach the package as `source_trip_map` bindings).
- **Pinned by:** `Fallback routes use train designations…`, `Unknown catalog lines use the train
  designation fallback`.
- **Constants:** `czptt`.
- **Diagnostic:** none.
- **Proposed verdict:** replace: carry PA id, TR id, train number, public line number as model
  fields; no pipe-delimited strings (AGENTS rule 6).

### Stage 7 — coordinates

#### CZPTT-R089 — Name evidence per point
- **What:** for each `(country, code)`, the set of `normalizedName(call.name)` (call name =
  R053 name, i.e. SR70 or source) over all accepted operational calls, blanks removed.
- **Where:** `CzPttBundle.convert` (184–192).
- **Why:** OSM name matching.
- **Pinned by:** `OSM nodes resolve PLC and name matches`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### CZPTT-R090 — Name normalisation
- **What:** replace `ß→ss`, `ẞ→SS`, `Ł→L`, `ł→l`, `Ø→O`, `ø→o`, `Æ→AE`, `æ→ae`; Unicode NFD;
  remove `NonSpacingMark` characters; replace each run of characters that are not letters or
  digits (`[^\p{L}\p{N}]+`) with one space; trim; `ToUpperInvariant`.
- **Where:** `CzPttCoordinates.normalizedName` (152–170).
- **Why:** cross-border names with diacritics.
- **Pinned by:** `PerformanceEquivalenceTests.CZPTT name normalization matches the previous
  implementation`, OSM tests.
- **Constants:** transliteration table → `data/names/transliteration.csv`.
- **Diagnostic:** none.
- **Proposed verdict:** port (the one name normaliser, AGENTS rule 5).

#### CZPTT-R091 — Railway name core
- **What:** remove a trailing `\s*\([^)]*\)\s*$` from the raw name; normalise (R090); split on
  spaces; strip trailing tokens while the token is in {`BF`, `FBF`, `GR`, `HP`, `HST`, `N`, `NZ`,
  `PZS`, `S`, `ST`, `Z`, `ZAST`, `ZASTAVKA`} (the normalised forms of `Bf Fbf Gr Hp Hst N Nz Pzs S
  St Z Zast Zastavka`) or matches `^R[0-9]+$`; if nothing remains → `normalizedName(raw)`. On the
  expected (timetable) side the input is an **already normalised** name (R089), so the
  parenthesis rule never fires there (B6).
- **Where:** `CzPttCoordinates.railwayNameCore` (179–191); `CzPttBundle.coreNameCandidates`,
  `fuzzyNameCandidates`.
- **Why:** test name "railway suffix" (`Unterretzbach z`, `Bärenstein (Annaberg)`).
- **Pinned by:** `Unknown-country OSM names allow railway suffix and fuzzy matching`,
  `PerformanceEquivalenceTests.CZPTT name normalization matches…`.
- **Constants:** suffix list → `data/names/railway_suffix_tokens.txt`; route-section pattern
  `^R[0-9]+$`.
- **Diagnostic:** none.
- **Proposed verdict:** port with symmetric application.

#### CZPTT-R092 — Name similarity
- **What:** `editSimilarity(a, b)` = 1 if equal; 0 if either empty; else `1 − levenshtein(a, b)
  / max(len a, len b)` (UTF-16 code units). `nameSimilarity` = max of similarity on full
  normalised names, on cores (R091), and on cores with spaces removed. The bounded variant returns
  the same value or "below minimum".
- **Where:** `editSimilarity`, `prepareName`, `preparedNameSimilarityAtLeast`,
  `preparedNameSimilarity`, `nameSimilarity` (193–264).
- **Why:** fuzzy matching of abbreviated names (`Głuchołas.F-ka Mebli`).
- **Pinned by:** `PerformanceEquivalenceTests.CZPTT prepared fuzzy matching matches the previous
  implementation`, `Unknown-country OSM names allow railway suffix and fuzzy matching`.
- **Constants:** none (threshold in R094).
- **Diagnostic:** none.
- **Proposed verdict:** port (one implementation; count chars, not UTF-16 units).

#### CZPTT-R093 — Protected qualifier tokens (fuzzy only)
- **What:** protected tokens = normalised `Ost West Nord Süd Mitte východ západ sever jih juh
  střed Wschód Zachód Północ Południe Górna Dolna město mesto miasto centrum`. A fuzzy candidate
  name is compatible with an expected name iff every protected token of the expected name occurs
  among the candidate's tokens exactly or as a prefix of a candidate token. Not applied to PLC,
  exact or core matching.
- **Where:** `protectedFuzzyNameTokens`, `requiredQualifierTokens`, `preparedQualifierCompatible`
  (266–294).
- **Why:** NATIONAL_CZPTT.md: "`Bad Schandau Ost` cannot match an OSM candidate named only `Bad
  Schandau`".
- **Pinned by:** `Fuzzy matching preserves Ost and estimated GTFS names are marked`.
- **Constants:** → `data/names/protected_qualifier_tokens.txt`.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### CZPTT-R094 — OSM candidate groups in authority order
- **What:** groups tried in order, first group with a plausible pick wins: `ref_eu_plc` (OSM
  nodes whose PLC = upper(`country + code`)); `normalized_exact_name` (OSM nodes with a name whose
  R090 form equals one of the point's names); `normalized_railway_name` (R091 cores equal);
  `normalized_fuzzy_name` (all OSM nodes with best compatible similarity ≥ `0.58` over all name
  pairs, sorted by `(−score, objectId)`, first `32`). The fuzzy score is used only for this cut,
  not for choosing among survivors.
- **Where:** `CzPttBundle.convert` (197–308).
- **Why:** NATIONAL_CZPTT.md coordinate authority order (steps 2, 4, 5, 6).
- **Pinned by:** `OSM nodes resolve PLC and name matches`, `Unknown-country OSM names allow
  railway suffix and fuzzy matching`, `Invalid and conflicting SR70 identities fall back to exact
  OSM PLC`.
- **Constants:** `0.58` → `czptt.coordinates.fuzzy_min_similarity = 0.58`; `32` →
  `czptt.coordinates.fuzzy_max_candidates = 32`; method names (section 5).
- **Diagnostic:** see R095/R096.
- **Proposed verdict:** port (as a coordinate pass with its own rule IDs).

#### CZPTT-R095 — Corridor plausibility veto
- **What:** for a candidate at `c` and each occurrence of the point in any accepted PA
  (operational calls, all positions): edges to the previous/next call whose point already has a
  selected coordinate. Edge `(a → b)` is evidence only if `dep(a) ?? arr(a)` and `arr(b) ??
  dep(b)` exist and the latter is **strictly** greater; it is plausible iff
  `haversine ≤ 2000 + Δs / 3600 × 150000` metres. An occurrence is plausible iff all its evidence
  edges are; occurrences without evidence are ignored. The candidate is plausible iff there is no
  evidence at all or at least one occurrence is plausible. Implausible candidates are recorded and
  the group continues with the rest.
- **Where:** `edgePlausibility`, `candidatePlausible`, `trySelectCandidate` (313–411).
- **Why:** NATIONAL_CZPTT.md ("vetoed only when every occurrence with timed selected neighbors
  would require more than 150 km/h plus 2 km slack").
- **Pinned by:** `OSM name matching rejects wrong country ambiguity and corridor breaks`, `One
  anomalous timing occurrence does not veto an otherwise plausible name match`, `Invalid and
  conflicting SR70 identities fall back to exact OSM PLC`.
- **Constants:** `2000` → `czptt.coordinates.corridor_slack_m = 2000`; `150000 m/h` →
  `czptt.coordinates.corridor_speed_kmh = 150`.
- **Diagnostic:** `corridorRejectedOsmCandidates` entries `"{country}:{code}:{method}:{objectId}"`.
- **Proposed verdict:** port.

#### CZPTT-R096 — Ambiguous OSM candidates are ranked, not quarantined
- **What:** more than one plausible candidate in a group → diagnostic, then the minimum by
  `(−evidenceCount, countryHint, totalDistance, objectId)` is **selected**: evidence count =
  number of selected neighbour calls over all occurrences, total distance = sum of haversine
  distances to them, country hint `0` if the candidate country equals the point's, `1` if unknown,
  `2` if different; object id ordinal.
- **Where:** `candidateCorridorFit`, `trySelectCandidate` (363–432).
- **Why:** NATIONAL_CZPTT.md (country tags are ranking hints).
- **Pinned by:** `OSM name matching rejects wrong country ambiguity and corridor breaks`.
- **Constants:** none.
- **Diagnostic:** `ambiguousOsmCandidates` entries `"{country}:{code}:{method}:{id1,id2,…}"`.
- **Proposed verdict:** unsure. AGENTS forbids silently guessing a coordinate; this guesses with a
  diagnostic. Either port with an explicit "accept ranked ambiguity" policy flag or quarantine
  (Q11).

#### CZPTT-R097 — SR70 authority and OSM disagreement
- **What:** for every point of accepted PAs with country `CZ` and an SR70 coordinate (R018–R020),
  the coordinate is fixed: source `sz_sr70`, object `sz-sr70:{code}`, method
  `country_primary_code`; it is never re-evaluated. For each OSM node whose PLC equals `CZ{code}`
  and lies more than `1` m away, a disagreement is recorded. Non-CZ points never use SR70 even
  with the same code.
- **Where:** `CzPttBundle.convert` (439–469).
- **Why:** NATIONAL_CZPTT.md "SR70 is always retained when available"; code comment "SR70 is
  authoritative for every point on a path, passenger stop or not".
- **Pinned by:** `Authoritative SR70 survives agreeing and closer disagreeing OSM PLC`, `A
  railway point without passenger service takes its SR70 coordinate over an OSM name match`, `SR70
  coordinates never leak to a foreign point with the same code`.
- **Constants:** `1` m → `czptt.coordinates.sr70_osm_disagreement_m = 1.0`; `"CZ"` →
  `czptt.sr70.country = "CZ"`.
- **Diagnostic:** `osmSr70Disagreements` (`{sourceLocationId, retainedSource="sz_sr70",
  candidateObjectId, distanceMeters}`); `authoritativeSr70Resolutions`.
- **Proposed verdict:** port.

#### CZPTT-R098 — Resolution order and re-evaluation
- **What:** resolution order = distinct points in order of first appearance over accepted PAs
  (PA id order, then source sequence). Pass 1: each point without a coordinate is resolved
  (R094–R096) against the coordinates selected so far (earlier picks constrain later ones; the
  first point of a corridor has no neighbours and is accepted unconditionally). Then 2 more
  passes: each non-SR70 point is removed and re-resolved against all other current picks (may
  change or lose its coordinate). Rejection/ambiguity diagnostics accumulate over all passes.
- **Where:** `CzPttBundle.convert` (433–490).
- **Why:** code comment: "Re-evaluate with neighboring OSM choices now available. A rejected
  primary candidate may be replaced by an alias or exact-name candidate."
- **Pinned by:** `Invalid and conflicting SR70 identities fall back to exact OSM PLC` (shows
  order dependence), OSM tests.
- **Constants:** `2` → `czptt.coordinates.reevaluation_passes = 2`.
- **Diagnostic:** as R095/R096.
- **Proposed verdict:** port the intent; consider an order-independent formulation (B30, B13).

#### CZPTT-R099 — Estimation eligibility
- **What:** only points with at least one **passenger** call that belongs to a generated trip
  ("passenger-referenced") and no real coordinate are estimated. Timing-only points are never
  estimated.
- **Where:** `CzPttBundle.convert` (508–513, 619–623).
- **Why:** NATIONAL_CZPTT.md ("A purely timing/operational identity is never route-estimated").
- **Pinned by:** `Unresolved timed-only points are retained only in operational Parquet`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### CZPTT-R100 — Route estimate
- **What:** for each occurrence of an eligible point with a time (`arr ?? dep`): anchors are the
  nearest earlier and later calls of the same PA having a **real** coordinate and a time; computed
  twice, once requiring passenger anchors and once not. Both anchors and `tR > tL` → `route_time`:
  ratio = clamp(`(t − tL) / (tR − tL)`, 0, 1), latitude and longitude interpolated linearly in
  degrees; `callSpan = iR − iL`; `anchorDistance = haversine(L, R)`. Only one anchor →
  `route_end_north`: latitude = anchor.lat + `300 × |Δindex| / 111320`, longitude = anchor.lon,
  `callSpan = |Δindex|`, distance `0`. Both anchors but `tR ≤ tL` → no candidate for that tier.
  Tiers: passenger two-sided `0`, any two-sided `1`, passenger one-sided `2`, any one-sided `3`.
  Winner = minimum `(tier, callSpan, anchorDistance, −serviceDays, paId, sourceSequence)`,
  serviceDays = count of `'1'` in the PA bitmap. Result: source `estimated`, no object id, method
  `route_time` or `route_end_north`.
- **Where:** `estimateCandidates`, `estimates` (500–657).
- **Why:** NATIONAL_CZPTT.md (deterministic ranking; the 300 m-per-call northward fallback).
- **Pinned by:** `Dense passenger corridor wins independent of input order`, `SR70 coordinates
  never leak to a foreign point with the same code` (`route_end_north`), `Fuzzy matching
  preserves Ost…` (`route_time`), `OSM name matching rejects wrong country…`.
- **Constants:** `300` → `czptt.coordinates.route_end_offset_m_per_call = 300`; `111320` →
  `geo.metres_per_degree_latitude = 111320` (or compute with `jr_model::geo`).
- **Diagnostic:** `estimatedResolutions`.
- **Proposed verdict:** port the estimate; the north offset is a placeholder convention worth
  re-deciding (Q12).

#### CZPTT-R101 — Estimates are never anchors
- **What:** estimates are computed from the real-coordinate map only and merged afterwards; an
  estimate never anchors another estimate.
- **Where:** `CzPttBundle.convert` (526–530, 654–657).
- **Why:** NATIONAL_CZPTT.md ("estimates are never averaged or reused as anchors").
- **Pinned by:** `Estimated coordinates never become anchors`.
- **Constants:** none.
- **Diagnostic:** remaining points in `unresolvedPassengerPointIds`.
- **Proposed verdict:** port.

#### CZPTT-R102 — Drop unresolved timing-only points from GTFS
- **What:** unresolved points that are not passenger-referenced: their station/child stop ids are
  removed from `stop_times` (and therefore from `stops`, unless still referenced),
  `cz_trip_stop_zones` rows for removed calls are dropped, and their operational calls lose their
  generated trip ids (no `source_call_metadata` rows). The trip is not re-validated: it may lose a
  segment edge or fall below two stop times.
- **Where:** `CzPttBundle.convert` (754–813).
- **Why:** NATIONAL_CZPTT.md (`gtfs` mode: only with a real coordinate).
- **Pinned by:** `Unresolved timed-only points are retained only in operational Parquet`.
- **Constants:** none.
- **Diagnostic:** `unresolvedPointIds`, `unresolvedByCountry`.
- **Proposed verdict:** port; re-check trip validity afterwards (B12).

#### CZPTT-R103 — Apply coordinates and the ` [?]` marker
- **What:** each retained stop whose point (child via its call, BUS child via its parent) has a
  coordinate gets `stop_lat/stop_lon = decimal(double)`. Estimated → name gets `" [?]"` appended
  once (not if already ending so). Applies to parent, child and BUS child; headsigns, route labels
  and sidecar names stay unmarked.
- **Where:** `CzPttBundle.convert` (159–180, 784–801); `Gtfs.markApproximateStopName`.
- **Why:** NATIONAL_CZPTT.md ("Estimated station parents and children receive one ` [?]`
  suffix").
- **Pinned by:** `Fuzzy matching preserves Ost and estimated GTFS names are marked`, `CZPTT bundle
  writer emits all operational Parquet sidecars` (lat/lon).
- **Constants:** `" [?]"` → `data/czptt/strings.toml` `approximate_stop_suffix`.
- **Diagnostic:** none.
- **Proposed verdict:** replace: the model carries `coordinate_precision = estimated`; the suffix
  is a `jr-publish` presentation decision.

#### CZPTT-R104 — Unresolved passenger points stay published
- **What:** an unresolved passenger-referenced point keeps its stops and stop times with empty
  coordinates; `fillStandardRequiredFields` later writes `0`/`0`. Listed in
  `unresolvedPassengerPointIds`. (Python fails the build only for non-`CZ` ones.)
- **Where:** `CzPttBundle.convert` (710–722); `Gtfs.fillStandardRequiredFields`;
  `national_czptt._verify_foreign_coordinate_acceptance`.
- **Why:** PROGRESS.md "foreign passenger CZPTT points without coordinates fail".
- **Pinned by:** `Estimated coordinates never become anchors`.
- **Constants:** none.
- **Diagnostic:** `unresolvedPassengerPointIds`.
- **Proposed verdict:** replace: quarantine with a diagnostic; never publish `0,0`.

### Stage 8 — sidecars

#### CZPTT-R105 — operational_points rows
- **What:** one row per `(country, code)` of accepted operational calls (all positions, including
  unresolved timing-only), representative = call with the ordinal-smallest name, sorted by
  `(country, code)`; coordinates nullable (R097–R101).
- **Where:** `CzPttBundle.convert` (851–890).
- **Why:** NATIONAL_CZPTT.md bundle layout.
- **Pinned by:** `CZPTT bundle writer emits all operational Parquet sidecars`, `SR70 names
  override…`, `Unresolved timed-only points are retained only in operational Parquet`, `A railway
  point without passenger service takes its SR70 coordinate…`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** replace: points are model entities; the name choice becomes R053's
  single rule.

#### CZPTT-R106 — operational_calls rows
- **What:** every call of every accepted PA, sorted by `(pa, sequence)`; times are the R028/R037
  values (mirrored and repaired), not raw.
- **Where:** `CzPttToGtfs.convert` (506–535); `CzPttBundle.convert` (892–921).
- **Why:** NATIONAL_CZPTT.md.
- **Pinned by:** `Untimed split edges are interpolated only in GTFS`, `Flagged backward passenger
  times…`, `Negative source-day offsets shift only GTFS times`, `Location line applicability
  clears in both directions` (raw line codes).
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port (with raw times, B19).

#### CZPTT-R107 — source_call_metadata rows
- **What:** one row per (operational call, generated trip id) after R102; `stop_sequence` always
  equals `source_sequence`; sorted by `(trip, stop_sequence)`.
- **Where:** `CzPttBundle.convert` (923–945).
- **Why:** projection bridge.
- **Pinned by:** `CZPTT bundle writer emits all operational Parquet sidecars` (`call_key` empty).
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** drop the redundant column; the bridge is the model's trip-call link.

#### CZPTT-R108 — source_note_metadata rows
- **What:** one row per (note, trip id), or one row with null trip for notes without trips;
  `valid_from/valid_to` written with F# `string` on a NodaTime `LocalDate` (default `"D"` pattern,
  invariant culture: long-date text, not ISO); sorted by `(note id, trip id or "")`.
- **Where:** `CzPttBundle.convert` (947–986).
- **Why:** NATIONAL_CZPTT.md.
- **Pinned by:** `CZPTT bundle writer emits all operational Parquet sidecars`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** replace (typed dates in the model; B20).

#### CZPTT-R109 — source_feature_metadata rows
- **What:** R077 + R078 features, sorted by feature id.
- **Where:** `CzPttBundle.convert` (988–1013).
- **Why:** NATIONAL_CZPTT.md.
- **Pinned by:** `CZPTT bundle writer emits all operational Parquet sidecars`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** replace (model features).

#### CZPTT-R110 — source_ids_coverage_metadata rows
- **What:** for accepted PAs, every root `CZIPTS`/`CZCalendarIPTS` pair in parameter order;
  `source_sequence` = 1-based position among those pairs; coverage id `{pa}:{n}`. `CZIPTS`:
  `source_code = f0`, `calendar_id = f5`, raw from/to code and occurrence `f1..f4`;
  `ids_system_id` and `coverage_role` from the **first** catalog `ids` record with the code
  (validity ignored), role `fare_zone` if its note matches `(?i)\bpásmo\s+.+\s*$` else `coverage`
  (unknown code → null). `CZCalendarIPTS`: `calendar_id = f0`, role `calendar`, other fields null.
- **Where:** `CzPttBundle.idsRole`, `idsSystem`, `convert` (1015–1081).
- **Why:** NATIONAL_CZPTT.md ("Every KADR IDS entry remains represented as coverage metadata").
- **Pinned by:** `CZPTT bundle writer emits all operational Parquet sidecars`.
- **Constants:** role strings; regex (variant of R085's).
- **Diagnostic:** none.
- **Proposed verdict:** port the facts; one catalog lookup with validity (section 8, B18).

#### CZPTT-R111 — source_ids_coverage_trip_metadata rows
- **What:** `CZCalendarIPTS` → every generated trip of the PA. `CZIPTS` (≥ 5 fields): matching
  calls = operational calls whose upper primary code equals `trim(upper(code))` or whose upper
  `country+code` equals it (country-aware); occurrence = integer > 0 else fallback (from → 1,
  to → number of matches), 1-based; `first.seq ≤ last.seq` → trips of calls in range, distinct,
  sorted; else none. Sorted by `(coverage id, trip id)`.
- **Where:** `CzPttBundle.convert` (1083–1152).
- **Why:** NATIONAL_CZPTT.md.
- **Pinned by:** none directly.
- **Constants:** none.
- **Diagnostic:** none (silent when unresolved).
- **Proposed verdict:** replace with the single interval resolver (section 8).

#### CZPTT-R112 — Parquet writer settings
- **What:** Snappy, row group 65536, metadata `obehy.schema_version=1`,
  `obehy.source_format=czptt`, `obehy.bundle_format=czptt-v1`, `obehy.table=<file stem>`,
  `obehy.row_count`.
- **Where:** `CzPttBundle.writeParquet` (67–84).
- **Why:** package contract.
- **Pinned by:** `CZPTT bundle writer emits all operational Parquet sidecars`.
- **Constants:** as listed.
- **Diagnostic:** none.
- **Proposed verdict:** drop if the sidecars disappear into the model (rule 1); otherwise
  `jr-publish`.

### Stage 9 — package

#### CZPTT-R113 — Service id deduplication
- **What:** services with identical `calendar_dates` sets are merged; the new id is
  `czptt:service:exc:{i}` with `i` = 0-based group index in order of the ordinal-sorted original
  service ids (`exc` because there is no `calendar.txt`). Trips are re-pointed.
- **Where:** `Gtfs.deduplicateCalendarWithPrefix` (137–211); `CzPttPackage.write` (59).
- **Why:** smaller calendar tables.
- **Pinned by:** none for CZPTT.
- **Constants:** prefix `czptt:service`.
- **Diagnostic:** none.
- **Proposed verdict:** replace: calendar dedup in `jr-publish` with content-derived, stable ids
  (B21).

#### CZPTT-R114 — Filler values for required GTFS fields
- **What:** agency without url → `jrutil://invalid` (never happens for CZPTT, R080); stop without
  coordinates → `0`/`0`.
- **Where:** `Gtfs.fillStandardRequiredFields` (117–134).
- **Why:** comment: "fill them with nonsense" for GTFS compliance.
- **Pinned by:** none for CZPTT.
- **Constants:** `jrutil://invalid`, `0`.
- **Diagnostic:** none.
- **Proposed verdict:** drop for CZPTT (R104 quarantines instead).

#### CZPTT-R115 — Diagnostics and manifest
- **What:** `diagnostics.json` object (section 5.2); manifest `{schema_version: 1,
  source_format: "czptt", source: {source_id: "national-czptt", payload_kind: "file"|"directory",
  payload_sha256}}`; `payload_sha256` = file SHA-256, or for a directory SHA-256 of the
  `"\n"`-joined, ordinal-sorted lines `"{relative path with /}={file sha256}"`.
- **Where:** `CzPttPackage.write` (63–96), `inputDigest` (31–39).
- **Why:** package contract.
- **Pinned by:** `CZPTT bundle writer emits all operational Parquet sidecars`.
- **Constants:** `national-czptt`.
- **Diagnostic:** n/a.
- **Proposed verdict:** replace (one diagnostics model with codes; manifest in `jr-publish`).

---

## 4. Identity

All built with `$"…"` interpolation; `esc` = `Uri.EscapeDataString`.

| Entity | Format | Inputs | Notes |
|---|---|---|---|
| PA id | `{ObjectType%O}:{Company}:{Core}:{Variant}:{TimetableYear}` → `Pa:54:000000001234:00:2026` | PA identifier | `%O` gives the C# enum name (`Pa`/`Tr`) |
| TR id | same → `Tr:54:000000001234:00:2026` | TR identifier | only in `cz_trips.source_trip_ids` |
| Python flatten identity | `"|".join(sorted("PA:…", "TR:…", related…))` | all Planned + Related identifiers, XML text | different from the F# PA id (casing, scope) |
| Station | `czptt:stop:{esc(country)}:{esc(primaryCode)}` | `CountryCodeISO`, `LocationPrimaryCode` | |
| Child (no subsidiary) | `{station}:unspecified` | | |
| Child (subsidiary) | `{station}:platform:{esc(LocationSubsidiaryCode)}` | subsidiary code (not name) | |
| NAD boarding point | `{station}:platform:BUS` | | **collides** with a real subsidiary coded `BUS` (R055) |
| Route (KADR line) | `czptt:route:line:{esc(line)}:{esc(RU)}:{esc(routeType)}` | line code, RU, route type | no date in the id |
| Route (fallback) | `czptt:route:fallback:{esc(RU)}:{esc(category)}:{esc(routeType)}:{esc(trainNumber ?? category)}` | | category is the abbreviation (e.g. `Os`) |
| Trip | `czptt:trip:{esc(paId)}:{segmentIndex+1}` → `czptt:trip:Pa%3A54%3A000000001234%3A00%3A2026:1` | PA id, segment index | gaps after NAD drop (R047) |
| Service (raw) | `czptt:service:{esc(paId)}:calendar:journey` | PA id | replaced by R113 |
| Service (published) | `czptt:service:exc:{i}` | group index | unstable across builds |
| Block | `czptt:block:{esc(paId)}` or `czptt:block:{esc(paId)}:mode:{k}` | PA id, mode block | |
| Agency | `czptt:agency:{esc(RU)}` | RU code | |
| Note | `czptt:note:{esc(paId)}:{central\|noncentral\|calendar}:{index+1}` | PA id, parameter position | |
| Note feature | `{noteId}:feature:{kind}:trip:{esc(tripId)}:{sequence\|trip}` | | trip id escaped twice (`%253A`) |
| Request feature | `{paId}:sequence:{n}:activity:0030:feature:on_request:trip:{esc(tripId)}` | | PA id **unescaped** here |
| Request feature source object | `{paId}:sequence:{n}:activity:0030` | | |
| Zone | `czptt:ids:{code}` | IDS code | code **unescaped** |
| Zone provenance | `czptt:CZIPTS:{code}:{first}-{last}:calendar={f5\|journey}` | | sequences 1-based |
| Source location id | `{country}:{primaryCode}` | | unescaped; sidecars and diagnostics |
| Coordinate object | `sz-sr70:{code}`, `osm:node:{id}` | | |
| IDS coverage id | `{paId}:{n}` | position among CZIPTS/CZCalendarIPTS | |
| Unresolved point ids | `czptt:stop:{esc(country)}:{esc(code)}` | | rebuilt inline in `CzPttBundle` (duplicate builder) |
| Source trip ids | `PA={paId}\|TR={trId}` | | pipe-delimited |
| Feed version | `czptt:{yyyyMMdd'T'HHmmss}` | max `CZPTTCreation` | |
| Diagnostic keys | `{country}:{code}:{method}:{objectId}`; `{country}:{code}:{method}:{id,id,…}` | | corridor / ambiguity lists |

Transfers, stop times and calendar dates have no ids.

### Places that parse an ID back

1. `CzPttBundle.convert` (172): `stop.id.EndsWith(":platform:BUS")` to find NAD boarding points and
   map them to their parent's point.
2. `Serving/PackageGtfsRelations.czpttParts` (44–55): splits `cz_trips.source_trip_ids` on `|`,
   takes the `PA=` token as the PA id; the part number is the trip id's suffix after the **last**
   `:` parsed as `Int16`.
3. `Serving/PackageGtfsRelations.operationalPaths` (66), `pointSources` (105),
   `operationalLocations` (151–155): rebuild `"czptt:stop:" + source_location_id` (unescaped) and
   expect it to equal the escaped station id.
4. `Serving/PackageKeyRelations` (111–118): splits `source_trip_ids` into `PA=`/`TR=` tokens →
   `czptt:pa` / `czptt:tr` source keys (values `Pa:…` / `Tr:…`); `train_number` →
   `czptt:train_number` (schema pattern `^[0-9]+$`).
5. `Serving/PackageKeyRelations.railKeys` (144–161): `"czptt:stop:" + source_location_id` for CZ
   5-digit codes → `sr70` keys; for child stops whose parent is such a station and whose
   `platform_code` is non-empty and ≠ `BUS` → `sr70:track` key `{code}:{platform_code}`
   (platform_code = subsidiary **name** when present, not the code in the id).
6. `Serving/PackageKeyRelations.pathCalls` (229–232): compares `stop_sequence` with
   `source_sequence` (always equal, so empty).
7. `Serving/Validation.fs` (226): feed prefix `czptt:` required.
8. Python `national_czptt._czptt_stop_identity` (809–819): strips `czptt:stop:`, splits on `:`,
   URL-unquotes country and code; `_verify_gtfs_stops` checks parent/child structure, equal names
   and SR70 coordinates; `_verify_foreign_coordinate_acceptance` reads
   `unresolvedPassengerPointIds` and treats ids not starting `czptt:stop:CZ:` as foreign.

All eight conflict with AGENTS rule 6; each needs a carried field instead.

---

## 5. Output facts

### 5.1 Parquet sidecars (scratch directory → package `sidecars`)

| Table | Columns (type, nullable) | Meaning |
|---|---|---|
| `operational_points` | `source_location_id` (str), `country_code` (str), `primary_code` (str), `source_name` (str), `latitude` (double?), `longitude` (double?), `coordinate_source` (str?), `coordinate_source_object_id` (str?), `coordinate_match_method` (str?) | one row per railway point of accepted PAs |
| `operational_calls` | `source_pa_id`, `source_sequence` (int), `source_location_id`, `passenger_call` (bool), `arrival_seconds` (int?), `departure_seconds` (int?), `subsidiary_code` (str?), `subsidiary_name` (str?), `active_line_code` (str?) | complete PA paths; seconds from local midnight of the bitmap day, may be negative or > 86400 |
| `source_call_metadata` | `gtfs_trip_id`, `stop_sequence` (int), `source_pa_id`, `source_sequence` (int) | trip-call → PA-call bridge |
| `source_note_metadata` | `source_note_id`, `source_pa_id`, `note_kind`, `source_code`?, `gtfs_trip_id`?, `label`?, `raw_value`, `valid_from`?, `valid_to`?, `resolved` (bool), `from_sequence` (int?), `to_sequence` (int?) | notes × trips |
| `source_feature_metadata` | `source_feature_id`, `gtfs_trip_id`, `call_sequence` (int?), `source_code`, `feature_kind`, `note_id`?, `source_object_id` | trip/call features |
| `source_ids_coverage_metadata` | `source_coverage_id`, `source_pa_id`, `source_sequence` (int), `record_type`, `source_code`?, `ids_system_id`?, `coverage_role`?, `from_location_code`?, `from_occurrence`?, `to_location_code`?, `to_occurrence`?, `calendar_id`?, `source_value` | IDS facts |
| `source_ids_coverage_trip_metadata` | `source_coverage_id`, `gtfs_trip_id` | IDS → trips |

Enumerated strings:

- `coordinate_source`: `sz_sr70`, `osm`, `estimated`.
- `coordinate_match_method`: `country_primary_code`, `ref_eu_plc`, `normalized_exact_name`,
  `normalized_railway_name`, `normalized_fuzzy_name`, `route_time`, `route_end_north`.
- `coordinateDiagnostics.resolutionMethod`: `sr70-authoritative-then-osm-then-route-estimate`
  (`not-applied` from `CzPttToGtfs.convert` alone).
- `note_kind`: `czptt_central_note`, `czptt_local_note`, `czptt_calendar_note`.
- `feature_kind`: the R077 kinds and `on_request`.
- `record_type`: `CZIPTS`, `CZCalendarIPTS`; `coverage_role`: `fare_zone`, `coverage`, `calendar`.
- Boundary adjustment `reason`: `coalesced-with-operator-boundary`,
  `deferred-to-next-passenger-call`, `ignored-after-final-passenger-call`.
- Rejection `reason`: `no activity 0001 passenger call`, `departure precedes arrival`,
  `event precedes previous event`, `fewer than two selected GTFS calls`.

Serving consequence: `PackageRows.coordinateSource` maps `sz_sr70`→`sr70`, `osm`→`osm`, but
`estimated` matches none of its patterns and becomes `catalogue`; and since CZPTT has no
`source_stop_metadata`, GTFS-stop locations get `coordinate_precision = exact` even when
estimated (B1).

### 5.2 `diagnostics.json` (schema 1)

Keys: `schema_version` (1), `operational_points` (`gtfs`|`sidecar`), `accepted_pa_count`,
`rejected_journeys` (`{paId, reason, sequence, previousSeconds, currentSeconds}`),
`cancelled_pa_ids`, `sidecar_boundary_approximations` (free-text: R037, R050, R051, R082
messages mixed), `line_boundary_adjustments` (`{paId, sourceSequence, appliedSequence, reason}`),
`ids_diagnostics` (free-text: R073–R075, R079, R085), `merge_diagnostics` (R014),
`coordinate_diagnostics` (`resolutionMethod`, `resolvedPointCount`, `resolvedStopCount`,
`unresolvedByCountry[{countryCode, pointCount, stopCount}]`, `unresolvedPointIds`,
`unresolvedPassengerPointIds`, `conflictingSr70Codes`, `authoritativeSr70Resolutions[]`,
`osmGapFills[]`, `estimatedResolutions[]` (each `{sourceLocationId, countryCode, primaryCode,
coordinateSource, coordinateSourceObjectId, coordinateMatchMethod}`), `osmSr70Disagreements[]`,
`invalidSr70Identities`, `ambiguousOsmCandidates`, `corridorRejectedOsmCandidates`).
Python reads `coordinate_diagnostics.unresolvedPassengerPointIds` (falls back to
`unresolvedPointIds`).

### 5.3 Non-standard content in the GTFS tables

- ` [?]` stop-name suffix for estimated points (R103).
- NAD boarding point child: `platform_code = BUS`, fixed Czech `stop_desc` (R054).
- `route_color`/`route_text_color` (R061), extended route types `100`, `102`, `103`, `105`,
  `106`, `714`.
- `transfers.txt` types `2` (stop-pair, 0 s, NAD) and `4` (in-seat linked trips) (R082, R084).
- `timepoint=0` for inferred and untimed calls (R068).
- `stops.zone_id = czptt:ids:{code}` only when unanimous (R087).
- Czech tables (compiler-internal, not in `gtfs.zip`): `cz_routes` (`route_id`,
  `public_line_number`, `source_provenance`), `cz_trips` (`trip_id`, `train_number`,
  `source_trip_ids`, `coverage_sources`), `cz_trip_stop_zones` (`trip_id`, `stop_sequence`,
  `zone_id`, `zone_code`, `ids_system_id`, `source_provenance`).

### 5.4 What the model must carry (derived from the above)

Per point: country, primary code, source names, SR70 name, coordinate with source, object id,
method and precision. Per call: PA, source sequence, raw arrival/departure options, repaired times
and repair flag, inferred-edge flag, passenger flag, activities, subsidiary code and name, raw and
projected line code, RU, category, NAD flag. Per trip part: PA id, TR id, part number, train
number, mode block, KADR line (with validity), public line number. Notes (kind, code, label, raw,
interval, calendar, resolution), features (kind, scope, source code, note), IDS coverage facts and
zone memberships (with calendar), transfers as trip-to-trip facts, per-entity diagnostics.

---

## 6. Known or probable bugs

- **B1 — Estimated coordinates are published as exact catalogue coordinates.** Evidence:
  `PackageRows.coordinateSource` checks `osm`, `route-time`, `route-end`, `sr70`, `gapfill`,
  `overlay`; CZPTT writes `estimated` (method `route_time`/`route_end_north`, underscores), so the
  serving `coordinate_source` is `catalogue`; `coordinate_precision` falls back to `exact`
  because CZPTT has no `source_stop_metadata` (`PackageGtfsRelations` 124–126). Not pinned.
- **B2 — Three incompatible interval resolvers.** Note endpoints use a 0-based occurrence
  (pinned: `Partial repeated-location…` treats `1` as the second match), CZIPTS zones a 1-based
  one, the IDS trip projection a 1-based one with first/last fallbacks; two ignore the country.
  At most one can match SŽ's encoding (section 8, Q3).
- **B3 — Negative day offsets probably place the trip one day late.** `gtfsTimeShift` adds whole
  days to the times but leaves the bitmap dates unchanged, so a call at offset −1 23:58 is
  published at 23:58 on the bitmap day instead of the day before. Pinned by `Negative source-day
  offsets shift only GTFS times` (which checks times only). Correct only if the bitmap day is not
  the offset-0 day (Q2).
- **B4 — Machine-timezone dependence.** XmlSerializer converts `xs:dateTime` values carrying an
  offset to the machine's local time; `LocalDate.FromDateTime(StartDateTime)` and the
  `CZPTTCreation` feed version would then depend on the runner's timezone (Actions runs in UTC).
  Harmless if SŽ writes naive datetimes (the fixtures do); not verified on real data.
- **B5 — Subsidiary collapse handles pairs only** (R042): the merged call keeps the first
  sourceIndex, so the third of three consecutive subsidiary rows fails the adjacency check.
- **B6 — Asymmetric railway-core matching** (R091): the timetable side passes already-normalised
  names into `railwayNameCore`, so a trailing parenthetical is stripped on the OSM side only.
- **B7 — Unknown activity code aborts the run, order-dependently** (R030).
- **B8 — A cancellation extending outside the message validity is lost** (R015): `ExtendTo`'s
  assertion is compiled out, the index throws, and the merger logs and skips it; the timetable
  stays fully active. A bitmap/interval length mismatch drops the message the same way.
- **B9 — Directory input order is file-system order** (R009), so cancellation application can
  differ between machines for directory inputs (the production path uses one ZIP).
- **B10 — Notes of rejected PAs are published** (R076): `noteContexts` is filled before the
  rejection checks; they reach `service_note` with no assignment.
- **B11 — In-seat transfer between trips that do not meet** (R082): after a dropped NAD journey
  (`NAD runs without two passenger calls are omitted` pins one type-4 transfer between trips ending
  at `93001` and starting at `93002`) or an omitted journey, a `transfer_type=4` links different
  stops.
- **B12 — Coordinate-based drops do not re-validate trips** (R102): a `gtfs`-mode trip whose
  boundary is an unresolved timing-only point loses its edge call and can end up with one stop
  time; its transfers still reference it.
- **B13 — Stale coordinate diagnostics** (R098): `corridorRejected` and `ambiguous` accumulate
  across the three passes, including candidates later accepted or for points later resolved
  otherwise.
- **B14 — Sidecar operator diagnostic** (R051) reports only the first RU change and also changes
  outside the passenger window that move nothing.
- **B15 — Ambiguous OSM candidates are guessed** (R096), contrary to "never silently guess a
  coordinate" (diagnosed, but still chosen).
- **B16 — Fuzzy score ignored among plausible candidates** (R094/R096): a 0.59 match can beat a
  0.99 match on corridor evidence or object id.
- **B17 — CZIPTS calendars ignored** (R085): a calendar-limited IDS membership becomes a zone on
  every service date; `CZCalendarIPTS` is never interpreted.
- **B18 — IDS role/system ignore catalog validity** (R110) while the zone path honours it, so the
  coverage sidecar can call an entry `fare_zone` that produced no zone (or vice versa).
- **B19 — `operational_calls` are not raw** (R028, R037): mirrored ALA/ALD and repaired values,
  contrary to NATIONAL_CZPTT.md. Pinned by `Flagged backward passenger times…` (asserts the
  repaired value in operational calls).
- **B20 — Note validity dates as long-date text** (R108): `string LocalDate` uses NodaTime's
  `"D"` pattern; the serving reader only copes because `PackageRows.date` falls back to
  `DateTime.Parse`.
- **B21 — Unstable published service ids** (R113): `czptt:service:exc:{i}` renumbers when any PA
  is added or removed.
- **B22 — `sr70:track` keys use the platform name** (section 4 item 5): with
  `LocationSubsidiaryName = 12S` and code `12`, the key is `57076:12S` while the stop id says
  `platform:12`; the ServingContractTests assertion only holds when name = code.
- **B23 — Train-number fallback to the TR core** (R057): a 12-character core (pattern
  `[0-9A-Z*\-]{12}`) becomes the trip's train number, `trip_short_name` and the
  `czptt:train_number` key, whose schema pattern is `^[0-9]+$`.
- **B24 — Estimation gives up on inverted anchors** (R100): when both anchors exist but
  `tR ≤ tL`, no candidate is produced for that tier instead of falling back to one-sided.
- **B25 — SR70 parsing quirks** (R018/R019): one malformed row poisons a code with valid rows; a
  header row would be parsed as data; Python's verifier uses a different parse (skips bad rows, no
  range check), so the two can disagree about which codes have coordinates.
- **B26 — OSM `public_transport=station|stop_position` nodes are candidates** (R022), contrary to
  NATIONAL_CZPTT.md; stop positions are track-side points, not station coordinates.
- **B27 — One catalog version per PA** (R056, R085): line and IDS records are chosen at the PA's
  first validity date for all its dates.
- **B28 — First-wins attributes across PAs** (R053, R055, R060): stop names, platform codes and
  route names depend on PA-id order when sources disagree; deterministic but arbitrary and silent.
- **B29 — Changes at the terminal call are ignored** (R044): a category/RU/NAD change on the
  last call never shows. Possibly intended (line expiry test), unclear for NAD.
- **B30 — Order-dependent OSM selection** (R098): the first point in resolution order is accepted
  without neighbours and then vetoes its neighbours (`Invalid and conflicting SR70 identities…`
  shows `57076` accepted, `57016`'s correct PLC rejected and estimated instead).
- **B31 — Root-level `CZInconsistentTime` ignored** (R037), pinned by `CZInconsistentTime root
  parameter accepts monotonic source times`; NATIONAL_CZPTT.md says the PA is rejected.
- **B32 — Diagnostic text formats F# options** (R037: `arrival=Some(28800)`, `None` prints empty).
- **B33 — Cross-PA gaps compare unaligned day frames** (R083): a train at offset +1 and a NAD at
  offset 0 are compared in raw seconds while overlap is checked on bitmap days.
- **B34 — `on_request` features on non-passenger calls** (R078) whose stop times say "no
  service".
- **B35 — `Seq.minBy` name for operational points** (R105) picks the ordinal-smallest source name,
  which need not be the one used for the GTFS stop (R053 first-wins).

---

## 7. Code vs. NATIONAL_CZPTT.md

1. **Inconsistent time:** doc says a PA is rejected when `CZInconsistentTime=1` is present; code
   repairs flagged location calls (R037), ignores the root parameter, rejects only via chronology.
2. **Raw times:** doc says `operational_calls` retains the original nullable source times and "raw
   operational timing values remain unchanged"; code stores mirrored and repaired times (B19).
3. **Line expiry boundary:** doc says for `line → no line` the boundary is one source point
   earlier; code puts it at the first call without the line (`Location line applicability clears
   in both directions` asserts the line trip ends at `57016`, the first unlined call).
4. **Internal line change in sidecar mode:** doc says the boundary becomes the **preceding**
   passenger call and only in `sidecar` mode; code defers to the **next** passenger call (or
   coalesces with an RU boundary) in both modes (R040) and records it in
   `line_boundary_adjustments`, not as an approximation.
5. **Fallback routes:** doc says services without a dated KADR line are grouped by operator,
   category, mode and the unordered first/last passenger pair, with short names like
   `RJ Praha – Břeclav`, municipality-derived endpoint labels, a 20-character cap, `…stadt →
   …st.` abbreviations and a display direction by service days. None of this exists: the fallback
   route is one per RU/category/route type/train number with short name `RJ 01234` (R058/R059,
   pinned by `Fallback routes use train designations…` and `Missing Nazev20 gets a compact route
   label…`).
6. **Coordinate authority step 3** (reviewed source-identity-to-OSM alias) does not exist (removed
   per PROGRESS.md 2026-10-08).
7. **OSM eligibility:** doc says only `railway=station|halt|stop` nodes, "unrelated
   public-transport stop positions … discarded"; code also accepts `public_transport=station` and
   `public_transport=stop_position` (R022).
8. **Estimate tiers:** doc says operational anchors are considered only "if no passenger-anchor
   candidate exists"; code ranks two-sided operational anchors (tier 1) above one-sided passenger
   anchors (tier 2) (R100).
9. **Stop names:** doc says parent and child names "remain identical to the source station name";
   code uses the SR70 name for Czech points when unique (R053, pinned).
10. **Sidecar match methods:** doc lists `sz_sr70/country_primary_code`, `osm/ref_eu_plc`,
    `osm/normalized_exact_name`; code also emits `normalized_railway_name`,
    `normalized_fuzzy_name`, and `estimated` with `route_time`/`route_end_north`.
11. **Conflict identity:** doc says conflicting timetables with the same complete PA identity fail;
    Python checks the sorted set of all Planned + Related identifiers (R003), F# checks the PA alone
    (R012) on re-serialized XML.
12. **Default mode:** doc says `--operational-points sidecar` is the default; the JrUtil CLI
    default is `gtfs` (Python passes `sidecar`).
13. **Ambiguity:** doc's "Neighboring timetable calls can veto a name match" is implemented, but it
    is silent that ambiguous candidates are ranked and one chosen (R096).
14. **Unresolved passenger points:** doc implies every passenger-referenced location gets a real or
    estimated coordinate; code publishes unresolved ones at `0,0` (R104), and Python fails only for
    foreign ones.
15. **Calendar-limited IDS:** doc says fare bands "produce catalog-resolved trip/stop zone
    memberships"; it does not mention that `CZIPTS` calendars are ignored (B17).
16. **NAD transfers "only when both public endpoints have the same country and primary location
    code":** true for in-PA transfers; cross-PA transfers (R083) are not mentioned at all, nor the
    300000 number offset or the 600 s window.
17. **Category changes:** doc says category changes split trips like route/operator/mode changes;
    true, but a change at the final call does not split (R044).
18. **`Every KADR IDS entry remains represented as coverage metadata`:** only entries of accepted
    PAs (R110).

---

## 8. Duplicated logic

1. **Interval endpoint resolution** (keep one):

   | Copy | Code normalisation | Country | Occurrence base | Blank occurrence | Invalid occurrence | Candidate calls |
   |---|---|---|---|---|---|---|
   | `resolveNoteEndpoint` (notes) | trim, upper, drop 2 leading letters | ignored | 0-based | first match | unresolved | all normalized calls |
   | `resolveIntervalEndpoint` (CZIPTS zones) | drop 2 leading letters (no trim/upper) | ignored | 1-based | only if exactly one match | `-1` → unresolved | all normalized calls |
   | `coverageTrips` (IDS trip sidecar) | trim, upper; compare to code or country+code | honoured when prefixed | 1-based | from → first, to → last | fallback as blank | operational calls after R102 |

2. **Network-specific parameter pairing:** `CzPttNormalize.parameterPairs` and
   `CzPttBundle.parameterPairs` (identical; `nullOpt` vs `Option.ofObj`).
3. **PA id:** `CzPttMerger.getPaidStr`, `CzPttNormalize.paId`, `CzPttBundle.paId` (same format;
   `Seq.find` vs `Array.find`), plus the Python identity (R003) which differs.
4. **IDS catalog lookup:** `validIdsRecord` (validity, latest `valid_from`) vs `idsRole`/`idsSystem`
   (first record, no validity); fare-band regexes `\bpásmo\s+(.+?)\s*$` vs `\bpásmo\s+.+\s*$`;
   `catalogIdsSystemId` vs `idsSystem` (identical logic).
5. **Catalog validity window:** `lineForDate` and `validIdsRecord` (same filter/sort pattern).
6. **Calendar bitmap interpretation:** `parseCalendar` (Start..End-or-Start, length must match),
   `activeJourneyDates` and `calendarExceptions` (Start + index, End ignored), `calendarsOverlap`
   (Start + length), `serviceDaysByPa` (count of `1`), `feedInfo` (End or Start), note calendars
   (`yyyyMMdd` + bitmap, end ignored). One `DateSet` constructor should replace all.
7. **Time interpolation by source index:** `forceMonotonicFlaggedTimes` and `interpolateTiming`
   (same formula; different neighbour rules: repaired vs original calls, flagged neighbours
   excluded in one only, `span > 0` guard in one only). `route_time` estimation interpolates by time,
   not index (different concept, keep separate).
8. **Picking one time from a call:** `arr ?? dep` / `dep ?? arr` appears in `locationTimes`,
   `forceMonotonicFlaggedTimes`, `chronologyError`, `copyTiming`, `interpolateTiming`,
   `crossPaTransfers`, `CzPttBundle.departureTime`/`arrivalTime`/`callTime`.
9. **Name normalisation:** `normalizedName`, `railwayNameCore`, `prepareName` (one family, keep);
   separate ad-hoc normalisers: `compactLocationCode`, `publicAgencyName`, `cleanLine` (trim),
   `normalizedCountryCode`, Python `_sr70_coordinates`.
10. **SR70 parsing:** F# `loadSr70Coordinates` vs Python `_sr70_coordinates` (BOM handling,
    invalid-row handling, range check differ).
11. **Point display name fallback:** `stopName` (fallback `""`) vs `pointDisplayName` (fallback
    `"{country} {code}"`).
12. **Stop id construction:** `CzPttEntities.stationIdFor` vs the inline builder for
    `unresolvedPointIds` in `CzPttBundle` vs the serving's unescaped `"czptt:stop:" + id` vs
    Python's parser.
13. **Note → trip projection:** `intersectingTrips` (notes), `segmentByCall` (zones, operational
    calls), `coverageTrips` (IDS sidecar), `noteFeatures` covered calls.
14. **Whole-journey coverage test:** `noteCoversJourney` and `wholeTrip` in `noteFeatures` (same
    predicate).
15. **"resolved" interval test** `first ≤ last`: central notes, non-central notes, features, zones.
16. **"Unknown {code}" default company name:** catalog loader and `agency`.
17. **Distance:** only `Geo.haversineMetres` (good), but `route_end_north` uses a flat
    `111320` m/degree conversion.
18. **Duplicate detection:** Python SHA-256 of raw bytes vs F# SHA-256 of re-serialized XML.

---

## 9. Open questions for the architect

- **Q1 — PA/TR id casing.** Keep `Pa:`/`Tr:` (published as `czptt:pa`/`czptt:tr` keys and inside
  every trip id, and possibly used by Oběhy realtime), or switch to the XML text `PA:`/`TR:`?
- **Q2 — Day offset semantics.** Is `PlannedCalendar` the calendar of offset 0? If so, negative
  offsets should move the service date back (B3). What is the reference day for positive offsets
  when the PA starts abroad?
- **Q3 — Occurrence encoding** in `CZCentralPTTNote`/`CZNonCentralPTTNote`/`CZIPTS`: 0-based or
  1-based, and what does a blank mean? Should the country prefix in `CZ57076` be honoured?
- **Q4 — Timestamps with offsets.** Do real messages carry offsets in `StartDateTime`,
  `CZPTTCreation`, `Timing/Time`? Should a `Time` offset other than the Prague one be honoured?
- **Q5 — SR70 CSV layout.** How many columns does the real file have, is there a header, and is
  "last two columns are lat/lon" correct? Should one bad row poison a code?
- **Q6 — OSM `public_transport` nodes:** keep (code) or drop (contract)?
- **Q7 — Ignored activities:** should `CZ13` (unpublished stop) or `0002` (internal stop) suppress
  a `0001` call? Should `CZ02`/`0031`/`0033` affect anything?
- **Q8 — Terminal-call changes:** is ignoring a key change on the last call intended for NAD and
  RU, or only for line expiry?
- **Q9 — Catalog versions:** should line marks and IDS records be resolved per service date
  (splitting a PA's calendar) rather than at the first validity date?
- **Q10 — Unused note fields:** meaning of `CZCentralPTTNote` field 5 and `CZNonCentralPTTNote`
  fields 5–8, and the structure of `CZCalendarIPTS` beyond field 0.
- **Q11 — OSM ambiguity:** accept ranked ambiguity as a named rule with a diagnostic, or
  quarantine per AGENTS?
- **Q12 — Estimate placeholder:** keep the 300 m-per-call northward offset for open route ends, or
  leave such points unresolved?
- **Q13 — Notes of rejected PAs:** publish or drop?
- **Q14 — Cross-PA NAD matching** (R083) is inference: compiler or an enrichment pass? Should its
  transfers stay trip-specific in the model with the MOTIS stop-pair rewrite only in
  `jr-publish`?
- **Q15 — One default operational-point mode:** `gtfs` (CLI) or `sidecar` (Python, contract)?
- **Q16 — Flattening in Rust:** read `sources/` directly (dropping `derived/messages.zip` and its
  digest in `run-manifest.json`) or keep the ZIP as an input contract?
- **Q17 — Unknown catalog codes** (category, RU without company, IDS code): diagnostics only, or
  quarantine?
- **Q18 — Unresolved Czech passenger points:** fail like foreign ones, quarantine, or publish
  without coordinates?

---

## 10. Rules added by the rewrite

#### CZPTT-R116 — Running days outside the horizon
- **What:** a timetable message's running days (R065) that fall outside the timetable year's GVD
  window, widened by `horizon.days_before` / `horizon.days_after` days (Q2), are dropped. One
  diagnostic per message gives the number dropped. Cancellation days outside the window are
  ignored (they cannot remove anything).
- **Where:** `jr-czptt` `merge.rs`.
- **Why:** calendars may reach into the neighbouring timetable year; a `DateSet` describes only
  the horizon, and those days belong to the other year's build.
- **Constants:** `czptt.horizon.days_before = 1`, `czptt.horizon.days_after = 1`.
- **Diagnostic:** `czptt_dates_outside_horizon` (warning).

#### CZPTT-R117 — Railway points of a trip part
- **What:** after the parts are final (R041–R050), every non-passenger call of the path is added
  to at most one part. With `low`/`high` the smallest/largest source index among a part's
  passenger calls and `path_low`/`path_high` the same over all retained parts, a call at index
  `i` joins the part that is not a replacement-bus part and has `low < i < high`, or
  `low = path_low` and `i < low`, or `high = path_high` and `i > high`. A replacement-bus part
  gets none; a point of a path whose first or last part is a replacement bus, before or after
  it, belongs to no part. A part's calls are its passenger calls and its railway points, by
  source index; railway points keep the path's normalized times (R028, R037, R066).
- **Where:** `jr-czptt` `segment/railway.rs`.
- **Why:** Q15: there is no operational-point mode; trips carry their railway points as the
  serving contract defines (serving §3.3), and GTFS publishes passenger calls only.
- **Constants:** none.
- **Diagnostic:** `czptt_railway_points_unassigned` (info), once per path, listing the
  sequences of points that belong to no part.

#### CZPTT-R118 — National keys of trips and points
- **What:** every trip carries `czptt:pa` (the PA identifier text), `czptt:tr` (the TR
  identifier text, when the message has one) and `czptt:train_number` (the part's train number,
  R057, only when it is non-empty and all ASCII digits), each valid from the first to the last
  running date of the trip. Every place whose country is `keys.sr70_country` and whose code is
  `keys.sr70_code_length` ASCII digits carries `sr70` = the code; every `Track` boarding point of
  such a place carries `sr70:track` = `<code>:<subsidiary code>` (serving verdict 16). Place and
  track keys are valid from the first to the last running date over all trips. Binding
  `identity`; no call keys (serving verdict 3).
- **Where:** `jr-czptt` `assemble.rs`.
- **Why:** realtime and other feeds find trains and railway points by these national
  identifiers (serving §4 rows 4, 5, 9, 10); the identifiers are carried as keys, never parsed
  back from IDs (AGENTS rule 6).
- **Constants:** `czptt.keys.sr70_country = "CZ"`, `czptt.keys.sr70_code_length = 5`.
- **Diagnostic:** none (a train number taken from the TR core is diagnosed by R057).

#### CZPTT-R119 — Calendar-limited IDS memberships
- **What:** a `CZIPTS` value's sixth field names a `CZCalendarIPTS` value of the same message
  (`id|from|to|bitmap|`, the bitmap counting days from `from`, cut at `to`); the membership
  holds on the bitmap's days only. A calendar that cannot be found or read is diagnosed, and the
  membership then holds every day.
- **Where:** `jr-czptt` `notes/zones.rs`.
- **Why:** B17 (fixed after the switch, `docs/roadmap.md`): SŽ limits a membership to the days
  the IDS contract covers. GVD 2026: no annual message, 90 of 44,794 change messages, all year.
- **Constants:** `czptt.zones.calendar_parameter = "CZCalendarIPTS"`.
- **Diagnostic:** `czptt_ids_calendar_unread` (warning).

#### CZPTT-R120 — A trip split where its IDS zones differ by day
- **What:** the days on which some of a path's memberships do not hold are grouped by which
  ones; each group is a copy of every part of the path (`SplitTrip`) with the zones that hold
  (`SetCallZones` on the calls that differ), named `identity::czptt::ids_variant` after those
  memberships as written. The days on which every membership holds keep the trip's ID (when
  there are none, the first group does). A pass (`ids_days::edits`) the pipeline applies right
  after the compiler, so the copies carry the notes, keys and transfers of the trip.
- **Where:** `jr-czptt` `assemble.rs` (decision), `ids_days.rs` (edits).
- **Why:** a call's zones are per call, not per day; a passenger's fare depends on the day.

#### CZPTT-R121 — The direction of a train
- **What:** per route, the trip with the most distinct passenger places (then the smallest
  public ID) has direction 0; a trip takes the direction of an already directed trip it shares
  at least two passenger places with (the one sharing most, then the smallest public ID), or
  the other one when more of the consecutive shared places are visited in reverse than in
  order. Rounds repeat until nothing changes; a trip still undecided, or whose comparison ties,
  has no direction.
- **Where:** `jr-czptt` `assemble/direction.rs`.
- **Why:** serving verdict 1 (revisited after the switch, `docs/roadmap.md`): a rail line's
  route stops held both directions of travel in one list.
- **Diagnostic:** `czptt_direction_undecided` (info).

#### CZPTT-R122 — A catalogue line version boundary inside a path
- **What:** a part whose catalogue line (R056) has another version, or none, on the path's last
  running day than on its first is diagnosed; the first day's version still applies.
- **Where:** `jr-czptt` `assemble/routes.rs`.
- **Why:** B27: choosing the version per day would split trips by date, and no line or IDS
  entry of the KADR catalogue changes version inside GVD 2026 (measured 2026-11-25). The
  diagnostic shows the day it starts to matter.
- **Diagnostic:** `czptt_line_version_changes` (warning).

#### Dropped by the rewrite
- **CZPTT-R048** (edge timing copied across a shared point): dropped: unreachable since parts are
  cut from passenger calls. Adjacent parts share one boundary call, so one side cannot be timed
  while the other is not.
- **CZPTT-R047**, its "dropped below two passenger calls" branch: dropped for the same reason (a
  part cut from passenger calls has at least two). R047 stands as "a replacement-bus part holds
  passenger calls only".
