# After the switch: roadmap

jrutil-rs replaced F# on 2026-11-23. During the parity port, the rule inventories
(`docs/rules/*.md`) recorded as limitations every change that would alter what Oběhy publishes,
and put them off until after the switch. This file decides them (2026-11-24) and orders the work.
IDs are those of the inventory named in each row (`czptt`, `jdf-read`, `jdf-merge`,
`jdf-convert`, `overlay`, `posts`, `serving`).

Each phase is judged as AGENTS.md rule 10 says: `jrutil compare` against the previous release on
the golden subset, with every difference in a ledger category. A national comparison is run only
where a phase says so.

## Decisions

### Changes of what is published (decided by the user)

| Inventory | Items | Decision |
|---|---|---|
| jdf-convert | B1, B2, B23, Q3 | Done in phase 3 (JDF-R444). JDF 1.11 says a `Zaslinky` code "is the default for all trips of the line", so a route-stop `x` makes every call there a request stop. A trip's `T` (must be ordered) and `!` (runs on condition; the user chose to board it like `T`) give phone-agency boarding to every call, or only to the calls marked with the same code when there are any. Arrangement beats request; `(` and `)` exclude each other with the most specific level (call, route stop, stop) deciding; `$` holds from any level. |
| jdf-read | B4, B5 | One name normalisation for MHD and non-MHD batches, and a passenger-facing name joins only the stated parts, so it never contains `,,` (JDF-R036, R037, R415). Revised in phase 2: the structured fields keep the moved form (a nearby place without a district is published as the district), as the registry holds it. The registry cannot be migrated back: a row such as `Velký Újezd,rozc.Výkleky,,` may be a real district or a moved nearby place, and nothing records which. No consumer reads the two fields apart, and stop identity, the provisional hash and registry matching ignore which column a part is in, so keeping the form moves no stop. |
| jdf-convert | B4, Q11 | Typed `Caskody` rows are published as calendar designations. |
| jdf-convert | B21, Q15 | Done in phase 3 (JDF-R483, R484), as the user chose: trips are split by operator. Per day the first matching `Altdop` row decides; each other operator's days are a `SplitTrip` copy under that operator's variant of the route. No model or contract change was needed. A trip one operator runs on all its days keeps its ID and only changes route. |
| jdf-convert | B3, Q12 | `Mistenky` rows for trip `0` become route notes. |
| jdf-convert | B5, Q4 | The agency's information phone is published, not the office number. |
| jdf-merge | B2, B3 (B4 follows) | Fix the bugs: a location-less alias no longer bypasses the distance gates, and suffix retrieval works both ways. Batch-name order stays the stated rule; there is no order-free redesign. |
| posts | B5, B6, B7, Q3, Q11, Q14 | Fix the road graph and retrain. Link roads are routable. A barrier blocks by type: `lift_gate`, `gate` and `swing_gate` pass for bus/psv, while `bollard`, `block`, `kerb` and the other types block unless the node grants `bus`/`psv`. The bus/psv one-way and turn exemptions are read. The training table is produced from Rust, a reviewed list replaces the pack statistic, and the scorer is retrained. |
| posts | B9, B10, Q6, Q15 | Contract minor: an estimated post has its own coordinate source, and an `Area` decision gets a precision that says it is approximate. The learned scores go to their proper fields. |
| serving | 1 | Done in phase 5 (CZPTT-R121): per route, the train with the most stations is direction 0 and the others follow the order of the stations they share (at least two) with a directed train. Nationally 72,944 of 81,900 trains get a direction; 8,930 share too little with any and keep none. |

### Clear fixes (architect)

