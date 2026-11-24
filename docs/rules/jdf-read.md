# JDF reading and batch fixing: rule inventory (F# reference)

Status: **proposal for review**. Nothing here is implemented yet. This inventory describes what the
F# JrUtil (`E:/Git/obehy/jrutil`) does today when it reads a JDF batch and runs `fix-jdf` on it.
The verdicts are proposals for the architect.

Sources read in full: `jrutil/src/Jdf/Jdf.fs`, `JdfModel.fs`, `Jdf109Model.fs`, `Jdf110Model.fs`,
`Jdf109To110.fs`, `Jdf110To111.fs`, `JdfParser.fs`, `JdfSerializer.fs`, `JdfFix.fs`,
`JdfFixups.fs`; `CsvParser.fs`, `CsvSerializer.fs`, `CsvMetadata.fs`, `UnionCodec.fs`,
`AutoCopy.fs`, `ReflectionUtils.fs`, `Utils.fs`, `FileUtils.findPathCaseInsensitive`;
`GeoData/StopMatcher.fs`, `StopMatcherSynonyms.txt`, `OsmStops.fs`, `ExternalCsv.fs`,
`CzRegions.fs`, `EurTowns.fs`, `Common.fs`; `jrutil-multitool/Commands/JdfCommands.fs` (`fixJdf`),
`Context.fs`, `Program.fs`; tests `JdfParserTests.fs`, `JdfFixupsTests.fs`, `StopMatcherTests.fs`,
`ExternalCsvTests.fs`, `PerformanceEquivalenceTests.fs`; `jrunify-ext-geodata/README.md` and the
`other/*.csv`, `registry/*.csv` shapes; `repo/src/obehy/national_jdf.py` (`_fix_arguments`,
`stage_nested_jdf_batches`, `_verify_fixed_batches`) and `osm_snapshot.py`
(`JDF_POST_CANDIDATE_FILTERS`, `REGIONS`). Read for hand-over only: the first lines of
`JdfInternationalFilter.applyInternationalRoutePolicyInternal`, `canonicalCountry`,
`JdfMerger` (how it consumes `Polzast*`), `JdfStopReconciliation.normalizeStopName`,
`Serving/PackageRows.coordinateSource`.

Real-data peeks: 180 randomly chosen batches of `work/overlay-golden/inputs/jdf/batches`
(2,487 ZIPs, `vld-N.zip` and `drahy-N.zip`), Zastavky/Linky/VerzeJDF only, plus the three
`jrutil/data` files and all of `jrunify-ext-geodata/other/*.csv`.

Conventions used below:

- "batch" = one JDF directory or ZIP; "stop" = one `Zastavky.txt` record; "call" = one
  `Zasspoje.txt` record (`TripStop`).
- "LAEA" = EPSG:3035 (ETRS89-extended / LAEA Europe). Every geometric operation in `fix-jdf`
  (distances, centroids, offsets, polygon tests) is **planar in LAEA metres**, not haversine.
- "name string" = `jdfStopNameString stop` = `"{town},{district or ""},{nearbyPlace or ""}"`.
- "exact match" = a `StopMatch` with `score = 1.0f` (R042).
- "checked" source = a candidate whose `source` does not start with `osm:` (an external
  catalogue row); "OSM" source = starts with `osm:`.
- "called" = a call whose arrival or departure is a `StopTime` (not `|`, `<` or empty).
- Proposed policy keys live in `policy/jdf.toml` (prefix `jdf.`); proposed data files in
  `data/jdf/` or `data/names/`.
- Test names are the F# `JdfFixupsTests` method names unless another class is named.
- The F# `int`/`decimal` conversions, `string` formatting and `Seq.sort` on strings are
  culture-invariant / ordinal unless stated.

---

## Architect's verdicts

**After the switch (2026-11-24):** the limitations and post-switch changes recorded below are
decided in `docs/roadmap.md`, which takes precedence where the two differ.

**These override the "Proposed verdict" lines below.** Where a rule is not listed, its proposed
verdict stands. Rule IDs in the Rust code are the IDs of this file. The bar is parity with what
F# publishes (the stop registry already holds these names and numbers), so published behaviours
are ported even where they are debatable; crashes, silent drops and dead work are fixed.

The bounded batch set holds JDF 1.11 (2,229 batches), 1.10 (256) and 1.8 (2): all three are read.

| Q | Decision |
|---|---|
| Q1 | Port R037: the rewritten name triple is what is published and what the registry keys on. |
| Q2 | Port the MHD renaming (R056–R060) as named rules with a diagnostic per renamed batch. |
| Q3 | Port the inference of okres and country (R053, R055, R061–R063), each inferred value diagnosed. |
| Q4 | Port: the centroid of the candidates (R048, R049). |
| Q5 | One distance function: `jr_model::geo` (great-circle). Centroids are means in a local tangent plane. No LAEA projection. The small differences go in the parity ledger. |
| Q6 | Port R070 as is. |
| Q7 | Port the JDF estimator (R076–R078) as its own rules; it shares the interpolation helper with rail, not the selection rule. |
| Q8 | The model holds the country code as the feed publishes it: JDF codes (`CZ`, `D`, `A`, `SK`, `PL`, …) for road. One mapping table (policy) translates between JDF, ISO and the gazetteer where sources are compared (fixes B7 for matching only). |
| Q9 | Publish the source okres code; `OL`/`OC` are aliases for matching and geometry lookups (fixes the crash of B3). |
| Q10 | Read 1.8/1.9 and 1.10. Fix B1: the 1.9 `timeType` is the note type. |
| Q11 | A malformed batch is dropped with an error diagnostic and makes the package ineligible for publication; it does not abort the build. A record without its terminator is a diagnostic, not a silent drop (B12). |
| Q12 | Drop R032: the international policy is evaluated where it is applied. |
| Q13 | Port: placeholder coordinates are the geodata repository's validator's job; the plausibility check is the compiler's defence. |
| Q14, Q15 | Port (no country for foreign OSM nodes; the tram filter as it is). Recorded as limitations. |
| Q16, Q17 | Port R050 and R051. Recorded as limitations. |
| Q18 | Matching runs per batch, as today: a batch is the trip context the rules need. The context-free part (candidate retrieval and exact selection per name, okres and country) is memoised across batches. Stop reconciliation follows. |
| Q19 | Port R066. |
| Q20 | Found on the national feed (2026-11-23), both deviations from F#: R048 counts only the okresy a group's rows state, so a row without one agrees with any (a gap-fill row with `TN` next to catalogue rows without an okres left six Slovak stops unlocated, and six Polish ones the same way); and R082 (new): a stop that nothing else places takes its reviewed gap-fill rows, as a last resort after the kilometre matching (Mastník and Plazy, whose bare municipality names lose their match, and Nové Heřminovy, whose catalogue rows lie 1.4 km apart). Letting gap-fill rows overrule the catalogues was tried and withdrawn: it moved eight stops off their registry positions and sent Loket,u dálnice 1.9 km from where three catalogues agree. |

Further:

- R001–R004, R080, R081 (the command, its output files) are replaced by the pipeline; there are
  no fixed batches on disk. What `Polzast`, `PolzastZdroj` and `JrutilPostCandidateEvidence`
  carried are fields of the fixed batch in memory (section 5.6).
- R014 (the Lucene index) is replaced by a token index with the same matching semantics
  (R039–R041); the synonym file is data shared by every name matcher.
- The reader (R017–R024) reports facts: absent fields stay absent, per version. The version
  defaults (R025–R031) are rules of the compiler.
- Fixed and ledgered: B1, B2 (both polygons of an okres are kept), B3, B12. Left as they are and
  recorded as limitations: B4, B5, B8, B9, B17.

---

## 1. Data flow

Oběhy calls (`national_jdf._fix_arguments`):

```
jrutil-multitool fix-jdf --strict --progress-events --batch-output=zip --jobs=… --memory-budget=…
  --international-route-policy=regional-adjacent [--no-estimated-posts]
  --ext-geodata=<jrunify-ext-geodata>/other --cz-pbf=<workdir>/osm/jdf-post-candidates.osm.pbf
  --logfile=<logs>/fix.log <work>/batches <fixed_root>
```

`--no-estimated-posts` is passed unless the build has inferred posts or evidence capture enabled.
`<work>/batches` holds one ZIP per nested batch of the national VLD and dráhy archives, named
`<source>-<stem>.zip`; Python guarantees one `VerzeJDF.txt` per ZIP and unique names
(`stage_nested_jdf_batches`) and afterwards checks that `fixed_root` holds exactly one ZIP per
input stem (`_verify_fixed_batches`). The OSM file is the regional merge of the Geofabrik extracts
`czech-republic`, `austria`, `germany/bayern`, `germany/sachsen`, `slovakia`,
`poland/dolnoslaskie`, `poland/opolskie`, `poland/slaskie`, reduced by `osmium tags-filter
--omit-referenced` to `n/highway=bus_stop`, `n/public_transport=platform,pole,station`,
`n/railway=tram_stop`, `n/amenity=bus_station` (filter schema `jdf-post-candidates-v1`).

**Stage 0 — command (`JdfCommands.fixJdf`).** Reads options (R001), creates the output directory,
then Stages 1–3 once, then Stages 4–12 per batch in parallel with results committed in input
order, then Stage 13. Rules R001–R004.

**Stage 1 — external catalogues (`ExternalCsv.otherStopsFromPathForJdfMatch`).** Input:
`--ext-geodata` path (file or directory). Output: `StopToMatch<JdfStopGeodata>[]` in file order.
Rules R005–R008.

**Stage 2 — OSM stops (`OsmStops.getCzOtherStops`, `czOtherStopsForJdfMatch`;
`CzRegions.czechTownByPoint`).** Input: `--cz-pbf`. Output: `StopToMatch<JdfStopGeodata>[]` in
PBF order. Rules R009–R013.

**Stage 3 — matcher (`StopMatcher` constructor).** One Lucene RAM index over external rows then
OSM rows. Also lazily built per run: okres polygons, okres adjacency, Czech town matcher, European
town matcher. Rules R014–R016, R039–R041.

**Stage 4 — batch discovery (`Jdf.findJdfBatchPaths`, `Seq.sort`).** Rule R002.

**Stage 5 — parse (`Jdf.parseJdfBatchPath` + `Jdf.jdfBatchDirParser`).** Version detection,
per-file parsers, upgrade 1.8/1.9 → 1.10 → 1.11 (`Jdf109To110.jdf109To110Converter`,
`Jdf110To111.jdf110To111Converter`). Output: one `JdfModel.JdfBatch` (1.11 shape). Rules
R017–R031.

**Stage 6 — international decisions (`JdfInternationalFilter.applyInternationalRoutePolicy`).**
Evaluated on the **unfixed** source batch; only the decisions are kept, for logging in Stage 13.
The batch is not changed. Rule R032 (the policy's own rules belong to the international-filter
inventory).

**Stage 7 — degenerate batch (`JdfFixups.dropDegenerateBatch`).** Rule R033.

**Stage 8 — names and matching (`JdfFixups.fixPublicCisJrBatch`).** Single-route check, trip
matrices, stop-name normalisation, primary matching (`matchStops`: exact matching, town-only
vetoes, secondary km matching, town geography), MHD naming detection and choice,
`addRegionFromMatch`, okres/country propagation (`fillStopRegionsFromPrevious` ×≤4), post
candidate collection. Output: batch with rewritten `stops` and `postCandidateEvidence`, one match
option per stop, geography counters. Rules R034–R067.

**Stage 9 — plausibility (`JdfFixups.rejectImplausibleMatches`) and evidence filter
(`JdfFix.fixBatch`).** Rules R068–R071.

**Stage 10 — log-only checks (`JdfFixups.checkMatchDistances` on `JdfFix.longestTripStops`,
`checkMissingRegionsCountries`).** Rules R072–R073.

**Stage 11 — locations (`JdfFixups.addStopLocations`, `estimateMissingStopLocations`).**
Rules R075–R079.

**Stage 12 — evidence switch and write (`JdfFix.fixBatch`, `Jdf.jdfBatchDirWriter`).** Rules
R080–R081.

**Stage 13 — summaries (`MatchDiagnostics.log`, `JdfInternationalFilter.logInternationalRouteDecisions`).**
Rule R074.

**Hand-over.** `merge-jdf` reads the fixed batches (merge inventory): it consumes the rewritten
`Zastavky` (names, okres, country), `Polzast.txt` (precise locations used for the 75 m stop
reconciliation and preferred over estimated ones), `PolzastZdroj.txt` and
`JrutilPostCandidateEvidence.txt`. Calendar compilation, the international route filter, GTFS
conversion and the post estimator run later (`jdf-to-bundle`) and are other inventories. Note
that `jdf-to-bundle` reads its input through a second parser path
(`Jdf.jdfCompilationParser`) that applies the same version upgrades (section 8).

---

## 2. Source facts

### 2.1 Batch discovery and file lookup

- A path is a batch if its file name equals `VerzeJDF.txt` (case-insensitive → the containing
  directory is the batch), or its extension is `.zip` (case-insensitive), or it is a directory,
  which is searched recursively: files first, then subdirectories, in `Directory.Enumerate*`
  order; `fix-jdf` then sorts the whole list ordinally (R002).
- Inside a directory batch a file is found by `Directory.GetFiles` + `ToLower()` name equality
  (culture-sensitive); more than one hit fails (`Multiple files found when looking for …`).
- Inside a ZIP the first entry whose `entry.Name` (file part only, folders ignored) equals the
  wanted name under `ToLower()` is used; duplicates in different folders are silently resolved
  to the first.
- Version: `VerzeJDF.txt` is read as text (CP1250, BOM-detecting), trimmed, matched with
  `^"([0-9.]+)"[;,]`; group 1 is the version (`""` when the regex fails). Dispatch: `1.11`,
  `1.10`, `1.9` and `1.8` (the latter two share one model); anything else fails the batch
  (`Unknown JDF version: "…"`). A missing `VerzeJDF.txt` fails (`Option.get`).

Files read per version (R = required, missing fails the batch; O = optional, missing → empty;
— = never read even if present):

| File | F# record | 1.8/1.9 | 1.10 | 1.11 |
|---|---|---|---|---|
| `VerzeJDF.txt` | `JdfVersion` | R | R | R |
| `Zastavky.txt` | `Stop` | R | R | R |
| `Oznacniky.txt` | `StopPost` | — | O | O |
| `Dopravci.txt` | `Agency` | R | R | R |
| `Linky.txt` | `Route` | R | R | R |
| `LinExt.txt` | `RouteIntegration` | — | O | O |
| `Zaslinky.txt` | `RouteStop` | R | R | R |
| `Spoje.txt` | `Trip` | R | R | R |
| `SpojSkup.txt` | `TripGroup` | — | O | O |
| `Zasspoje.txt` | `TripStop` | R | R | R |
| `Udaje.txt` | `RouteInfo` | O | O | O |
| `Pevnykod.txt` | `AttributeRef` | R | R | R |
| `Caskody.txt` | `ServiceNote` | R | R | R |
| `Navaznosti.txt` | `Transfer` | — | O | O |
| `Altdop.txt` | `AgencyAlternation` | O | O | O |
| `Altlinky.txt` | `AlternateRouteName` | O | O | O |
| `Mistenky.txt` | `ReservationOptions` | O | O | O |
| `Polzast.txt` (JrUtil) | `StopLocation` | — | — | O |
| `PolzastZdroj.txt` (JrUtil) | `StopLocationSource` | — | — | O |
| `JrutilPostCandidateEvidence.txt` | `PostCandidateEvidence` | — | — | O |
| `JrutilRoutingDemands.txt` (merge output) | `RoutingDemand` | — | — | O |
| `JrutilRouteVersions.txt` (merge output) | `RouteVersion` | — | — | O |

Golden sample (60 batches): versions `1.11` ×54, `1.10` ×6, no 1.8/1.9; every batch has the nine
required files, `LinExt` 59, `Udaje` 59, `Navaznosti` 25, `Altdop` 1; 13 entries are empty files.

### 2.2 Record grammar (`JdfParser.getJdfParser`)

- Bytes decoded by `StreamReader(stream, CP1250)`; `detectEncodingFromByteOrderMarks` is on by
  default, so a UTF-8/UTF-16 BOM silently overrides CP1250 (R022).
- Text is read in 1 MiB blocks; the record terminator is exactly the four characters `";\r\n`
  (ordinal search, works across block boundaries).
- Text between terminators (a "line") must start with `"`, else the batch fails
  (`JDF line did not start with quote`). An empty line (two terminators in a row) throws
  `IndexOutOfRange`. A blank line or stray whitespace between records fails (it does not start
  with `"`).
