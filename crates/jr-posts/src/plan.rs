//! Plan and publication (POST-R130–R140): the decisions become estimated posts with
//! registry-pinned ordinals, the calls of decided contexts board there, authored posts take
//! their positions, and two reports say what was decided and which ordinals are new.

use std::collections::{BTreeMap, HashMap};

use jr_formats::geodata::registry::Post;
use jr_formats::post_evidence::Pack;
use jr_formats::post_evidence::ids::sha256_hex;
use jr_jdf::assemble::PostFacts;
use jr_model::identity;
use jr_model::timetable::{BoardingPoint, BoardingPointKind, Position, Precision};
use jr_model::{
    BoardingPointId, Coord, Diagnostic, Diagnostics, Edit, EditBuilder, PlaceId, ReportTable, Rule,
    Severity, Timetable,
};

use crate::capture::{FactIndex, trip_pattern};
use crate::evaluate::{Constants, DecisionKind, Decisions};

mod registry;

use registry::Location;

/// Each estimated location (a stop and the hypothesis its decided contexts chose) is one
/// estimated post; Area decisions are published like physical ones (verdict Q6).
///
/// The plan only translates decisions; it decides nothing.
pub const POST_R130: Rule = Rule {
    id: "POST-R130",
    summary: "one estimated post per (stop, chosen hypothesis), Area included",
};

/// Locations that take fresh ordinals are numbered in the order of their F# location key
/// `estimated:sha(stop|hypothesis)`.
///
/// Deterministic numbering, equal to F#'s for equal hypotheses.
pub const POST_R131: Rule = Rule {
    id: "POST-R131",
    summary: "fresh ordinals in location-key order",
};

/// An active registered `est:<k>` post of the stop is reused by the closest location within
/// 25 m; retired ordinals are neither matched nor issued again (verdict Q13).
///
/// A published post ID survives changes of the evidence behind it.
pub const POST_R132: Rule = Rule {
    id: "POST-R132",
    summary: "registry ordinals reused within 25 m, never a retired one",
};

/// Fresh ordinals are listed for review: `stop_id,post_key,lat,lon,reason` with coordinates
/// at six decimals and reason `new`.
///
/// The registry is promoted from this list.
pub const POST_R133: Rule = Rule {
    id: "POST-R133",
    summary: "fresh ordinals go to the post_candidates review table",
};

/// The estimated post `jdf:stop:<N>:est:<k>` is a `Post` boarding point of the stop at the
/// hypothesis medoid, with the stop's precision and coordinate source (verdict Q15, B13 kept),
/// the stop's wheelchair boarding, no public code.
///
/// What riders see as the post of the calls that board there.
pub const POST_R134: Rule = Rule {
    id: "POST-R134",
    summary: "estimated posts at the medoid with the stop's coordinate provenance (B13)",
};

/// An unlabelled call whose context was located boards at its estimated post.
///
/// Evidence replay has made every decision; calls only look theirs up.
pub const POST_R135: Rule = Rule {
    id: "POST-R135",
    summary: "calls of located contexts board at the estimated post",
};

/// Calls of undecided contexts, of stops without contexts and of other modes keep their
/// boarding point (the unspecified point unless authored).
///
/// Nothing is guessed where nothing was decided.
pub const POST_R136: Rule = Rule {
    id: "POST-R136",
    summary: "undecided calls keep their boarding point",
};

/// An authored or textual post positioned by R122 takes the hypothesis medoid, with the
/// stop's precision and coordinate source.
///
/// The carrier's post keeps its identity and gains a position.
pub const POST_R137: Rule = Rule {
    id: "POST-R137",
    summary: "authored posts take their estimated position",
};

/// Only posts calls use are added: every location is chosen by a context of a published trip
/// (verdict Q10), so every added post is used.
///
/// Unused points are not published (JDF-R423).
pub const POST_R138: Rule = Rule {
    id: "POST-R138",
    summary: "every estimated post is used by a published call",
};

/// The summary uses F#'s `compiler.estimated_posts` counter names where they mean the same.
///
/// Oběhy checks the five counters.
pub const POST_R139: Rule = Rule {
    id: "POST-R139",
    summary: "summary counters under the F# names",
};

/// One decisions table, `post_decisions`, per context (verdict Q12).
///
/// Diagnostics for review; nobody read the broken F# columns.
pub const POST_R140: Rule = Rule {
    id: "POST-R140",
    summary: "the post_decisions table",
};

/// The plan's rules.
pub const RULES: &[Rule] = &[
    POST_R130, POST_R131, POST_R132, POST_R133, POST_R134, POST_R135, POST_R136, POST_R137,
    POST_R138, POST_R139, POST_R140,
];

/// What publication added.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PlanCounts {
    pub locations: usize,
    pub area_locations: usize,
    pub registry_reused: usize,
    pub registry_new: usize,
    pub calls_estimated: usize,
    pub authored_positioned: usize,
}

