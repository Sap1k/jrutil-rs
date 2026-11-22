//! Rail coordinates (`docs/rules/czptt.md`, CZPTT-R018–R025, R089–R101, R104).
//!
//! Gives railway places (stations and operational points: places with a `country` and a
//! `source_code`) their positions. In order of authority: the SR70 catalogue for Czech points;
//! an OpenStreetMap node found by PLC, exact name, railway name core or fuzzy name, vetoed when
//! the timetable could not get there in time; failing both, for passenger places only, an
//! estimate along the run between real positions.
//!
//! A *path* is a run: the published trips sharing `Trip::run.key`, by part, the boundary call
//! of consecutive parts counted once. A trip without a run is a path of its own. Occurrences
//! of a point, their neighbours and the anchors of an estimate are taken along paths.
//!
//! OSM selection is sequential by design (R098): earlier picks constrain later ones. The fuzzy
//! scan over the candidates of one point runs in parallel; its result does not depend on it.

use std::collections::BTreeMap;

use jr_geo::names::{self, NameTables};
use jr_model::timetable::{CoordSource, Position};
use jr_model::{Diagnostic, Diagnostics, Edit, EditBuilder, Edits, Rule, Severity, Timetable};
use serde::Deserialize;

mod estimate;
mod evidence;
mod network;
mod select;

pub use evidence::Evidence;

use estimate::EstimateMethod;
use network::Network;
use select::{Resolved, Resolver};

/// The pass name recorded in the timetable's history.
pub const PASS: &str = "rail_coordinates";

/// A row's code is its first `code_length` characters; shorter codes drop the row. A name is
/// trimmed and is no name when blank or the placeholder. A coordinate needs both numbers within
/// WGS84 bounds. Only columns 3 and 4 are coordinates (verdict Q5: four columns, no header).
///
/// SR70 codes carry a check digit the timetable's primary codes lack (`533398` is `53339`).
pub const CZPTT_R018: Rule = Rule {
    id: "CZPTT-R018",
    summary: "SR70 code is its first 5 characters; blank or placeholder names and bad coordinates are absent",
};

/// A code with any row lacking a usable coordinate gets no SR70 coordinate, even if other rows
/// are good (verdict Q5).
///
/// A broken row means the catalogue entry cannot be trusted; OSM fills such points.
pub const CZPTT_R019: Rule = Rule {
    id: "CZPTT-R019",
    summary: "an SR70 code with any unusable coordinate row has no SR70 coordinate",
};

/// A code whose rows carry more than one distinct coordinate (exact equality) gets none.
///
/// SR70 sometimes lists one shortened code at two places; neither can be chosen.
pub const CZPTT_R020: Rule = Rule {
    id: "CZPTT-R020",
    summary: "an SR70 code with conflicting coordinates has no SR70 coordinate",
};

/// A code's name is its one distinct name; several names are ambiguous and give none. Names
/// apply to points of the SR70 country only, whatever their coordinates.
///
/// The compiler names Czech points by SR70 (CZPTT-R053); a code with two names cannot.
pub const CZPTT_R021: Rule = Rule {
    id: "CZPTT-R021",
    summary: "an SR70 code names its point when all its rows agree on one name",
};

/// A node is a candidate when one of the policy's tags has one of the listed values.
///
/// Railway stations and halts are tagged on nodes; the public-transport branch is kept by
/// verdict Q6.
pub const CZPTT_R022: Rule = Rule {
    id: "CZPTT-R022",
    summary: "OSM candidates are nodes tagged railway=station|halt|stop or public_transport=station|stop_position",
};

/// `ref:EU:PLC` with whitespace removed and upper-cased, kept only as two letters and five
/// digits.
///
/// The PLC is country plus primary code, the timetable's own identity of a point.
pub const CZPTT_R023: Rule = Rule {
    id: "CZPTT-R023",
    summary: "OSM PLC is ref:EU:PLC compacted and upper-cased, kept only as CC00000",
};

/// A candidate's country is its PLC's letters, else the first country tag, mapped from alpha-3.
/// It only ranks ambiguous candidates (R096).
///
/// Country tags are sparse and inconsistent in OSM; they are hints, not gates.
pub const CZPTT_R024: Rule = Rule {
    id: "CZPTT-R024",
    summary: "OSM candidate country is the PLC prefix, else addr:country or is_in:country_code",
};

