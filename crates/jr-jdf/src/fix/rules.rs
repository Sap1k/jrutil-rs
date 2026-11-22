//! The rules of stop matching (`docs/rules/jdf-read.md`). The name rules (JDF-R039–R042,
//! R054) are `jr_geo`'s.

use jr_model::Rule;

/// A catalogue row needs a finite latitude and longitude in range; a row without them is
/// dropped with an error (F# failed the whole run; `Evidence::build` has no error path, so the
/// error diagnostic makes the build ineligible instead).
///
/// The reviewed catalogues are hand-edited CSV.
pub const R006: Rule = Rule {
    id: "JDF-R006",
    summary: "a catalogue row without a valid latitude and longitude is an error and dropped",
};

/// A catalogue row's okres and country are its columns as written, blank as absent; its source
/// is `external:{file stem}`, its observation `external:{path}:{line}`; a catalogue the policy
/// lists is a reviewed correction (`Gapfill`).
///
/// Positions must say which catalogue they came from, for review and for the merge's
/// preference of reviewed corrections.
pub const R007: Rule = Rule {
    id: "JDF-R007",
    summary: "catalogue rows keep okres and country as written and carry their source",
};

/// Rows equal in name, coordinates, okres and country are one row, the first in path order.
///
/// Catalogues copy rows from one another.
pub const R008: Rule = Rule {
    id: "JDF-R008",
    summary: "exact duplicate catalogue rows are collapsed, the first kept",
};

/// A node is a stop when it has no `train` key, has `tram` if it has `railway`, and carries one
/// of the stop tags (B9 kept, verdict Q15).
///
/// The extract holds every public-transport node; railway stations are the rail compiler's.
pub const R009: Rule = Rule {
    id: "JDF-R009",
    summary: "OSM stop nodes: stop tags, no train key, railway only with tram",
};

/// A node's name is `name`, else `name:cs`; `official_name` is its full name.
///
/// OSM spells stops in several tags.
pub const R010: Rule = Rule {
    id: "JDF-R010",
    summary: "an OSM stop is named by name, else name:cs; official_name is the full name",
};

/// A node's okres is the okres of the one municipality containing it; its country is the home
/// country exactly when it has an okres (foreign nodes have none: B8, verdict Q14). In several
/// municipalities it has neither, with a warning.
///
/// OSM nodes carry no okres, and the okres is part of a JDF stop's identity.
pub const R011: Rule = Rule {
    id: "JDF-R011",
    summary: "an OSM stop's okres comes from the municipality containing it",
};

/// A node with `official_name` is named by it. A node with only a name inside a municipality is
/// named `{municipality},{name}` unless its name already accounts for every token of the
/// municipality name (R042's quality, synonyms applied, relatedness through a shared word).
///
/// Stops inside towns often omit the town's name on the pole.
pub const R012: Rule = Rule {
    id: "JDF-R012",
    summary: "an OSM stop name without its municipality gets the municipality prepended",
};

/// What a node says about modes (road, tram, affirmed or denied) and lifecycle, and its audit
/// tags, are kept as facts for the post estimator.
///
/// Modality is only what the source supplies; `local_ref` separates numbered bays.
pub const R013: Rule = Rule {
    id: "JDF-R013",
    summary: "OSM stops carry their mode, lifecycle and audit tags as post evidence",
};

/// One area per okres code with every polygon the file gives it (B2 fixed: both `LT`
/// polygons count); adjacency is computed once.
///
/// The okres is part of a JDF stop's identity.
pub const R015: Rule = Rule {
    id: "JDF-R015",
    summary: "okres polygons: one area per code, every polygon of it",
};

/// The municipality and European town names are name indexes with the matching profile; the
/// municipality polygons are brought to WGS84 once.
///
/// Town names decide okres and country where no match does.
pub const R016: Rule = Rule {
    id: "JDF-R016",
    summary: "the municipality and European town gazetteers are name indexes",
};

/// A batch with fewer than two distinct stops called at a scheduled time is dropped.
///
/// Such a batch has no trip a passenger can take.
pub const R033: Rule = Rule {
    id: "JDF-R033",
    summary: "a batch with fewer than two distinct called stops is dropped",
};