| Inventory | Items | Decision |
|---|---|---|
| czptt, jdf-read | B24, B18 | When both anchors exist but are inverted, the estimate falls back to one side. The interpolation helper is shared. |
| czptt | B16 | Among plausible fuzzy candidates, only those with the best similarity are ranked. |
| jdf-read | B8, Q14 | Foreign OSM stops get their country. |
| jdf-read | B9, Q15 | `railway=tram_stop` without `tram=*` is kept. |
| jdf-read | B17, Q13 | The geodata validator rejects placeholder coordinates (jrunify-ext-geodata). |
| overlay | B6, Q10 | A source's place zones apply only to trips that source runs. |
| overlay | B1, Q1 | Trips are assigned one-to-one, within a time tolerance measured on the golden slice. |
| overlay | B2, Q2 | Thresholds are per source, from the profiles. Zones are scoped to their tariff. |
| posts | B8 | Lifecycle classes: `proposed` is not eligible, and `current`/`temporary` count as current. |
| posts | B11, Q5 | An authored post is positioned without depending on unrelated unlabelled calls. |
| czptt | B17, R085 | Done in phase 5 (CZPTT-R119, R120): a membership with a `CZCalendarIPTS` holds on its days only; the days on which some membership does not hold are split off as their own trip (`…:ids:<hash>`) with the zones that hold. |
| czptt | B27, Q9 | Phase 5 measured it: no KADR line or IDS entry changes version inside GVD 2026, so the version stays chosen at the first date (splitting by date would change nothing). CZPTT-R122 reports a path the day a version boundary falls inside it. |

### Staying limitations

These are not changed, and no change is planned:

- czptt B28 (first-wins attributes), B29 (changes at the terminal call), B4 and B9 (moot in
  production);
- czptt B30 (order-dependent OSM selection). R098's re-evaluation passes already revisit a pick
  made before its neighbours were known. An order-free scheme (authority rounds) was tried in
  phase 1 and rejected: it locks in a wrong PLC tag that weaker evidence supported by SR70 should
  overrule. What remains is two picks the timetable's times cannot both allow, where neither is
  wrong;
- jdf-merge B6, B7, B11, B12 (the last is a registry-tooling matter), and Q5 beyond B3;
- jdf-read B14, B22;
- the latent jdf-convert items B12, B17, B18, B19, B28;
- posts B1–B4 (diagnostics only).

## Timeline

One phase at a time; each phase lands as one or two coarse commits.

1. **Small correctness fixes.** Inverted anchors (czptt B24, jdf-read B18); czptt B16; foreign
   OSM country (jdf-read B8, with `eur_countries.json.gz` in the gazetteer); tram stops
   (jdf-read B9); the placeholder validator (jdf-read B17). Overlay B6 moved to phase 4, where
   zones get their tariff scope, and posts B8 and B11 to phase 6, where they can be judged by
   labelled accuracy with the recapture.
2. **Stops.** jdf-merge B2 and B3 and the consistent name structure without `,,` (no registry
   migration: see the B4/B5 row). The check: registry IDs against the last release, with no
   located stop moving more than 75 m.
3. **JDF semantics.** Done (2026-11-25). Request stops from route stops and trips, typed
   `Caskody` rows as notes, `Altdop` by splitting trips, route reservation notes and the
   information phone. No contract change was needed. National road check against phase 2: same
   260,654 trips and 4,733,082 calls; 1.97 M calls change boarding (route-stop `x`), 233,890
   notes added (typed `Caskody`), 1,833 trips move to 290 operator routes (no partial split
   nationally), 232 agencies get their information phone.
4. **Overlay, step two.** Done (2026-11-25): a first-departure tolerance of 420 s (OVL-R133,
   measured: accepted bindings fall from ~700 per minute of deviation up to 7 min to under 80
   at 8 min, beyond is the neighbouring run of a dense line); per date and source each base trip
   goes to one source trip, best rank first, a loser taking its next-best option (OVL-R134,
   B1); profiles carry each source's own F# policy, tiers strongest first (OVL-R136, B2: PID
   loses the merged 25/10 m coordinate-identity tier and the inert stop zones, IDS JMK the inert
   companion tier); a source's place zones go only to the trips it binds or adds (OVL-R135, B6).
   Golden slice: `overlay_fact_conflict` 4,269 → 0, 904 source trips once withheld as
   unrepresented now represented, 1,505 bindings beyond 420 s dropped (their trips added or
   kept national), IDS JMK zones removed from 247,666 national and PID calls (published per call
   now: `call_zone` 86 k → 269 k rows). Rank beats revision within a source; a pending trip does
   not fall back to worse options; claims of two sources on one base trip still go through
   OVL-R082 (B23 fix kept).