- After removing the leading `"`, a whitespace-only line is skipped.
- Fields: `line.Split("\",\"")`. No unescaping: embedded quotes are kept verbatim (pinned); a field
  containing `","` cannot be represented.
- Text after the last terminator (a final record without `";\r\n`, or a whole LF-only file) is
  silently discarded.
- Field mapping is positional over the record's declared fields; a `CsvSpread(n)` field consumes
  `n` cells into an array. Fewer cells than needed → unwrapped `IndexOutOfRangeException` (batch
  fails); extra cells are ignored.
- A field decode failure raises `CsvParseException("Failed parsing field \"{name}\" of value
  \"{value}\": …")`; the batch fails, and with it the whole `fix-jdf` run (R004).

Golden sample: every non-empty file ends with `";\r\n`; no BOM.

### 2.3 Scalar decoding (`CsvParser.colParserForBase`, `JdfParser.jdfColParserFor`)

| Type | Decoding |
|---|---|
| `string` | verbatim, `""` kept |
| `T option` | `""` → `None`, else `Some (decode T)`; no trimming (`" "` → `Some " "`) |
| `int`, `int64` | F# `int`/`int64` (accepts surrounding whitespace, `0x`/`0o`/`0b` prefixes); `""` fails |
| `decimal` | F# `decimal` (invariant, `NumberStyles.Float`) |
| `bool` | `"0"` or `""` → false (comment: empty is not per spec but occurs), `"1"` → true, else fails |
| `LocalDate` | NodaTime `ddMMyyyy`, invariant |
| `LocalTime` | NodaTime `HHmm` (no `2400`) |
| union with `CsvParse` | `Attribute`, `TripStopTime` (below) |
| other union | case by `StrValue` literal, case-sensitive; unknown → `No union case for value "…"` |

Per parser instance a `ConcurrentDictionary` caches decoded values of every string, option and
union field by raw text (performance only).

Enumerations:

| Type | Codes |
|---|---|
| `TripStopTime` | `\|` Passing, `<` NotPassing, else `HHmm` → `StopTime` |
| `CompanyType` | `1`, `0`, `""` → Corporation (comment: "0" and "" parse bad batches); `2` → NaturalPerson |
| `RouteType` | `A` City, `B` CityAndAdjacent, `M` International (1.8), `N` InternationalNoNational, `P` InternationalOrNational, `V` Regional, `W` ExtraDistrict (1.8), `Z` ExtraRegional, `D` LongDistanceNational. `RegionalInternational` exists but has no code (never parsed) |
| `TransportMode` | `A` Bus, `E` Tram, `L` CableCar, `M` Metro, `P` Ferry, `T` Trolleybus |
| `ServiceNoteType` | `1` Service, `2` ServiceAlso, `3` ServiceOnly, `4` NoService, `5` ServiceOddWeeks, `6` ServiceEvenWeeks, `7` ServiceOddWeeksFromTo, `8` ServiceEvenWeeksFromTo |
| `GeodataPrecision` (Polzast) | `S` StopPrecise, `E` Estimated |
| `Attribute` (Pevnykod value) | a value matching `^\d+$` → `DayOfWeekService n` (any integer, not range-checked); else `X` WeekdayService, `+` HolidaySundayService, `R` ReservationAvailable, `#` OnlyWithReservation, `\|` NotStopping, `<` TakesDiversion, `@` WheelchairAccessible, `%` FoodAvailable, `W` ToiletsAvailable, `w` WheelchairAccessibleToilets, `x` RequestStop, `~` CityTransportTransfer, `(` ExitOnly, `)` BoardingOnly, `$` BorderStopOnly, `{` PartlyWheelchairAccessible, `}` VisuallyImpairedAccessible, `[` BaggageTransport, `O` BicycleTransport, `v` RailwayTransfer, `§` TravelExclusion0, `A` TravelExclusion1, `B` TravelExclusion2, `C` TravelExclusion3, `T` CommisionServiceOnly, `!` ConditionalService, `t` AccessibilityTerminal, `b` LineTransportTransfer, `U` MetroTransfer, `S` ShipTransfer, `J` AirportNearby, `P` ParkAndRideNearby; 1.10 only: `I` PartOfIntegratedTransport, `s` SelfServiceTicketTrain. One shared union for all versions, so "hybrid" files are accepted |

Attribute columns (`attributes` arrays in `Stop`, `RouteStop`, `Trip`, `TripStop`,
`AgencyAlternation`) are `int option` numbers referring to `Pevnykod.attributeId`; they are
resolved later (`Jdf.parseAttributes`, `Array.find`, throws on a dangling number). `fix-jdf` does
not resolve them.

### 2.4 Files and fields

Columns: **#** position; **Field** F# name; **Type** (`?` = option); **1.9** / **1.10** = present
in that version (position if different); **Used** = referenced after parsing by any F# consumer
(grep over `jrutil/src` outside the models; consumers are other inventories). "fix" = used by
`fix-jdf` itself.

**`VerzeJDF.txt`** (1.11: 6 fields; 1.10: 6; 1.9: only field 1 read, others ignored)

| # | Field | Type | 1.9 | Used |
|---|---|---|---|---|
| 1 | `version` | string | yes | dispatch; rewritten to `1.11` on output |
| 2 | `duNum` (číslo dopravního úřadu) | int? | ignored | merge, manifest |
| 3 | `region` | string? | ignored | manifest |
| 4 | `batchId` | string? | ignored | merge, manifest, GTFS |
| 5 | `creationDate` | date? | ignored | merge, manifest, rules |
| 6 | `generator` | string? | ignored | merge, manifest |

**`Zastavky.txt`** (12 fields, all versions)

| # | Field | Type | Used |
|---|---|---|---|
| 1 | `id` (stop number, local to the batch) | int64 | fix, all |
| 2 | `town` (obec) | string | fix (rewritten, R036–R038, R059) |
| 3 | `district` (část obce) | string? | fix (rewritten) |
| 4 | `nearbyPlace` (bližší místo; comment: "in practice used for the stop name") | string? | fix (rewritten) |
| 5 | `regionId` (okres code, `AB` = Praha) | string? | fix (filled, R053, R055, R061–R063) |
| 6 | `country` | string? | fix (filled) |
| 7–12 | `attributes` ×6 | int? | bundle |

Golden sample (180 batches): every stop has country `CZ` except 42 with empty country and okres
(all in one batch); no `OL`; no `[XX]` suffix; no comma in `town`; 1,514 stops have an empty
`district` and a non-empty `nearbyPlace` (`"Praha","","Muzeum"`); 267 are town-only
(`"Brněnský kopec","",""`, MHD batches).

**`Oznacniky.txt`** (1.10, 1.11: 7) — `stopId` int64, `stopPostId` int64, `name`?,
`description`?, `postName`? (used by GTFS), `reserved1`?, `reserved2`?. Not used by fix.

**`Dopravci.txt`** (1.10, 1.11: 13; 1.9: 12, no field 13)

| # | Field | Type | Used |
|---|---|---|---|
| 1 | `id` (IČO) | string | yes |
| 2 | `taxId` (DIČ) | string? | no |
| 3 | `name` | string | yes |
| 4 | `companyType` | CompanyType | no |
| 5 | `personName` | string | no |
| 6 | `officeAddress` | string | no |
| 7 | `officePhoneNum` | string | GTFS |
| 8 | `controlPhoneNum` | string? | no |
| 9 | `infoPhoneNum` | string? | no |
| 10 | `faxNum` | string? | no |
| 11 | `email` | string? | GTFS |
| 12 | `website` | string? | GTFS |
| 13 | `idDistinction` (with `id` the key) | int | yes; 1.9 → `0` |

**`Linky.txt`** (1.11: 17; 1.10: 16; 1.9: 10)

| # (1.11) | Field | Type | 1.10 | 1.9 | Used |
|---|---|---|---|---|---|
| 1 | `id` (six-digit licence line number) | string | 1 | 1 | yes |
| 2 | `name` | string | 2 | 2 | fix (R058), yes |
| 3 | `agencyId` | string | 3 | 3 | yes |
| 4 | `routeType` | RouteType | 4 | 4 | yes |
| 5 | `transportMode` | TransportMode | 5 | — → `A` | yes |
| 6 | `detour` (výluka) | bool | 6 | — → false | yes |
| 7 | `grouped` | bool | 7 | — → false | merge |
| 8 | `usesStopPosts` | bool | 8 | — → false | no |
| 9 | `oneWay` | bool | — → false | — → false | no |
| 10 | `reserved1` | string? | 9 | 5 | no |
| 11 | `licenceNum` | string? | 10 | 6 | no |
| 12 | `licenceValidFrom` | date? | 11 | 7 | no |
| 13 | `licenceValidTo` | date? | 12 | 8 | no |
| 14 | `timetableValidFrom` | date | 13 | 9 | yes |
| 15 | `timetableValidTo` | date (1.11 required) | 14, date? | 10, date? | yes; absent → `3000-01-01` (R030) |
| 16 | `agencyDistinction` | int | 15 | — → 0 | yes |
| 17 | `idDistinction` | int | 16 | — → 0 | yes |

**`LinExt.txt`** (1.10, 1.11: 7) — `routeId`, `entryNum` int (no), `transportSystemId` int64
(bundle), `routeName` string (rules), `preferential` bool (rules), `reserved1` string,
`routeDistinction` int.

**`Zaslinky.txt`** (1.10, 1.11: 9; 1.9: 7)

| # | Field | Type | 1.9 | Used |
|---|---|---|---|---|
| 1 | `routeId` | string | 1 | yes |
| 2 | `routeStopId` (order in the printed timetable) | int64 | 2 | fix (R035), yes |
| 3 | `zone` (comma-separated tokens) | string? | `reserved1` at 3, dropped; `zone` → None | bundle |
| 4 | `stopId` | int64 | 4 | yes |
| 5 | `approximateTime` (minutes from route start) | int? | — → None | no |
| 6–8 | `attributes` ×3 | int? | 5–7 | bundle |
| 9 | `routeDistinction` | int | — → 0 | yes |

**`Spoje.txt`** (1.10, 1.11: 14; 1.9: 12) — `routeId`, `id` int64 (odd = forward, even =
reverse), `attributes` ×10 int?, `tripGroupId` int64? (1.9 → None; used by merge),
`routeDistinction` int (1.9 → 0).

**`SpojSkup.txt`** (1.10, 1.11: 5) — `id` int64, `entryNum` int, `name`, `description`?,
`reserved1`. Copied through merge, never projected.

**`Zasspoje.txt`** (1.11: 15; 1.10: 12; 1.9: 10)

| # (1.11) | Field | Type | 1.10 | 1.9 | Used |
|---|---|---|---|---|---|
| 1 | `routeId` | string | 1 | 1 | yes |
| 2 | `tripId` | int64 | 2 | 2 | fix, yes |
| 3 | `routeStopId` | int64 | 3 | 3 | fix, yes |
| 4 | `stopId` | int64 | 4 | 4 | fix, yes |
| 5 | `stopPostId` (Označníky number) | int64? | 5 | — → None | yes |
| 6 | `stopPostNum` (text, "standard says int") | string? | 6 | 5 | yes |
| 7–9 | `attributes` ×3 | int? | 7–8 (×2) | 6–7 (×2) | yes; upgraded arrays keep length 2 (R031) |
| 10 | `kilometer` | decimal? | 9 | 8 | fix (R052, R061–R063, R072), yes |
| 11 | `arrivalTime` | TripStopTime? | 10 | 9 | fix, yes |
| 12 | `departureTime` | TripStopTime? | 11 | 10 | fix, yes |
| 13 | `minArrivalTime` | TripStopTime? | — → None | — → None | call store only |
| 14 | `maxDepartureTime` | TripStopTime? | — → None | — → None | call store only |
| 15 | `routeDistinction` | int | 12 | — → 0 | fix (R068), yes |

**`Udaje.txt`** (1.10, 1.11: 4; 1.9: 3) — `routeId`, `id` int64, `text` string,
`routeDistinction` int (1.9 → 0).

**`Pevnykod.txt`** (3, all versions) — `attributeId` int, `value` Attribute, `reserved1`?.

**`Caskody.txt`** (1.10, 1.11: 9; 1.9: 8)

| # | Field | Type | 1.9 | Used |
|---|---|---|---|---|
| 1 | `routeId` | string | 1 | yes |
| 2 | `tripId` | int64 | 2 | yes |
| 3 | `id` | int64 | 3 | yes |
| 4 | `designation` | string | 4 | yes |
| 5 | `noteType` | ServiceNoteType? | `timeType` at 5, **lost** (R029) | calendar |
| 6 | `dateFrom` | date? | 6 | calendar |
| 7 | `dateTo` | date? | 7 | calendar |
| 8 | `note` | string? | 8 | yes |
| 9 | `routeDistinction` | int | — → 0 | yes |

**`Navaznosti.txt`** (1.11: 12; 1.10: 11) — `transferType` string (`m`/`M`), `routeId`,
`tripId` int64, `routeStopId` int64, `transferRouteId` int64?, `transferStopId` int64?,
`transferStopPostId` int64?, `transferEndStopId` int64?, `transferEndStopPostId` int64?
(comment: these point to the global stop register, not batch stops), `waitMinutes` int?, `note`?
(1.11 only; 1.10 → None), `routeDistinction` int. All used by the bundle.

**`Altdop.txt`** (1.10, 1.11: 15; 1.9: 13) — `routeId`, `tripId` int64, `agencyId`,
`attributes` ×6, `timeType`? (unused), `reserved1`?, `dateFrom`?, `dateTo`?,
`agencyDistinction` int (1.9 → 0), `routeDistinction` int (1.9 → 0). Copied through merge,
never projected.

**`Altlinky.txt`** (1.10, 1.11: 4; 1.9: 3) — `routeId`, `altRouteNum` int (unused), `country`,
`routeDistinction` (1.9 → 0). Copied through merge, never projected.

**`Mistenky.txt`** (1.10, 1.11: 4; 1.9: 3) — `routeId`, `tripId` int64, `note`,
`routeDistinction` (1.9 → 0). Used by the bundle.

**JrUtil extension files** (read only for 1.11; source CIS batches never contain them):
`Polzast.txt` `stopId, lat, lon, precision`; `PolzastZdroj.txt` `stopId, source`;
`JrutilPostCandidateEvidence.txt` 13 fields (section 5); `JrutilRoutingDemands.txt`
`demandId, modeFamily, previousStopId?, nextStopId?, previousLat?, previousLon?, nextLat?,
nextLon?, searchClass`; `JrutilRouteVersions.txt` `routeId, routeDistinction, originalValidFrom,
originalValidTo, sourceCreationDate?`. The last two are merge outputs.

### 2.5 Version upgrade (`Jdf109To110`, `Jdf110To111`, reflection copy `AutoCopy.getAutoCopier`)

Fields are copied **by name**; a target field without a same-named source field takes the listed
default; source fields without a same-named target are dropped silently.

1.8/1.9 → 1.10 (`nameOptGetters`: any option-typed target field missing in the source → `None`;
any array-typed batch field missing → empty array):

- `JdfVersion`: `version` copied; `duNum`, `region`, `batchId`, `creationDate`, `generator` → None.
- `Agency.idDistinction` → `0`.
- `Route`: `agencyDistinction` → 0, `idDistinction` → 0, `transportMode` → `Bus`, `detour`,
  `grouped`, `usesStopPosts` → false.
- `RouteStop`, `Trip`, `RouteInfo`, `ServiceNote`, `AgencyAlternation`, `AlternateRouteName`,
  `ReservationOptions`, `TripStop`: `routeDistinction` → 0, `agencyDistinction` → 0 where
  present; option fields missing in 1.9 (`RouteStop.zone`, `RouteStop.approximateTime`,
  `Trip.tripGroupId`, `TripStop.stopPostId`, **`ServiceNote.noteType`**) → None.
- Batch arrays missing in 1.9 (`stopPosts`, `routeIntegrations`, `tripGroups`, `transfers`) → `[]`.

1.10 → 1.11 (`nameGetters`, explicit only):

- `Route.oneWay` → false; `Route.timetableValidTo`: `Some d` → `d`, `None` → `LocalDate(3000,
  1, 1)` (comment: "just an arbitrary large date").
- `TripStop.minArrivalTime`, `maxDepartureTime` → None.
- `Transfer.note` → None.
- `stopLocations`, `stopLocationSources`, `postCandidateEvidence`, `routingDemands`,
  `routeVersions` → `[]`.

No test exercises any upgrade.

### 2.6 External stop catalogues (`--ext-geodata`, `ExternalCsv`)