/// A batch with other than one route is diagnosed; trips are keyed by route, distinction and
/// trip, so several routes are handled.
///
/// CIS exports have one line per batch.
pub const R034: Rule = Rule {
    id: "JDF-R034",
    summary: "a batch is expected to have one route",
};

/// The trip matrices: odd and even trips, one per kilometre pattern, rows the sorted route stops
/// in travel order. A row whose stop is not in `Zastavky` is left out with a warning (F#
/// aborted).
///
/// Secondary matching and propagation walk the line stop by stop.
pub const R035: Rule = Rule {
    id: "JDF-R035",
    summary: "trip matrices: one representative trip per direction and kilometre pattern",
};

/// A town written `Praha [AB]` is the town `Praha` in okres `AB` of the home country (not on
/// the MHD path, B4).
///
/// Some CIS exports append the okres code to the town name.
pub const R036: Rule = Rule {
    id: "JDF-R036",
    summary: "an okres code in brackets after the town name is the stop's okres",
};

/// A town-only name with two commas is the triple; else an empty district takes the nearby
/// place (B5 kept: the result is published, verdict Q1).
///
/// CIS names are inconsistent about which column holds what.
pub const R037: Rule = Rule {
    id: "JDF-R037",
    summary: "stop names are normalised to the triple, the nearby place moving to the district",
};

/// On the non-MHD path a town-only name with one comma is town and district.
///
/// Outside city lines the part before the comma is the town.
pub const R038: Rule = Rule {
    id: "JDF-R038",
    summary: "a town-only name with one comma is split into town and district",
};

/// With a country, an exact candidate needs the same country, compared through the JDF/ISO table
/// (verdict Q8: `D` is `DE`); a candidate without country is rejected.
///
/// A stop's country is honoured.
pub const R043: Rule = Rule {
    id: "JDF-R043",
    summary: "a stop with a country accepts only candidates of that country",
};

/// A home-country stop with an okres takes the candidates of its okres if there are any, else
/// those R046 lets through; a foreign stop or one without okres is not filtered.
///
/// Mixing strict and border candidates makes a known match look ambiguous.
pub const R044: Rule = Rule {
    id: "JDF-R044",
    summary: "candidates of the stop's okres win; others only by the border tolerance",
};

/// Okres codes compare through the alias table (`OL` is `OC`), also for polygon lookups
/// (verdict Q9; B3 fixed); the published code is the source's.
///
/// CIS exports use `OL` for Olomouc, the boundary data `OC`.
pub const R045: Rule = Rule {
    id: "JDF-R045",
    summary: "okres codes compare through the alias table (OL is OC)",
};

/// A candidate of another okres passes when the okresy are adjacent and it lies near the
/// expected okres's boundary (great-circle distances, verdict Q5).
///
/// Catalogues and timetables disagree about the okres of stops on an okres border.
pub const R046: Rule = Rule {
    id: "JDF-R046",
    summary: "a candidate of an adjacent okres passes within the border tolerance",
};

/// Catalogue candidates are tried first; OSM candidates only when they give no group.
///
/// Reviewed catalogues are more reliable, but an ambiguous catalogue set must not hide a usable
/// OSM group.
pub const R047: Rule = Rule {
    id: "JDF-R047",
    summary: "catalogue candidates before OSM candidates, never mixed",
};

/// A group is usable when it shares one okres and country and lies within the radius of its
/// centroid; otherwise the stop is not matched (diagnosed as ambiguous).
///
/// Candidates far apart are different places with one name.
pub const R048: Rule = Rule {
    id: "JDF-R048",
    summary: "a candidate group needs one okres and country and a small radius",
};

/// A match is the group's centroid (mean in the local tangent plane, verdict Q5); its name,
/// okres and country are the first candidate's in corpus order (R014 verdict); its source the
/// sorted distinct sources joined by `+`.
///
/// Several catalogues and posts describe one stop place.
pub const R049: Rule = Rule {
    id: "JDF-R049",
    summary: "a match is the centroid of its group; the first candidate names it",
};

