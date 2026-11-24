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
| jdf-convert | B1, B2, B23, Q3 | Confirm the JDF meaning of a route-stop `x` and of a trip's `T`/`!` against the specification first. Then they make the calls request stops. Precedence is most specific first: call, then route stop, then stop. |
| jdf-read | B4, B5 | One name normalisation for MHD and non-MHD batches. The structured fields (`district`, `nearby_place`) keep the source's meaning. A passenger-facing name joins only the non-empty parts, so it never contains `,,`. The registry rows are migrated in the same change (jrunify-ext-geodata). |
| jdf-convert | B4, Q11 | Typed `Caskody` rows are published as calendar designations. |
| jdf-convert | B21, Q15 | The alternate operator (`Altdop`) is published on the dates it runs the trip. This needs a model field or a new edit kind, so it is raised as an architecture decision in that phase. |
| jdf-convert | B3, Q12 | `Mistenky` rows for trip `0` become route notes. |
| jdf-convert | B5, Q4 | The agency's information phone is published, not the office number. |
| jdf-merge | B2, B3 (B4 follows) | Fix the bugs: a location-less alias no longer bypasses the distance gates, and suffix retrieval works both ways. Batch-name order stays the stated rule; there is no order-free redesign. |
| posts | B5, B6, B7, Q3, Q11, Q14 | Fix the road graph and retrain. Link roads are routable. A barrier blocks by type: `lift_gate`, `gate` and `swing_gate` pass for bus/psv, while `bollard`, `block`, `kerb` and the other types block unless the node grants `bus`/`psv`. The bus/psv one-way and turn exemptions are read. The training table is produced from Rust, a reviewed list replaces the pack statistic, and the scorer is retrained. |
| posts | B9, B10, Q6, Q15 | Contract minor: an estimated post has its own coordinate source, and an `Area` decision gets a precision that says it is approximate. The learned scores go to their proper fields. |
| serving | 1 | A CZPTT train gets a direction derived from the order of its line's stations, so a line's route stops are split by direction. A new rule and a ledger category. |

### Clear fixes (architect)

| Inventory | Items | Decision |
|---|---|---|
| czptt, jdf-read | B24, B18 | When both anchors exist but are inverted, the estimate falls back to one side. The interpolation helper is shared. |
| czptt | B16, B30 | The fuzzy score ranks the plausible candidates, and OSM selection no longer depends on resolution order. |
| jdf-read | B8, Q14 | Foreign OSM stops get their country. |
| jdf-read | B9, Q15 | `railway=tram_stop` without `tram=*` is kept. |
| jdf-read | B17, Q13 | The geodata validator rejects placeholder coordinates (jrunify-ext-geodata). |
| overlay | B6, Q10 | A source's place zones apply only to trips that source runs. |
| overlay | B1, Q1 | Trips are assigned one-to-one, within a time tolerance measured on the golden slice. |
| overlay | B2, Q2 | Thresholds are per source, from the profiles. Zones are scoped to their tariff. |
| posts | B8 | Lifecycle classes: `proposed` is not eligible, and `current`/`temporary` count as current. |
| posts | B11, Q5 | An authored post is positioned without depending on unrelated unlabelled calls. |
| czptt | B17, R085 | CZIPTS zones respect their calendar. |
| czptt | B27, Q9 | The catalogue version is chosen per date. |

### Staying limitations

These are not changed, and no change is planned:

- czptt B28 (first-wins attributes), B29 (changes at the terminal call), B4 and B9 (moot in
  production);
- jdf-merge B6, B7, B11, B12 (the last is a registry-tooling matter), and Q5 beyond B3;
- jdf-read B14, B22;
- the latent jdf-convert items B12, B17, B18, B19, B28;
- posts B1–B4 (diagnostics only).

## Timeline

One phase at a time; each phase lands as one or two coarse commits.

1. **Small correctness fixes.** Inverted anchors; czptt B16 and B30; foreign OSM country; tram
   stops; the placeholder validator; overlay B6; posts B8 and B11.
2. **Stops.** jdf-merge B2 and B3, the consistent name structure without `,,`, and the registry
   migration, all together because each of them moves stop identities. The check: registry IDs
   against the last release, with no located stop moving more than 75 m.
3. **JDF semantics.** The specification check, then request stops from route stops and trips,
   `Caskody` designations, `Altdop` per date, route reservation notes and the information phone.
   A contract minor where needed, landing with Oběhy's matching `static_build.py` and API
   change. One national comparison with the user, because the request-stop change touches about
   a million calls.
4. **Overlay, step two.** One-to-one assignment, per-source thresholds, zones scoped to their
   tariff. Judged by the drop in `overlay_fact_conflict` events on the golden slice.
5. **Rail.** CZIPTS calendars, the catalogue version per date, the direction of each train.
6. **Posts.** The graph fixes, a recapture, the training table from Rust, the reviewed list, the
   retrained scorer, and the contract minor for estimated and area posts. The bar is labelled
   accuracy no worse than now. One national comparison with the user.
7. *Optional, not scheduled:* M5 (reading the raw OSM snapshot) and M6 (SŽ přípoje XLSX, OSM
   track→platform, foreign rail GTFS).

Why this order: the uncontested fixes go first. Stops (phase 2) come before the JDF semantics
because request-stop flags and names rest on stable stop identity. The overlay reads the road
base, so it follows the JDF semantics. Posts come last because retraining needs the output of the
earlier phases to have settled.
