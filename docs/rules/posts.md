# Boarding-post estimator rule inventory (F# reference)

Status: **proposal for review, nothing implemented.** The inventory describes what the F# JrUtil
(`E:/Git/obehy/jrutil`) does when `jdf-to-bundle` estimates which physical post (stance,
*označník*) a road call uses, and what Oběhy feeds it. The verdicts are proposals for the
architect.

This file owns `POST-R001` to `POST-R199`. Neighbouring inventories:

- `docs/rules/jdf-read.md`: reading and `fix-jdf`, including the post-candidate facts stop
  matching hands over (JDF-R066, R067, R071; Rust `fix/posts.rs`, `fix/osm.rs`).
- `docs/rules/jdf-merge.md`: merging, the stop registry reader (R211), post-candidate evidence
  per merged stop (R236), and the routing demands (R252), whose content this file owns
  (POST-R150).
- `docs/rules/jdf-convert.md`: `jdf-to-bundle` with estimation off; boarding points R418–R423
  (unspecified, authored, textual posts). This file covers only what estimation adds or changes.
- `docs/rules/serving.md`: how a GTFS child stop becomes a `location` row of kind
  `boarding_point` (SERVING-R004, R005).

Sources read in full:

- `jrutil/src/Jdf/`: `JdfPostPlan.fs`, `JdfPostInference.fs`, `JdfPostInferenceEvaluator.fs`,
  `JdfPostInferencePolicy.fs`, `JdfPostPolicyEvaluation.fs`, `JdfPostScorer.fs`,
  `JdfPostEvidence.fs` (capture), `JdfPostEvidenceReader.fs`, `JdfPostEvidenceStore.fs`,
  `JdfRoutingCache.fs`, `JdfBundle.fs`, `JdfBundleEvidence.fs`, `JdfBundleManifest.fs`,
  `JdfBundleModel.fs`; the post parts of `JdfBundleTables.fs` (`scanCallDerivedFacts`, the
  `derived_post_*`, `post_candidate_evidence`, `post_physical_hypotheses` tables, lines 115–246
  and 520–790), `JdfToGtfs.fs` (`inferredPostId`, `getGtfsStopsWithPlan`,
  `getGtfsStopTimeRowsInternal`, `getCzStopsWithPlan`, preparation), `JdfBundleInput.fs`
  (`validateRoutingPbfManifest`), `JdfMerger.fs` (routing demands, 153–206),
  `JdfModel.fs` (`PostCandidateEvidence`, `RoutingDemand`, `validatePostCandidateEvidence`),
  `JdfFixups.fs` (`candidateId`, `clusteredPostCandidates`), `JdfGtfsRules.fs` (ID builders,
  `derivedStopPosts`, `stopPostNumbersById`).
- `jrutil/src/GeoData/`: `Osm.fs` (all 1,925 lines), `StopRegistry.fs`, `Common.fs`
  (projection), `ExternalCsv.fs`, the observation part of `OsmStops.fs`.
- `jrutil/src/Execution.fs` (`estimatedPostActivation`); `jrutil-multitool/Commands/
  ConvertCommands.fs`, `Program.fs` (options).
- Tests: `jrutil.tests/JdfPostScorerTests.fs`, `StopRegistryTests.fs`, the router tests of
  `JdfFixupsTests.fs` (114–512), the post tests of `JdfBundleTests.fs` (1–235, 405–1100,
  1240–1500), `PerformanceEquivalenceTests.fs` (routing cache identity). There is no separate
  OSM test file; `ExecutionTests.fs` pins only `Candidate collection does not require a
  routing PBF`.
- Python scorer: `jrutil/scripts/post-scorer/README.md`, `post_scorer/features.py`,
  `collective.py`, `golden.py`, the decision and threshold parts of `model.py`,
  `run-regions.ps1` (arguments).
- Oběhy (read-only): `repo/src/obehy/national_jdf.py` (build arguments, policy choice),
  `osm_snapshot.py` (filters, demand envelope, 560–910), `PROGRESS.md` (post-estimator status
  and timings), `STATIC_PIPELINE.md`, `BASE_PLAN.md` (section 6), `AGENTS.md`.
- Data: `repo/src/obehy/data/post-inference/learned-v1.json` (the production policy document),
  the first lines of `jrunify-ext-geodata/registry/posts.csv` and the registry README section,
  the golden manifest `work/jrutil-rs-golden/jdf/out/fs-ref/bundle-reg/manifest.json`.
- Rust reference (unchanged): `jr-jdf/src/fixed.rs` (`PostCandidate`), `fix/posts.rs`,
  `fix/osm.rs`, `merge.rs` (`Stop`), `assemble/places.rs`.

Ground truth: **there is no golden output with estimation on.** The golden bundle
(`bundle-reg`) was built with `execution_mode: "disabled"` (every counter 0), and
`docs/golden.md` lists `bundle-posts` as still to come. Every behaviour below is read from code
and tests; no count is quoted from real data except the registry file (57,237 `est:` rows, all
`active`, no `post:` rows).

## How the estimator works, end to end

1. Stop matching (`fix-jdf`) attaches to each matched stop the raw observations near it (OSM stop
   nodes, catalogue points); `merge-jdf` keeps them per merged stop and writes routing demands
   (pairs of consecutive route stops next to an observed stop).
2. Oběhy clips a road/tram OSM extract to rectangles around those demands and hands JrUtil the
   clipped PBF.
3. `jdf-to-bundle` builds a directed road/tram graph from the PBF, then turns every call of a bus,
   trolleybus or tram trip at a stop with two or more distinct observation positions into a
   *context* (stop, line, direction, stop pattern, position, role in a repeated-stop block,
   authored post).
4. For each context it routes from the previous stop's anchor to the next stop's anchor (up to
   three cheapest distinct corridors) and attaches every candidate position to each corridor:
   detour cost, side of the road, distance, heading, the road "thread" it sits on. These facts
   are the *evidence pack* (evidence-v2), written to disk and re-read.
5. The evaluator merges candidates closer than 2 m into *hypotheses*, scores them, and decides per
   context. The default in Oběhy is a learned two-stage conditional-logit scorer embedded in the
   policy document; the hand-tuned heuristic is the JrUtil default and still decides authored
   posts.
6. A decided hypothesis becomes an *estimated post* `jdf:stop:<N>:est:<k>` at the hypothesis'
   medoid position; `<k>` is pinned by the registry (`posts.csv`, reuse within 25 m). Calls
   without an authored or textual post board there; undecided calls keep the stop's
   `:unspecified` point. Authored `Oznacniky` posts and textual posts are never replaced, but an
   authored post can take an estimated position.

Conventions:

- "stop" is a merged JDF stop number `N` (`jdf:stop:N`). "Point" or "route point" is one distinct
  observed coordinate at a stop. "Hypothesis" is a consolidated group of route points.
- `equirect(a, b)` is the local metric approximation used throughout:
  `dy = (lat_a − lat_b) × 110540`, `dx = (lon_a − lon_b) × 111320 × cos(mean lat)`, distance
  `sqrt(dx² + dy²)`. Variants that differ are named where used (section 8).
- `sha(x)` is lower-case hex SHA-256 of the UTF-8 bytes of `x`. `stableId(prefix, parts)` is
  `prefix + sha(parts joined with "|")` (`JdfPostEvidence.stableId`).