/// A town-only stop loses its match when the first comma part of the matched name is a European
/// town name (the first exact entry in file order).
///
/// Czech towns share names with foreign ones (verdict Q16: ported, a limitation).
pub const R050: Rule = Rule {
    id: "JDF-R050",
    summary: "a town-only stop matched to a European town name is not matched",
};

/// A town-only stop named exactly like a municipality (after the town table) loses its primary
/// match; only secondary matching can position it.
///
/// A bare town name says nothing about which stop in the town is meant (verdict Q17).
pub const R051: Rule = Rule {
    id: "JDF-R051",
    summary: "a town-only stop named like a municipality has no primary match",
};

/// Unmatched stops are matched among all their candidates near the last matched position of
/// each representative trip, within the kilometre difference plus the tolerance, when every
/// trip that suggests something suggests the same.
///
/// The kilometres of a line disambiguate a common name.
pub const R052: Rule = Rule {
    id: "JDF-R052",
    summary: "secondary matching by kilometre distance along the trips",
};

/// An unmatched stop takes okres and country from its town name: one municipality okres, or a
/// European town's country written as its JDF code (verdict Q8); the first exact European name
/// in file order is used.
///
/// Town names locate stops that no catalogue knows (verdict Q3, each inferred value diagnosed).
pub const R053: Rule = Rule {
    id: "JDF-R053",
    summary: "okres and country of an unmatched stop from its town name",
};

/// A stop without okres takes its match's okres and country.
///
/// The match knows where the stop is.
pub const R055: Rule = Rule {
    id: "JDF-R055",
    summary: "a stop without okres takes the okres and country of its match",
};

/// Candidate MHD towns: per town-only stop the towns `t` whose `t,{name}` is exactly a candidate
/// (sequence predicate) while the bare name is not; towns supported by enough stops, best first.
/// Ties keep the order of first support, which follows corpus order.
///
/// City-line batches put the stop name alone in the town column.
pub const R056: Rule = Rule {
    id: "JDF-R056",
    summary: "MHD town candidates: towns that complete enough town-only names",
};

/// Town groups count as their first town and are all tried in order.
///
/// Some city lines span several municipalities.
pub const R057: Rule = Rule {
    id: "JDF-R057",
    summary: "MHD town groups count as their first town",
};

/// A candidate town named by the route name gets a bonus.
///
/// The route name usually names the city.
pub const R058: Rule = Rule {
    id: "JDF-R058",
    summary: "the town in the route name boosts an MHD town candidate",
};

/// MHD renaming of a raw town-only stop: `{town},{first part},{rest}` for the first town that
/// gives a match, else the parts as the triple when the first part is a municipality, else the
/// first town.
///
/// The first comma part may be a town after all.
pub const R059: Rule = Rule {
    id: "JDF-R059",
    summary: "MHD-named stops are renamed with the town",
};

/// The MHD names are used when they match enough more stops than the plain ones.
///
/// Renaming is only worth it when it explains the batch better (verdict Q2, diagnosed).
pub const R060: Rule = Rule {
    id: "JDF-R060",
    summary: "MHD naming is chosen when it matches enough more stops",
};

/// Okres and country are carried along each trip to stops that lack them.
///
/// Consecutive stops of a line are usually in the same okres.
pub const R061: Rule = Rule {
    id: "JDF-R061",
    summary: "okres and country propagate along the trips",
};

/// A propagated okres is reverted unless a trip shows the stop is closer, in kilometres, than
/// the last matched stop's distance to its okres's edge.
///
/// A line may cross an okres border between two stops.
pub const R062: Rule = Rule {
    id: "JDF-R062",
    summary: "a propagated okres is kept only when the kilometres rule out a border",
};

/// A home-country stop still without okres takes the one okres of the municipalities named like
/// its town that lie within reach of a trip's last matched position.
///
/// Town polygons and kilometres together locate a stop's okres.
pub const R063: Rule = Rule {
    id: "JDF-R063",
    summary: "okres from municipalities named like the town within kilometre reach",
};