/// What the plan returns.
#[derive(Debug)]
pub(crate) struct Planned {
    pub decisions: ReportTable,
    pub candidates: ReportTable,
    pub counts: PlanCounts,
}

/// Rounds to 15 significant digits, as F#'s `float → decimal` conversion of the medoid does.
fn decimal(value: f64) -> f64 {
    format!("{value:.14e}").parse().unwrap_or(value)
}

/// R130–R140.
#[allow(clippy::too_many_arguments)]
pub(crate) fn plan(
    timetable: &Timetable,
    facts: &PostFacts,
    pack: &Pack,
    decisions: &Decisions,
    registry: &[Post],
    constants: &Constants,
    builder: &mut EditBuilder<'_>,
    diagnostics: &mut Diagnostics,
) -> Planned {
    let index = FactIndex::new(facts);
    let place_of: HashMap<u64, PlaceId> = facts.stops.iter().map(|s| (s.number, s.place)).collect();
    let mut by_stop: BTreeMap<i64, Vec<&Post>> = BTreeMap::new();
    for post in registry {
        by_stop.entry(post.stop_id).or_default().push(post);
    }
    let mut counts = PlanCounts::default();

    // R130: the locations, per stop in stop order.
    let mut locations: BTreeMap<usize, BTreeMap<usize, bool>> = BTreeMap::new();
    for decision in &decisions.contexts {
        if let (true, Some(hypothesis)) = (decision.kind.is_located(), decision.hypothesis) {
            let area = locations
                .entry(decision.stop)
                .or_default()
                .entry(hypothesis)
                .or_insert(true);
            *area &= decision.kind == DecisionKind::Area;
        }
    }
    let mut points: HashMap<(usize, usize), (BoardingPointId, u32)> = HashMap::new();
    let mut candidate_rows = Vec::new();
    for (&stop_index, hypotheses) in &locations {
        let stop = &decisions.stops[stop_index];
        let Some(&place) = place_of.get(&stop.stop) else {
            continue;
        };
        let place_row = timetable.places.get(place);
        let list: Vec<(usize, Location)> = hypotheses
            .keys()
            .map(|&h| {
                let hypothesis = &stop.hypotheses[h];
                let key = format!(
                    "estimated:{}",
                    sha256_hex(format!("{}|{}", stop.stop, hypothesis.id).as_bytes())
                );
                let coord = Coord::new(decimal(hypothesis.longitude), decimal(hypothesis.latitude));
                (h, Location { key, coord })
            })
            .collect();
        let registered = by_stop
            .get(&(stop.stop as i64))
            .map(Vec::as_slice)
            .unwrap_or_default();
        let only: Vec<Location> = list.iter().map(|(_, l)| l.clone()).collect();
        let ordinals = registry::ordinals(&constants.registry, registered, &only);
        // Publication in location-key order (the F# order of inferred locations).
        let mut order: Vec<usize> = (0..list.len()).collect();
        order.sort_by(|&a, &b| list[a].1.key.cmp(&list[b].1.key));
        for i in order {
            let (h, location) = &list[i];
            let (ordinal, fresh) = ordinals[i];
            let hypothesis = &stop.hypotheses[*h];
            let stop_id = identity::jdf::stop(stop.stop);
            // B13 kept (verdict Q15): the stop's precision and coordinate source.
            let position = Position {
                coord: Coord::new(hypothesis.longitude, hypothesis.latitude),
                precision: place_row
                    .and_then(|p| p.position)
                    .map_or(Precision::Estimated, |p| p.precision),
                source: place_row.and_then(|p| p.position).and_then(|p| p.source),
            };
            let point = builder.add_boarding_point(
                POST_R134,
                BoardingPoint {
                    id: identity::jdf::estimated_post(&stop_id, ordinal),
                    place,
                    kind: BoardingPointKind::Post,
                    public_code: None,
                    source_code: None,
                    source_name: None,
                    description: None,
                    position: Some(position),
                    wheelchair_boarding: place_row
                        .map(|p| p.wheelchair_boarding)
                        .unwrap_or_default(),
                },
            );
            points.insert((stop_index, *h), (point, ordinal));
            counts.locations += 1;
            if hypotheses[h] {
                counts.area_locations += 1;
            }
            if fresh {
                counts.registry_new += 1;
                candidate_rows.push(vec![
                    stop.stop.to_string(),
                    format!("est:{ordinal}"),
                    format!("{:.6}", location.coord.lat),
                    format!("{:.6}", location.coord.lon),
                    "new".to_owned(),
                ]);
                diagnostics.push(
                    Diagnostic::new(
                        "post_estimate_new_ordinal",
                        Severity::Info,
                        identity::jdf::estimated_post(&stop_id, ordinal).to_string(),
                        "estimated post not yet in the registry",
                    )
                    .rule(POST_R133),
                );
            } else {
                counts.registry_reused += 1;
            }
        }
    }

    // R135: calls of located unlabelled contexts.
    let located: HashMap<&str, BoardingPointId> = decisions
        .contexts
        .iter()
        .filter_map(|d| {
            let point = points.get(&(d.stop, d.hypothesis?))?;
            Some((&*pack.contexts.context_id[d.context as usize], point.0))
        })
        .collect();
    for trip in &facts.trips {
        let Some(pattern) = trip_pattern(timetable, &index, trip, &constants.capture.modes) else {
            continue;
        };
        for call in &pattern.calls {
            if call.authored.is_some() {
                continue;
            }
            if let Some(&point) = located.get(call.context_id.as_str()) {
                builder.push(
                    POST_R135,
                    Edit::SetCallBoardingPoint {
                        trip: trip.trip,
                        call: call.call as u32,
                        boarding_point: Some(point),
                    },
                );
                counts.calls_estimated += 1;
            }
        }
    }

    // R137: authored posts with a position.
    let authored_points: HashMap<(PlaceId, &str), BoardingPointId> = facts
        .authored_keys
        .iter()
        .filter_map(|(point, key)| {
            let place = timetable.boarding_points.get(*point)?.place;
            Some(((place, &**key), *point))
        })
        .collect();
    for position in &decisions.authored_positions {
        let Some(&place) = place_of.get(&position.stop) else {
            continue;
        };
        let Some(&point) = authored_points.get(&(place, &*position.authored_post_key)) else {
            continue;
        };
        let place_position = timetable.places.get(place).and_then(|p| p.position);
        builder.push(
            POST_R137,
            Edit::SetBoardingPointPosition {
                point,
                position: Some(Position {
                    coord: Coord::new(position.longitude, position.latitude),
                    precision: place_position.map_or(Precision::Estimated, |p| p.precision),
                    source: place_position.and_then(|p| p.source),
                }),
            },
        );
        counts.authored_positioned += 1;
    }

    Planned {
        decisions: decisions_table(pack, decisions, &points),
        candidates: ReportTable {
            name: "post_candidates",
            header: vec!["stop_id", "post_key", "lat", "lon", "reason"],
            rows: candidate_rows,
        },
        counts,
    }
}