- Input: one file, or a directory searched recursively for `*.csv`, ordered by the ordinal
  comparison of the `/`-separated path relative to the root (R005).
- Each file is read with `File.ReadAllText` (UTF-8, BOM-detecting) and parsed by
  `FSharp.Data.CsvFile.Parse(text, hasHeaders = false)` (comma, `"` quoting). Every row is data.
- Row: exactly 5 columns `name,latitude,longitude,okres,country` (R006). `name` verbatim;
  latitude/longitude invariant `float`; okres/country blank → None (R007).
- `jrunify-ext-geodata/other` today: 28 files, 107,976 rows, all 5 columns, UTF-8 without BOM.
  Countries: `CZ` 107,399, `D` 284, `PL` 134, `SK` 68, empty 52, `A` 36, `UA` 2, `DE` 1. Okres
  codes: the 77 codes of `cz_regions` plus `OL` (1 row), `DL` (10, PL), `SI` (6, SK), `TN` (4,
  SK), `PU` (2, SK), and empty. Names use the JDF triple with commas (`"Liberec,,Americká"`,
  `"Hevlín, škola"`, `Adamov`).
- Not geodata, not loaded: `registry/stops.csv` (`id,town,district,nearby_place,okres,country,
  lat,lon,status,note`), `registry/posts.csv` (`stop_id,post_key,lat,lon,status,note`),
  `registry/overlay_places.csv` (`source_id,group_key,place_id,status,note`); all with a header
  row. Passing the repository root would fail R006 on them.

### 2.7 OSM stop extraction (`--cz-pbf`, `OsmStops`)

- Only nodes. Eligibility in R009. Tags read: `train` (presence), `railway`, `tram` (presence and
  value), `highway`, `public_transport`, `amenity`, `name`, `name:cs`, `official_name`, `bus`,
  `psv`, `trolleybus`, `access`, `vehicle`, `motor_vehicle`, `abandoned`, `disused`,
  `construction`, `proposed`, `temporary`, `lifecycle`, `local_ref`.
- Name: `name`, else `name:cs`; full name `official_name` (R010). Okres: from the Czech
  municipality polygon containing the node (R011). Country: `CZ` when an okres was found, else
  none (R011). Name completion with the municipality name (R012).

### 2.8 Geographic data files (`jrutil/data`)

| File | Shape | Loaded by |
|---|---|---|
| `cz_regions.json.gz` | gzip GeoJSON `FeatureCollection`, CRS84 (WGS84 lon/lat), 78 `Polygon` features, properties `cat` (int), `name` (okres name, mis-encoded: `\ufffdzem\ufffd Hlavn\ufffdho m?sta Prahy`), `code` (okres code). 77 distinct codes; **`LT` occurs twice** (15 and 40 vertices) | `CzRegions.czechRegionPolygons`: `Map code → polygon` reprojected to LAEA (last feature per code wins, R015) |
| `cz_towns.json.gz` | gzip GeoJSON, EPSG:3035, 6,356 `Polygon` features, properties `name` (municipality, UTF-8), `region` (okres code, the same 77 codes). 662 names occur more than once (`Nová Ves` ×14) | `czechTownsPolygons` (array), `czechTownByPoint` (STR-tree), `czTownNameMatcher` |
| `eur_towns_countries.csv` | headerless CSV `name,countryCode,lat?,lon?`, 839,834 rows; country lower-case ISO alpha-2 of 38 European countries (not CZ); 102,958 rows without coordinates (name ambiguous by more than 0.1°). Built from osmnames (`EurTowns.fs` doc comment has the script). Includes Czech exonyms as alternative names: `Praha,sk,,`, `Most,be,,`, `Wien,at,,`, `Dresden,de,,` | `EurTowns.eurTownCountries` (FSharp.Data `CsvProvider`), `eurTownNameMatcher` |
| `GeoData/StopMatcherSynonyms.txt` | Solr synonym format, lower-case, comma lists (R040) | `StopMatcher.makeAnalyzer` |

Hard-coded in `CzRegions.townSynonyms` (R054) and `JdfFixups.mhdTownsJoined` (R057).

---

## 3. Rules

### Stage 0 — command

#### JDF-R001 — fix-jdf options and defaults
- **What:** `<JDF-in-dir>` (first value), `<JDF-out-dir>`; `--ext-geodata` optional (absent → no
  external rows); `--cz-pbf` optional (absent → no OSM rows); `--international-route-policy`
  default `keep-all` (`keep-all` | `regional-adjacent`, else error); estimated-post evidence is
  collected unless `--no-estimated-posts` (`Execution.estimatedPostActivation`, `collectEvidence =
  not disabled`; `--routing-osm-pbf` is irrelevant here); `--batch-output` default `directory`,
  lower-cased, `directory` | `zip`, else `invalidArg`. `--strict` is accepted and ignored.
  `--jobs`, `--memory-budget`, `--progress-events`, `--logfile` are execution plumbing.
- **Where:** `JdfCommands.fixJdf` 22–36; `Context.internationalRoutePolicy`,
  `estimatedPostActivation`.
- **Why:** unknown.
- **Pinned by:** none.
- **Constants:** `keep-all`, `directory`.
- **Diagnostic:** `Fixing JDF with {Jobs} workers and {BatchOutput} batch output`.
- **Proposed verdict:** replace: the build spec names the inputs; there is no intermediate output
  format and no separate fix command.

#### JDF-R002 — Batch discovery and order
- **What:** §2.1 discovery from the input path; the resulting path list is sorted ordinally
  (`Seq.sort`) and processed in parallel, results committed in that order.
- **Where:** `Jdf.findJdfBatchPaths` 266–278; `JdfCommands.fixJdf` 71–72.
- **Why:** comment: discover without opening ZIPs so workers open them inside their bound.
- **Pinned by:** none.
- **Constants:** `VerzeJDF.txt`, `.zip`.
- **Diagnostic:** `Processing JDF batch {BatchPath}`, `Completed JDF batch {BatchPath}`, progress
  events `batch_started`/`batch_completed`/`batch_failed`.
- **Proposed verdict:** replace: the caller enumerates batches (sorted) and hands readers to
  `jr-formats` (AGENTS layering).

#### JDF-R003 — Output name and container
- **What:** output name = `Path.GetFileNameWithoutExtension(batchPath)`; `zip` → `<out>/<name>.zip`
  written atomically; `directory` → `<out>/<name>/`. Two inputs with the same stem in different
  folders overwrite each other (Python prevents this upstream).
- **Where:** `JdfCommands.fixJdf` 82–98.
- **Why:** unknown.
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** drop (no intermediate JDF files).

#### JDF-R004 — Any failing batch aborts the run
- **What:** every exception while parsing, fixing or writing a batch is captured per batch, and
  when results are consumed in order the first `Error` is re-raised: the whole `fix-jdf` fails.
  `--strict` changes nothing.
- **Where:** `JdfCommands.fixJdf` 85–113.
- **Why:** unknown (README documents `--strict` only for `merge-jdf`).
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** progress `batch_failed` with exception type and message; exception.
- **Proposed verdict:** replace (unsure): a malformed entity or batch is a diagnostic and the
  batch is quarantined; an unreadable ZIP is an input error (AGENTS working conventions). Q11.

### Stage 1 — external catalogues

#### JDF-R005 — External catalogue file set
- **What:** path is a file → that file alone (full path); a directory → all `*.csv` under it
  recursively, sorted by ordinal comparison of the relative path with `/` separators; neither →
  `External geodata path does not exist`; a directory without CSV →
  `External geodata directory contains no CSV files`.
- **Where:** `ExternalCsv.otherStopFiles`, `otherStopsFromPath` 64–80.
- **Why:** README: "stable relative-path order".
- **Pinned by:** `ExternalCsvTests.Loads one geodata CSV file`, `Loads nested directory
  deterministically and removes exact duplicates`, `Rejects geodata directory without CSV files`.
- **Constants:** `*.csv`.
- **Diagnostic:** `ArgumentException`.
- **Proposed verdict:** port (in the `jrutil` CLI, which owns the file system).

#### JDF-R006 — External row validation
- **What:** every row must have exactly 5 columns (`External geodata row must have five columns:
  {file} row {n}`); columns 2 and 3 must parse as invariant `Double` (`Invalid latitude|longitude
  in {file} row {n}: '{v}'`), be finite, latitude in [−90, 90], longitude in [−180, 180]. Any
  failure fails the whole run. No other plausibility check (a coordinate in Estonia is accepted,
  B19).
- **Where:** `ExternalCsv.otherStopsFromPath` 82–100.
- **Why:** README: the stop-only five-column contract; test name "obsolete six-column town rows".
- **Pinned by:** `ExternalCsvTests.Rejects obsolete six-column town rows`, `Rejects malformed
  geodata CSV`, `Rejects non-finite and out-of-range geodata coordinates`.
- **Constants:** 5 columns; coordinate ranges.
- **Diagnostic:** `ArgumentException` (build failure).
- **Proposed verdict:** port: a reviewed catalogue that does not parse is an input error.

#### JDF-R007 — External row decoding and provenance
- **What:** `Name` = column 1 verbatim; `Region` = column 4, `Country` = column 5, each `None` when
  `IsNullOrWhiteSpace`, otherwise verbatim (no trimming, no canonicalisation, `OL`/`DE` kept);
  `Source` = `external:{file name without extension}`; `SourceObjectId` =
  `external:{relative}:sha256:{hex}` (section 4); point = WGS84 (lon, lat) reprojected to LAEA;
  `candidateObservation` = `{observationId = SourceObjectId; sourceKind = "external";
  sourceObjectId = SourceObjectId; observedAt = None; rawTags = ""; explicitModes = "";
  deniedModes = ""; lifecycle = "active"; supportWeight = 1}`.
- **Where:** `ExternalCsv.otherStopsFromPath` 101–117, `otherStopRowsForJdfMatch` 31–62.
- **Why:** provenance for post evidence (README `--capture-exclude-source=external:PID.csv:`).
- **Pinned by:** `ExternalCsvTests.Loads stop rows with source provenance` (`external:legacy`).
- **Constants:** `external:`, `:sha256:`, `external` (kind), `active`, `1`.
- **Diagnostic:** none.
- **Proposed verdict:** port the facts; the provenance strings become model fields (source
  catalogue, row identity), built in `jr_model::identity`. Fix the single-file `"."` quirk
  (section 4).

#### JDF-R008 — External exact duplicates
- **What:** rows equal in `(Name, Lat, Lon, Region, Country)` (parsed values) are collapsed across
  all files, the first in R005 order kept with its `Source`/`SourceObjectId`.
- **Where:** `ExternalCsv.otherStopsFromPath` 118.
- **Why:** README "remove exact duplicate rows before constructing the stop matcher".
- **Pinned by:** `ExternalCsvTests.Loads nested directory deterministically and removes exact
  duplicates`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port.

### Stage 2 — OSM stops

#### JDF-R009 — OSM node eligibility
- **What:** a node is kept iff it has no `train` key, AND (it has no `railway` key OR it has a
  `tram` key), AND at least one of `highway=bus_stop`, `public_transport=platform`,
  `public_transport=pole`, `public_transport=station`, `railway=tram_stop`, `amenity=bus_station`
  (exact, case-sensitive). Consequence: a `railway=tram_stop` node without any `tram=*` tag is
  excluded (B9).
- **Where:** `OsmStops.getCzOtherStops` 70–81.
- **Why:** unknown (railway stations are CZPTT's).
- **Pinned by:** none.
- **Constants:** the tag list → `jdf.osm.eligible`, `jdf.osm.excluded_keys = ["train"]`,
  `jdf.osm.railway_requires_key = "tram"`.
- **Diagnostic:** none.
- **Proposed verdict:** port, fixing B9 (unsure whether intended).

#### JDF-R010 — OSM stop names
- **What:** `name` tag, else `name:cs`; `officialName` = `official_name`. No trimming.
- **Where:** `OsmStops.getCzOtherStops` 138–141.
- **Why:** unknown.
- **Pinned by:** none.
- **Constants:** tag names → `jdf.osm.name_tags = ["name", "name:cs"]`,
  `jdf.osm.full_name_tag = "official_name"`.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### JDF-R011 — OSM okres and country from municipality polygons
- **What:** the node's LAEA point is tested against all Czech municipality polygons (STR-tree on
  envelopes, then `Contains`); exactly one → its `region` code; none → no okres; several → no
  okres and `Log.Error`. Country = `CZ` iff an okres was found, else `None`. Every foreign node
  (and any Czech node outside every polygon) therefore has no country and no okres.
- **Where:** `CzRegions.czechTownByPoint`; `OsmStops.czOtherStopNameRegion`,
  `czOtherStopsForJdfMatch` 150–175.
- **Why:** code comment (CzRegions): okres is part of JDF stop identity.
- **Pinned by:** none.
- **Constants:** `"CZ"`.
- **Diagnostic:** `Too many results for CZ town query! {Results}` (error).
- **Proposed verdict:** port the okres derivation; replace the country derivation with a
  position-based country so foreign JDF stops can match OSM (Q15).

#### JDF-R012 — OSM name completion with the municipality
- **What:** with `tn`, `r` the containing municipality and okres (R011):
  1. no `official_name`, no name → name `""`, okres none;
  2. `official_name` n, no municipality → `n`, none;
  3. `official_name` n, municipality → `n`, `r` (comment: "official_name [is] the full name");
  4. no `official_name`, name n, no municipality → `n`, none;
  5. no `official_name`, name n, municipality → if every token of `tn` occurs in `n`
     (`nameSimilarity(tokens n, tokens tn) = 1`, synonyms applied, R040–R042) then `n`, else
     `"{tn},{n}"`; okres `r`.
- **Where:** `OsmStops.czOtherStopNameRegion` 49–66.
- **Why:** comment: "Stops within cities often omit the city's name on the pole".
- **Pinned by:** none.
- **Constants:** `,` joiner.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### JDF-R013 — OSM post-evidence facts
- **What:** per node: `explicitModes` = `road` if `highway=bus_stop` or `amenity=bus_station`
  (value compare case-insensitive) or `bus`/`psv` affirmative (`yes`|`designated`|`official` after
  trim + lower); then `tram` if `railway=tram_stop` or `tram` affirmative; joined `;`.
  `deniedModes` = `road` if `bus` or `psv` is `no` (trim, case-insensitive), `tram` if `tram` is
  `no`; joined `;`. `lifecycle` = the first of `abandoned`, `disused`, `construction`, `proposed`,
  `temporary` that exists as a key or equals the `lifecycle` tag value (case-insensitive), else
  `active`. `rawTags` = for the keys `highway; public_transport; railway; amenity; bus; psv; tram;
  trolleybus; access; vehicle; motor_vehicle; abandoned; disused; construction; proposed;
  temporary; local_ref` in this order, `key=value` with `;` in the value replaced by `,`, joined
  `;`. `candidateObservation = {observationId = "osm:node:{id}"; sourceKind = "osm";
  sourceObjectId = "osm:node:{id}"; observedAt = None; supportWeight = 1; …}`; `source =
  "osm:node:{id}"`.
- **Where:** `OsmStops.getCzOtherStops` 83–147, `czOtherStopsForJdfMatch` 161–172.
- **Why:** comments: modality is only what the source supplies; `local_ref` separates numbered bays
  for the learned post scorer.