5. **Rail.** Done (2026-11-25). National rail check against phase 3: 26 trip parts split off
   by IDS calendar (372 calls, their notes, keys and transfers copied), 72,944 trains get a
   direction and route stops are listed per direction, otherwise identical. The package
   validator now accepts a part published as several trips on different days (a part must
   share its boundary call with some trip of the previous part).
6. **Posts.** Done (2026-11-25) except the retraining. Lifecycle classes (B8, policy), authored
   posts positioned by their own contexts (B11), the road graph (B5 link roads, also in Oběhy's
   routing-base filter `jdf-transit-routing-base-v2`; B6 barriers by kind; B7 bus one-way and
   turn exemptions), and serving 5.4: estimated posts are sourced `post_estimate`, an
   Area-only post is `approximate`, learned decisions report their probabilities instead of a
   margin (B9, B10, B13). Golden slice, labelled accuracy over 1,636,191 calls with a known real
   post (baseline phase 5 with the old routing base, candidate phase 6 with the new one):
   correct 72.43 % → 73.51 % (+17,734 calls), wrong 3.15 % → 3.19 %, no estimate 20.30 % →
   19.19 %; the wrong share of estimated calls stays 3.95 %. These golden figures are not
   comparable as a measure of the estimator (the regional catalogues the labels come from are
   among the candidates; `docs/golden.md`).

   **Retraining: done, `learned-v2` shipped (2026-11-25).** The training table comes from the
   Rust estimator (POST-R141, verdict Q14): `[jdf.posts] training = true` reports the scorer's
   rows exactly as scored and each context's calls; `tools/posts/training_table.py` labels them
   from an overlay package (the scorer README's rules) and adds the shipped model's decisions as
   the baseline; `tools/posts/train_scorer.py` trains and judges.
   - **Data:** a national capture without PID.csv, MPVNet_PID.csv and IDSJMK.csv (the leakage
     guard), labelled from the national package with both overlays: 149,495 labelled contexts
     (PID and IDS JMK).
   - **Split:** stops in five folds by `stable_fold`, as `learned-v1` was made: folds 1-4 train,
     fold 0 is the test, so neither model saw a test stop. Thresholds come from out-of-fold
     scores on folds 1-4, calibrated per stratum to `learned-v1`'s precision on those stops
     (fitted there, so in its favour). Both models are judged on fold 0 the same way: every
     published post counts (area decisions too) and counts are per call.
   - **Result on fold 0:** 306,600 of 323,340 published calls correct (0.948) against
     298,498 of 315,944 (0.945): +8,102 correct, wrong share 5.18 % against 5.52 %. Better in both
     regions, at complex stops (0.944 against 0.935) and stops with bays (0.907 against 0.884).
   - **Golden slice** (catalogues kept, every labelled call overlay-bound in production): correct
     73.51 % → 74.36 %, wrong 3.19 % → 3.50 %. Not comparable, like every golden posts figure:
     there the regional catalogues are candidates and production publishes the overlay's real
     post.
   - A first attempt on the golden slice alone (69,244 contexts) was not better; it was also
     judged against `learned-v1` thresholds calibrated on the test fold. The reviewed list (Q11)
     remains for a later model.
7. *Optional, not scheduled:* M5 (reading the raw OSM snapshot) and M6 (SŽ přípoje XLSX, OSM
   track→platform, foreign rail GTFS).

Why this order: the uncontested fixes go first. Stops (phase 2) come before the JDF semantics
because request-stop flags and names rest on stable stop identity. The overlay reads the road
base, so it follows the JDF semantics. Posts come last because retraining needs the output of the
earlier phases to have settled.