- `ordinal` comparison of strings is .NET `String.CompareOrdinal` (also F# `compare` on strings).
- Proposed policy keys live in `policy/posts.toml` (prefix `posts.`); constants the F# compiled in
  are proposed there unless marked "format constant" (part of the evidence-v2 contract).
- Diagnostic codes in backticks are F# codes where one exists; proposed new codes are "(new)".

---

## 1. Data flow

| # | Stage | In → out | Rules | F# function |
|---|---|---|---|---|
| 0 | Mode selection | options → `Disabled` / `CaptureOnly` / `Live` | R001–R005 | `JdfBundle.execute` |
| 1 | Policy load | `--post-inference-policy` or compiled default → policy v2 + scorer, validated; loaded in every mode | R004 | `JdfBundleEvidence.policyAndScorer`, `JdfPostInferencePolicy.loadPolicyWithScorer` |
| 2 | Graph build (parallel with JDF parse) | demand-clipped PBF → memory-mapped CSR graph, restrictions | R010–R019 | `PackedRoutingGraph.OpenWithProgress (buildGlobalSnaps=false)` in a `Task.Run` |
| 3 | JDF parse, calendar, international filter, transport-mode rules | merged JDF → filtered batch (jdf-convert stages 3–6) | (jdf-convert) | `writeBundleCore` |
| 4 | Snap preparation | every precise stop location and every observation coordinate → up to 8 snaps per mode per coordinate | R020 | `PackedRoutingGraph.PrepareKnownSnaps` (parallel over edge chunks) |
| 5 | Observations and route points | `batch.postCandidateEvidence` → admitted observations (300 m from the precise stop position), grouped into route points by exact coordinate | R030–R033 | `captureToStoreRestrictedWithCancellation` 429–489 |
| 6 | Contexts | trips of Bus/Trolleybus/Tram routes, one per distinct (route, distinction, direction, pattern, authored keys), → work rows at stops with ≥ 2 route points, with previous/next anchor coordinates | R040–R047 | same, 490–599 |
| 7 | Routing | distinct routing keys per (stop, mode) → routed evidence (≤ 3 corridor variants, attachments of every route point); cached across runs by routing key and graph-region digest; parallel FIFO workers merged by ordinal | R050–R057 | same, 600–760; `Osm.routeContextEvidence`; `JdfRoutingCache` |
| 8 | Join and movement family | work rows × routed evidence → contexts (ordinal, movement family), variants, attachments | R055 | same, 761–851 |
| 9 | Pack write | five Parquet relations + `manifest.json` in a temp dir (live) or the capture dir; validated before activation | R060–R061 | `JdfBundleEvidence.writePostEvidenceStore` |
| 10 | Pack open | full validation again (hashes, schemas, metadata, ordering, keys, sentinels, counts) | R061 | `JdfPostEvidenceStore.openValidatedStore` |
| *capture-only stops here* | | | | |
| 11 | Joined replay | contexts × variants × attachments → one `ReplayScore` row per (context, variant, route point), stop-major | R062 | `JdfPostEvidenceReader.replayScoreRows` |
| 12 | Per stop: consolidation | route points + rows → hypotheses, rows per hypothesis | R070–R073 | `consolidateReplayRows` |
| 13 | Per stop: heuristic | rows → one decision per movement family | R080–R089 | `evaluateReplayPolicy` |
| 14 | Per stop: learned (v3 learned scorer only) | rows → one decision per unlabelled context | R100–R109 | `learnedStopDecisions`, `JdfPostScorer.scoreStop` |
| 15 | Assignments | one row per context (spooled store) + hypotheses | R120–R121 | `evaluateWithScorer` |
| 16 | Authored positions | authored contexts' heuristic decisions + published hypotheses → positions per (stop, authored key) | R122 | `evaluateWithScorer` 164–201 |
| 17 | Plan | assignments → call selections keyed by pattern context, inferred locations, ordinals | R130–R131 | `JdfPostPlan.postEstimationPlanFromInferenceResult` |
| 18 | Registry | ordinals pinned by `posts.csv`; review CSV of new ordinals | R132–R133 | `JdfPostPlan.applyStopRegistry`, `StopRegistry.assignInferredPostOrdinals` |
| 19 | Graph released | routing graph disposed before the call stream | — | `writeBundleCore` 436 |
| 20 | Calls | each unlabelled call → estimated point when its pattern context has a selection, else `:unspecified` | R135–R136 | `getGtfsStopTimeRowsInternal`, `inferredSelectionsForTrip` |
| 21 | Stops | estimated points as GTFS child stops; authored posts take estimated positions | R134, R137, R138 | `getGtfsStopsWithPlan`, `getCzStopsWithPlan` |
| 22 | Diagnostics artifact | `derived_post_assignments`, `derived_post_locations`, `post_candidate_evidence`, `post_physical_hypotheses` (only with `--diagnostics-out`) | R140 | `JdfBundleTables.getTableProducers`, `writeDerivedPostAssignmentsParquet` |
| 23 | Manifest | `compiler.estimated_posts` section | R139 | `JdfBundleManifest.writeManifest` 290–338 |
| 24 | Cleanup | temp evidence dir deleted on success, kept (logged) on failure | R063 | `writeBundleCore` 677–680 |

Granularity:

- **Once per build:** policy load; graph build; snap preparation; observation/route-point
  stores; stop-level source classification (learned, over the whole pack); routing-cache load
  and save; tile fingerprints of the graph (lazy, on the first cache lookup).
- **Per stop:** route points, anchors (medoid), consolidation, effective modes, the heuristic
  decisions (per movement family), the learned scorer (per stop, all contexts together),
  registry ordinals.
- **Per trip pattern:** one set of contexts per distinct (route, distinction, direction,
  pattern hash, authored-key string); equal contexts of different lines merge only if every
  routing input matches (R046).
- **Per routing key:** one routed evidence computation per distinct (stop, mode, previous
  coordinates, route point coordinates, next coordinates) within a (stop, mode) group.
- **Per call:** only the lookup of its pattern context's selection (R135).

Caches: snaps keyed by (mode, exact projected coordinate bits) (R020); routed evidence across runs
keyed by routing key, valid while the graph digest of its read region is unchanged (R056);
per-edge thread identities and per-(mode mask, class, previous, current) thread ends inside one
graph (R022). Parallel: graph build overlaps parsing; snap preparation; capture routing (FIFO
queue, `routingWorkers` = min(requested jobs, CPU count), admitted by memory budget). The
evaluator, plan and publication are single-threaded.

---

## 2. Inputs and outputs, field by field

### 2.1 The routing extract

Oběhy prepares it in two steps (`osm_snapshot.py`):

1. **Base filter** `jdf-transit-routing-base-v1` (`osmium tags-filter`, referenced objects kept):
   `w/highway=motorway,trunk,primary,secondary,tertiary,unclassified,residential,living_street,
   service,road,busway`, `w/railway=tram`, `w/construction`, `n/barrier=*`,
   `r/type=restriction`.
2. **Demand clip** `jdf-routing-envelope-v2` (`osmium extract --strategy=complete_ways
   --polygon=…`): the union of one rectangle per routing demand (POST-R151).

JrUtil requires `<pbf>.manifest.json` with `filter_schema = "jdf-routing-envelope-v2"`, a
non-blank string `source_key`, and `output.bytes` equal to the PBF size
(`JdfBundleInput.validateRoutingPbfManifest`); anything else is an `ArgumentException`. The
manifest's SHA-256 and the PBF's SHA-256 go into the bundle manifest (R139).

What the graph reads from the PBF (R010–R018):

| OSM object | Read | Use |
|---|---|---|
| node with id and coordinates | id, lat, lon, tags `barrier`, `bus`, `psv` | vertex (projected to EPSG:3035, stored as `float32`); barrier flag |
| way with id and nodes | tags `highway`, `railway`, `access`, `vehicle`, `motor_vehicle`, `bus`, `psv`, `oneway`, `junction`, `construction`, `abandoned`, `disused`, `proposed` | directed edges per consecutive node pair, flags road/tram/severe/service |
| relation with tag `restriction` | members `from` (way), `to` (way), `via` (node); value prefix `only_` | turn restrictions |

The PBF must be type-ordered with nodes ascending by ID ("Routing PBF is not ID ordered"
otherwise). A per-way audit TSV (`way-audit.tsv`: way id, flags, 18 audited tags) is written to
the temp directory and never read.

### 2.2 Post candidates and their facts

Source: `JrutilPostCandidateEvidence.txt` in the merged JDF (JdfModel `PostCandidateEvidence`,
written by `fix-jdf`, merged by `merge-jdf` keyed (stop, observation id)). Columns in record
order:

| Field | Type | Meaning | Read by the estimator |
|---|---|---|---|
| `stopId` | int64 > 0 | merged stop number | yes |
| `candidateId` | text | `sha(sourceObjectIds sorted joined "\|")`, else `sha("{lat:F6},{lon:F6}")` (`JdfFixups.candidateId`), bare 64 hex | only by the diagnostic table (B1) |
| `observationId` | text, unique per stop | `osm:node:<id>` for OSM; `external:<relative csv path>:sha256:<hex>` for catalogue rows | identity, source class, hypothesis ID |
| `sourceKind` | text | `osm` or `external` | copied to the pack only |
| `sourceObjectId` | text? | equal to `observationId` for both sources | capture exclusion prefixes (R030) |
| `observedAt` | text? | always null today | copied only |
| `lat`, `lon` | decimal | WGS84 | route point identity (decimal text!) and coordinates |
| `supportWeight` | decimal > 0 | always `1` today | heuristic source adjustment |
| `rawTags` | text | OSM: `key=value` pairs joined `;` for the audit keys `highway, public_transport, railway, amenity, bus, psv, tram, trolleybus, access, vehicle, motor_vehicle, abandoned, disused, construction, proposed, temporary, local_ref` (a `;` in a value becomes `,`); catalogue: `""` | learned: `local_ref`, `public_transport=platform` |
| `explicitModes` | text | OSM: `road` if `highway=bus_stop` or `amenity=bus_station` or `bus`/`psv` ∈ {yes, designated, official}; `tram` if `railway=tram_stop` or `tram` affirmative; joined `;`. Catalogue `""` | mode gates, modality adjustment |
| `deniedModes` | text | OSM: `road` if `bus` or `psv` = `no`; `tram` if `tram=no` | modality conflict |
| `lifecycle` | text | OSM: first of `abandoned, disused, construction, proposed, temporary` present as key or as `lifecycle=` value, else `active`; catalogue `active` | current/obsolete flags |

Validation on read and write (`validatePostCandidateEvidence`): stop > 0, observation id non-blank
and unique per stop, source kind non-blank, coordinates in range, support > 0, every mode token
(split on `; , space`, lower-cased) ∈ {road, tram, trolleybus}, lifecycle (trimmed, lower) ∈
{"", active, current, temporary, construction, proposed, disused, abandoned}. A violation is an
exception.

The Rust fix stage hands over `PostCandidate { stop, coord, observation, facts }` with facts
`source_kind`, `explicit_modes`, `denied_modes`, `lifecycle` and `tag:<key>` audit tags
(`fix/osm.rs` R013). It has **no** `supportWeight`, `sourceObjectId`, `observedAt` and no exact
decimal text of the coordinate; see section 5.

### 2.3 The evidence pack (evidence-v2)

A directory with exactly six files: `manifest.json` and the five relations below (any extra or
missing file is rejected). Every Parquet file: Snappy, row groups of 65,536 rows (the writer
chunks; F# `Seq.chunkBySize 65536`), custom metadata:

| Key | Value |
|---|---|
| `obehy.evidence_format` | `post-inference-evidence-v2` |
| `obehy.evidence_schema_version` | `2` |
| `obehy.router_evidence_version` | `packed-directed-v3` |
| `obehy.variant_enumeration_version` | `directed-thread-top3-v1` |
| `obehy.capture_tool_version` | `--converter-version` (+ restriction suffix, R005) |
| `obehy.pack_id` | pack ID (below) |
| `obehy.relation` | the file name, e.g. `contexts.parquet` |
| `obehy.source_id` | snapshot descriptor `source_id` (non-blank) |
| `obehy.snapshot_id` | `sha256:<merged JDF payload SHA-256>` |
| `obehy.routing_pbf_sha256` | SHA-256 of the routing PBF |
| `obehy.capture_routed_excess_metres` | `1000` (invariant culture) |
| `obehy.capture_maximum_corridor_variants` | `3` |
| `obehy.maximum_search_states` | `100000` |
| `obehy.maximum_search_distance_metres` | `30000` |

Stop identities are written as `jdf:stop:<N>` (`JdfGtfsRules.jdfStopId`) and parsed back with the
regex `(?:jdf|cis):stop:(?<id>[0-9]+)` (the store requires a full match; the reader accepts a
partial one).

**`observations.parquet`** — one row per admitted observation; order and key
(stop number, observation id) strictly increasing (ordinal).

| Column | Type | Null | Content |
|---|---|---|---|
| `gtfs_stop_place_id` | string | no | `jdf:stop:<N>` |
| `route_point_id` | string | no | R031 |
| `observation_id` | string | no | as in 2.2 |
| `source_kind` | string | no | as in 2.2 |
| `source_object_id` | string | yes | as in 2.2 |
| `observed_at` | string | yes | as in 2.2 |
| `latitude`, `longitude` | double | no | `float` of the decimal |
| `support_weight` | double | no | `float` of the decimal, ≥ 0 |
| `raw_tags`, `explicit_modes`, `denied_modes`, `lifecycle` | string | no | verbatim from 2.2 |

**`route_points.parquet`** — one row per (stop, distinct coordinate); key (stop number, route
point id) strictly increasing.

| Column | Type | Null | Content |
|---|---|---|---|
| `gtfs_stop_place_id` | string | no | `jdf:stop:<N>` |
| `route_point_id` | string | no | R031 |
| `representative_observation_id` | string | no | ordinal-smallest member observation |
| `observation_ids` | string | no | distinct member ids, ordinal-sorted, joined `;`; must equal the observations' membership |
| `latitude`, `longitude` | double | no | the shared coordinate |
| `has_current_lifecycle` | bool | no | any member lifecycle (trimmed, lower) ∈ {"", `active`} |
| `has_obsolete_lifecycle` | bool | no | any member lifecycle ∈ {`abandoned`, `disused`, `construction`} |
| `source_support_weight` | double | no | sum of member support weights |
| `explicit_modes`, `denied_modes` | string | no | member values split on `; , space`, trimmed, upper-cased, distinct, ordinal-sorted, joined `;` |

**`contexts.parquet`** — one row per logical context; written in capture ordinal order, which is
the work-key order (R046): (stop number, mode, line, distinction, direction, pattern hash,
position, role, authored key with `None` first). The store checks (stop, mode, line,
distinction, direction, pattern, position, role, authored-or-"", context id) strictly increasing.

| Column | Type | Null | Content |
|---|---|---|---|
| `context_id` | string | no | R044 |
| `gtfs_stop_place_id` | string | no | `jdf:stop:<N>`; the stop must have route points |
| `mode` | string | no | `A` (bus), `E` (tram), `T` (trolleybus) |
| `line_id` | string | no | `Linky.id` (licence number) |
| `route_distinction` | int | no | merge distinction |
| `direction` | int | no | 1 when the trip number is even (reverse), else 0 |
| `pattern_hash` | string | no | R041 |
| `pattern_position` | int | no | 0-based index among usable calls |
| `same_stop_block_role` | string | no | `through`, `incoming`, `outgoing`, `interior` (R042) |
| `movement_family_id` | string | no | R055 |
| `context_previous_stop_id`, `context_next_stop_id` | string | yes | `jdf:stop:<N>` of the anchor stop (R045); null for `outgoing`/`interior` previous, `incoming`/`interior` next, or no anchor |
| `assignment_kind` | string | no | `authored` iff `authored_post_key` set, else `unlabelled` |
| `authored_post_key` | string | yes | `id:<n>` or `num:<text>` (R043) |
| `same_stop_block_id` | string | yes | R042; null iff role `through` |

**`corridor_variants.parquet`** — per context, ranks contiguous from 0, at most 3 (the store
rejects rank ≥ 3).

| Column | Type | Null | Content |
|---|---|---|---|
| `context_id`, `gtfs_stop_place_id` | string | no | foreign key to the context |
| `variant_rank` | int | no | 0, 1, 2 |
| `corridor_id` | string | yes | R023; null iff unavailable |
| `absolute_cost_metres` | double | yes | penalised path cost; non-decreasing over ranks |
| `relative_cost_metres` | double | yes | `max(0, cost − cost₀)`; 0 at rank 0 |
| `relative_cost_fraction` | double | yes | relative / cost₀ (0 when cost₀ ≤ 0); 0 at rank 0 |
| `path_length_metres` | double | yes | sum of edge lengths |
| `directed_edge_count` | int | no | edges in the path |
| `repeated_directed_edge_count` | int | no | count − distinct count |
| `service_edge_count` | int | no | edges with the service flag |
| `restricted_access_edge_count` | int | no | edges with the severe flag |
| `access_penalty_metres` | double | yes | `max(0, cost − length)` |
| `ingress_thread_id`, `egress_thread_id` | string | yes | directed thread of the first / last path edge (R022) |
| `routing_availability` | string | no | `available` or `unavailable` |
| `invariant_failure_reason` | string | yes | `snap-unavailable` or `disconnected` for the single unavailable rank-0 row |

Sentinel rules (store): `unavailable` only at rank 0, with every optional and numeric column
null, every count 0 and a reason; `available` with all of `corridor_id`, `ingress`, `egress`
and all five numerics set, finite, ≥ 0, no reason; counts ≥ 0 and each ≤ `directed_edge_count`.

**`route_point_evidence.parquet`** — for every variant of every context, one row per route point
of the context's stop, in the stop's route-point order (coverage is checked).

| Column | Type | Null | Content |
|---|---|---|---|
| `context_id`, `gtfs_stop_place_id` | string | no | |
| `variant_rank` | int | no | |
| `corridor_id` | string | yes | equal to the variant's |
| `route_point_id` | string | no | |
| `routed_excess_metres` | double | yes | R053, ≥ 0 |
| `snap_edge_id` | int | yes | graph edge index (graph-specific; diagnostic) |
| `snap_way_id` | int64 | yes | OSM way of the snap edge |
| `snap_fraction` | double | yes | [0, 1] |
| `snap_projected_longitude`, `snap_projected_latitude` | double | yes | snap point back in WGS84 |
| `snap_distance_metres` | double | yes | ≥ 0, perpendicular distance to the snap edge |
| `corridor_face_id` | string | yes | `<thread identity>:<C\|R\|L>` (R054) |
| `corridor_distance_metres` | double | yes | ≥ 0; **equal to `snap_distance_metres`** |
| `signed_lateral_offset_metres` | double | yes | negative = right of travel |
| `corridor_heading_degrees` | double | yes | azimuth of travel at the attachment |
| `attachment_heading_degrees` | double | yes | azimuth of the snap edge |
| `heading_difference_degrees` | double | yes | angle between the two |
| `proximity_distance_metres` | double | yes | ≥ 0; **equal to `corridor_distance_metres`** |
| `routing_availability` | string | no | `available` / `unavailable` |
| `invariant_failure_reason` | string | yes | `baseline-unavailable`, `search-limit`, `routed-excess-or-disconnected`, `evidence-horizon`, `corridor-unavailable` |

Unavailable attachment: every numeric, snap ids and face null, reason set; available: all set,
finite, no reason; an unavailable variant has only unavailable attachments.

**`manifest.json`** (snake case, indented, trailing `\n`): exactly the keys `evidence_format`,
`schema_version`, `pack_id`, `capture_tool_version`, `merged_jdf_sha256`,
`routing_pbf_sha256`, `osm_snapshot` (always null), `router_evidence_version`,
`variant_enumeration_version`, `capture_ceilings` {`routed_excess_metres`: 1000,
`maximum_corridor_variants`: 3}, `maximum_search_states` (100000),
`maximum_search_distance_metres` (30000), `context_count`, `route_point_count`,
`observation_count`, `corridor_variant_count`, `route_point_evidence_count`, `files` (five
entries `{path, sha256, bytes, rows, schema_fingerprint}`). Any extra or missing key is rejected
("shape is obsolete or incomplete"); a different `evidence_format` is rejected with "Evidence v1
is intentionally rejected; recapture v2".

- `pack_id = sha(join("|", [captureToolVersion, mergedJdfSha256.lower, routingPbfSha256.lower,
  "post-inference-evidence-v2", "2", "packed-directed-v3", "directed-thread-top3-v1", "1000",
  "3", "100000", "30000"]))`.
- `schema_fingerprint = sha(join("|", "<name>:<.NET CLR type full name>:<IsNullable>"))`, e.g.
  `gtfs_stop_place_id:System.String:False`. The type names are .NET's (`System.String`,
  `System.Double`, `System.Int32`, `System.Int64`, `System.Boolean`); a Rust writer that wants
  F#-readable packs must emit them literally.

### 2.4 The policy document

Two accepted forms (`loadPolicyWithScorer`): a v2 policy (`schema_version: 2`, heuristic scorer),
or a v3 document `{"schema_version": 3, "policy": {…v2…}, "scorer": {"kind": "heuristic" |
"learned", "model": {…}}}`. `documentSha256 = sha(file text)`. No path → the compiled default
`conservativeRoutedV4` with the heuristic. Oběhy passes `learned-v1.json` whenever estimated posts
are on (`national_jdf.effective_post_inference_policy`), whose `policy` equals the compiled
default.

Every v2 key (snake case; serializer `JsonNamingPolicy.SnakeCaseLower`; missing keys
deserialize to 0/false/null and then usually fail validation):

| Key | Default | Validation | Meaning | Read by |
|---|---|---|---|---|
| `schema_version` | 2 | must be 2 | | load |
| `policy_id` | `conservative-routed-v4\|tuned-safe-v3\|kostany-diagnostic-best-safe` | non-blank | provenance | manifest |
| `consolidation.maximum_diameter_metres` | 2.0 | > 0 | route points merge only within this distance | R070 |
| `consolidation.maximum_chainage_difference_metres` | 20.0 | ≥ 0 | along-corridor separation limit | R070 |
| `consolidation.maximum_heading_difference_degrees` | 30.0 | [0, 180] | corridor-heading difference limit | R070 |
| `consolidation.opposite_side_tolerance_metres` | 2.0 | ≥ 0 | sides conflict only beyond this offset | R070 |
| `consolidation.distinguished_context_minimum_count` | 2 | ≥ 1 | this many distinguishing contexts block a merge | R070 |
| `consolidation.distinguished_context_minimum_score` | 0.55 | [0, 1] | | R070 |
| `consolidation.distinguished_context_minimum_margin` | 0.15 | [0, 1] | | R070 |
| `geometry_weights.alignment / side / proximity / routed_excess` | 0.40 / 0.25 / 0.20 / 0.15 | each ≥ 0, sum = 1 ± 1e-6 | geometry score weights | R082 |
| `hard_gates.centreline_neutral_tolerance_metres` | 2.0 | ≥ 0 | offset band scored 0.5, left-side gate | R081, R082 |
| `hard_gates.maximum_corridor_distance_metres` | 100.0 | > 0 | gate | R081 |
| `hard_gates.maximum_routed_excess_metres` | 500.0 | > 0, ≤ captured 1000 | gate and routed-fit scale | R081, R082 |
| `spatial_isolation.minimum_separation_metres` | 12.0 | ≥ 0 | | R085 |
| `spatial_isolation.maximum_adjustment` | 0.05 | [0, 1] | | R085 |
| `resolution.minimum_plausible_score` | 0.90 | [0, 1], ≤ physical score | **unused in production** (`plausibleCandidate` is called only by a test) | — |
| `resolution.minimum_physical_score` | 0.98 | [0, 1] | | R087 |
| `resolution.minimum_physical_margin` | 0.02 | [0, 1] | | R087 |
| `resolution.material_routed_advantage` | 0.05 | [0, 1] | | R087 |
| `consensus.minimum_contexts` | 1 | ≥ 1 | | R086, R087 |
| `consensus.minimum_winning_share` | 0.67 | [0, 1] | | R086 |
| `consensus.minimum_per_context_score` | 0.90 | [0, 1] | contradiction threshold | R086 |
| `consensus.minimum_per_context_lead` | 0.02 | [0, 1] | | R086 |
| `established_post.minimum_supporting_contexts` | 3 | ≥ 1 | | R086 |
| `established_post.minimum_runner_up_ratio` | 2.0 | finite, ≥ 1 | | R086 |
| `established_post.maximum_geometry_disadvantage` | 0.15 | [0, 1] | | R086 |
| `established_post.maximum_popularity_adjustment` | 0.10 | [0, 1] | | R085 |
| `established_post.per_context_adjustment` | 0.02 | ≥ 0, ≤ maximum | | R085 |
| `alternative_corridors.absolute_tie_metres` | 0.0 | ≥ 0 | | R080 |
| `alternative_corridors.relative_tie_fraction` | 0.02 | [0, 1] | | R080 |
| `alternative_corridors.maximum_variants` | 3 | 1..3, ≤ captured | | R080 |
| `modality.source_support_per_weight` | 0.01 | ≥ 0 | | R083 |
| `modality.maximum_source_support_adjustment` | 0.05 | [0, 1] | | R083 |
| `modality.explicit_support_adjustment` | 0.05 | [0, 1] | | R083 |
| `modality.estimated_support_adjustment` | 0.03 | [0, 1] | | R083 |
| `modality.conflict_adjustment` | −0.05 | [−1, 0] | also the lower clamp of the combined adjustment | R083 |
| `modality.maximum_combined_supporting_adjustment` | 0.10 | [0, 1] | upper clamp | R083 |
| `authored_resolution.minimum_physical_score` | 0.75 | [0, 1] | | R122 |
| `authored_resolution.minimum_physical_margin` | 0.20 | [0, 1] | | R122 |
| `authored_resolution.require_unanimous_contexts` | true | — | | R122 |

The learned path reads only `consolidation.*` (hypotheses), `hard_gates.*` (eligibility, routed
fit, side band); the authored path and the heuristic read everything else. The manifest's
`policy_sha256` is `sha` of the policy re-serialized by F# (not of the file).

### 2.5 The learned scorer model

Embedded as `scorer.model` (format `post-scorer-two-stage-v1`, `JdfPostScorer.parseModel`; Python
`TwoStageModel.to_json`):

```
{ "format": "post-scorer-two-stage-v1",
  "stage1": { "format": "post-scorer-conditional-logit-v1", "features": [17 names],
              "mean": [...], "scale": [...], "weights": [...], "null_utility": u1,
              "threshold": ... (ignored by F#) },
  "stage2": { same, features = stage1 features ++ 6 collective names },
  "thresholds": { "simple": ts, "complex": tc, "area": ta (optional) } }
```

Checks: formats; non-empty features; vectors of equal length; all finite; scales > 0; feature
names distinct; stage-2 features exactly stage-1 features followed by `reverse_mass,
has_reverse, same_movement_mass, has_same_movement, same_next_mass, same_previous_mass`;
thresholds finite and ≥ 0 (`area` read only when it is a number).

Stage-1 features in production order (`learned-v1.json` = Python `FEATURE_COLUMNS`, frozen
2026-09-28): `side, alignment, proximity, corridor_distance, routed_fit,
routed_excess_metres_capped, excess_minus_context_min, eligible, has_local_ref, is_bay,
post_support, post_support_deficit, is_tram, is_trolleybus, anchor_previous_missing,
anchor_next_missing, service_edge_count`. Definitions: R106. Any other name throws
"Learned scorer feature … is not available in JrUtil".

Score: utility of a candidate row `x` = Σᵢ `(xᵢ − meanᵢ) / scaleᵢ × weightᵢ` (sequential sum in
feature order); per context, softmax over its candidates plus a null option of utility
`null_utility` (subtract the maximum of all utilities including the null one before `exp`).
Decision: R108. Production thresholds: simple 0.60, complex 0.58, area 0.67; null utilities
−2.2196763980164 (stage 1), −2.09725240202854 (stage 2).

### 2.6 The registry posts file

`registry/posts.csv` (UTF-8, BOM stripped, RFC 4180, header exactly
`stop_id,post_key,lat,lon,status,note`). Per row: `stop_id` must be in `stops.csv`; `post_key`
is `est:<k>` (k > 0, canonical decimal, no leading zero) or `post:<text>` (non-empty, no `:` or
whitespace); (stop, key) unique; `lat`/`lon` both set or both empty, parsed invariant, |lat| ≤ 90,
|lon| ≤ 180; `est:` rows need coordinates; `status` ∈ {active, retired}; `note` free. Only `est:`
rows with coordinates participate (R132); `retired` is parsed and ignored. Today: 57,237 rows,
all `est:` and `active`.

### 2.7 What is published

| Output | Content |
|---|---|
| Estimated boarding point | GTFS child stop `jdf:stop:<N>:est:<k>` (R134): parent's fields, `location_type` 0, `parent_station = jdf:stop:<N>`, `platform_code` null, coordinates = hypothesis medoid; `cz_stops` row (`stopPlaceId`, `cisStopId`, no post id, no source ids). Serving: `location` kind `boarding_point`, `public_code` null; `coordinate_precision` / `coordinate_source` inherited from the parent's `source_stop_metadata` (B13) |
| Authored post with an estimated position | the authored child stop (jdf-convert R419) with `stop_lat/lon` replaced (R137) |
| Calls | `trip_call.boarding_point_id` = the estimated point for unlabelled calls whose context was decided (R135), else `:unspecified` (R136); `location_id` the parent |
| Manifest | `compiler.estimated_posts` (R139) |
| Diagnostics artifact | `post-inference/derived_post_assignments.parquet`, `derived_post_locations.parquet`, `post_candidate_evidence.parquet`, `post_physical_hypotheses.parquet` (R140) |
| Review file | `--stop-registry-candidates` CSV rows `stop_id, post_key, lat, lon, reason=new` (R133) |
| Capture-only output | the evidence pack, optionally `post_context_calls.parquet` (R005) |
| Feature export (`jdf-export-post-features`) | `diagnostic_scores.parquet`, `hypotheses.parquet`, `assignments.parquet`, `policy.json` (R141) |

No diagnostic of `diagnostics.json` is specific to the estimator except
`conflicting_post_numbers` (jdf-convert R422), which only reads post numbers. Estimator failures
are exceptions; abstentions are silent except in the counters and the diagnostics artifact.

---

## 3. Rules

### 3.1 Execution modes and inputs

#### POST-R001 — Execution mode matrix
- **What:** `execute` first requires `maximumWorkers`, `routingWorkers`, `memoryBudgetBytes` > 0.
  The mode is chosen from (`postInferenceEvidenceOnly`, `routingPbfPath`, `estimatedPosts`):
  (true, Some, true) → `CaptureOnly`; (false, Some, true) → `Live`; (false, None, any) →
  `Disabled`; anything else → `ArgumentException` "Invalid post-inference execution-mode
  combination". `estimatedPosts` is `not --no-estimated-posts && --routing-osm-pbf given`
  (`Execution.estimatedPostActivation`), so `--no-estimated-posts --routing-osm-pbf=…` fails
  (B15). CaptureOnly additionally requires `--capture-post-inference-evidence=DIR` and forbids
  `--post-inference-policy`. Live and Disabled forbid `--capture-post-inference-evidence`,
  `--capture-stop-region`, `--capture-exclude-source`, `--export-post-context-calls`.
  `writeBundleCore` re-checks: capture path needs evidence-only; capture needs estimation and a
  PBF. Disabled empties `batch.postCandidateEvidence` before conversion and uses the empty plan.
- **Where:** `JdfBundle.execute` 685–718; `writeBundleCore` 244–252, 336–339;
  `ConvertCommands.jdfToBundle` 46–64; `Execution.fs` 42–45.
- **Why:** "Evidence capture currently requires --post-inference-evidence-only so policy
  evaluation cannot contaminate the captured route points" (error text).
- **Pinned by:** `Capture-only dispatch never enters policy evaluation or GTFS publication`,
  `Restricted captures mark their packs and are rejected outside capture-only execution`,
  `Candidate collection does not require a routing PBF`.
- **Constants:** none.
- **Diagnostic:** exceptions only.
- **Proposed verdict:** replace: the build spec names the post stage as `off`, `capture` or
  `estimate`; the pass runs when the spec says so, and capture is "estimate without edits plus
  write the pack". No flag matrix, so B15 disappears.

#### POST-R002 — Required inputs
- **What:** Live and CaptureOnly require the merged JDF to contain the relation
  `JrutilPostCandidateEvidence.txt` (checked before the graph is opened: "Routed post inference
  requires JrutilPostCandidateEvidence.txt; the supplied JDF input has no observation
  relation"), a valid routing PBF manifest (section 2.1) and the snapshot descriptor whose
  digest matches the input (jdf-convert JDF-R400/R401).
- **Where:** `writeBundleCore` 256–259, 285; `capturePostInferenceEvidenceOnly` 73–78;
  `JdfBundleInput.validateRoutingPbfManifest` 161–182.
- **Why:** the routing PBF must have been made by the supported Osmium envelope policy (error
  text).
- **Pinned by:** `Routed bundle rejects candidate-free JDF before opening routing graph`,
  `Routing PBF manifest accepts only the current envelope policy`.
- **Constants:** `jdf-routing-envelope-v2` (format constant of the OSM hand-over).
- **Diagnostic:** exceptions.
- **Proposed verdict:** port as input validation in `jrutil` (the CLI layer); the pass itself
  receives an already-built graph and the merged stops' candidates.

#### POST-R003 — Which batch is estimated
- **What:** estimation runs on the batch after the international filter and the transport-mode
  rules (jdf-convert stages 5–6), i.e. on the published routes with their effective modes. Trips
  that will be dropped for having no service day (JDF-R413) are still in the batch and produce
  contexts (B17).
- **Where:** `writeBundleCore` 334–339, 384–391; `capturePostInferenceEvidenceOnly` 104–111.
- **Why:** unknown.
- **Pinned by:** none directly.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port the position in the pipeline (`posts` after `route rules`, as
  `ARCHITECTURE.md` lists); exclude trips without service days (Q10).

#### POST-R004 — Policy loading
- **What:** the policy is loaded and validated in every mode except CaptureOnly, even when
  estimation is disabled (B19). No path → `conservativeRoutedV4` + heuristic. A v2 file → that
  policy + heuristic. A v3 document → its `policy` + `scorer.kind` `heuristic` or `learned`
  (with `model`). Other schema versions, missing `policy`/`scorer`/`model` objects, or other kinds
  are errors. Validation (`validatePolicy`, captured horizon 1000) as in section 2.4; at
  evaluation the policy is validated again against the pack's ceilings
  (`validatePolicyForEvidence`: `maximum_routed_excess_metres` ≤ captured routed excess,
  `maximum_variants` ≤ captured variants). `documentSha256 = sha(file text)`.
- **Where:** `JdfPostInferencePolicy.loadPolicyWithScorer` 296–333, `validatePolicy` 190–269,
  `validatePolicyForEvidence` 271–276; `JdfBundleEvidence.policyAndScorer` 474–479;
  `writeBundleCore` 253–255.
- **Why:** "The document hash covers the embedded model" (comment).
- **Pinned by:** `Policy v2 round-trips snake-case fields and rejects non-finite domains`,
  `Default post-inference policy is the tuned best-safe policy`,
  `Learned v3 policy publishes a live bundle with learned provenance`,
  `Model parsing rejects a stage 2 without the collective features`.
- **Constants:** schema versions 2 and 3; defaults of section 2.4.
- **Diagnostic:** exceptions; manifest fields (R139).
- **Proposed verdict:** port the document format unchanged (it is the plan's model format);
  read it in `jr-formats`; thresholds stay in the document, not in `policy/posts.toml`, because
  the document is versioned as one unit with its model. Load only when the stage runs.

#### POST-R005 — Capture restrictions and the context-call export
- **What:** CaptureOnly only. `--capture-stop-region=MINLON,MINLAT,MAXLON,MAXLAT` (four finite
  invariant numbers, non-empty box inside ±180/±90): only stops whose chosen precise location is
  inside the box (inclusive) get contexts; observations, route points and anchors stay complete.
  `--capture-exclude-source=a,b` (trimmed, non-blank, no `;` or `,`; distinct, ordinal-sorted):
  observations whose `sourceObjectId` starts with a prefix (ordinal) are dropped. The capture
  tool version becomes `converterVersion + "+region:" + lon,lat,lon,lat` (each `ToString("R")`)
  `+ "+exclude-source:" + prefixes joined ";"`; the converter version itself must not contain
  either marker. `--export-post-context-calls=FILE` writes `post_context_calls.parquet` (same
  metadata keys as the pack, relation `post_context_calls`): one row per usable call of every
  Bus/Trolleybus/Tram trip at an in-region stop, with `gtfs_trip_id, route_id,
  route_distinction, trip_id, call_index, route_stop_id, stop_id, stop_occurrence` (0-based
  repeat count of the stop within the trip), `context_id, same_stop_block_role, mode, direction,
  authored_post_key`.
- **Where:** `JdfPostEvidence.fs` 113–200; `capturePostInferenceEvidenceOnly` 64–72, 176–188;
  `JdfBundleEvidence.writePostContextCalls` 92–147.
- **Why:** "a training pack cannot contain the catalogue its labels come from"; "Region packs are
  training/review data, never publication input" (comments; post-scorer README "Leakage
  guards").
- **Pinned by:** `Capture stop region parses strictly and marks its tool version`,
  `Excluded capture sources remove only matching observations`,
  `Region capture routes only in-region stops exactly as a full capture does`,
  `Context-call enumeration reproduces every captured context identity`.
- **Constants:** markers `+region:`, `+exclude-source:`.
- **Diagnostic:** none.
- **Proposed verdict:** port as capture options of the `capture` mode (training tooling depends
  on them); keep the tool-version marking so a restricted pack can never be mistaken for a full
  one.

### 3.2 The road and tram graph

#### POST-R010 — Graph input, projection and storage
- **What:** every PBF node with id and coordinates becomes a vertex; coordinates are projected
  WGS84 → EPSG:3035 (ETRS89-extended / LAEA Europe, `latitude_of_center 52`,
  `longitude_of_center 10`, false easting 4,321,000, false northing 3,210,000, ProjNet) and
  stored as **`float32`** (≈ 0.25–0.5 m resolution at Czech magnitudes). Nodes must arrive in
  ascending ID order; ways are resolved by binary search over the packed nodes (a way node not in
  the file is skipped, breaking the chain there). Edge length = Euclidean distance of the two
  projected `float32` points, stored as `float32`. All later geometry (snapping, headings,
  sides) is in these projected metres. The graph is a CSR (outgoing offsets, reverse
  offsets/edges) in memory-mapped temp files under `%TEMP%/jrutil-routing-<guid>`.
- **Where:** `Osm.fs` `OpenWithProgress` 1657–1923; `GeoData/Common.fs` 20–56.
- **Why:** "Conversion-local directed topology. Osmium is still the sole preprocessing engine;
  this reader only packs the already demand-clipped, type-ordered PBF" (comment).
- **Pinned by:** every router test in `JdfFixupsTests.fs` builds a graph.
- **Constants:** node stride 24, edge stride 24, snap stride 16 bytes (layout only).
- **Diagnostic:** "Packed routing graph ready: {NodeCount} nodes, {EdgeCount} directed edges"
  (log); progress phases `graph-nodes`, `graph-edges`, `graph-csr`, `graph-reverse`,
  `graph-snaps`.
- **Proposed verdict:** replace: `jr-geo` road/tram graph from `jr-formats` OSM facts, in memory,
  `f64` (or `f32` deliberately, see Q2), one projection helper; no temp files.

#### POST-R011 — Road way eligibility
- **What:** a way carries the road flag when (all tag values trimmed and lower-cased):
  `highway` ∈ {motorway, trunk, primary, secondary, tertiary, unclassified, residential,
  living_street, service, road, busway}; and not closed: `highway ≠ construction` and none of
  the keys `construction`, `abandoned`, `disused`, `proposed` present (any value); and not
  generally blocked without a transit permission: blocked when any of `access`, `vehicle`,
  `motor_vehicle` ∈ {no, private}, permitted when `bus` or `psv` ∈ {yes, designated, official,
  permissive}. `*_link` highways, `track`, `pedestrian`, `bus_guideway` are not routable (B5).
  `access=destination`, `motor_vehicle=agricultural` etc. do not block. Conditional tags are
  ignored (they are only audited).
- **Where:** `Osm.fs` 34–37 (`roadHighways`), 254–284 (`affirmativeAccess`,
  `hardNegativeAccess`, `roadRoutingFlags`).
- **Why:** unknown beyond the code.
- **Pinned by:** `Packed router respects direction mode and access precedence` (`access=no` +
  `bus=yes` routes a bus).
- **Constants:** the highway list; affirmative {yes, designated, official, permissive}; negative
  {no, private}; closed keys. → `posts.graph.road_highways`, `posts.graph.closed_keys`,
  `posts.graph.access_*` (data, not code).
- **Diagnostic:** none.
- **Proposed verdict:** port the classifier with the lists in policy data; fixing B5 is Q3.

#### POST-R012 — Severe access penalty
- **What:** a road way whose `bus` or `psv` is `no` or `private` keeps the road flag but gets the
  severe flag: every edge of it costs 8 × its length.
- **Where:** `Osm.fs` 283, 389–390 (`edgePenalty`, `edgeCost`).
- **Why:** unknown (a soft block keeps a route available where the tagging is wrong).
- **Pinned by:** none.
- **Constants:** factor `8.0` → `posts.graph.restricted_access_factor`.
- **Diagnostic:** `restricted_access_edge_count` and `access_penalty_metres` in the variant row.
- **Proposed verdict:** port.

#### POST-R013 — Service road class
- **What:** `highway=service` sets the service flag. It counts `service_edge_count` in variants
  (a learned feature) and splits road threads (R022).
- **Where:** `Osm.fs` 284.
- **Why:** "Local thread identity keeps a service bay distinct from its through road" (test).
- **Pinned by:** `Local thread identity keeps a service bay distinct from its through road`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### POST-R014 — Tram way eligibility
- **What:** `railway=tram` and none of the closed keys → tram flag. A way may carry both road
  and tram flags (street track) and is then one edge with both.
- **Where:** `Osm.fs` 285–289, 1758.
- **Why:** unknown.
- **Pinned by:** `Packed router respects direction mode and access precedence` (tram-only edge
  does not route a bus).
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### POST-R015 — One-way
- **What:** `oneway` ∈ {yes, true, 1} → forward edges only; ∈ {-1, reverse} → backward only;
  otherwise `junction=roundabout` → forward only; otherwise both. Applies to road and tram flags
  alike. `oneway:bus`, `oneway:psv`, `busway=opposite_lane` are ignored (B7).
- **Where:** `Osm.fs` 290–295, 1769–1780.
- **Why:** unknown.
- **Pinned by:** `Packed router respects direction mode and access precedence`.
- **Constants:** the value lists → `posts.graph.oneway_*`.
- **Diagnostic:** none.
- **Proposed verdict:** port; bus contraflow is Q3.

#### POST-R016 — Barrier nodes
- **What:** a node with any `barrier` tag, unless its `bus` or `psv` is affirmative, is blocked:
  no edge is created from or to it, for road and tram alike. This includes `lift_gate`, `gate`,
  `kerb`, `toll_booth`, `border_control` (B6).
- **Where:** `Osm.fs` 263–268, 1774–1775.
- **Why:** "Packed router hard-blocks barriers" (test name).
- **Pinned by:** `Packed router hard-blocks barriers` (`barrier=bollard`).
- **Constants:** none (any value).
- **Diagnostic:** none.
- **Proposed verdict:** port with a reviewed list of blocking barrier values (data), see Q3.

#### POST-R017 — Edge construction
- **What:** for each way with flags ≠ 0, id and nodes: for each consecutive node pair (left,
  right) both found, `left ≠ right`, neither blocked: a forward edge left→right and/or a backward
  edge right→left (R015), each with `(from, to, length, wayId, flags)`. Edges are placed in CSR
  order by a counting scatter in PBF stream order (edge index = position among the from-node's
  outgoing edges in input order).
- **Where:** `Osm.fs` 1755–1784, 1810–1863.
- **Why:** "Counting scatter produces deterministic CSR directly from the type/ID-ordered PBF
  stream" (comment).
- **Pinned by:** router tests.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port the semantics; edge numbering is internal and must not leak into
  anything compared (it leaks today via `snap_edge_id`, diagnostic only).

#### POST-R018 — Turn restrictions
- **What:** a relation with a `restriction` tag and members `from` (way), `to` (way), `via`
  (node, present in the graph) becomes `{fromWay, toWay, viaNode, only = value starts with
  "only_"}`. Via-way restrictions, `restriction:bus`, `restriction:psv`,
  `restriction:conditional`, `except=*` and the `type` tag are ignored (B7). A transition from
  edge `in` to edge `out` at node `v` is checked only when `v` is a via node: every restriction
  with (`viaNode = v`, `fromWay = way(in)`), in order (via, from, only, to), must allow it: an
  `only` restriction allows only `toWay = way(out)`, a `no` restriction forbids
  `toWay = way(out)`; the first refusal stops. U-turns and same-way continuation are otherwise
  unrestricted.
- **Where:** `Osm.fs` 334–342, 366–388, 1785–1805.
- **Why:** unknown.
- **Pinned by:** `Packed router indexes unrelated turn restrictions` (performance only; no test
  pins a refusal).
- **Constants:** prefix `only_`.
- **Diagnostic:** `restriction_lookups`, `restriction_rules_examined` (log "Routing metrics").
- **Proposed verdict:** port; add a test that pins a refused turn.

#### POST-R019 — Modes
- **What:** routing modes `RoadBus`, `Trolleybus`, `TramRouting`. Bus and trolleybus both require
  the road flag (there is no trolley-wire network); tram requires the tram flag. The context mode
  `E` → tram, `T` → trolleybus, anything else → bus.
- **Where:** `Osm.fs` 47, 296; `JdfPostEvidence.routeWork` 682–686.
- **Why:** unknown.
- **Pinned by:** `Packed router respects direction mode and access precedence`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port (trolleybus as road); a wire network is not in the OSM filter.

#### POST-R020 — Snapping
- **What:** a projected point snaps to every edge carrying the mode's flag whose segment lies
  within 75 m (perpendicular distance to the clamped projection, fraction ∈ [0, 1], fraction 0
  for zero-length edges); candidates are ordered by (distance, edge id, fraction), the first 8
  kept, then only those within best + 15 m. Prepared snaps (`PrepareKnownSnaps`) compute the same
  set for every precise stop coordinate and every observation coordinate up front: the 8 best by
  (distance, edge id) per (coordinate, mode) by scanning every edge against a 200 m query grid,
  then the +15 m band; lookups key on the exact bits of the projected coordinate. With
  `buildGlobalSnaps=false` (production) there is no global snap index, so an unprepared
  coordinate gets no snaps ("snap-unavailable"); every coordinate the capture routes is prepared.
- **Where:** `Osm.fs` 394–446 (`pointSegmentSnap`, `snapRange`, `snap`), 1387–1502
  (`PrepareKnownSnaps`), 1874–1915 (global index).
- **Why:** "Exact coordinates: a rounded key let whichever nearby coordinate was prepared first
  decide the snaps of every coordinate sharing the key" (comment).
- **Pinned by:** `Query-specific snaps match the global snap index`.
- **Constants:** radius `75.0` m, `8` candidates, band `15.0` m, grid cells 250 m (global) and
  200 m (prepared) → `posts.router.snap_radius_m`, `snap_candidates`, `snap_band_m`.
- **Diagnostic:** progress `prepare-routing-snaps`, `routing-snaps`; log "Prepared routing snaps
  for {QueryCount} conversion coordinates".
- **Proposed verdict:** port the snap rule; replace the two implementations by one spatial index
  in `jr-geo`.

#### POST-R021 — Shortest path (edge-based A*)
- **What:** states are directed edges; a state's distance is the cost up to the edge's end node.
  Start: for each start snap on edge e, `(1 − fraction) × cost(e)` (depth 1). Heuristic: the
  distance from a node to the circle around the target edges' end nodes (centre = mean of the
  distinct end nodes of all target edges, radius = the largest distance to it), floored at 0.
  Priority `(distance + h(to-node), edge id)`. A popped state is skipped when stale (queued
  estimate > current estimate + 1e-4), when its distance ≥ the best result, or when its distance
  > 30,000. On a target edge (with `requireTransition`, only at depth > 1), each target snap gives
  `distance − (1 − targetFraction) × cost(e)`; values ≥ −1e-4 are clamped at 0 and kept when
  smaller, ties broken by the smaller (origin edge, target edge, origin fraction, target
  fraction). Expansion: outgoing edges of the end node with the mode flag whose transition is
  allowed (R018); relax `distance + cost(out)`; the origin snap propagates. At most 100,000
  expansions. Result: the cost and the edge path (predecessors back to the origin edge), else
  `search-limit` if 100,000 expansions were used, else `disconnected`; no start or no target →
  `snap-unavailable`. Costs are penalised metres (R012), so the 30 km bound is on cost, not
  length. Correct for a single target (the only production use, R051); with several targets the
  `distance < result` pruning can miss a target whose suffix is long (dead code only, R024).
- **Where:** `Osm.fs` 447–566 (`routeDetailed`, `route`).
- **Why:** comments ("Distance to the enclosing target circle is an admissible O(1) lower
  bound…", "PriorityQueue keeps superseded entries…").
- **Pinned by:** `Packed router uses fractional edge positions and directed right side` (route
  between fractional points ≈ 111 m).
- **Constants:** expansions `100000`, cost bound `30000.0`, tolerance `1e-4` (format constants:
  `maximum_search_states`, `maximum_search_distance_metres` are recorded in the pack).
- **Diagnostic:** reasons `snap-unavailable`, `search-limit`, `disconnected`.
- **Proposed verdict:** replace by a clean edge-based Dijkstra/A* in `jr-geo` with the same cost
  model, bounds and deterministic tie-break; keep the two ceilings as format constants.

#### POST-R022 — Threads, directed threads and faces
- **What:** the *local thread* of an edge is the maximal chain through nodes with exactly one
  onward neighbour. Mode mask = edge flags ∧ (road | tram) (1 road, 2 tram, 3 both); class = 8 if
  the edge has the service flag, else 0. Neighbours of a node, excluding the node arrived from:
  the distinct other ends of its outgoing and incoming edges whose flags overlap the mask and
  (mask = 2 or whose service bit equals the class). A walk from (previous, current) follows the
  single neighbour until a node with ≠ 1 neighbours (end = that node's OSM id, loop false) or a
  revisited node (end = smallest OSM id visited, loop true). For edge (left → right):
  `leftEnd = walk(previous=right, start=left)`, `rightEnd = walk(previous=left, start=right)`;
  identity = first 16 bytes of `SHA-256("{mask}|{class}|{min end}|{max end}|{loop}")` as
  lower-case hex (32 characters), `loop` printed `True`/`False` (either walk looped).
  `reversed` = `osmId(left) > osmId(right)` if looped, else `leftEnd > rightEnd`. Directed
  thread = identity + `:+` (not reversed) or `:-`. Face = identity + `:` + `C`/`R`/`L` (R054).
  Thread ends are cached per (mask, class, previous, current) and identities per edge, shared by
  all workers; results do not depend on the visiting order.
- **Where:** `Osm.fs` 753–840 (`localThreadIdentity`, `localFaceIdentity`,
  `directedThreadIdentity`).
- **Why:** "A thread is a maximal chain of nodes with exactly one onward neighbour… Both results
  are the same from every start on the thread" (comment); the face is "stable across degree two
  OSM way splits" (test).
- **Pinned by:** `Local routed face is stable across degree two OSM way splits`,
  `Local thread identity keeps a service bay distinct from its through road`.
- **Constants:** class value `8`, mask values 1/2/3 (part of the identity payload).
- **Diagnostic:** none.
- **Proposed verdict:** port the definition (threads drive movement families, R055, and
  consolidation, R070). The exact payload need not match F# unless Rust and F# packs must be
  interchangeable (Q1).

#### POST-R023 — Corridor identity
- **What:** `corridor_id` = FNV-1a 64 (offset 14695981039346656037, prime 1099511628211) over,
  for each path edge in order, the 8 little-endian bytes of OSM id(from), OSM id(to), way id;
  printed as 16 lower-case hex digits. (A second identity over raw edge indices exists only in
  the dead router path.)
- **Where:** `Osm.fs` 671–684.
- **Why:** "over OSM node and way IDs, so it does not depend on the graph's edge numbering"
  (comment).
- **Pinned by:** `Raw corridor capture is deterministic complete and truncated to three`.
- **Constants:** FNV parameters.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### POST-R024 — Router code not on the production path
- **What:** `Route` (with a 250,000-entry rounding cache), `RouteVia`, `RouteViaMany`,
  `EvaluateCandidates`, `EvaluateCandidatesWithAnchors` (`routeViaManyEvidence`: one baseline
  over all anchor snaps, a 25 m / 5 % ambiguity band, tied alternatives, per-candidate scores
  with hard-coded 90°/2 m/50 m), `EvaluateBoundaryCandidates*`, `InspectSnaps`,
  `CandidateRouteEvidence`, `RoutedCorridorEvidence` are called only by tests. About 450 lines of
  `Osm.fs`. Several router tests (`RouteViaMany shares…`, `Neighbouring candidate anchors…`,
  `Paired anchor alternatives…`, `Candidate geometry does not resolve…`, the side assertions of
  `Packed router uses fractional edge positions…`) pin only this path.
- **Where:** `Osm.fs` 919–924, 1056–1376, 1511–1550, 1615–1640.
- **Why:** history (the pre-evidence estimator).
- **Pinned by:** the tests named.
- **Constants:** 25 m, 0.05, 32 anchor snaps, 90°, 2 m, 50 m.
- **Diagnostic:** none.
- **Proposed verdict:** drop; re-express the side-of-road assertions as tests of R054.

### 3.3 Observations and route points

#### POST-R030 — Observation admission
- **What:** an observation enters the pack when (a) its `sourceObjectId` does not start with an
  excluded prefix (R005), (b) its stop has a precise location (`StopPrecise`; with several,
  the one with the smallest (lat, lon)), and (c) its distance to that location is ≤ 300 m
  (`equirect`, squared comparison). Stops without a precise location get no observations, hence
  no estimation.
- **Where:** `JdfPostEvidence.fs` 324–339, 429–448.
- **Why:** test name "Post evidence excludes candidates outside the precise parent centroid
  radius".
- **Pinned by:** `Post evidence excludes candidates outside the precise parent centroid radius`
  (299.999 m in, 300.001 m out; no precise location → none).
- **Constants:** `300.0` m → `posts.candidates.max_distance_from_stop_m`.
- **Diagnostic:** none (B3 is where it matters).
- **Proposed verdict:** port; the precise-location choice must be the published one (B14).

#### POST-R031 — Route points and their identity
- **What:** admitted observations are grouped by (stop, exact coordinate); each group is a route
  point. `route_point_id = "route-point:" + sha("{stop}|{lat}|{lon}")` where `lat`, `lon` are the
  **decimals' invariant `ToString()`**, i.e. the text as parsed from the JDF extension file,
  trailing zeros included. Route points of a stop are ordered by this id (ordinal), which fixes
  the attachment order, consolidation order and several tie-breaks.
- **Where:** `JdfPostEvidence.routePointId` 292–296; grouping 456–476.
- **Why:** unknown.
- **Pinned by:** `Published capture bytes ignore input order workers and spill chunks`.
- **Constants:** prefix `route-point:`.
- **Diagnostic:** none.
- **Proposed verdict:** replace: a content-derived id from `jr_model::identity` over the stop and
  a canonical coordinate text (e.g. 7 decimals), carried as a field; it is a pack-internal key and
  never published.

#### POST-R032 — Route point facts
- **What:** `representative_observation_id` = ordinal-min member; `observation_ids` distinct
  ordinal-sorted; `has_current_lifecycle` = any lifecycle ∈ {"", active};
  `has_obsolete_lifecycle` = any ∈ {abandoned, disused, construction} (`current`, `temporary`,
  `proposed` are neither, B8); `source_support_weight` = sum; explicit/denied modes = split on
  `; , space`, trimmed, upper-cased, distinct, ordinal-sorted (`ROAD`, `TRAM`, `TROLLEYBUS`).
- **Where:** `JdfPostEvidence.fs` 318–322, 465–476.
- **Why:** unknown.
- **Pinned by:** `Evaluation is deterministic and diagnostics do not change decisions` (obsolete
  observation becomes a hypothesis member).
- **Constants:** lifecycle sets → `posts.candidates.current_lifecycles`,
  `obsolete_lifecycles`.
- **Diagnostic:** none.
- **Proposed verdict:** port with the sets in policy; resolve B8.

#### POST-R033 — Stops that are estimated
- **What:** only stops with **two or more route points** get contexts. A stop with one observed
  position never gets an estimated post (its calls stay `:unspecified`), even if that position is
  certain.
- **Where:** `JdfPostEvidence.fs` 518–519.
- **Why:** unknown (a single candidate cannot be "chosen").
- **Pinned by:** none directly.
- **Constants:** `2`.
- **Diagnostic:** none.
- **Proposed verdict:** port; whether a lone candidate should become a post is Q7.

### 3.4 Contexts

#### POST-R040 — Trips and usable calls
- **What:** every trip of a route whose effective mode is `Bus`, `Trolleybus` or `Tram`. Calls
  are filtered to usable ones (not departure `|` or `<`, not both times absent) and ordered by
  `routeStopId × (−1 if the trip number is even else 1)` (stable). Direction = 1 if the first
  usable call's trip number is even, else 0. Mode code: Tram `E`, Trolleybus `T`, else `A`.
- **Where:** `JdfPostEvidence.fs` 309–316, 351–361, 499–512.
- **Why:** JDF trip-number parity gives direction (jdf-convert JDF-R440).
- **Pinned by:** `Context-call enumeration reproduces every captured context identity`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port, using the model's call order and `Trip` direction instead of
  recomputing them (rule 5).

#### POST-R041 — Pattern hash and pattern deduplication
- **What:** `pattern_hash = sha(usable calls' stop numbers in order, joined ",")` (no prefix).
  Contexts are built once per distinct identity `"{route}|{distinction}|{direction}|{pattern}|
  {authored keys of the usable calls, "" for none, joined ";"}"`, first trip in batch order.
- **Where:** `JdfPostEvidence.fs` 298–300, 502–512; duplicate `JdfPostPlan.completePatternHash`
  217–224.
- **Why:** "Timetable call count is not evidence cardinality: identical trip patterns share one
  routed context" (`estimatePostEvidenceOutputBytes` comment).
- **Pinned by:** `Capture preflight counts deduplicated route patterns rather than timetable
  calls`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** replace by the model's stop-pattern index (`ARCHITECTURE.md`: "Stop
  patterns are an index a pass derives"); the hash is a pack field, built in
  `jr_model::identity`.

#### POST-R042 — Same-stop blocks and roles
- **What:** consecutive usable calls at the same stop form a block [start, finish). Role:
  `through` if the block has one call; else `incoming` (first), `outgoing` (last), `interior`.
  `same_stop_block_id = stableId("same-stop-block:", [route, distinction, pattern, start,
  finish])` for blocks of length > 1, else null.
- **Where:** `JdfPostEvidence.contextCallsForOrdered` 363–397; duplicate in
  `JdfToGtfs.inferredSelectionsForTrip` 420–444.
- **Why:** a vehicle that waits at a terminus (arrival and departure rows) may use different
  posts for arriving and leaving.
- **Pinned by:** `Evaluation is deterministic…` (a block id exists).
- **Constants:** prefix `same-stop-block:`.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### POST-R043 — Authored keys and assignment kind
- **What:** a call's authored key is `id:<stopPostId>` when it has a post id, else
  `num:<trimmed stopPostNum>` when the number is non-blank, else none.
  `assignment_kind = authored` with a key, else `unlabelled`. Only unlabelled contexts can
  receive an estimated post; authored contexts only position their authored post (R122).
- **Where:** `JdfPostEvidence.authoredPostKey` 302–307, 555–557; `JdfPostPlan.authoredPostKey`
  211–215; untrimmed variant in `JdfBundleTables.scanCallDerivedFacts` 199–203 (B4).
- **Why:** an authored post is the carrier's statement and is never overridden.
- **Pinned by:** `Evaluation is deterministic…` (authored contexts exist).
- **Constants:** prefixes `id:`, `num:`.
- **Diagnostic:** none.
- **Proposed verdict:** port with one key builder.

#### POST-R044 — Context identity
- **What:** `context_id = stableId("context:", [stop, mode, line, distinction, direction,
  pattern hash, position, role, authored key or ""])`; the store recomputes and checks it.
- **Where:** `JdfPostEvidence.fs` 392–396; `JdfPostEvidenceStore.fs` 474–479.
- **Why:** "Capture and label extraction share this enumeration so their context IDs cannot drift
  apart" (comment).
- **Pinned by:** `Context-call enumeration reproduces every captured context identity`.
- **Constants:** prefix `context:`.
- **Diagnostic:** none.
- **Proposed verdict:** port in `jr_model::identity` (it is the join key with training labels).

#### POST-R045 — Anchors
- **What:** for a block at stop S (in the deduplicated pattern), the previous anchor is found by
  scanning usable calls backwards from the block start, skipping calls at S, and taking the
  first stop that **has an anchor**; stops without one are skipped, so the anchor can lie several
  stops back. Likewise forwards for the next anchor. A stop's anchor coordinate is the medoid of
  its route points (minimum sum of **squared** `equirect` distances, ties by route point id) if
  it has route points, else its chosen precise location; otherwise it has none. Per role:
  `through` and `incoming` use the previous anchor (one coordinate), `outgoing` and `interior`
  use S's own route point coordinates as "previous" with no previous stop id; `through` and
  `outgoing` use the next anchor, `incoming` and `interior` S's own points; a missing anchor also
  falls back to S's own points with a null stop id.
- **Where:** `JdfPostEvidence.fs` 410–416 (`medoidCoordinate`), 490–492 (`anchors`), 513–562.
- **Why:** unknown (the medoid of the neighbour's own candidates approximates where the vehicle
  stood there).
- **Pinned by:** `Region capture routes only in-region stops exactly as a full capture does`.
- **Constants:** none.
- **Diagnostic:** `context_previous_stop_id`/`context_next_stop_id` null; learned features
  `anchor_previous_missing`, `anchor_next_missing`.
- **Proposed verdict:** port; note the anchor may fall outside the demand clip (B20).

#### POST-R046 — Canonical work order, deduplication and routing keys
- **What:** work rows are sorted by the work key (stop, mode, line, distinction, direction,
  pattern, position, role, authored key option); adjacent rows with equal keys must have equal
  routing inputs (else "Equivalent post-inference contexts disagree on routing inputs") and
  collapse to one; the context ordinal is the rank in this order. `routing key =
  stableId("routing-evidence:", [stop, mode, prev, points, next])` where each coordinate list is
  the IEEE-754 bits of lon then lat as 16 lower-case hex digits each, concatenated per
  coordinate and joined ",". Within each (stop, mode) group only the first work row of each
  routing key is routed.
- **Where:** `JdfPostEvidence.fs` 563–637.
- **Why:** "Keys of every ordered work row, in store order, so the evidence joins below do not
  hash each row's geometry again" (comment).
- **Pinned by:** `Capture preflight counts deduplicated route patterns rather than timetable
  calls`, `Published capture bytes ignore input order workers and spill chunks`.
- **Constants:** prefix `routing-evidence:`.
- **Diagnostic:** progress `capture-routing-admission` (workers, routing evidence, logical
  contexts).
- **Proposed verdict:** port the dedup; the routing key becomes the stage-cache key of one routed
  post context (`ARCHITECTURE.md`, "The stage cache").

#### POST-R047 — Region filter of contexts
- **What:** with a capture region, a block gets contexts only when S's chosen precise location
  lies in the box (R005).
- **Where:** `JdfPostEvidence.fs` 493–498, 519.
- **Why:** see R005.
- **Pinned by:** `Region capture routes only in-region stops exactly as a full capture does`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port with R005.

### 3.5 Routed evidence per context

#### POST-R050 — Anchor snaps
- **What:** for the previous coordinates and, separately, the next coordinates: project each,
  take its first 4 snaps (R020), concatenate, distinct by (edge, fraction), group by directed
  thread of the edge and keep per thread the snap with the smallest (distance, edge, fraction),
  sort by (distance, edge, fraction), keep at most 8.
- **Where:** `Osm.routeContextEvidence` 934–949.
- **Why:** "This ceiling prevents an anomalous dense station from multiplying searches without
  applying any publication-policy threshold" (comment).
- **Pinned by:** `Raw corridor capture is deterministic complete and truncated to three` (input
  order and worker count do not change the result).
- **Constants:** `4` snaps per coordinate, `8` anchors → `posts.router.snaps_per_anchor`,
  `posts.router.max_anchor_snaps`.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### POST-R051 — Corridor variants
- **What:** if either side has no anchor snap → one unavailable variant (rank 0, reason
  `snap-unavailable`) and one unavailable attachment per route point (`baseline-unavailable`).
  Otherwise route every (start, finish) pair separately with R021 (`requireTransition` false), keep
  the found paths, distinct by corridor id (first kept), sort by (cost, corridor id), keep 3. None
  found → unavailable variant with reason `disconnected`. Variant facts as in section 2.3
  (`relative = max(0, cost − cost₀)`, `fraction = relative / cost₀` or 0, `access penalty =
  max(0, cost − length)`, ingress/egress = directed thread of the first/last edge).
- **Where:** `Osm.routeContextEvidence` 950–996.
- **Why:** `VariantEnumerationVersion = "directed-thread-top3-v1"`; "policy-neutral directed
  corridor … no publication threshold is applied here" (type comment).
- **Pinned by:** `Raw corridor capture is deterministic complete and truncated to three`.
- **Constants:** `3` variants (format constant `capture_ceilings.maximum_corridor_variants`).
- **Diagnostic:** reasons in the pack.
- **Proposed verdict:** port (up to 8 × 8 searches per context is the main routing cost; a
  one-to-many search giving the same set is allowed if decisions agree, see section 9).

#### POST-R052 — Attachment of a route point to a variant
- **What:** per variant: one forward Dijkstra from the variant's origin snap and one reverse
  Dijkstra from its target snap, both bounded by `cost + 1000` and by 100,000 expansions (forward
  initial `(1 − f) × cost(e)`, reverse initial `f × cost(e)`, reverse value = cost from the start
  of an edge to the target). If either hit the expansion limit, every attachment of the variant
  is unavailable (`search-limit`). Per route point: its snaps (R020) that both searches reached
  give `left = forward[e] − (1 − f) × cost(e)`, `right = reverse[e] − f × cost(e)`; snaps with
  either < −1e-4 drop; `via = max(0, left) + max(0, right)`; pick the minimum by (via, distance,
  edge, fraction); none → `routed-excess-or-disconnected`. `excess = max(0, via − cost)`;
  `excess > 1000` → `evidence-horizon`.
- **Where:** `Osm.routeContextEvidence` 997–1037; `exploreForward` 567–608; `exploreReverse`
  609–655.
- **Why:** "Evidence capture deliberately extends beyond the current 500 m publication policy.
  Policy replay can tighten the limit without rebuilding the graph" (comment in the twin path).
- **Pinned by:** `Raw corridor capture is deterministic complete and truncated to three`.
- **Constants:** horizon `1000.0` m (format constant `capture_ceilings.routed_excess_metres`),
  `100000` expansions.
- **Diagnostic:** reasons in the pack.
- **Proposed verdict:** port.

#### POST-R053 — Attachment geometry
- **What:** first along the variant's own path: the snap edge must occur exactly once in the path
  (else none); otherwise along the via path (forward parents back to the origin, the snap edge,
  reverse successors to the target), same condition; none → `corridor-unavailable`. From the
  snap's projected point walk 15 m backwards and 15 m forwards along the path (edge by edge, from
  the snap fraction). Direction `d` = after − before, or the snap edge vector when |d|² < 1e-6.
  `signed = (dx (py − sy) − dy (px − sx)) / max(1e-6, |d|)` with p the projected candidate and s
  the snap point (negative = right of travel). `corridor heading = (atan2(dx, dy) in degrees +
  360) mod 360`; `attachment heading` = the same for the snap edge (from → to); `heading
  difference` = the smaller angle between them. Face side: `edgeSigned` computed with the snap
  edge vector, negated when the edge's thread is reversed; |·| ≤ 2 → `C`, < 0 → `R`, else `L`.
  `corridor_distance = proximity_distance = snap_distance` (distance to the snap edge, not to the
  corridor). The snap point is projected back to WGS84.
- **Where:** `Osm.candidateGeometry` 841–904; `routeContextEvidence` 1038–1054.
- **Why:** "Bind geometry to the exact directed fractional state which routing selected… A
  repeated visit to the same directed edge cannot yet be assigned to one chainage occurrence…
  Abstain instead of silently borrowing the first visit's tangent" (comments).
- **Pinned by:** `Packed router uses fractional edge positions and directed right side` (through
  the dead path, which shares `candidateGeometry`).
- **Constants:** window `15.0` m, side band `2.0` m, `1e-6` → `posts.router.heading_window_m`,
  `posts.router.face_centre_band_m`.
- **Diagnostic:** `corridor-unavailable`.
- **Proposed verdict:** port; rename the distance field in the model (section 7).

#### POST-R054 — Movement family
- **What:** `movement_family_id = stableId("movement-family:", [stop, mode, rank-0 ingress
  thread or "", rank-0 egress thread or "", role])`. Contexts at one stop with the same mode,
  role and rank-0 entry and exit threads form one family, across lines and patterns, authored or
  not.
- **Where:** `JdfPostEvidence.withFamily` 794–803; checked in `JdfPostEvidenceStore` 597–604.
- **Why:** unknown in a comment; the heuristic decides one post per family (R085).
- **Pinned by:** `Hash-consistent semantic corruption is rejected before evaluator entry`.
- **Constants:** prefix `movement-family:`.
- **Diagnostic:** none.
- **Proposed verdict:** port (it is how trips "conflicting" at a stop are reconciled; see R086).

#### POST-R055 — Join of routed evidence to contexts
- **What:** routed rows are merged by ordinal; for each (stop, mode) group the routed evidence is
  cached by routing key and every work row of the group takes the evidence of its key ("Routed
  evidence is missing a logical context geometry" / "… contains an unused context geometry" if
  the join is incomplete). Corridor variants and attachments are emitted per context (a routed
  result shared by several contexts is repeated for each).
- **Where:** `JdfPostEvidence.fs` 761–851.
- **Why:** comments.
- **Pinned by:** `Published capture bytes ignore input order workers and spill chunks`.
- **Constants:** none.
- **Diagnostic:** progress `capture-routing-contexts`.
- **Proposed verdict:** port the join; in memory, contexts reference a shared routed result by
  handle instead of copying it.

#### POST-R056 — Routing cache
- **What:** optional (`--routing-cache=DIR`, on by default in Oběhy). File
  `routing-contexts-<sha(routerIdentity)>.bin` (gzip, binary: format version 2, count, then per
  entry: key, min/max tile x/y, digest, last-used day, payload length, JSON payload of the
  evidence with snap edges replaced by `(from OSM id, to OSM id, way id, occurrence)`). The
  router identity is `format-2|<RoutingSourceSha256 assembly metadata or module version id>|
  <ProjNet, OsmSharp, FSharp.Core versions>|<runtime>|<architecture>`. A computation records the
  box of every projected point it read (snap queries record ±500 m, edges their end points,
  thread walks their nodes); the entry stores the 1 km tile range of that box and an XxHash128
  digest of per-tile multiset fingerprints (two XxHash3 seeds over each edge record: node ids,
  way, flags, length, coordinates, adjacency ranks, assigned to every tile within 75 m of the
  segment; and each restriction at its via node's tile). A hit requires an equal digest and every
  snap edge resolvable in the current graph. Save writes entries used this run plus unused ones
  younger than 45 days (sorted by key) and deletes cache files of other router identities. An
  unreadable file is ignored with a warning.
- **Where:** `JdfRoutingCache.fs` 1–203; `Osm.fs` 698–750, 1554–1606.
- **Why:** "A hit returns exactly what routing would, even after the demand clip or the OSM data
  changed elsewhere… The cache never affects output" (module comment); PROGRESS item 3.
- **Pinned by:** `Routing cache reuses evidence exactly across graph changes outside the read
  region`, `Published capture bytes ignore input order workers and spill chunks` (cold vs warm),
  `Routing cache identity uses the embedded routing source hash`.
- **Constants:** retention 45 days, 1 km tiles, 75 m tile margin, format 2.
- **Diagnostic:** log "Routing cache: {Hits} reused, {Invalidated} invalidated by graph changes,
  {Misses} routed, {Entries} entries saved to {RoutingCachePath}".
- **Proposed verdict:** replace by the stage cache with the grain "one routed post context"
  (`ARCHITECTURE.md`): key = routing key + router version + digest of the graph region read.
  Keep the region-digest idea; it is what makes the cache survive daily OSM changes.

#### POST-R057 — Parallel capture and determinism
- **What:** routing units go through one bounded FIFO queue to `min(routingWorkers,
  budget / max context charge)` workers (charge = 64 KiB + points × 3 × 512 per stop); each
  worker appends to its own ordered store; stores are merged by ordinal. Graph-internal
  parallelism is switched off during capture. The pack bytes do not depend on worker count,
  input order, memory budget or cache temperature.
- **Where:** `JdfPostEvidence.fs` 651–760.
- **Why:** "One shared FIFO queue: a slow unit occupies only its own worker" (comment).
- **Pinned by:** `Published capture bytes ignore input order workers and spill chunks`,
  `Capture cancellation and output collision leave no worker spools`.
- **Constants:** 64 KiB, 512 B (admission only).
- **Diagnostic:** progress events.
- **Proposed verdict:** replace by rayon over routing keys with results collected in key order;
  drop memory admission (rule 8); keep the byte-identity test.

### 3.6 The evidence pack

#### POST-R060 — Pack writing
- **What:** relations of section 2.3 written in the store order to a sibling temp directory
  `.<name>.tmp-<guid>`, manifest last, the pack opened with full validation (R061), then moved
  into place; on failure the temp directory is deleted. In Live mode the pack goes to
  `%TEMP%/jrutil-post-evidence-<guid>` with capture tool version = converter version.
  CaptureOnly first checks free disk: estimate = 384 × observations + 320 × route points + 1024 ×
  contexts + 512 × variant upper bound + 384 × attachment upper bound, plus 256 MiB reserve.
- **Where:** `JdfBundleEvidence.writePostEvidenceStore` 259–445; `JdfBundleModel` 47, 67–76;
  `capturePostInferenceEvidenceOnly` 119–133.
- **Why:** atomic publication (comments).
- **Pinned by:** `Capture cancellation and output collision leave no worker spools`,
  `Published capture bytes ignore input order workers and spill chunks`.
- **Constants:** 384, 320, 1024, 512, 384 bytes; reserve 268,435,456.
- **Diagnostic:** progress `capture-evidence-*`, `capture-disk-preflight`.
- **Proposed verdict:** port the writer in `jr-publish` for capture mode only; drop the disk
  estimate.

#### POST-R061 — Pack validation
- **What:** on open: manifest shape, format and versions, SHA-256 identities well-formed, pack id
  recomputed, ceilings equal to the supported ones, counts ≥ 0, exactly the six files, each
  file's size, SHA-256, rows and fingerprint as declared; each Parquet's schema (names, types,
  nullability in order) and metadata equal to the expected ones; then semantics in one streaming
  pass: ordering and uniqueness (section 2.3), observation ↔ route point membership, numeric
  ranges, context identity and family recomputed, block ids recomputed (`same-stop-block:` over
  line, distinction, pattern, min position, max position + 1) and consistent per stop, neighbour
  nulls consistent with roles, variant sentinels and monotone costs, attachment coverage and
  sentinels, counts equal to the manifest. Optional identity expectations (merged JDF, PBF,
  tool version) are checked when given.
- **Where:** `JdfPostInference.loadEvidenceManifest` 575–632, `validateEvidencePack` 634–668;
  `JdfPostEvidenceStore.fs` 120–637.
- **Why:** "The sole production policy-evaluation boundary. The store has already passed
  structural, hash, ordering, key, FK, sentinel, and coverage checks" (evaluator comment).
- **Pinned by:** `Hash-consistent semantic corruption is rejected before evaluator entry`,
  `Malformed evidence matrix is rejected before evaluator entry`, `Evidence validation does
  not accept legacy derived scores`, `Evidence v1 is rejected before any Parquet file is
  opened`, `Evidence manifest rejects extra v2 fields before Parquet access`, `Evidence file
  tampering is rejected by the manifest hash`, `Capture tool version participates in evidence
  pack identity`.
- **Constants:** format constants of section 2.3.
- **Diagnostic:** exceptions.
- **Proposed verdict:** port as the evidence-pack reader in `jr-formats` (used by offline
  evaluation and by `jrutil` when a pack is given); a live build that holds the pack in memory
  validates by construction (Q8).

#### POST-R062 — Joined replay rows
- **What:** contexts, variants and attachments are walked together (each must be complete and in
  order) into one row per (context, variant, route point) carrying the context facts, the
  variant's relative costs and service-edge count, and the attachment facts. The variant's
  failure reason replaces a missing attachment reason, and any reason forces availability
  `unavailable`. Stop numbers are parsed from `jdf:stop:<N>`.
- **Where:** `JdfPostEvidenceReader.replayScoreRows` 90–289, `readReplayRoutePoints` 291–357.
- **Why:** "Read the three routing relations as one ordered join" (comment).
- **Pinned by:** `Evaluation is deterministic and diagnostics do not change decisions` (one
  joined traversal in publication mode).
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port as an in-memory index over the pack structs.

#### POST-R063 — Live pack lifetime
- **What:** the live pack is deleted after a successful build and kept (logged "Inference
  evidence retained at {EvidencePath}") after a failure. The bundle manifest reads its manifest
  and attachment file hashes before deletion.
- **Where:** `writeBundleCore` 393–399, 677–680; `JdfBundleManifest` 314–321.
- **Why:** comment "The routing pass emits raw route-point facts into a temporary evidence pack,
  the same contract capture writes".
- **Pinned by:** `Routed bundle writes post-inference diagnostics only to the diagnostics
  artifact` (no scratch survives).
- **Constants:** none.
- **Diagnostic:** log line.
- **Proposed verdict:** drop the temp pack (Q8); record a content digest of the in-memory pack in
  the manifest instead.

### 3.7 Consolidation into hypotheses

#### POST-R070 — Compatibility of two route points
- **What:** points L, R of one stop are compatible when all hold: `equirect(L, R)` ≤ maximum
  diameter (2 m); not (lifecycles differ — current flag or obsolete flag — and either is
  obsolete); and over every (context, variant) where both have rows: faces, when both present,
  are equal; sides not opposite (both |offset| > 2 m with different signs); corridor headings,
  when both present, differ by ≤ 30°, and the chainage `|dx sin(h_L) + dy cos(h_L)|` (dx, dy =
  L − R in metres, h_L = L's corridor heading) is ≤ 20 m; and fewer than 2 contexts
  *distinguish* them, a context distinguishing when max(gL, gR) ≥ 0.55 and |gL − gR| ≥ 0.15
  with g the geometry score (R082, no support adjustments).
- **Where:** `JdfPostPolicyEvaluation.consolidateReplayRows` 19–78; predicates
  `JdfPostInference` 386–409.
- **Why:** unknown beyond the predicate names.
- **Pinned by:** `Every initial policy-grid field crosses its production behavior boundary`,
  `Every mandatory policy subsystem has a production boundary regression`.
- **Constants:** policy `consolidation.*`, `hard_gates.*` (section 2.4).
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### POST-R071 — Merge order
- **What:** clusters start as single points ordered by route point id; repeatedly, among all
  pairs (i < j) whose points are pairwise compatible, merge the pair with the smallest (diameter
  = largest pairwise distance, identity = the union's route point ids sorted and joined "|");
  the merged cluster (points sorted by id) takes index i; stop when no pair qualifies.
- **Where:** `consolidateReplayRows` 79–113.
- **Why:** unknown.
- **Pinned by:** `Evaluation is deterministic…`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port.

#### POST-R072 — Hypothesis identity and facts
- **What:** `hypothesis id = sha("{stop}|{distinct observation ids of the cluster, ordinal-sorted,
  joined "|"}")` (route point ids if the cluster has no observations; bare hex, no prefix).
  Medoid = the point with the minimum (sum of `equirect` distances to the cluster, route point
  id); facts: any current, any obsolete, summed support weight, union of explicit and of denied
  modes (first-seen order), the members.
- **Where:** `consolidateReplayRows` 114–140; `evaluateWithScorer` 76–86.
- **Why:** unknown.
- **Pinned by:** `Feature export writes the evaluator's diagnostic rows hypotheses and
  decisions`.
- **Constants:** none.
- **Diagnostic:** `hypotheses.parquet` in feature export.
- **Proposed verdict:** port; the id via `jr_model::identity`.

#### POST-R073 — Rows per hypothesis
- **What:** rows are grouped by ((context, variant), family, hypothesis of the row's route
  point); per group the first row is kept with: routed excess, corridor distance, signed offset =
  medians of the present values (null if none); face, corridor heading, attachment heading = the
  single distinct present value, else null (B16); alternative count max; cost gap min; tied
  agree = all; failure = null if any member has none, else the ordinal-smallest reason. Median of
  an even count is the mean of the middle two.
- **Where:** `consolidateReplayRows` 141–172; `medianFloat` (`JdfPostEvidenceReader` 437–441).
- **Why:** unknown.
- **Pinned by:** `Evaluation is deterministic…`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port; fix B16 deliberately or ledger it.

### 3.8 The heuristic decision

#### POST-R080 — Alternative corridors
- **What:** per context, the selected variant ranks are rank 0 and every rank < `maximum_variants`
  whose relative cost ≤ `absolute_tie_metres` or relative fraction ≤ `relative_tie_fraction`.
  Per (context, hypothesis): the rank-0 row is kept (a hypothesis without one is dropped from the
  context); `alternative count` = number of selected ranks; `cost gap` = smallest positive
  relative cost among the selected rows; valid = the hypothesis has a row for every selected rank
  and each is free of failure, available and has a face; `tied agree` = valid and exactly one
  distinct face; failure = the rank-0 failure if any, else none when valid and one face, else
  `alternative-corridor-invariant`.
- **Where:** `evaluateReplayPolicy` 186–217; `JdfPostInference.selectedCorridorVariantRanks`
  531–542.
- **Why:** test name "Alternative corridor thresholds reinterpret captured costs without
  routing".
- **Pinned by:** `Alternative corridor thresholds reinterpret captured costs without routing`.
- **Constants:** policy `alternative_corridors.*`.
- **Diagnostic:** failure `alternative-corridor-invariant`.
- **Proposed verdict:** port.

#### POST-R081 — Eligibility gates and the mode gate
- **What:** effective modes of a hypothesis: its explicit modes, or when empty the distinct sorted
  union of explicit modes of other hypotheses of the same stop within 15 m (`equirect` between
  medoids). Compatibility of a mode value v with context mode m: v = `SHARED` or v = m, or m = A
  and v ∈ {BUS, ROAD}, or m = E and v = TRAM, or m = T and v ∈ {TROLLEYBUS, BUS, ROAD}.
  Rejection reason, first that applies: `obsolete-lifecycle` (obsolete and not current), else
  `mode-incompatible` (effective modes non-empty and none compatible), else the topology reason
  (row failure), else `corridor-distance` (> 100 m), else `routed-excess` (> 500 m), else
  `confident-left-side` (offset > 2 m). Rejected rows take no part in ranking.
- **Where:** `evaluateReplayPolicy` 240–259; `JdfPostInference` 442–481 (`modeCompatible`,
  `modeIncompatible`, `effectiveModes`), 711–717 (`hardGateReason`).
- **Why:** "Most, nádraží: tram-only stop positions and catalogue points beside them are not bus
  posts; a catalogue point beside a bus platform is" (test comment); `effectiveModes` doc
  comment.
- **Pinned by:** `Every mandatory policy subsystem has a production boundary regression`.
- **Constants:** `ModeInheritanceRadiusMetres = 15.0` → `posts.modes.inheritance_radius_m`;
  policy `hard_gates.*`.
- **Diagnostic:** rejection reasons (feature export only).
- **Proposed verdict:** port; one mode-compatibility table shared with the learned path (section
  8).

#### POST-R082 — Geometry score
- **What:** `alignment = max(0, 1 − Δ(corridor heading, attachment heading) / 90)` (0 if either
  missing); `side` = 0.5 if |offset| ≤ 2, 1.0 if offset < 0, 0.0 if > 0 or missing; `proximity =
  max(0, 1 − corridor distance / 50)` (0 if missing); `routed fit = max(0, 1 − excess / 500)` (0
  if missing); `geometry = clamp(0.40 alignment + 0.25 side + 0.20 proximity + 0.15 routed fit,
  0, 1)`.
- **Where:** `JdfPostInference.geometryScore`, `geometryComponents`, `evidenceGeometryScore`,
  `routedFit` 670–709.
- **Why:** Czech traffic drives on the right; a post on the right of travel, close to the road,
  aligned with it and without detour is likely (test "directed right side").
- **Pinned by:** `Every mandatory policy subsystem has a production boundary regression`.
- **Constants:** alignment range `90.0`° (`CaptureAlignmentRangeDegrees`), proximity range
  `50.0` m (`CaptureProximityRangeMetres`) → `posts.geometry.alignment_range_deg`,
  `proximity_range_m`; weights and tolerances from the policy.
- **Diagnostic:** `diagnostic_scores.parquet` (feature export).
- **Proposed verdict:** port (the learned features use these components too).

#### POST-R083 — Support adjustments
- **What:** `source = min(0.05, max(0, supportWeight − 1) × 0.01)`; `modality` = −0.05 if a denied
  mode is compatible with the context, else 0.05 if an explicit mode is compatible, else 0.03 if
  there are no explicit modes, else 0; `support = clamp(source + modality, −0.05, 0.10)`;
  `final row score = supportingScore(geometry, support)` where `supportingScore(g, s) =
  clamp(g + s (1 − g), 0, 1)` for s ≥ 0 and `clamp(g + s g, 0, 1)` for s < 0.
- **Where:** `JdfPostInference.modalitySupportAdjustments` 483–495, `combinedSupportAdjustment`
  511–514, `supportingScore` 704–706; `evaluateReplayPolicy` 263–269.
- **Why:** unknown.
- **Pinned by:** `Every mandatory policy subsystem has a production boundary regression`.
- **Constants:** policy `modality.*`.
- **Diagnostic:** `source_adjustment`, `modality_adjustment` in feature export.
- **Proposed verdict:** port with the heuristic, or drop with it (Q4).

#### POST-R084 — Context winners
- **What:** context key = (stop, mode, line, distinction, direction, pattern, position, role,
  family) — **without the authored key** (B12). Per key: eligible rows sorted by (−final,
  −geometry, hypothesis id); the winner is the first, `lead = final − runner-up final` (or final
  alone).
- **Where:** `evaluateReplayPolicy` 270–285.
- **Why:** unknown.
- **Pinned by:** `Evaluation is deterministic…`.
- **Constants:** none.
- **Diagnostic:** none.
- **Proposed verdict:** port with the context id as key (fixes B12).

#### POST-R085 — Family candidate scores
- **What:** per (stop, family): candidates = eligible rows of the family grouped by hypothesis;
  `geometry = median`, `final = median` of the rows' scores; `popularity = min(0.10, 0.02 ×
  min(5, number of contexts of other families at the stop whose winner is this hypothesis))`;
  `final = supportingScore(final, popularity)`; `final = supportingScore(final, isolation)` with
  `isolation = 0.05 × min(1, d / 12)` for d = distance to the nearest other hypothesis of the
  stop (formula with 111,320 for both axes, B18; 0 when there is none or the separation is 0);
  `routed = median` routed fit; faces, any alternative, all agree. Sorted by (−final, −geometry,
  hypothesis id).
- **Where:** `evaluateReplayPolicy` 222–237, 286–318; `JdfPostInference`
  `establishedPopularityAdjustment` 507–509, `spatialIsolationAdjustment` 432–438.
- **Why:** unknown.
- **Pinned by:** `Every mandatory policy subsystem has a production boundary regression`.
- **Constants:** cap `5` (code) → `posts.heuristic.popularity_context_cap`; policy
  `established_post.*`, `spatial_isolation.*`.
- **Diagnostic:** none.
- **Proposed verdict:** port with the heuristic (Q4).

#### POST-R086 — Contradiction, consensus, established post
- **What:** for the top candidate c: `winnerShare` = contexts of the family whose winner is c /
  contexts of the family; `contradictory` = some context winner w ≠ c has final ≥ 0.90 and lead
  ≥ 0.02; consensus = contexts ≥ 1 and share ≥ 0.67 and not contradictory; `established` = count
  of R085 for c, `runner` = the largest such count of another hypothesis (0 if none);
  `establishedStrong` = established ≥ 3 and established ≥ 2.0 × max(1, runner) and geometry
  margin ≥ −0.15 and not contradictory and (no alternatives or they agree).
- **Where:** `evaluateReplayPolicy` 325–341; `JdfPostInference` 411–414, 497–505, 516–518.
- **Why:** families can contain contexts whose own best candidate differs (different lines, or
  an authored and an unlabelled call); a confident dissenter vetoes.
- **Pinned by:** `Every mandatory policy subsystem has a production boundary regression`.
- **Constants:** policy `consensus.*`, `established_post.*`.
- **Diagnostic:** none.
- **Proposed verdict:** port with the heuristic (Q4).

#### POST-R087 — Physical eligibility
- **What:** with `margin` = final − runner final (or final), `geometryMargin` and
  `routedAdvantage` likewise: physical when final ≥ 0.98 and (margin ≥ 0.02 or (routed advantage
  ≥ 0.05 and geometry margin ≥ −1e-6)) and not contradictory and (contexts < minimum contexts or
  consensus or established strong) and (no alternatives or they agree). With
  `minimum_contexts = 1` the first disjunct never holds.
- **Where:** `JdfPostInference.physicalResolutionEligible` 416–426; `evaluateReplayPolicy`
  342–348.
- **Why:** the policy id says it: conservative, tuned "best-safe". Reaching 0.98 needs geometry
  ≈ 0.97 (aligned, right side, within ≈ 7 m, no detour) before adjustments.
- **Pinned by:** `Every initial policy-grid field crosses its production behavior boundary`.
- **Constants:** policy `resolution.*`, tolerance `1e-6`.
- **Diagnostic:** none.
- **Proposed verdict:** port with the heuristic (Q4).

#### POST-R088 — Heuristic decision output
- **What:** one decision per (stop, family): `Physical` with hypothesis, score = final, margin,
  medoid coordinates, the first face; else `Centroid` with no scores. Decisions are sorted by
  (stop, mode, line of the family's first row, family). `unsafePhysical` is computed but always
  false for physical decisions and never read.
- **Where:** `evaluateReplayPolicy` 319–363.
- **Why:** unknown.
- **Pinned by:** `Evaluation is deterministic…`.
- **Constants:** none.
- **Diagnostic:** `assignments.parquet` (feature export).
- **Proposed verdict:** port; drop `unsafePhysical`.

#### POST-R089 — Unused heuristic knobs
- **What:** `resolution.minimum_plausible_score` and the function `plausibleCandidate` are only
  validated and tested; `ReplayScore.alignment/side/proximity` are always 0 from the reader and
  recomputed; `alternativeCostGap` is diagnostic only.
- **Where:** `JdfPostInference.plausibleCandidate` 428–430; `JdfPostEvidenceReader` 268.
- **Why:** history.
- **Pinned by:** `Every initial policy-grid field crosses its production behavior boundary`.
- **Constants:** 0.90.
- **Diagnostic:** none.
- **Proposed verdict:** drop; the document keeps accepting the key (format) and ignores it.

<!-- RULES3 -->