/// R061–R063 run over matrix 1, reversed, matrix 2, reversed, while geography is missing.
///
/// Propagation forward and backward reaches both ends of a line.
pub const R064: Rule = Rule {
    id: "JDF-R064",
    summary: "propagation runs in four passes while geography is missing",
};

/// A stop on several rows takes the row result with the most geography (the first among
/// equals); stops keep their source order.
///
/// A stop served in both directions is seen twice.
pub const R065: Rule = Rule {
    id: "JDF-R065",
    summary: "one geography per stop, the most complete row result",
};

/// A stop's post candidates are its accepted exact candidates within the radius of the selected
/// group's centroid whose country fits the group.
///
/// The post estimator decides physical clustering from the source facts.
pub const R066: Rule = Rule {
    id: "JDF-R066",
    summary: "post candidates: accepted exact candidates near the selected group",
};

/// Post candidates are distinct by observation and sorted by it.
///
/// One observation is one candidate.
pub const R067: Rule = Rule {
    id: "JDF-R067",
    summary: "post candidates are distinct and sorted by observation",
};

/// Trips in travel order (route stop order, reversed for even trips) with times past midnight
/// moved to the next day.
///
/// JDF times carry no day offset.
pub const R068: Rule = Rule {
    id: "JDF-R068",
    summary: "trips in travel order with the day rollover",
};

/// Consecutive matched timed calls farther apart than slack plus elapsed time at the speed
/// conflict; the others support each other.
///
/// A wrong match is often kilometres away from its neighbours.
pub const R069: Rule = Rule {
    id: "JDF-R069",
    summary: "consecutive matches must be reachable at the plausible speed",
};

/// Conflicts are resolved by group size, conflict count and catalogue source; ties reject both;
/// repeated until no conflict remains. Each rejected stop is diagnosed.
///
/// A long consistent run outvotes a short run of wrong matches.
pub const R070: Rule = Rule {
    id: "JDF-R070",
    summary: "implausible matches are rejected by consistent groups",
};

/// Post candidates of stops without a surviving match are dropped.
///
/// They were chosen around a rejected position.
pub const R071: Rule = Rule {
    id: "JDF-R071",
    summary: "post candidates follow the surviving match",
};

/// Consecutive matches farther apart than (|Δkm| + tolerance) × factor are reported, on every
/// trip, once per pair. Nothing changes.
///
/// Some timetables have unrealistic kilometres.
pub const R072: Rule = Rule {
    id: "JDF-R072",
    summary: "matches far apart for their kilometres are reported",
};

/// Stops without country, or in the home country without okres, are reported (a warning when
/// the stop is called).
///
/// Okres and country are part of the stop's identity.
pub const R073: Rule = Rule {
    id: "JDF-R073",
    summary: "stops without country or okres are reported",
};

/// The okres and country filter evaluations of the chosen stop set's primary selections are
/// reported once per batch.
///
/// They show how often the border tolerance decides.
pub const R074: Rule = Rule {
    id: "JDF-R074",
    summary: "geography filter counts are reported per batch",
};

/// A stop with a surviving match is at the match, a precise position from its source class.
///
/// The match is where the catalogues put the stop.
pub const R075: Rule = Rule {
    id: "JDF-R075",
    summary: "a matched stop is positioned at its match",
};

/// A timed call's stop without a precise position is interpolated by scheduled time between the
/// nearest timed calls with one; inverted anchors give nothing (B18 kept).
///
/// Stops between known stops lie between them.
pub const R076: Rule = Rule {
    id: "JDF-R076",
    summary: "unmatched stops are interpolated by scheduled time",
};

/// With an anchor on one side only, the stop is placed north of it by the offset per call
/// (CZPTT-R100 formulation).
///
/// A visible placeholder rather than no position.
pub const R077: Rule = Rule {
    id: "JDF-R077",
    summary: "a stop past the last anchor is placed north of it",
};

/// A stop's estimate is the centroid of its route-time estimates, else of its route-end ones;
/// estimated and unlocated stops are diagnosed.
///
/// A stop on several trips gets one position.
pub const R078: Rule = Rule {
    id: "JDF-R078",
    summary: "estimates are aggregated per stop; unlocated stops are reported",
};