/// A candidate's names are the policy's name tags in order, trimmed, blanks and repeats dropped
/// (`alt_name` is not split). Candidates are ordered by numeric node ID (verdict for R025).
///
/// Foreign names come in several languages and spellings; every tagged one may match.
pub const CZPTT_R025: Rule = Rule {
    id: "CZPTT-R025",
    summary: "OSM candidate names are the listed name tags; candidates ordered by node ID",
};

/// A point's name evidence is its place's name (one per place: the SR70 name for Czech points,
/// the source name otherwise).
///
/// The source gives nothing else to find a foreign point by.
pub const CZPTT_R089: Rule = Rule {
    id: "CZPTT-R089",
    summary: "a railway point is matched by name using its place name",
};

/// OSM candidates are tried in groups, the first group with a plausible candidate winning: PLC
/// equal to country and code, exact normalised name, equal railway cores, then fuzzy (best
/// qualifier-compatible similarity at least the policy minimum; the best by score, then node
/// ID, up to the policy cap). The fuzzy score only makes that cut.
///
/// The authority order of NATIONAL_CZPTT.md: identity before name, exact before approximate.
pub const CZPTT_R094: Rule = Rule {
    id: "CZPTT-R094",
    summary: "OSM candidates by PLC, then exact name, railway core, fuzzy name; first plausible group wins",
};

/// A candidate is vetoed when every occurrence of the point with timed neighbours that already
/// have a position would need more than slack plus speed × time. An edge counts when the later
/// time is strictly greater; an occurrence without such edges is no evidence; a candidate
/// without any evidence is plausible.
///
/// Common names repeat across Europe; a same-named station hundreds of kilometres away is not
/// the point. One anomalous timing (a source time slip) must not veto a good match.
pub const CZPTT_R095: Rule = Rule {
    id: "CZPTT-R095",
    summary: "OSM candidate vetoed when no timed occurrence fits 2 km + 150 km/h to placed neighbours",
};

/// Several plausible candidates in a group: the best by most placed neighbours, then country
/// hint (same, unknown, different), then smallest total distance to them, then node ID is
/// accepted and diagnosed (verdict Q11).
///
/// Two OSM nodes often tag one station (the building and a stop position); quarantining would
/// lose the point for nothing.
pub const CZPTT_R096: Rule = Rule {
    id: "CZPTT-R096",
    summary: "ambiguous OSM candidates are ranked by corridor evidence, country, distance, node ID",
};

/// A point of the SR70 country with an SR70 coordinate takes it and is never re-evaluated,
/// passenger stop or not; OSM nodes with its PLC farther than the policy distance are reported.
/// Points of other countries never use SR70, even with the same code.
///
/// SR70 is the infrastructure manager's catalogue; OSM disagreement is information, not doubt.
pub const CZPTT_R097: Rule = Rule {
    id: "CZPTT-R097",
    summary: "SR70 is authoritative for points of its country; disagreeing OSM PLC nodes are reported",
};

/// Points are resolved in order of first appearance over paths sorted by run key (a trip
/// without a run by its public ID), then call order, each against the positions chosen so far;
/// then, the policy's number of times, every non-SR70 point is re-resolved against all other
/// current picks. Diagnostics report the final evaluation only (B13, fixed).
///
/// Neighbour picks constrain a point; a pick made before its neighbours were known is revisited.
pub const CZPTT_R098: Rule = Rule {
    id: "CZPTT-R098",
    summary: "points resolve in first-appearance order, then re-resolve twice against all other picks",
};

/// Only places with a passenger call in a published trip and no real position are estimated;
/// a timing-only point without a real position stays without one (diagnosed).
///
/// A point nobody boards at is not shown; a guessed position would only mislead.
pub const CZPTT_R099: Rule = Rule {
    id: "CZPTT-R099",
    summary: "only passenger places are estimated; unresolved timing-only points stay unpositioned",
};

/// For each timed occurrence, anchors are the nearest earlier and later calls of the path with
/// a real position and a time, once requiring passenger calls and once not. Two anchors with a
/// later right time interpolate by time (`route_time`); one anchor places the point the policy
/// offset north per call (`route_end_north`, verdict Q12); two anchors without a later right
/// time give nothing (B24 stands). Tiers: passenger two-sided, any two-sided, passenger
/// one-sided, any one-sided. The best by tier, call span, anchor distance, most service days,
/// run key and call sequence wins.
///
/// A passenger stop must be shown somewhere; between its neighbours by time is the best guess.
pub const CZPTT_R100: Rule = Rule {
    id: "CZPTT-R100",
    summary: "passenger places without a position are estimated by time between real anchors, else past the end",
};