fn kind_text(kind: DecisionKind) -> &'static str {
    match kind {
        DecisionKind::Physical => "physical",
        DecisionKind::Area => "area",
        DecisionKind::Abstained => "abstained",
        DecisionKind::Unseen => "unseen",
        DecisionKind::Centroid => "centroid",
        DecisionKind::Authored => "authored",
    }
}

fn number(value: Option<f64>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}

/// R140: one row per context, in pack order.
fn decisions_table(
    pack: &Pack,
    decisions: &Decisions,
    points: &HashMap<(usize, usize), (BoardingPointId, u32)>,
) -> ReportTable {
    let c = &pack.contexts;
    let rows = decisions
        .contexts
        .iter()
        .map(|d| {
            let i = d.context as usize;
            let chosen = decisions.chosen(d);
            let ordinal = d
                .hypothesis
                .and_then(|h| points.get(&(d.stop, h)))
                .map(|(_, ordinal)| format!("est:{ordinal}"));
            vec![
                c.context_id[i].to_string(),
                c.stop[i].to_string(),
                c.mode[i].to_string(),
                c.line_id[i].to_string(),
                c.route_distinction[i].to_string(),
                c.direction[i].to_string(),
                c.pattern_hash[i].to_string(),
                c.pattern_position[i].to_string(),
                c.role[i].as_str().to_owned(),
                c.authored_post_key[i].as_deref().unwrap_or("").to_owned(),
                c.movement_family_id[i].to_string(),
                kind_text(d.kind).to_owned(),
                chosen.map(|h| h.id.clone()).unwrap_or_default(),
                chosen
                    .map(|h| h.observation_ids.join(";"))
                    .unwrap_or_default(),
                number(chosen.map(|h| h.latitude)),
                number(chosen.map(|h| h.longitude)),
                ordinal.unwrap_or_default(),
                number(d.score),
                number(d.margin),
                number(d.learned.map(|l| l.post_probability)),
                number(d.learned.map(|l| l.area_probability)),
            ]
        })
        .collect();
    ReportTable {
        name: "post_decisions",
        header: vec![
            "context_id",
            "stop_id",
            "mode",
            "line_id",
            "route_distinction",
            "direction",
            "pattern_hash",
            "pattern_position",
            "same_stop_block_role",
            "authored_post_key",
            "movement_family_id",
            "kind",
            "hypothesis_id",
            "observation_ids",
            "latitude",
            "longitude",
            "post_key",
            "score",
            "margin",
            "post_probability",
            "area_probability",
        ],
        rows,
    }
}