- **Pinned by:** none here (post estimator tests consume the fields).
- **Constants:** all lists above → `jdf.osm.post_evidence.*` (or the posts inventory's policy).
- **Diagnostic:** none.
- **Proposed verdict:** port as reader facts (raw tags kept as a map, not a joined string); the
  interpretation belongs to the posts inventory.

### Stage 3 — matcher corpus and geographic data

#### JDF-R014 — Matcher corpus composition and order
- **What:** one index over `external rows (R005–R008 order) ++ OSM rows (PBF order)`. The
  document order breaks ties in Lucene's result order and thereby decides which candidate is
  "first" (R049: the name and okres reported for a match).
- **Where:** `JdfCommands.fixJdf` 57–58; `StopMatcher.index`.
- **Why:** unknown.
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** `Reading external stops`, `Reading OSM stops` (timed ops).
- **Proposed verdict:** replace: candidates are ordered explicitly (source class, catalogue,
  object ID); no dependence on an index's internal order.

#### JDF-R015 — Okres polygons
- **What:** `cz_regions.json.gz` → `Map code → Polygon` in LAEA; a repeated code keeps the last
  feature (one of the two `LT` polygons is lost, B2). Used for adjacency (R046), border tolerance
  (R046) and edge distance (R062). Unknown codes are absent.
- **Where:** `CzRegions.czechRegionPolygons`.
- **Why:** comment: okresy are part of JDF stop identity.
- **Pinned by:** the okres tests (`Adjacent okres candidate…`) build on it.
- **Constants:** the data file → `data/geo/cz_okres.geojson` (or the reviewed geodata checkout).
- **Diagnostic:** none.
- **Proposed verdict:** port with one geometry per code (multipolygon), fixing B2.

#### JDF-R016 — Municipality and European town gazetteers
- **What:** `czTownNameMatcher` = a `StopMatcher` over all 6,356 municipality names (data
  `(okres, polygon)`); `eurTownNameMatcher` = a `StopMatcher` over all 839,834 European names
  (data `(upper(country), lat?, lon?)`). Both memoised per process.
- **Where:** `JdfFixups.czTownNameMatcher`, `eurTownNameMatcher`; `CzRegions.czechTownsPolygons`;
  `EurTowns.eurTownCountries`.
- **Why:** comments at R050/R051/R053.
- **Pinned by:** none.
- **Constants:** the data files → `data/geo/cz_municipalities.geojson`,
  `data/geo/eur_town_names.csv`.
- **Diagnostic:** `Creating Czech town name matcher`, `Creating European town name matcher`.
- **Proposed verdict:** port as data; the matcher replaced by a name-key lookup (R041).

### Stage 5 — reading

#### JDF-R017 — File lookup inside a batch
- **What:** §2.1: directory → case-insensitive name, duplicates fail; ZIP → first entry whose
  file-name part matches case-insensitively, folders ignored, duplicates silently first.
- **Where:** `FileUtils.findPathCaseInsensitive`; `JdfParser.tryParseJdfTextFromZip`;
  `Jdf.fileParserTry`.
- **Why:** comment: "filename inside batch (case-insensitive)".
- **Pinned by:** `JdfParserTests.JDF ZIP writer is deterministic uncompressed and path parser
  compatible` (both containers).
- **Constants:** file names (§2.1).
- **Diagnostic:** `Multiple files found when looking for {name} in {dir} (case insensitive)`.
- **Proposed verdict:** port with one behaviour for both containers: duplicates (after
  case-folding, ignoring folders) are an input error.

#### JDF-R018 — Version detection
- **What:** §2.1 regex on the trimmed `VerzeJDF.txt`; `1.11`, `1.10`, `1.9`, `1.8` accepted; other
  → batch fails. The version record itself is then parsed with the version's record type.
- **Where:** `Jdf.jdfBatchDirVersion`, `jdfBatchDirParser`.
- **Why:** comment: "There were only minor changes from 1.8 to 1.9".
- **Pinned by:** none.
- **Constants:** regex `^"([0-9.]+)"[;,]`; version list.
- **Diagnostic:** `Unknown JDF version: "{v}"`.
- **Proposed verdict:** port.

#### JDF-R019 — Required and optional files per version
- **What:** the §2.1 table: missing required file → batch fails (`Option.get`); missing optional
  → empty relation; files not in the version's list are not read.
- **Where:** `Jdf.jdf111BatchDirParserWithCalls`, `jdf110BatchDirParserWithCalls`,
  `jdf109BatchDirParserWithCalls`.
- **Why:** unknown (follows the spec's optional files).
- **Pinned by:** none.
- **Constants:** the table.
- **Diagnostic:** exception.
- **Proposed verdict:** port; a missing required file is a batch diagnostic (R004).

#### JDF-R020 — Record grammar
- **What:** §2.2 exactly: terminator `";\r\n`; leading `"` required; whitespace-only record
  skipped; split on `","`; no unescaping; trailing unterminated text discarded silently.
- **Where:** `JdfParser.getJdfParser`.
- **Why:** comment: the spec says quotes inside text are not doubled, so `","` is an unescapable
  separator.
- **Pinned by:** `JdfParserTests.JDF parser retains a terminator split across read buffers`, `JDF
  parser preserves empty fields and undoubled quotes`, `JDF parser rejects records without an
  opening quote`.
- **Constants:** `";\r\n`, `","`, `"`.
- **Diagnostic:** `JDF line did not start with quote`.
- **Proposed verdict:** port the grammar; an unterminated tail and an LF-only file become
  diagnostics instead of silent loss (B12).

#### JDF-R021 — Field count
- **What:** fewer cells than the record type needs → batch fails (`IndexOutOfRange`); more →
  ignored.
- **Where:** `CsvParser.getRowParser`.
- **Why:** unknown.
- **Pinned by:** none.
- **Constants:** per-file field counts (§2.4).
- **Diagnostic:** unwrapped exception.
- **Proposed verdict:** port: short record → that record dropped with a diagnostic; extra cells →
  diagnostic once per file (unsure whether extra cells should be silent: real 1.10 files are
  exact).

#### JDF-R022 — Text encoding
- **What:** CP1250; a byte-order mark overrides it (UTF-8/UTF-16).
- **Where:** `JdfParser.jdfEncoding`, `getJdfParser` (`StreamReader` default); `Jdf.tryReadJdfText`.
- **Why:** JDF is CP1250 by specification; the BOM handling is a .NET default, not a decision.
- **Pinned by:** none.
- **Constants:** code page 1250.
- **Diagnostic:** none.
- **Proposed verdict:** port CP1250; a BOM is a diagnostic (unsure whether to honour it).

#### JDF-R023 — Scalar decoding
- **What:** §2.3 table: `""` → None for optional fields; required fields must decode; dates
  `ddMMyyyy`, times `HHmm`; bool `0`/`""`/`1`.
- **Where:** `CsvParser.colParserForBase`; `JdfParser.jdfColParserFor`.
- **Why:** comment: an empty bool "occurs in real-world data".
- **Pinned by:** `JdfParserTests.JDF parser preserves empty fields and undoubled quotes`.
- **Constants:** patterns.
- **Diagnostic:** `Failed parsing field "{f}" of value "{v}": …`.
- **Proposed verdict:** port in the reader as typed facts; `""` for a bool is reported as absent,
  and the `false` default becomes a named compiler rule (AGENTS rule 2: no hidden defaults).

#### JDF-R024 — Call times
- **What:** `|` → Passing, `<` → NotPassing, otherwise `HHmm`; empty → None. Times are clock
  times without day offset (day changes are inferred later, R068).
- **Where:** `JdfModel.TripStopTime.CsvParse`.
- **Why:** JDF specification.
- **Pinned by:** fixtures (`Impossible middle match…` uses `NotPassing`).
- **Constants:** `|`, `<`, `HHmm`.
- **Diagnostic:** parse exception.
- **Proposed verdict:** port.

#### JDF-R025 — Fixed codes (Pevnykod)
- **What:** §2.3 `Attribute`: all-digit value → day-of-week code with that integer (no 1–7
  check); otherwise the literal code, case-sensitive; codes of 1.10 and 1.11 accepted in any
  version.
- **Where:** `JdfModel.Attribute.CsvParse`.
- **Why:** comment: the type is shared because versions drop options (`s` in 1.10 only).
- **Pinned by:** none directly.
- **Constants:** the code table → `data/jdf/fixed_codes.csv` (code, meaning, versions).
- **Diagnostic:** `No union case for value "…"`.
- **Proposed verdict:** port: the reader keeps the code text; the compiler maps it, diagnoses
  unknown codes and day numbers outside 1–7.

#### JDF-R026 — Enumerations
- **What:** §2.3: `RouteType`, `TransportMode`, `ServiceNoteType`, `GeodataPrecision` by literal;
  `CompanyType` `""` and `0` read as Corporation.
- **Where:** `JdfModel` unions; `UnionCodec.getUnionParser`.
- **Why:** comment on `CompanyType`: "to parse bad batches".
- **Pinned by:** none.
- **Constants:** the code tables.
- **Diagnostic:** `No union case for value "…"`.
- **Proposed verdict:** port; `CompanyType` is unused downstream, so the reader may keep the raw
  text only.

#### JDF-R027 — Post-evidence validation on read
- **What:** after parsing a 1.11 batch and before writing any batch:
  `JdfModel.validatePostCandidateEvidence` requires `stopId > 0`, non-blank `observationId` and
  `sourceKind`, unique `(stopId, observationId)`, lat in [−90, 90], lon in [−180, 180],
  `supportWeight > 0`, every mode in `explicitModes`/`deniedModes` (split on `;` `,` space, trimmed,
  lower-cased) in {`road`, `tram`, `trolleybus`}, `lifecycle` (trimmed, lower) in {``, `active`,
  `current`, `temporary`, `construction`, `proposed`, `disused`, `abandoned`}. Failure → batch
  fails.
- **Where:** `JdfModel.validatePostCandidateEvidence`; `Jdf.jdf111BatchDirParserWithCalls`,
  `jdfBatchDirWriter`; `JdfFix.fixBatch` 75.
- **Why:** unknown (defensive contract of the extension file).
- **Pinned by:** none.
- **Constants:** the two sets.
- **Diagnostic:** `ArgumentException` with the observation ID.
- **Proposed verdict:** drop: no extension files exist in the rewrite; the model's types enforce
  the same facts.

#### JDF-R028 — Upgrade defaults 1.8/1.9 → 1.10
- **What:** §2.5: distinctions `0`, transport mode `Bus`, `detour`/`grouped`/`usesStopPosts`
  false, missing option fields None, missing relations empty, version metadata None.
- **Where:** `Jdf109To110.jdf109To110Converter`, `jdf109TripStopTo110Converter`, `nameOptGetters`.
- **Why:** unknown.
- **Pinned by:** none.
- **Constants:** `0`, `Bus`, `false`.
- **Diagnostic:** none.
- **Proposed verdict:** replace: the reader returns version-specific facts with absent values as
  `None`; each default the compiler needs is a named rule (e.g. "a 1.9 line without mode is a
  bus"). Unsure whether 1.8/1.9 still occur (none in the sample, Q10).

#### JDF-R029 — 1.8/1.9 Caskody note type
- **What:** the 1.9 field `timeType` (position 5) has no same-named target; the 1.11 field
  `noteType` is option-typed and absent in the source, so it becomes `None`. Every service-note
  type of a 1.8/1.9 batch is lost (B1).
- **Where:** `Jdf109To110.jdf109To110Converter` (`serviceNoteAC` via `idDistGG`).
- **Why:** none (accident of name-based copying).
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port fixed: field 5 is the note type in every version.

#### JDF-R030 — Upgrade defaults 1.10 → 1.11
- **What:** `oneWay` false; a missing `timetableValidTo` becomes `3000-01-01`; `minArrivalTime`,
  `maxDepartureTime`, `Transfer.note` None; extension relations empty.
- **Where:** `Jdf110To111.jdf110To111Converter`, `jdf110TripStopTo111Converter`.
- **Why:** comment: "just an arbitrary large date".
- **Pinned by:** none.
- **Constants:** `3000-01-01`.
- **Diagnostic:** none.
- **Proposed verdict:** replace: an open validity end is `None` in the facts; the merge/calendar
  inventory decides how an open end is bounded (it clamps to the GVD).

#### JDF-R031 — Upgraded attribute arrays keep their length
- **What:** 1.9/1.10 `TripStop.attributes` (2 cells) is copied as a length-2 array into the 1.11
  record whose field declares 3; the serializer pads the third cell with `""`.
- **Where:** `AutoCopy.getAutoCopier`; `CsvSerializer.getRowSerializer`.
- **Why:** none.
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** drop (typed facts hold a list of codes, not fixed-width columns).

### Stage 6–7 — batch-level checks

#### JDF-R032 — International route decisions are only logged here
- **What:** `applyInternationalRoutePolicy policy batch` runs on the parsed, **unfixed** batch
  (stop countries as in the source; a stop with an okres but no country counts as `CZ`, one with
  neither as unknown); `prepareGtfsCalendar batch` is computed first even for `keep-all`. The
  filtered batch is discarded; only `decisions` are kept and logged once after all batches.
- **Where:** `JdfFix.fixBatch` 45–47, 93; `JdfCommands.fixJdf` 117–118;
  `JdfInternationalFilter.applyInternationalRoutePolicy`.
- **Why:** module comment: "classify its international routes".
- **Pinned by:** `InternationalRouteFilterTests` (other inventory).
- **Constants:** see the international-filter inventory.
- **Diagnostic:** `logInternationalRouteDecisions` summary (other inventory).
- **Proposed verdict:** drop here: the policy is applied (and diagnosed) where it changes the
  timetable; evaluating it on unfixed geography gives different answers (B21).

#### JDF-R033 — Degenerate batch
- **What:** called stop IDs = distinct `stopId` of calls with a `StopTime` arrival or departure;
  fewer than 2 → every relation is emptied except `version`, `postCandidateEvidence` and
  `routingDemands`; no matching runs; the (almost) empty batch is still written; match counters
  are zero.
- **Where:** `JdfFixups.dropDegenerateBatch` 812–834; `JdfFix.fixBatch` 49–53.
- **Why:** gapfill README ("ignores degenerate trips with fewer than two distinct stop places");
  log text.
- **Pinned by:** `Batch with one distinct called stop is dropped completely`.
- **Constants:** `2` → `jdf.batch.min_called_stops = 2`.
- **Diagnostic:** `Dropping degenerate JDF batch with fewer than two distinct called stops`
  (warning).
- **Proposed verdict:** port (the batch contributes nothing; diagnostic with the batch name).

#### JDF-R034 — One route per batch expected
- **What:** `routes.Length <> 1` → error logged, processing continues; the route-name town
  (R058) uses `routes.[0]` (zero routes → `IndexOutOfRange`, batch and run fail, B15). All trips and
  route stops of all routes are pooled by trip ID / route stop ID in R035.
- **Where:** `JdfFixups.fixPublicCisJrBatch` 852–854, 864.
- **Why:** CIS exports have one line per batch.
- **Pinned by:** none.
- **Constants:** `1`.
- **Diagnostic:** `Expected just one route in batch, got {RouteCount}` (error).
- **Proposed verdict:** port as a diagnostic, and key all per-trip work by `(route, distinction,
  trip)` so multi-route batches are handled correctly.

#### JDF-R035 — Trip matrices
- **What:** route-stop IDs = all `Zaslinky.routeStopId` sorted ascending (duplicates kept). Trips
  = calls grouped by `tripId` only. For odd trip IDs (forward) and even trip IDs (reverse)
  separately: each trip becomes an array over the sorted route-stop IDs of its call at that route
  stop or none (lookup map from route stop ID, last call wins); trips are grouped by their
  kilometre pattern (array of `kilometer` options) and the first trip of each group is kept
  ("one trip per physical route"); even trips' arrays are reversed (travel order); the result is
  transposed (rows = route stops in travel order, columns = representative trips); rows where no
  column has a call are removed. Matrix 1 = odd, matrix 2 = even. A row is joined to its stop by
  the first present call's `stopId`; a stop ID missing from `Zastavky` throws `JoinException`
  (batch and run fail).
- **Where:** `JdfFixups.groupedTripsMatrices` 581–604; `Utils.innerJoinOn` users.
- **Why:** comment: "grous them twice: by direction and then by pattern".
- **Pinned by:** none.
- **Constants:** odd = forward (`Jdf.tripIsReverse`: even trip ID).
- **Diagnostic:** `Could not match left key …` (exception).
- **Proposed verdict:** port as the shared helper of R052 and R061, keyed by `(route,
  distinction, trip)`.

### Stage 8a — stop names

#### JDF-R036 — Okres suffix in the town name
- **What:** if `town` matches `(.*) \[(..)\]$`: `town` = group 1, `regionId` = group 2 (any two
  characters), `country` = `CZ`, overwriting existing values. Applied to the stops of the non-MHD
  path and of MHD detection, **not** to the MHD rename path (B4).
- **Where:** `JdfFixups.moveRegionFromName` 146–157; `fixPublicCisJrBatch` 859.
- **Why:** comment: "Some JDF batches in CIS JŘ public exports have the nearby town two-letter id
  appended to the town name like so: "Praha [AB]"".
- **Pinned by:** none.
- **Constants:** regex; `CZ` → `jdf.names.okres_suffix_country = "CZ"`.
- **Diagnostic:** none.
- **Proposed verdict:** port (unsure: no occurrence in the 180-batch sample), on every path.

#### JDF-R037 — Stop name normalisation (written to the output)
- **What:** three branches on the stop:
  1. `district` and `nearbyPlace` both None and `town` matches `([^,]+),([^,]*),([^,]*)`
     (unanchored: the first match; text after a third comma is lost, B6) → `town` = g1, `district`
     = g2 or None if `""`, `nearbyPlace` = g3 or None if `""`;
  2. else if `district` is None → `district` = old `nearbyPlace`, `nearbyPlace` = None (the
     dominant real case `"Praha","","Muzeum"` becomes `"Praha","Muzeum",""`);
  3. else unchanged.
  The result replaces the stop's name fields in the fixed batch (non-MHD path), so it reaches
  merge and the published `location` (B5).
- **Where:** `JdfFixups.normaliseStopName` 169–185; `fixPublicCisJrBatch` 862, 867.
- **Why:** comment: names are inconsistent; "give it a normalised name that can later be compared
  between batches, not necessarily [the] official stop registry name".
- **Pinned by:** none.
- **Constants:** regex.
- **Diagnostic:** none.
- **Proposed verdict:** replace: a matching key only; the published triple stays as in the source
  (Q1).

#### JDF-R038 — Single-comma split for non-MHD batches
- **What:** after R037, if `district` and `nearbyPlace` are None and `town` matches
  `([^,]+), *([^,]+)` (unanchored) → `town` = g1 (trailing spaces kept), `district` = g2. Only on
  the non-MHD path.
- **Where:** `JdfFixups.normaliseNonMhdStopName` 189–196.
- **Why:** comment: "When we know (guess, really) that a batch doesn't use MHD naming, we can split
  stop names with single commas".
- **Pinned by:** none.
- **Constants:** regex.
- **Diagnostic:** none.
- **Proposed verdict:** replace together with R037 (matching key; unsure whether the split should
  be published).

### Stage 8b — exact matching

#### JDF-R039 — Name tokens
- **What:** the query is the name string (`"{town},{district},{nearby}"`, None as `""`).
  Tokenisation (`stopNameToTokens`): replace `.` by `. `, `,` and `-` by space, U+00A0 by space,
  `ToLower()` (current culture), split on space, drop blank tokens. Diacritics are kept;
  punctuation other than `. , -` stays inside tokens (`(Hlavní` stays a token).
- **Where:** `StopMatcher.preprocessStopName`, `stopNameToTokens`; `Jdf.jdfStopNameString`;
  `JdfFixups.matchStopByName`.
- **Why:** comment: "Make sure successive acronyms are tokenized as separate".
- **Pinned by:** `StopMatcherTests.Basic stop search` (`praha dejvice` ↔ `Praha-Dejvice`).
- **Constants:** the replacement table → `data/names/jdf_token_rules.toml`.
- **Diagnostic:** none.
- **Proposed verdict:** replace with the one name normaliser (`jr-geo`) extended by a synonym
  step (section 8, Q5); culture-invariant lower-casing.

#### JDF-R040 — Abbreviation synonyms
- **What:** Solr synonym lists, `expand=true`, applied at index time and to single tokens when
  scoring: `nám.=náměstí`; `hl.=hlavní`; `rozc.=rozcestí`; `odb.=odbočka`; `žel.=železniční`;
  `aut.=autobusové`; `nádr.=st.=nádraží=stanice=stanoviště`; `zast.=zastávka`; `rest.=restaurace`;
  `sídl.=sídliště`; `nem.=nemocnice`; `háj.=hájenka=hájovna`; `obú=obecní úřad`; `měú=městský
  úřad`; `nákup.=nákupní`; `stř.=středisko`; `pož.=požární`; `zbroj=zbrojnice`; `ul.=ulice`;
  `hor.=h.=horní`; `dol.=d.=dolní=der`; `vel.=velký=velká=velké`; `n.=nad=nisou`; `p.=pod`;
  `l.=labem`; `orl.=orlicí=orlických`; `vlt.=vltavou`; `svit.=svitavou`;
  `jiz.=jizerských=jizerou`; `met.=metují`; `cidl.=cidlinou`; `bez.=bezd.=bezdězem`;
  `jev.=jevišovkou`; `mor.=moravě=moravou`; `des.=desnou`; `bystř.=bystřicí`; `sáz.=sázavou`;
  `dřev.=dřevnicí`; `h.=horách`; `zdrav.=zdravotnická`; `heřm.=heřmanice`; `l.=lázně`;
  `sv.=svatý=svatá=svaté=svatého=svaté`; `a.=an`; `i.=in=im`; `w.=wald`. `l.`, `h.`, `d.` and
  `n.` belong to two lists each, so the relation is not transitive (`labem` and `lázně` both
  expand to `l.`, R042).
- **Where:** `GeoData/StopMatcherSynonyms.txt`; `StopMatcher.makeAnalyzer`.
- **Why:** file comment: "matching different versions of stop names together"; `aut.nádr.` and
  `aut.st.` are used as synonyms.
- **Pinned by:** none.
- **Constants:** the file → `data/names/stop_abbreviations.txt` (reviewed data).
- **Diagnostic:** none.
- **Proposed verdict:** port as data; define the equivalence explicitly (token → set of
  canonical forms) instead of Lucene's analyser semantics.

#### JDF-R041 — Candidate retrieval
- **What:** a candidate is retrieved iff every query token (raw, not expanded) occurs among the
  candidate's index tokens (its own tokens plus their synonyms); at most 10,000 results, in
  Lucene relevance order with ties in index order (R014). No tokens → no candidates.
- **Where:** `StopMatcher.stopNameToQuery`, `matchStop`.
- **Why:** comment: Lucene's order is taken and re-scored.
- **Pinned by:** `StopMatcherTests.Basic stop search`.
- **Constants:** `10000` → drop (no cap needed without Lucene).
- **Diagnostic:** none.
- **Proposed verdict:** replace: an inverted token index (no Lucene). Only two predicates are ever
  used: "contains every query token" (MHD candidates, R056) and score 1 (everywhere else).

#### JDF-R042 — Name score and exactness
- **What:** score = (number of candidate tokens whose synonym expansion intersects the expansion
  of the query tokens) / (number of candidate tokens), `float32`. A match is *exact* iff score =
  `1.0f`. With R041 this means: every query token is in the candidate (via the candidate's
  synonyms) and every candidate token has a synonym-related query token; order and repetition
  are ignored. A separate predicate `checkExactMatch(a, b)` requires equal length and, position by
  position, equal expansion sets (order matters); it is used by the MHD rules (R056,
  R058).
- **Where:** `StopMatcher.nameSimilarity`, `checkExactMatch`; `JdfFixups.exactMatches` 207.
- **Why:** comment: "How many of the matched stop's words are also in the query".
- **Pinned by:** `StopMatcherTests.Basic stop search`; `Historical OL code matches canonical OC
  strictly` (score 1 fixtures).
- **Constants:** `1.0`.
- **Diagnostic:** none.
- **Proposed verdict:** port as two named predicates ("same token set modulo abbreviations",
  "same token sequence modulo abbreviations"), with a symmetric equivalence.

#### JDF-R043 — Country filter
- **What:** among exact candidates: if the stop has a country `c`, the candidate's country must be
  `Some c` (literal comparison; a candidate without country is rejected; `D` ≠ `DE`, B7); a stop
  without country accepts any candidate. Each rejection increments `countryRejects`.
- **Where:** `JdfFixups.exactMatches` 203–216.
- **Why:** comment: "If the stop has a country assigned, honour it."
- **Pinned by:** `Adjacent okres candidate outside tolerance is rejected` (second half, `D`
  candidate), `Non-adjacent okres and wrong country candidates are rejected`.
- **Constants:** none.
- **Diagnostic:** counter `countryRejects` (R074).
- **Proposed verdict:** port with one canonical country vocabulary in the model (Q8).

#### JDF-R044 — Okres filter, strict first
- **What:** after R043: stop country present and ≠ `CZ` → no okres check; stop okres None → no
  check; otherwise (country `CZ` or None, okres `r`): candidates whose canonical okres (R045)
  equals canonical `r` ("strict"); if any → only those (`strictRegionMatches += count`); else the
  country-filtered candidates that pass R046.
- **Where:** `JdfFixups.exactMatches` 218–238.
- **Why:** comment: "Only broaden to an adjacent-boundary candidate when no strict okres candidate
  exists. Mixing both sets makes a known strict match appear geographically ambiguous."
- **Pinned by:** `Strict okres candidate wins over an adjacent boundary candidate`, `Foreign exact
  match does not require a Czech okres code`.
- **Constants:** `CZ` → `jdf.match.home_country = "CZ"`.
- **Diagnostic:** counter `strictRegionMatches`.
- **Proposed verdict:** port.

#### JDF-R045 — `OL` is `OC`
- **What:** okres code `OL` is compared as `OC` on both sides in R044/R046; the source value is
  not rewritten. Nowhere else (R062 uses the raw code, B3).
- **Where:** `JdfFixups.canonicalRegionCode` 108–110.
- **Why:** comment: "OL occurs in current CIS JŘ exports, while the JDF/SPZ okres code and the
  checked-in boundary data use OC for Olomouc."
- **Pinned by:** `Historical OL code matches canonical OC strictly`.
- **Constants:** `OL → OC` → `data/jdf/okres_aliases.csv`.
- **Diagnostic:** none.
- **Proposed verdict:** port as data, applied everywhere an okres code is compared (Q9).

#### JDF-R046 — Adjacent-okres border tolerance
- **What:** a candidate with a different okres passes iff the two okres polygons are adjacent
  (planar polygon distance ≤ `100` m, all pairs precomputed once) and the candidate point lies
  within `1000` m of the expected okres polygon's boundary (cached per `(okres, x, y)`); counters:
  pass → `borderRegionMatches`, adjacent but farther → `outsideBorderToleranceRejects`, candidate
  without okres or not adjacent or unknown code → `nonAdjacentRegionRejects`.
- **Where:** `JdfFixups.czechRegionAdjacency`, `regionsAdjacent`, `withinExpectedRegionBorder`,
  `regionMatches` 89–141.
- **Why:** test names: catalogue and JDF okres disagree for stops on an okres border.
- **Pinned by:** `Adjacent okres candidate on expected boundary is accepted`, `Adjacent okres
  candidate outside tolerance is rejected`, `Non-adjacent okres and wrong country candidates are
  rejected`.
- **Constants:** `100` m → `jdf.match.okres_adjacency_m = 100`; `1000` m (`maxRadiusMetres`) →
  `jdf.match.okres_border_tolerance_m = 1000`.
- **Diagnostic:** the three counters.
- **Proposed verdict:** port.

#### JDF-R047 — Checked catalogues before OSM
- **What:** exact candidates after R043–R046 are split into checked (source not `osm:`) and OSM.
  The checked set is tried first with R048; if it yields no group (empty or ambiguous), the OSM
  set is tried. The two are never mixed.
- **Where:** `JdfFixups.selectedExactMatches` 297–313.
- **Why:** comment: "Checked catalogues get the first chance, but an ambiguous checked set must
  not suppress an otherwise usable OSM fallback."
- **Pinned by:** `Checked exact match wins over an OSM exact match`.
- **Constants:** prefix `osm:` → a model field (source class), not a string test.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### JDF-R048 — Candidate group validity
- **What:** a group is usable iff non-empty, all candidates share one `(okres, country)` pair, and
  the radius (largest planar distance from the arithmetic mean of the points) is `< 1000` m;
  otherwise no match.
- **Where:** `JdfFixups.validCandidateGroup` 282–295; `Common.pointsRadius`.
- **Why:** doc comment: "Matches that are too spread out … aren't considered for matching".
- **Pinned by:** indirectly by the okres tests.
- **Constants:** `1000` m → `jdf.match.max_group_radius_m = 1000`.
- **Diagnostic:** `Not considering match for stop {StopId} since radius {Radius:n1} is too high`
  (debug).
- **Proposed verdict:** port: ambiguity is quarantined (AGENTS), with a coded diagnostic.

#### JDF-R049 — Position and attributes of a match
- **What:** the match's point = centroid of the group's points (mean in LAEA); name, okres and
  country = the first candidate's (R014 order); source = distinct candidate sources, sorted
  ordinally, joined with `+` (`external:IDSJMK+external:MPVNet_ODIS`, `osm:node:1+osm:node:2`).
- **Where:** `JdfFixups.topStopMatch` 315–337.
- **Why:** unknown (several catalogues and posts describe one stop place).
- **Pinned by:** `Checked exact match wins over an OSM exact match` (single candidate).
- **Constants:** `+`.
- **Diagnostic:** none.
- **Proposed verdict:** port the centroid (Q4); the source becomes a list of provenance records,
  not a joined string.

#### JDF-R050 — Town-only stop: European town veto
- **What:** for a stop whose `district` and `nearbyPlace` are None, a match is discarded when the
  first comma part of the matched catalogue name is an exact name in the European gazetteer
  (839,834 names, including `Praha`, `Most`, `Wien`).
- **Where:** `JdfFixups.matchConflictsWithEurCity` 400–408; `matchStops` 791–792.
- **Why:** comment: "Some Czech towns share names with ones with other countries. Since we only
  have coordinates for Czech stops (for now), we need to check if we aren't possibly matching a
  foreign town to a Czech stop."
- **Pinned by:** none.
- **Constants:** the gazetteer (R016).
- **Diagnostic:** none.
- **Proposed verdict:** unsure: the premise ("only Czech coordinates") no longer holds (the
  catalogues have `D`, `PL`, `SK`, `A` rows and OSM covers neighbours); a country check (R043)
  may replace it. Q16.

#### JDF-R051 — Town-only stop named like a municipality
- **What:** for a town-only stop whose `town` is an exact Czech municipality name (after R054
  synonyms), the primary match is discarded; only secondary km matching (R052) can position it.
  This covers most single-stop villages (`Adamov`).
- **Where:** `JdfFixups.matchStops` 793–798; `matchesCzTownByName`.
- **Why:** log text: "its only name component matches a Czech town".
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** `Not considering stop matches for stop {StopId} since its only name component
  matches a Czech town` (debug).
- **Proposed verdict:** port (unsure); with a diagnostic so the number of villages left to R052
  and estimation is visible.

### Stage 8c — secondary matching and town geography

#### JDF-R052 — Secondary match by kilometre distance
- **What:** run twice, over matrix 1 then matrix 2, tolerance `1.0` km. Rows in travel order;
  per column (representative trip) the last confirmed `(point, km)`. For each row and column: if
  the stop has a match and the call has km → record `(point, km)`, suggestion = the match; if the
  stop has no match, the call has km and the column has a last position `(lp, lkm)` → candidates
  = **all** retrieved candidates (any score) with planar distance to `lp` `< |km − lkm| + 1.0`
  km, then R043–R049 on them (exact only) → suggestion (may be none); otherwise suggestion = the
  current match. Suggestions equal to the current match are discarded; if the first remaining
  suggestion is a match and all remaining suggestions are equal to it → it becomes the stop's
  match; otherwise unchanged (`Trips disagreed…`). Secondary picks do not update the column's
  last position within the pass. A stop on several rows keeps the first row result that has a
  match; stops on no row are appended after the rows (stop order changes, R065).
- **Where:** `JdfFixups.addSecondaryMatches` 608–676; `matchStops` 800–801.
- **Why:** doc comment: "matches … disambiguated by previous matches' positions and km data".
- **Pinned by:** none.
- **Constants:** `1.0` km → `jdf.match.secondary_km_tolerance = 1.0`.
- **Diagnostic:** `Picked position for {StopId} by secondary match`, `Trips disagreed on secondary
  match for {StopId}` (debug).
- **Proposed verdict:** port.

#### JDF-R053 — Okres and country from town names (unmatched stops)
- **What:** for a stop still without a match: country `CZ` and okres None → if the exact
  municipality matches of `town` (R054 synonyms applied) give exactly one distinct `(name,
  okres)` → set okres. Country and okres None → European exact match (first with score 1) and
  Czech matches: one Czech, no European → okres and country `CZ`; no Czech, a European →
  `country = upper(ISO alpha-2)` (`DE`, `AT`, `SK`, …; B7); else unchanged. Coordinates are never
  set ("Town polygons may fill missing geography, but never coordinates").
- **Where:** `JdfFixups.addGeographyFromTopTown` 417–437, `matchingCzTownRegions`; `matchStops`
  802–808.
- **Why:** code comment above.
- **Pinned by:** none.
- **Constants:** `CZ`.
- **Diagnostic:** none.
- **Proposed verdict:** port with a diagnostic (an inferred okres is an inferred identity part,
  Q3), writing JDF country codes.

#### JDF-R054 — Town name synonyms
- **What:** before a municipality lookup by name, an exact key lookup replaces `Dolní Malá Úpa` →
  `Malá Úpa`, `Horní Malá Úpa` → `Malá Úpa`, `Podhořany u Ronova n.D.` → `Podhořany u Ronova`.
  Applied in R051, R053, R056, R058, R059; **not** in R063 (B14).
- **Where:** `CzRegions.townSynonyms`; `JdfFixups.matchCzTownByNameRaw`.
- **Why:** comment: "names of towns in timetables don't match official town names … (hopefully
  forever) small list".
- **Pinned by:** none.
- **Constants:** → `data/jdf/town_synonyms.csv`.
- **Diagnostic:** none.
- **Proposed verdict:** port as data, applied in every municipality lookup.

#### JDF-R055 — Geography from the match
- **What:** after the MHD choice: a stop without okres and with a match takes the match's okres
  **and** country (both, even when the match has no country).
- **Where:** `JdfFixups.addRegionFromMatch` 240–245; `fixPublicCisJrBatch` 923.
- **Why:** unknown.
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port; better replace (unsure): the okres of a matched stop is the okres
  polygon containing its final coordinate.

### Stage 8d — MHD naming

#### JDF-R056 — MHD town candidates
- **What:** over the stops after R036+R037 (not R038): for each town-only stop, skip it if it has
  an exact match (R043–R046, counters incremented) **and** the first comma part of its town
  (trimmed) is an exact municipality name. Otherwise for every retrieved candidate (R041, any
  score): `t` = first comma part of the candidate name, trimmed; keep `t` iff
  `checkExactMatch(tokens "{t},{stop.town}", tokens candidate)` and not
  `checkExactMatch(tokens stop.town, tokens candidate)`; distinct `t` per stop, each mapped to the
  head of its R057 list. Count stops per town; keep towns with count `> 1`; sort descending by
  count (stable).
- **Where:** `JdfFixups.mhdNamingTownCandidates` 693–742.
- **Why:** doc comment: some batches "put everything in the town column. This is often used for
  intra-town lines, and the stop names don't contain the town name directly."
- **Pinned by:** none.
- **Constants:** `1` → `jdf.mhd.min_supporting_stops = 2`.
- **Diagnostic:** `Not considering stop {StopId} for MHD town due to existing match` (debug).
- **Proposed verdict:** port (Q2).

#### JDF-R057 — Joined MHD towns
- **What:** one hard-coded list `["Ústí nad Labem"; "Ústí n.L."; "Trmice"; "Telnice";
  "Petrovice,Krásný Les"]`: a candidate town in the list counts as its head (`Ústí nad Labem`);
  when the head wins, all five are tried in R059, in list order.
- **Where:** `JdfFixups.mhdTownsJoined` 680–683; `mhdNamingTownCandidates` 732–736;
  `fixPublicCisJrBatch` 892–897.
- **Why:** comment: "The CIS JŘ exports sometimes don't even have one consistent town for one
  timetable. Thankfully it seems rare, so we fix it manually."
- **Pinned by:** none.
- **Constants:** → `data/jdf/mhd_town_groups.csv`.
- **Diagnostic:** `Expanded MHD town to list: {Towns}` (debug).
- **Proposed verdict:** port as data.

#### JDF-R058 — Route-name town boost
- **What:** the route name is matched with `MHD ([^:]*) linka|MHD ([^ :]*)|([^,:-]*) *[,:-]`; the
  first successful group of 1–3 is the town if it is an exact municipality name. A candidate town
  `t` with `checkExactMatch(tokens t, tokens routeTown)` gets `+2`; candidates are re-sorted
  descending (stable) and the first wins.
- **Where:** `JdfFixups.routeDescTownNameRegex`, `townNameFromRouteDesc` 810, 836–844;
  `fixPublicCisJrBatch` 864–881.
- **Why:** comment: "Boost town that matches name in routes file".
- **Pinned by:** none.
- **Constants:** regex; `2` → `jdf.mhd.route_name_bonus = 2`.
- **Diagnostic:** `MHD town candidates: {Candidates}`, `Trying candidate town for MHD naming:
  {Town}` (debug).
- **Proposed verdict:** port.

#### JDF-R059 — MHD renaming of a stop
- **What:** applied to the **raw** source stops (no R036, R037, R038, B4). A stop with `district`
  or `nearbyPlace` is unchanged. Otherwise `parts = town.Split(",")` (more than 3 parts → warning);
  `rest` = `parts[1..]` joined by space, trimmed; for each town `T` of the R057 list (or the
  single town): candidate `{town = T; district = parts[0]; nearbyPlace = rest or None}`; the first
  candidate with a match (R043–R049) is used; else if `parts[0]` is an exact municipality name →
  `{town = parts[0]; district = parts[1] if ≥ 2 parts; nearbyPlace = parts[2] if exactly 3
  parts}`; else the first candidate. The renamed stop is what the fixed batch publishes.
- **Where:** `JdfFixups.fixMhdNaming` 744–781.
- **Why:** comment: "We need to check if this is actually an MHD name or if the first element is a
  town".
- **Pinned by:** none.
- **Constants:** `3` parts.
- **Diagnostic:** `MHD-named stop with more than two commas: {StopName}` (warning).
- **Proposed verdict:** port on the R036–R037 stops (unsure whether the renaming may change the
  published name, Q2).

#### JDF-R060 — MHD or not
- **What:** with a winning town, both stop sets are matched (R039–R053); the MHD set is used iff
  its number of matched stops `>` the non-MHD number `+ 1`; otherwise the non-MHD set. No winning
  town → non-MHD.
- **Where:** `JdfFixups.fixPublicCisJrBatch` 883–912.
- **Why:** unknown.
- **Pinned by:** none.
- **Constants:** `1` → `jdf.mhd.min_match_gain = 2` (strictly more than one).
- **Diagnostic:** `Comparing MHD stops with {MhdMatches} matches vs. non-mhd with {NonMhdMatches}
  matches`, `No town candidate for MHD stops, Proceeding with unmodified stops` (debug).
- **Proposed verdict:** port with a coded diagnostic naming the chosen town.

### Stage 8e — okres and country propagation

#### JDF-R061 — Propagation along trips
- **What:** rows in order; state `lastRegion`, `lastCountry`:
  - okres `r` and country `c` → state := (r, c), stop unchanged;
  - no okres, country `c` → if `c = lastCountry` the stop takes `lastRegion` (may be None); else
    state := (None, c), stop unchanged;
  - okres `r`, no country → `lastRegion := r`; the stop takes `lastCountry`;
  - neither → the stop takes `lastRegion` and `lastCountry`.
- **Where:** `JdfFixups.fillStopRegionsFromPrevious` 445–489.
- **Why:** comment: "tripStops in a trip are assumed to be sorted by the order they appear on the
  line".
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port (unsure, Q3).

#### JDF-R062 — Revert an uncertain propagated okres
- **What:** if R061 changed the okres, it is reverted (whole stop back, country too) unless at
  least one column has evidence against crossing: a column counts against the change iff its last
  matched Czech position `(c, r, km, point, edgeKm)` has `c` and `r` equal to the new country and
  okres and the call has km, and `|km − lastKm| > edgeKm`; columns without such data count as
  agreeing with reversion ("forall"), so with no evidence at all the change is always reverted.
  Last positions are updated after each row from the **original** stop: match present, km
  present, country `CZ`, okres `r` → `edgeKm` = planar distance from the match point to the shell
  of okres `r`'s polygon / 1000; an okres code missing from the polygons (e.g. `OL`) throws (B3).
- **Where:** `JdfFixups.fillStopRegionsFromPrevious` 490–510, 546–554;
  `distanceToCzRegionEdge` 439–441.
- **Why:** comment: "If we changed the region but our change is uncertain, revert it"; "could
  cross region boundary".
- **Pinned by:** none.
- **Constants:** km / 1000 conversion.
- **Diagnostic:** `Not setting region for {StopId} from previous stop since distance is too large
  (could cross region boundary)` (debug).
- **Proposed verdict:** port with R061 (fix B3 via R045).

#### JDF-R063 — Okres by municipality name and km distance
- **What:** if after R061–R062 the country is None or `CZ` and the okres None: exact municipality
  matches of the original `town` (no R054 synonyms, B14), kept iff some column has a last position
  `(lkm, lp)` and km with `polygon.Distance(lp)/1000 < |km − lkm| + 1`; distinct okres codes;
  exactly one → country `CZ`, okres set.
- **Where:** `JdfFixups.fillStopRegionsFromPrevious` 512–544.
- **Why:** comment: "Try to find region by looking at possible towns".
- **Pinned by:** none.
- **Constants:** `1` km → `jdf.geography.town_km_tolerance = 1.0`.
- **Diagnostic:** `Picked region by distance for {StopId}: {Region}`, `Can't pick region by
  distance for {StopId} from {Regions}` (debug).
- **Proposed verdict:** port with a diagnostic (Q3).

#### JDF-R064 — Propagation passes
- **What:** R061–R063 run over matrix 1, matrix 1 reversed (rows), matrix 2, matrix 2 reversed,
  each pass only if some stop still has no country, or country `CZ` and no okres.
- **Where:** `JdfFixups.fixPublicCisJrBatch` 914–929.
- **Why:** unknown.
- **Pinned by:** none.
- **Constants:** pass order.
- **Diagnostic:** none.
- **Proposed verdict:** port (order-dependent; consider a fixed point instead).

#### JDF-R065 — Stops on several rows; stop order
- **What:** a stop on several rows keeps one result: after R052 the first row result with a match;
  after R061–R064 the first row result sorted by `(okres present, country present)` descending.
  Stops on no row are appended in their previous order. The fixed batch's `Zastavky` order is the
  order produced by the last of these steps, not the source order.
- **Where:** `JdfFixups.addSecondaryMatches` 662–676; `fillStopRegionsFromPrevious` 558–577.
- **Why:** comment: "Stops can occur multiple times, pick the one with the most information".
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** replace: one value per stop chosen by a stated rule; source order kept.

### Stage 8f — post candidates

#### JDF-R066 — Post candidate collection
- **What:** for every stop of the chosen set (before R055–R065), if R047/R048 select a group:
  centre = centroid of the group; candidates = all exact candidates (R043–R046, recomputed) within
  `1000` m of the centre whose country is compatible (the group has no countries, or the candidate
  has none, or it is one of the group's). This includes OSM candidates when a checked group was
  selected, and vice versa.
- **Where:** `JdfFixups.candidateMatches` 358–375; `fixPublicCisJrBatch` 931–939.
- **Why:** comment: "Fixing preserves source facts only. Physical clustering is a policy decision
  and therefore belongs exclusively to the v2 evaluator."
- **Pinned by:** none.
- **Constants:** `1000` m → `jdf.posts.candidate_radius_m = 1000`.
- **Diagnostic:** none.
- **Proposed verdict:** port (as input facts of the posts pass).

#### JDF-R067 — Post candidate records
- **What:** candidates with an observation, distinct by `observationId` (first kept), sorted by
  `observationId` (ordinal); each → `PostCandidateEvidence{stopId; candidateId = sha256 of the
  observation's source object ID (section 4); observationId; sourceKind; sourceObjectId;
  observedAt; lat, lon = WGS84 of the candidate point as decimal; supportWeight; rawTags;
  explicitModes; deniedModes; lifecycle}`.
- **Where:** `JdfFixups.clusteredPostCandidates` 377–398, `candidateId` 339–356.
- **Why:** as R066.
- **Pinned by:** `JdfParserTests.JDF ZIP writer is deterministic…` (file round trip).
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port the facts; no hashed candidate ID string is needed in the model
  (the observation's identity suffices).

### Stage 9 — plausibility

#### JDF-R068 — Time-ordered trips and day rollover
- **What:** calls grouped by `(routeId, routeDistinction, tripId)`; sorted by `routeStopId`
  ascending for odd trips, descending for even; time = arrival if `StopTime`, else departure if
  `StopTime`, in minutes; a time more than `720` minutes before the previous timed call's
  (adjusted) time gets `+1440` until it is not. Untimed calls stay in the sequence with no time.
- **Where:** `JdfFixups.orderedTimedTrips` 993–1012, `tripStopClockMinutes` 984–991.
- **Why:** unknown (JDF times have no day offset).
- **Pinned by:** the R069/R070 tests (same day only).
- **Constants:** `720`, `1440` → `jdf.time.rollover_backstep_min = 720`.
- **Diagnostic:** none.
- **Proposed verdict:** port as the one JDF day-offset rule (the calendar inventory may own it;
  section 8).

#### JDF-R069 — Implausible consecutive matches
- **What:** per trip, the timed calls whose stop has a match, minus already rejected stops, taken
  pairwise in order; a pair with elapsed minutes `≥ 0` and different stop IDs is a conflict iff
  planar distance (km) `> 2 + elapsed × 2.5`, else a support. Edges are undirected and pooled over
  all trips.
- **Where:** `JdfFixups.rejectImplausibleMatches` 1014–1069.
- **Why:** comment: "2 km of local slack plus a deliberately generous 150 km/h. This catches
  impossible matches without policing timetables."
- **Pinned by:** `Impossible middle match is rejected and estimated by scheduled time`.
- **Constants:** `2.0` km → `jdf.plausibility.slack_m = 2000`; `2.5` km/min →
  `jdf.plausibility.speed_kmh = 150` (shared with `czptt.coordinates.corridor_*`, section 8).
- **Diagnostic:** see R070.
- **Proposed verdict:** port with the one distance function.

#### JDF-R070 — Conflict resolution by consistent groups
- **What:** while conflicts exist: union-find over support edges that are not also conflict edges;
  group size counted over all non-rejected matched stops (unused stops are singletons); strength
  of a stop = `(group size, −number of conflict neighbours, checked source)` compared
  lexicographically (`false < true`); for every conflict edge the weaker endpoint loses, and on a
  tie **both** lose; all losers are rejected at once and the loop repeats (neighbours of rejected
  stops become adjacent). Rejected stops lose their match.
- **Where:** `JdfFixups.rejectImplausibleMatches` 1071–1121.
- **Why:** comments: "Counting the whole group … lets a long consistent route outvote a short run
  of wrong matches"; "With no contextual reason to trust either endpoint, keeping both would
  preserve a known-impossible edge."
- **Pinned by:** `Longer consistent run outvotes a short run of wrong matches`, `Checked match
  survives a tied conflict with an OSM match`, `Indistinguishable conflicting matches are both
  rejected`.
- **Constants:** none.
- **Diagnostic:** `Rejected {MatchCount} stop matches contradicted by scheduled travel time`
  (warning; count only).
- **Proposed verdict:** port, diagnosing each rejected stop.

#### JDF-R071 — Post evidence follows the surviving match
- **What:** post candidates (R066) of stops whose match did not survive R069–R070 (or that never
  had one) are removed; the rest is validated (R027).
- **Where:** `JdfFix.fixBatch` 60–75.
- **Why:** unknown.
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port.

### Stage 10 — log-only checks

#### JDF-R072 — Match distance against kilometres
- **What:** on the calls of the trip with the most calls (key `(routeId, tripId)`, ties → first
  occurrence; file order, not re-sorted): for consecutive calls whose stop has a match and the call
  has km, warn iff planar distance (km) `> (|Δkm| + 4) × 2`. "on international route" is added if
  any stop has a country other than `CZ`. Nothing is changed.
- **Where:** `JdfFixups.checkMatchDistances` 1227–1276; `JdfFix.longestTripStops`.
- **Why:** comment: "Some timetables have unrealistically low distance values"; "Take one trip
  most likely to contain all stops' km distances (testing all takes too much time)".
- **Pinned by:** none.
- **Constants:** `4` km, `2.0` → `jdf.diagnostics.km_check_tolerance_km = 4`,
  `jdf.diagnostics.km_check_factor = 2`.
- **Diagnostic:** `Distance between matches is too high[ on international route] (got
  {MatchDist}, expected {ExpKm} between {StopId1} and {StopId2}, {Pos1} and {Pos2})` (warning).
- **Proposed verdict:** replace with a coded diagnostic over every trip (or drop: nothing depends
  on it; R069 does the real check).

#### JDF-R073 — Missing okres or country
- **What:** per stop of the fixed batch: no country and no okres → `Stop without country` (warning
  if the stop is called, else debug and `Unused stop…`); country `CZ` and no okres → `Czech stop
  without region` (same levels). Country None with an okres → nothing.
- **Where:** `JdfFixups.checkMissingRegionsCountries` 1278–1308.
- **Why:** unknown.
- **Pinned by:** none.
- **Constants:** `CZ`.
- **Diagnostic:** `{Stop|Unused stop} without country: {StopId} ({StopName})`, `{Czech stop|Unused
  Czech stop} without region: {StopId} ({StopName})`.
- **Proposed verdict:** replace with coded diagnostics.

#### JDF-R074 — Geography counters
- **What:** five counters per batch (`strictRegionMatches`, `borderRegionMatches`,
  `countryRejects`, `nonAdjacentRegionRejects`, `outsideBorderToleranceRejects`), summed over
  batches and logged once. They count evaluations, not stops (B10).
- **Where:** `JdfFixups.MatchDiagnostics`; `JdfCommands.fixJdf` 104–116.
- **Why:** comment: "fix-jdf sums the batches' counters and logs them once".
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** `Stop matcher geography summary: …` (information).
- **Proposed verdict:** replace with per-rule diagnostic counts.

### Stage 11 — locations

#### JDF-R075 — Locations from matches
- **What:** existing `Polzast`/`PolzastZdroj` rows are discarded (comment: "A fixed single-route
  batch does not carry pre-existing locations"); every stop with a surviving match gets a location
  `{lat, lon = WGS84 of the match point; precision = S}` and, if the match has a source, a
  `PolzastZdroj` row with it.
- **Where:** `JdfFixups.addStopLocations` 975–982, `stopLocationFromMatch`, `addMatchedLocations`.
- **Why:** comment above.
- **Pinned by:** `Impossible middle match is rejected and estimated by scheduled time`.
- **Constants:** `S`.
- **Diagnostic:** none.
- **Proposed verdict:** port (as the coordinate facts the road compiler hands to the coordinates
  pass; section 5).

#### JDF-R076 — Route-time estimate
- **What:** for every trip (R068) and every **timed** call whose stop has no precise location: the
  nearest earlier and later calls that are timed and whose stop has a precise location (estimates
  are never anchors). Both found and `t_right > t_left` → ratio = clamp(`(t − t_left) / (t_right −
  t_left)`, 0, 1), point linearly interpolated in LAEA, source `estimated:route-time`. Both found
  but `t_right ≤ t_left` → nothing (no fallback).
- **Where:** `JdfFixups.estimateMissingStopLocations` 1123–1177.
- **Why:** comment: "Untimed calls are passing/not-passing route points, not usable anchors."
- **Pinned by:** `Impossible middle match is rejected and estimated by scheduled time` (midpoint,
  source).
- **Constants:** `estimated:route-time` → model enum.
- **Diagnostic:** see R078.
- **Proposed verdict:** port.

#### JDF-R077 — Route-end-north estimate
- **What:** only an earlier anchor at index `a` (call index `i`) → point = anchor + `300 × (i − a)`
  metres in LAEA +Y; only a later anchor → anchor + `300 × (a − i)` metres +Y; indices count all
  calls of the sorted trip, timed or not. Source `estimated:route-end-north`. No anchor → nothing.
- **Where:** `JdfFixups.estimateMissingStopLocations` 1178–1190.
- **Why:** PROGRESS.md: "`estimated:route-end-north` past the last anchor"; a visible placeholder.
- **Pinned by:** `Unmatched route origin is offset north of its first known stop`.
- **Constants:** `300` m → `jdf.estimate.route_end_offset_m_per_call = 300`.
- **Diagnostic:** see R078.
- **Proposed verdict:** port (CZPTT Q12 kept the same placeholder), with the CZPTT formulation
  (section 8).

#### JDF-R078 — Estimate aggregation
- **What:** per stop, if any route-time estimates exist only those are used, else all
  route-end-north estimates; point = centroid (mean in LAEA) → WGS84; precision `E`; source = the
  distinct sources joined by `+` (in practice one value). Stops in no timed call get no location.
- **Where:** `JdfFixups.estimateMissingStopLocations` 1192–1224.
- **Why:** unknown.
- **Pinned by:** `Impossible middle match…`, `Unmatched route origin…`.
- **Constants:** `E`.
- **Diagnostic:** `Estimated {LocationCount} missing stop locations from route timing`
  (information).
- **Proposed verdict:** port, diagnosing stops left without any position.

#### JDF-R079 — Coordinate representation
- **What:** all geometry in LAEA (ProjNet transform, inverse initialised once for thread safety);
  output WGS84 values are `decimal(double)`, i.e. rounded to 15 significant digits
  (≈ 13 decimals at 50°).
- **Where:** `Common.wgs84ToEtrs89Ex`, `etrs89ExToWgs84`, `transformPoint`;
  `JdfFixups.stopLocationFromMatch`, `clusteredPostCandidates`, `estimateMissingStopLocations`.
- **Why:** comment: ProjNet's lazy inverse is not thread-safe.
- **Pinned by:** `Reverse coordinate transform is safe under parallel fix workers`.
- **Constants:** EPSG:3035 parameters (`Common.etrs89Ex` WKT).
- **Diagnostic:** none.
- **Proposed verdict:** replace: `f64` WGS84 in the model, distances by `jr_model::geo`; a
  projection only where a planar operation is needed (Q5).

### Stage 12 — output

#### JDF-R080 — Estimated-post evidence switch
- **What:** with `--no-estimated-posts`, `postCandidateEvidence` is emptied after fixing.
- **Where:** `JdfFix.fixBatch` 88–91.
- **Why:** `Context.estimatedPostActivation` comment: candidate discovery must happen in fix-jdf.
- **Pinned by:** none.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** replace: the facts are always produced; whether the posts pass runs is a
  pipeline choice.

#### JDF-R081 — Re-serialisation of the fixed batch
- **What:** the whole batch is written in the 1.11 layout: `VerzeJDF` with version `1.11` (other
  fields kept, None for upgraded 1.8/1.9); all 17 standard files plus `Polzast.txt` and
  `PolzastZdroj.txt` always (possibly empty); `JrutilPostCandidateEvidence.txt`,
  `JrutilRoutingDemands.txt`, `JrutilRouteVersions.txt` only if non-empty. Encoding CP1250 via
  `StreamWriter` (unrepresentable characters become `?`, B13); each record `"` + fields joined by
  `","` + `";\r\n`, no escaping; options None → `""`; bools `1`/`0`; dates `ddMMyyyy`; times
  `HHmm`; unions by their first `StrValue` (`CompanyType` `""`/`0` become `1`); decimals via F#
  `string`. ZIP entries uncompressed, timestamp 1980-01-01 00:00 UTC, in the order above.
- **Where:** `Jdf.jdfBatchDirWriter`, `fileWriter`; `JdfSerializer`; `CsvSerializer`.
- **Why:** README: "deterministic uncompressed ZIP per fixed input batch".
- **Pinned by:** `JdfParserTests.JDF ZIP writer is deterministic uncompressed and path parser
  compatible`.
- **Constants:** as listed.
- **Diagnostic:** none.
- **Proposed verdict:** drop (no intermediate JDF files; rule 1).

---

#### JDF-R082 — Reviewed gap-fill rows as the last resort
- **What:** not in F#. After R052, a stop still without a match takes the group of its exact
  candidates from the gap-fill catalogue (R043–R046 and R048 applied to them alone); R050 and
  R051 do not apply to it. Position and provenance as R049.
- **Where:** `fix/select.rs::reviewed_match`, `fix/primary.rs`.
- **Why:** a gap-fill row is a reviewer's answer for a stop nothing else places (verdict Q20).
- **Diagnostic:** `jdf_match_reviewed` (info).

## 4. Identity

| Entity | Format / key | Built in | Notes |
|---|---|---|---|
| Batch | input path; output name = file stem (`vld-123`) | `fixJdf` | Oběhy names inputs `<source>-<stem>.zip` and checks 1:1 output accounting |
| Version | `VerzeJDF.version` text (`1.11`) | `jdfBatchDirVersion` | output always `1.11` |
| Stop (in batch) | `Zastavky.id` (int64), local to the batch | source | uniqueness not checked; `Map`-based joins keep the last duplicate, the output keeps both |
| Stop reference | `Zaslinky.stopId`, `Zasspoje.stopId`, `Oznacniky.stopId`, `Polzast*.stopId`, `JrutilPostCandidateEvidence.stopId` | source / fix | a `Zasspoje.stopId` missing from `Zastavky` aborts (R035, R072) |
| Global stop register refs | `Navaznosti.transferStopId`, `…PostId`, `…EndStopId`, `…EndStopPostId` | source | comment: global register, not batch stops |
| Route | `(Linky.id, idDistinction)`; trip `(routeId, routeDistinction, tripId)`; route stop `(routeId, routeDistinction, routeStopId)` | source | fix keys trips by `tripId` only (R035), by `(routeId, tripId)` (R072), or by the full triple (R068); 1.8/1.9 distinctions are `0` |
| Okres | two-letter code (`AB` = Praha), `OL` ≡ `OC` in matching only | source / R053–R063 | not canonicalised in the output |
| Country | JDF codes (`CZ`, `D`, `A`, `SK`, `PL`, `UA`) from source and catalogues; ISO alpha-2 upper (`DE`, `AT`) from R053 | source / R053 | mixed vocabulary (B7) |
| External source | `external:{file stem}` | R007 | two files with the same stem in different folders share it |
| External row | `external:{relative}:sha256:{h}`, `relative` = path relative to `--ext-geodata` with `/`; `h` = lower-case hex SHA-256 of UTF-8 `"{relative}\|{c1}\|{c2}\|{c3}\|{c4}\|{c5}"` (parsed column texts) | R007 | for a single-file argument `relative` is `.` (`Path.GetRelativePath(file, file)`), so the ID does not name the file |
| OSM object | `osm:node:{id}` | R013 | used as source, observation ID and source object ID |
| External observation | = external row ID; unreachable fallback `"{source}\|{lat:F6}\|{lon:F6}"` | R007 | |
| Match source | sorted distinct sources joined by `+` | R049 | parsed back by substring tests in `PackageRows.coordinateSource` (`osm…` → `osm`, contains `gapfill` → `gapfill`, else `catalogue`) |
| Estimated source | `estimated:route-time`, `estimated:route-end-north` | R076–R078 | parsed back by substring (`route-time` → `route_time`, `route-end` → `route_end`) |
| Post candidate | `candidateId` = lower-case hex SHA-256 of UTF-8 `join("\|", sorted distinct sourceObjectIds)`; for one observation SHA-256 of its source object ID; without one, of `"{lat:F6},{lon:F6}"` (WGS84, latitude first) | R067 | hash of an ID that already contains a hash |
| Post evidence key | `(stopId, observationId)`, unique per batch | R027 | merged by `JdfMerger` with a conflict check |

Places in this area that parse an ID or string back: `selectedExactMatches` and
`rejectImplausibleMatches` (`source.StartsWith("osm:")`), `JdfPostEvidenceReader.observationSource`
(`osm:` / `external:` prefixes, splits on `:`), `PackageRows.coordinateSource` (substrings).
All conflict with AGENTS rule 6; each needs a carried field (source class, catalogue, estimate
method).

---

## 5. Output facts

What a fixed batch carries beyond the parsed source:

### 5.1 `Polzast.txt` (`StopLocation`, JrUtil extension)

| # | Column | Type | Meaning |
|---|---|---|---|
| 1 | `stopId` | int64 | batch stop |
| 2 | `lat` | decimal | WGS84 latitude, `decimal(double)` (15 significant digits) |
| 3 | `lon` | decimal | WGS84 longitude, same |
| 4 | `precision` | `S` \| `E` | `S` = matched (R075), `E` = estimated (R076–R078) |

At most one row per stop: matched rows first (in stop order), then estimates (first-estimate
order). Stops with neither have no row (published as `coordinates_missing` downstream).

### 5.2 `PolzastZdroj.txt` (`StopLocationSource`)

| # | Column | Type | Values |
|---|---|---|---|
| 1 | `stopId` | int64 | |
| 2 | `source` | string | `external:{stem}` or several joined `+` (sorted); `osm:node:{id}` or several joined `+`; `estimated:route-time`; `estimated:route-end-north`. Checked and OSM sources are never mixed in one value |

### 5.3 `JrutilPostCandidateEvidence.txt` (`PostCandidateEvidence`), written only if non-empty

| # | Column | Type | Values from fix-jdf |
|---|---|---|---|
| 1 | `stopId` | int64 | stop with a surviving match |
| 2 | `candidateId` | string | section 4 |
| 3 | `observationId` | string | `osm:node:{id}` or `external:{relative}:sha256:{h}` |
| 4 | `sourceKind` | string | `osm` \| `external` |
| 5 | `sourceObjectId` | string? | as 3 |
| 6 | `observedAt` | string? | always empty |
| 7 | `lat` | decimal | candidate's own WGS84 position (not the centroid) |
| 8 | `lon` | decimal | same |
| 9 | `supportWeight` | decimal | always `1` |
| 10 | `rawTags` | string | OSM audit tags (R013); `""` for external |
| 11 | `explicitModes` | string | `road`, `tram`, `road;tram` or `""` |
| 12 | `deniedModes` | string | same vocabulary |
| 13 | `lifecycle` | string | `active` (external), OSM R013 values |

### 5.4 Changes to source relations

- `Zastavky`: names rewritten (R037, R038 or R059), okres and country filled (R036, R053, R055,
  R061–R063), order changed (R065).
- `VerzeJDF.version` = `1.11`; 1.8–1.10 batches leave as upgraded 1.11 records (R028–R031).
- Every other relation is re-serialised (R081): byte changes but no semantic change except the
  normalisations listed there.
- A degenerate batch is written empty (R033).

### 5.5 Per-batch results that are not written

International route decisions (logged, R032) and geography counters (logged, R074).

### 5.6 What the model must carry instead (derived)

Per stop place: the source name triple as published, the matching key (normalised triple), okres
and country as given by the source and as inferred (with the rule that inferred them), the
coordinate with precision (`matched` / `estimated` / `missing`), its provenance list (catalogue
or OSM object IDs, or estimate method `route_time` / `route_end_north`), and the post candidate
observations (position, source class and object, modes, lifecycle, raw tags). Per batch: version,
batch metadata (DÚ number, region, batch ID, creation date, generator), whether MHD naming was
applied and with which town. Per diagnostic: rejected matches (R070), ambiguous groups (R048),
stops without geography (R073).

---

## 6. Known or probable bugs

- **B1 — 1.8/1.9 service-note types are lost.** `Jdf109Model.ServiceNote.timeType` and
  `JdfModel.ServiceNote.noteType` have different names; `nameOptGetters` fills the option-typed
  target with `None` (R029). Every `Caskody` rule of a 1.8/1.9 batch becomes untyped, changing the
  calendar. Not pinned (no upgrade test). No 1.8/1.9 batch in the 60-batch sample.
- **B2 — One `LT` okres polygon is lost.** `cz_regions.json.gz` has two features coded `LT`;
  `Map` keeps the last (40 vertices), dropping the 15-vertex one. Okres adjacency, border tolerance
  and edge distance are wrong for that part of Litoměřice (R015).
- **B3 — Unknown okres code aborts the run.** `distanceToCzRegionEdge` indexes the polygon map
  with the raw code; a matched Czech stop with km and okres `OL` (which R045 says CIS exports use)
  in a batch where some stop lacks geography throws `KeyNotFoundException`, failing the batch and
  the whole `fix-jdf` (R004). No `OL` in the 180-batch sample.
- **B4 — The MHD path skips the name normalisations.** `fixMhdNaming` runs on `jdfBatch.stops`
  (raw), not on the R036/R037 stops: a `[XX]` suffix stays in the town and is not moved to the
  okres; district/nearby of non-town-only stops stay as in the source while the non-MHD path swaps
  them. The same stop is published with different name structures depending on whether its batch
  was judged MHD.
- **B5 — The comparison form is published.** R037 branch 2 moves `nearbyPlace` into `district`
  for every stop with an empty district (1,514 of the sampled stops, e.g. `Praha,,Muzeum` →
  `Praha,Muzeum,`). The doc comment says the result is only for comparison; it is written to
  `Zastavky`, reconciled by merge (whose component key ignores empty parts) and published as
  `location.district` / `nearby_place`. The registry carries the swapped form
  (`registry/stops.csv`: `2,Velký Újezd,rozc.Výkleky,,OL,…`). Not pinned.
- **B6 — Names with a fourth part are truncated.** The unanchored double-comma regex keeps three
  parts and drops the rest (`A,B,C,D` → `A`, `B`, `C`); `fixMhdNaming`'s municipality branch
  drops everything when there are more than three parts.
- **B7 — Two country vocabularies.** R053 writes `upper(ISO)` (`DE`, `AT`) while JDF and the
  catalogues use `D`, `A`; R043 compares literally, so a stop `D` cannot match the catalogue row
  `Breitenau, ZOLL,…,DE` and a stop inferred `DE` cannot match the 284 `D` rows.
  `JdfInternationalFilter.canonicalCountry` maps `AT`/`A` and `DE`/`D` only there.
- **B8 — Foreign OSM stops have no country.** R011 sets a country only inside a Czech
  municipality, so a JDF stop with country `D`/`A`/`SK`/`PL` can never match an OSM node (R043),
  although the extract covers those regions.
- **B9 — Tram stops without a `tram` tag are excluded.** R009's `not railway or has tram` clause
  removes `railway=tram_stop` nodes lacking `tram=*`, contradicting the explicit `railway=tram_stop`
  inclusion in the same filter.
- **B10 — Geography counters are not counts of stops.** `exactMatches` is called repeatedly per
  stop (MHD candidate scan, MHD probes per town, both matching passes, secondary matching,
  `candidateMatches` twice), each call incrementing counters; the logged summary is inflated by an
  unknown factor.
- **B11 — `--strict` is ignored by fix-jdf** and any malformed batch aborts the national run
  (R004).
- **B12 — Silent loss of unterminated records.** A final record without `";\r\n` or an LF-only file
  is dropped without a message (R020); an LF-only `Zasspoje.txt` makes the batch degenerate and it
  is dropped with only a warning.
- **B13 — Writer is lossy and unescaped.** CP1250 encoding replaces non-CP1250 characters (possible
  in OSM `local_ref` and other raw tag values) by `?`; a value containing `","` or `";\r\n` would
  corrupt the record (R081).
- **B14 — Region propagation inconsistently uses synonyms.** R063 looks the town up without R054
  synonyms while R051/R053/R056 use them.
- **B15 — Zero-route batch crashes.** `jdfBatch.routes.[0]` with no route but ≥ 2 called stops
  throws (R034).
- **B16 — Unbounded process-wide caches.** The per-field value caches of the parsers created once
  per run, and `borderToleranceCache` keyed by point coordinates, grow with every batch.
- **B17 — Placeholder coordinates in the external catalogues are accepted.** `QRideDUK.csv` has 52
  rows at `59.7560959,24.8294525` (near Tallinn) with empty okres and country (`Trmice,Edisonova`,
  `Ústí n.L.,Brná`, …); R006 accepts them. They can match a stop without country or make a checked
  group ambiguous (R048), pushing it to OSM. Also `PlzenskyKraj.csv` tags Bavarian stops
  (`Bayer.Eisenstein`, `Höll`) as `CZ` without okres. Data issue; the reader has no check.
- **B18 — Inverted anchors give no estimate** (R076): both anchors present with `t_right ≤ t_left`
  produce nothing instead of a one-sided estimate (same as CZPTT B24).
- **B19 — Rejections are reported only as a count** (R070); which stops lost their match is not
  recorded.
- **B20 — Multi-route batches mix trips.** R035 groups calls by `tripId` only and joins route stops
  by `routeStopId` only (R034 logs but continues).
- **B21 — International decisions are computed on unfixed geography** (R032): stops whose okres or
  country fix-jdf fills later count as unknown. Harmless today because the decisions are only
  logged here.
- **B22 — Culture-sensitive lower-casing** in `preprocessStopName` and file lookup (`ToLower()`):
  results depend on the machine culture (Turkish `I`).

---

## 7. Documentation vs. code

1. **`normaliseStopName` comment** says CIS sometimes puts the nearby place into `district`; the
   code moves `nearbyPlace` into `district`. It says the result need not be an official name and
   is for comparison; it is written to the output (B5).
2. **`--strict`.** README: "Pass `--strict` to `merge-jdf` when a malformed input batch must fail
   the command"; the multitool help lists `-s --strict  Fail instead of skipping a malformed
   batch` for all commands; Oběhy passes it to `fix-jdf`, where it does nothing and failure is
   unconditional (R004).
3. **PROGRESS.md "national-jdf" pipeline** lists "`regional-adjacent` international policy" under
   `fix-jdf`; there it is only evaluated and logged on the unfixed batch (R032); it is applied by
   `jdf-to-bundle` (Oběhy passes the flag there too).
4. **PROGRESS.md "Unmatched stops are route-estimated (`estimated:route-time`, or
   `estimated:route-end-north` past the last anchor)"**: route-end-north is also used before the
   first anchor (R077); stops with no timed call, or between inverted anchors, get no estimate at
   all (R078, B18).
5. **JDF_SEMANTICS.md `location`** "retains municipality, district name, actual JDF district code,
   nearby place, country": district and nearby place are swapped by R037 (B5) or rebuilt by R059;
   the okres may be inferred (R053, R055, R061–R063) rather than the "actual JDF district code";
   `OL` is kept raw.
6. **jrunify-ext-geodata README** "External ISO country codes are normalized to the historical JDF
   identifiers by `download.py`": `JrUtil_Additions.csv` has a `DE` row; 52 rows have no country
   (B17).
7. **jrunify-ext-geodata README** "a candidate for an estimated stop must satisfy the same 2 km plus
   150 km/h scheduled-time ceiling as JrUtil": JrUtil's check (R069) is on consecutive matched timed
   calls with planar LAEA distance and day rollover; `gapfill.py` was not read, so equality is
   unverified (section 8).
8. **`JdfModel.Stop.regionId` comment** "Custom enum for CIS JŘ based on SPZ prefixes (see
   data/regions.json)": no `data/regions.json` exists; the codes come from `cz_regions.json.gz`.
9. **`EurTowns` doc comment / R050 comment** "we only have coordinates for Czech stops (for now)":
   the catalogues and the OSM extract now include foreign stops.
10. **`maxRadiusMetres` doc comment** describes only the group radius; the same constant is also
    the okres border tolerance (R046) and the post candidate radius (R066).
11. **README `--ext-geodata`** "remove exact duplicate rows": duplicates are compared on parsed
    values of all five columns (`50.0` = `50.00`), not on raw rows; the surviving row keeps the
    first file's provenance (R008).

---

## 8. Duplicated logic

1. **Plausibility "2 km + 150 km/h"** (keep one rule, shared constants):

   | Copy | Distance | Evidence | Zero elapsed | Resolution |
   |---|---|---|---|---|
   | CZPTT-R095 | haversine | candidate vs already selected neighbours, per occurrence; edge only if `t_b > t_a` strictly | not evidence | veto candidate; plausible if any occurrence is |
   | JDF-R069/R070 | planar LAEA | consecutive matched timed calls of each trip; `elapsed ≥ 0` | allowed (2 km) | group voting, ties reject both, iterate |
   | `gapfill.py` (Python, per README) | not read | estimated stops' candidates | ? | accept/reject |

   Unifiable as one predicate `plausible(distance, elapsed)` in `jr_model::geo` with
   `{slack_m = 2000, speed_kmh = 150}`; the evidence selection and resolution stay rule-specific.

2. **300 m-per-call placeholder**:

   | Copy | Offset | Index | Choice among occurrences |
   |---|---|---|---|
   | CZPTT-R100 `route_end_north` | latitude + `300 × |Δi| / 111320` degrees | operational calls | best by tier, call span, distance, service days, PA, sequence |
   | JDF-R077 | LAEA +Y `300 × Δi` m (grid north, a few degrees off true north in CZ) | all calls incl. untimed | centroid of all estimates |

   Also the interpolation: CZPTT linear in degrees, JDF linear in LAEA metres; both clamp to
   [0, 1]; both give up on inverted anchors. Unifiable into one estimator with one selection rule.

3. **Name normalisation**: CZPTT-R090 (`jr-geo`, NFD, marks removed, non-alphanumerics → space,
   upper-case); JDF-R039 tokens (lower-case, diacritics **kept**, only `. , -` and NBSP split,
   abbreviation synonyms); `JdfStopReconciliation.normalizeStopName` (merge, its own tokenizer);
   `OsmStops` town completion (R039 + R042). JDF matching depends on abbreviations (`n.L.` ↔ `nad
   Labem`) that R090 lacks, and on diacritics being kept only by accident (catalogues and JDF share
   them). Unifiable: R090's form + an abbreviation expansion table as data; a diacritics-insensitive
   match would widen JDF matching and needs a check on the golden subset.

4. **Name similarity**: CZPTT-R092 edit similarity vs JDF-R042 token-set equality modulo synonyms vs
   merge's Levenshtein/Dice. Different purposes; keep the token-set predicate as a named function in
   `jr-geo` next to R092.

5. **Distance**: JDF uses NTS planar distance in LAEA everywhere (R046, R048, R052, R062, R063,
   R066, R069, R072); CZPTT uses `haversineMetres`. One function (`jr_model::geo`), with LAEA only
   where polygons need it (Q5).

6. **"Checked" vs OSM source** test: `selectedExactMatches` and `rejectImplausibleMatches.isChecked`
   (identical, both `StartsWith("osm:")`); plus `JdfPostEvidenceReader.observationSource` and
   `PackageRows.coordinateSource` parsing the same strings. One source-class field.

7. **Centroid of a candidate group**: `topStopMatch` and `candidateMatches` (identical).

8. **Called stop predicate** (`StopTime` arrival or departure): `dropDegenerateBatch`,
   `checkMissingRegionsCountries.isUsed`, `tripStopClockMinutes` (arrival first).

9. **Trip ordering and keys**: `groupedTripsMatrices` (`tripId` only, parity, ascending route stop
   order then reversed), `orderedTimedTrips` (`(routeId, routeDistinction, tripId)`, sort by
   `routeStopId × direction`), `longestTripStops` (`(routeId, tripId)`, file order),
   `Jdf.tripIsReverse`. One "calls of a trip in travel order" helper keyed by the full triple.

10. **Kilometre-vs-distance tests** (four different formulas): R052 `d < |Δkm| + 1`, R062 `|Δkm| >
    edge distance`, R063 `polygon distance < |Δkm| + 1`, R072 `d > (|Δkm| + 4) × 2`. Could share
    one "reachable along the route" predicate with per-rule tolerances.

11. **Municipality lookups by name**: `matchesCzTownByName`, `matchingCzTownRegions`, R063's direct
    `czTownNameMatcher` call (no synonyms), R058. One lookup with R054 applied.

12. **Okres canonicalisation**: only in `exactMatches`/`regionMatches`; not in R062, R053, output.

13. **Country canonicalisation**: none in this area; `JdfInternationalFilter.canonicalCountry` in the
    next stage (`AT`/`A`, `DE`/`D`).

14. **Name splitting by comma**: R037, R038, R050 (`Split(",")[0]`), R056, R059.

15. **Batch parsers**: three near-identical `jdf1xxBatchDirParserWithCalls`, and two dispatchers
    (`jdfBatchDirParser` for fix/merge, `jdfCompilationParser` for the bundle, which finds
    `Zasspoje.txt` with `Seq.find` and `OrdinalIgnoreCase` instead of `ToLower`).

16. **`double → decimal` coordinate conversion**: `stopLocationFromMatch`,
    `clusteredPostCandidates`, `estimateMissingStopLocations`.

17. **The `1000` m constant** serves three unrelated thresholds (R046, R048, R066).

18. **Day rollover**: R068 here; the calendar/GTFS conversion has its own handling of times past
    midnight (other inventory). Keep one rule.

---

## 9. Performance-relevant structure

Repeated per batch although the answer depends only on the stop's name and given geography:

- **Candidate retrieval and exact selection.** For each stop: one Lucene query in the non-MHD
  matching pass, one in the MHD candidate scan, one per MHD town in `fixMhdNaming` probes, one in
  the MHD matching pass, each followed by `exactMatches` (re-run again in `candidateMatches`, twice).
  The same triples (`Praha,,Florenc`, `Brno,,Hlavní nádraží`) occur in hundreds of batches. Pure in
  `(name triple, okres, country)` and the corpus: R039–R049 and R066 can be computed once per
  distinct key for the whole run.
- **Gazetteer lookups.** `matchesCzTownByName` (Lucene over 6,356 names) for every town-only stop
  and MHD probe; `eurTownNameMatcher` (839,834 names) for every town-only stop with a match and in
  R053; R063 per stop and pass. Pure in the town string: one lookup table.
- **Token analysis.** `checkExactMatch` and `nameSimilarity` run the synonym analyser per token per
  comparison; a precomputed token → canonical-set map removes this.
- **Context-dependent work** (cannot be shared across batches): R052 secondary matching, R055,
  R061–R065 propagation, R060 MHD choice, R069–R070 plausibility, R076–R078 estimates.
- **Repeated geometry.** `distanceToCzRegionEdge` (polygon shell distance) per row and column in up
  to four passes; `withinExpectedRegionBorder` cached per `(okres, x, y)`; okres adjacency computed
  once (all pairs of the 77 polygons left after B2).
- **Quadratic bits.** `checkMissingRegionsCountries` scans all calls per stop; `selectedExactMatches`
  uses `Array.contains` inside a filter.
- **International pre-evaluation** (R032) prepares a GTFS calendar per batch only to log.
- **Re-serialisation** of every batch (including `Zasspoje`) only to add three small files (R081).

Cached today: okres polygons, adjacency, both gazetteer matchers, the external and OSM corpus and
its index (once per run, built before the worker budget is measured); `borderToleranceCache`;
per-field decoded-value caches of the parsers (shared by all batches, unbounded); one query
analyser per thread.

In the rewrite (ARCHITECTURE "stage cache"): the parsed batch is the natural cache grain; the
context-free matching of a stop key against the corpus is a second, run-wide memo that is
independent of the batch set.

---

## 10. Open questions for the architect

- **Q1 — Published stop names.** Keep R037's rewrite (`Praha,,Muzeum` → `Praha,Muzeum,`) in the
  published triple, or publish the source triple and use the normalised form only as a matching
  key (B5)? This changes `location.district` / `nearby_place` and the registry's alias keys.
- **Q2 — MHD renaming.** Town-only MHD stops are published as `{town},{name}` after a vote (R056–R060,
  thresholds `> 1` stop and `> +1` matches). Accept as a named, diagnosed rule, or quarantine
  batches where the vote is close?
- **Q3 — Inferred okres and country.** R053, R055, R061–R063 fill okres and country from matches,
  neighbours and town names; okres is part of stop identity in merge and the registry. Allowed
  with a diagnostic, or must an unknown okres stay unknown (AGENTS: never silently guess an
  identity)?
- **Q4 — Centroid of several exact candidates.** A stop place's coordinate is the centroid of all
  checked (or all OSM) candidates within a 1 km radius (R048, R049). Keep, or prefer one
  catalogue by a ranked list?
- **Q5 — Distance and projection.** Port the planar LAEA distances and centroids (byte-for-byte
  parity) or use `jr_model::geo` haversine everywhere (small, explainable differences)? Same
  question for the north offset (grid north vs latitude).
- **Q6 — Plausibility ties.** R070 rejects both ends of a tied conflict. Keep, or should a checked
  catalogue beat OSM before group size is compared?
- **Q7 — Estimation.** Align R076–R078 with CZPTT-R100 (one estimator, one selection rule: best
  candidate vs centroid of all)?
- **Q8 — Country vocabulary.** JDF codes (`CZ`, `D`, `A`, `SK`, `PL`, `UA`) or ISO alpha-2 in the
  model, with one mapping applied to source, catalogues, gazetteer and OSM (B7)?
- **Q9 — `OL` vs `OC`.** Canonicalise okres codes in the model (publish `OC`) or keep the source
  code and alias only for matching (R045, B3)?
- **Q10 — JDF 1.8/1.9.** Do they still occur in the national export? If not, drop the 1.9 reader
  and R028/R029; if yes, fix B1.
- **Q11 — Malformed batches.** Quarantine the batch with a diagnostic (AGENTS) or keep failing the
  whole build (F# R004, which Oběhy relies on with `--strict`)?
- **Q12 — International logging in fix.** Drop R032 here (it is re-evaluated where applied)?
- **Q13 — Placeholder catalogue coordinates.** Should the reader or a policy reject rows far outside
  the region (B17), or is that the geodata repository's validator's job?
- **Q14 — OSM foreign country.** Derive the country of OSM nodes from position (country polygons) so
  foreign JDF stops can match OSM (B8)?
- **Q15 — OSM tram filter.** Fix B9 (include `railway=tram_stop` without `tram=*`)? It changes
  matches.
- **Q16 — European town veto.** Keep R050 (its premise is outdated and the gazetteer contains
  `Praha` and `Most`), replace by the country check, or drop?
- **Q17 — Village stops.** R051 discards the exact match of every town-only stop named like a
  municipality, leaving it to km-based secondary matching or estimation. Intended?
- **Q18 — Where matching runs.** Per batch (the batch gives the trip context R052, R061–R063,
  R069 need) as today, or once per merged stop after reconciliation with trip context from all
  versions? This decides whether the road compiler reconciles names before or after coordinates.
- **Q19 — Post candidates.** R066 lists OSM candidates even when a checked catalogue decided the
  position (and vice versa). Intended as independent evidence for the posts pass?