/// Estimates are computed from real positions only and never anchor each other.
///
/// Chained guesses compound their error.
pub const CZPTT_R101: Rule = Rule {
    id: "CZPTT-R101",
    summary: "estimates never anchor estimates",
};

/// A passenger place left without any position gets no edit (published as missing, never
/// 0,0) and a diagnostic; the caller makes the package ineligible (verdict Q18).
///
/// Riders would be sent to the Gulf of Guinea.
pub const CZPTT_R104: Rule = Rule {
    id: "CZPTT-R104",
    summary: "an unresolved passenger place is diagnosed and makes the package ineligible",
};

pub const RULES: &[Rule] = &[
    CZPTT_R018,
    CZPTT_R019,
    CZPTT_R020,
    CZPTT_R021,
    CZPTT_R022,
    CZPTT_R023,
    CZPTT_R024,
    CZPTT_R025,
    CZPTT_R089,
    names::CZPTT_R090,
    names::CZPTT_R091,
    names::CZPTT_R092,
    names::CZPTT_R093,
    CZPTT_R094,
    CZPTT_R095,
    CZPTT_R096,
    CZPTT_R097,
    CZPTT_R098,
    CZPTT_R099,
    CZPTT_R100,
    CZPTT_R101,
    CZPTT_R104,
];

const BUILTIN: &str = include_str!("../../../policy/rail_coordinates.toml");

/// The pass's constants, read from `policy/rail_coordinates.toml`; the file names the rule of
/// every value.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub sr70: Sr70Policy,
    pub osm: OsmPolicy,
    pub names: NameTables,
    pub matching: MatchingPolicy,
    pub authority: AuthorityPolicy,
    pub resolution: ResolutionPolicy,
    pub estimate: EstimatePolicy,
}

impl Policy {
    /// The policy shipped with the pass.
    pub fn builtin() -> Policy {
        Policy::parse(BUILTIN).expect("policy/rail_coordinates.toml is valid")
    }

    pub fn parse(text: &str) -> Result<Policy, toml::de::Error> {
        toml::from_str(text)
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sr70Policy {
    pub country: Box<str>,
    pub code_length: usize,
    pub name_placeholder: Box<str>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OsmPolicy {
    pub eligible: Vec<TagValues>,
    pub plc_tag: Box<str>,
    pub plc_country_letters: usize,
    pub plc_code_digits: usize,
    pub country_tags: Vec<Box<str>>,
    pub name_tags: Vec<Box<str>>,
    pub country_alpha3: BTreeMap<Box<str>, Box<str>>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TagValues {
    pub key: Box<str>,
    pub values: Vec<Box<str>>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchingPolicy {
    pub fuzzy_min_similarity: f64,
    pub fuzzy_max_candidates: usize,
    pub corridor_slack_m: f64,
    pub corridor_speed_kmh: f64,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityPolicy {
    pub sr70_osm_disagreement_m: f64,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionPolicy {
    pub reevaluation_passes: usize,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EstimatePolicy {
    pub route_end_offset_m_per_call: f64,
    pub metres_per_degree_latitude: f64,
}

/// The counts published in diagnostics.json under `rail_coordinates`. Methods are the names of
/// the F# `coordinate_match_method`.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Summary {
    /// Railway places on published paths.
    pub places: usize,
    pub sr70: usize,
    pub osm_by_method: BTreeMap<String, usize>,
    pub estimated_by_method: BTreeMap<String, usize>,
    /// Passenger places left without a position: the package is not eligible (R104).
    pub unresolved_passenger_places: usize,
    pub unresolved_other_places: usize,
}

/// Positions every railway place on a published path that the evidence can place. Pure:
/// returns one [`Edit::SetPlacePosition`] per resolved place, in resolution order.
pub fn rail_coordinates(
    timetable: &Timetable,
    evidence: &Evidence,
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> (Edits, Summary) {
    let network = Network::build(timetable);
    let resolver = Resolver::new(&network, evidence, policy);
    let resolution = resolver.resolve();
    let real: Vec<_> = resolution
        .resolved
        .iter()
        .map(|resolved| resolved.as_ref().map(Resolved::coord))
        .collect();
    let estimates = estimate::estimates(&network, &real, policy);

    let mut summary = Summary {
        places: network.points.len(),
        ..Summary::default()
    };
    let mut edits = EditBuilder::new(timetable, PASS);
    for (index, point) in network.points.iter().enumerate() {
        let subject = point.id;
        if let Some(attempt) = &resolution.attempts[index] {
            for (method, candidate) in &attempt.rejected {
                diagnostics.push(
                    Diagnostic::new(
                        "czptt_osm_corridor_rejected",
                        Severity::Info,
                        subject,
                        format!(
                            "{}: {} cannot be reached in time from the placed neighbours",
                            method.name(),
                            evidence.object_id(*candidate)
                        ),
                    )
                    .rule(CZPTT_R095),
                );
            }
        }
        match &resolution.resolved[index] {
            Some(Resolved::Sr70(coord)) => {
                summary.sr70 += 1;
                for (candidate, distance) in
                    evidence.plc_disagreements(point.country, point.code, *coord, policy)
                {
                    diagnostics.push(
                        Diagnostic::new(
                            "czptt_osm_sr70_disagreement",
                            Severity::Info,
                            subject,
                            format!(
                                "SR70 retained; {} with this PLC is {distance:.0} m away",
                                evidence.object_id(candidate)
                            ),
                        )
                        .rule(CZPTT_R097),
                    );
                }
                edits.push(
                    CZPTT_R097,
                    Edit::SetPlacePosition {
                        place: point.place,
                        position: Position::exact(*coord, CoordSource::Sr70),
                    },
                );
            }
            Some(Resolved::Osm {
                coord,
                method,
                candidate,
                ranked,
            }) => {
                *summary
                    .osm_by_method
                    .entry(method.name().to_owned())
                    .or_default() += 1;
                let rule = match ranked {
                    Some(ranked) => {
                        let ids: Vec<String> = ranked
                            .iter()
                            .map(|&other| evidence.object_id(other))
                            .collect();
                        diagnostics.push(
                            Diagnostic::new(
                                "czptt_osm_ambiguous_ranked",
                                Severity::Warning,
                                subject,
                                format!(
                                    "{}: {} plausible candidates ({}), ranked first: {}",
                                    method.name(),
                                    ranked.len(),
                                    ids.join(","),
                                    evidence.object_id(*candidate)
                                ),
                            )
                            .rule(CZPTT_R096),
                        );
                        CZPTT_R096
                    }
                    None => CZPTT_R094,
                };
                edits.push(
                    rule,
                    Edit::SetPlacePosition {
                        place: point.place,
                        position: Position::exact(*coord, CoordSource::Osm),
                    },
                );
            }
            None => match &estimates[index] {
                Some(estimate) => {
                    *summary
                        .estimated_by_method
                        .entry(estimate.method.name().to_owned())
                        .or_default() += 1;
                    diagnostics.push(
                        Diagnostic::new(
                            "czptt_estimated_position",
                            Severity::Info,
                            subject,
                            format!("no SR70 or OSM position; {}", estimate.method.name()),
                        )
                        .rule(CZPTT_R100),
                    );
                    let source = match estimate.method {
                        EstimateMethod::RouteTime => CoordSource::RouteTime,
                        EstimateMethod::RouteEnd => CoordSource::RouteEnd,
                    };
                    edits.push(
                        CZPTT_R100,
                        Edit::SetPlacePosition {
                            place: point.place,
                            position: Position::estimated(estimate.coord, source),
                        },
                    );
                }
                None if point.passenger => {
                    summary.unresolved_passenger_places += 1;
                    diagnostics.push(
                        Diagnostic::new(
                            "czptt_unresolved_passenger_place",
                            Severity::Warning,
                            subject,
                            "passenger place without SR70, OSM or estimated position",
                        )
                        .rule(CZPTT_R104),
                    );
                }
                None => {
                    summary.unresolved_other_places += 1;
                    diagnostics.push(
                        Diagnostic::new(
                            "czptt_unresolved_place",
                            Severity::Info,
                            subject,
                            "timing-only point without SR70 or OSM position; not estimated",
                        )
                        .rule(CZPTT_R099),
                    );
                }
            },
        }
    }
    (edits.finish(), summary)
}

#[cfg(test)]
mod tests;
