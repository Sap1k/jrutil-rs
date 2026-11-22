//! The review reports of projection (OVL-R119, R121) and the diagnostics that stand once every
//! decision is final (R120).

use std::collections::{HashMap, HashSet};

use jr_model::{DateSet, Diagnostic, Rule};

use super::additions::Addition;
use super::batch::{Built, Context};
use super::headsign::Headsigns;
use super::keys::{self, Mapping};
use super::stops::Stops;
use crate::authority::{Authority, AuthorityLine, trip_subject};
use crate::matching::Matches;
use crate::matching::report::mode_name;
use crate::prepare::Prepared;
use crate::report::ReportTable;

/// The review reports are tables the command line writes (verdict Q9): F#'s
/// `source_trip_additions`, `headsigns`, `post_pruning` and `unmatched_trip_reasons` in their
/// column sets, and `source_trips` in place of the mapping files.
pub const OVL_R119: Rule = Rule {
    id: "OVL-R119",
    summary: "review reports as tables; source_trips replaces the mapping files",
};

/// A diagnostic stands only when its decision is final: an unresolved or ambiguous stop group
/// placed later, a validity failure of a trip bound in the end, an ambiguous date the
/// availability pass resolved, and the route, signature, validity, ambiguity and gap reasons of
/// a trip accepted on every day it runs are withdrawn.
pub const OVL_R120: Rule = Rule {
    id: "OVL-R120",
    summary: "diagnostics stand only for final decisions",
};

/// Every unmatched reason of a source trip, with what became of the trip (added on a source
/// line, projected, bound on other dates, withheld), and every active trip accepted on no day
/// without a reason (`trip_projection_unrepresented` when evidence existed, else
/// `trip_unclassified_unmatched`).
pub const OVL_R121: Rule = Rule {
    id: "OVL-R121",
    summary: "unmatched trip reasons with their disposition",
};

pub const RULES: &[Rule] = &[OVL_R119, OVL_R120, OVL_R121];

fn date(prepared: &Prepared<'_>, day: usize) -> String {
    prepared
        .base
        .timetable
        .horizon
        .date_at(day)
        .strftime("%Y%m%d")
        .to_string()
}

/// Per source, per trip: the days it is accepted (bound on a surviving part, or added).
pub fn accepted(context: &Context<'_, '_>) -> Vec<Vec<DateSet>> {
    let prepared = context.prepared;
    let mut accepted: Vec<Vec<DateSet>> = prepared
        .sources
        .iter()
        .map(|s| vec![DateSet::EMPTY; s.trips.len()])
        .collect();
    for (index, binding) in context.matches.bindings.iter().enumerate() {
        for (_, dates) in context.slices_for(index, &context.selections[index]) {
            let slot = &mut accepted[binding.source as usize][binding.trip as usize];
            *slot = *slot | dates;
        }
    }
    let mut added: HashSet<(u32, u32)> = HashSet::new();
    for addition in context.additions {
        added.extend(addition.members.iter().copied());
    }
    for projection in &context.authority.projections {
        if added.contains(&(projection.source, projection.trip)) {
            let slot = &mut accepted[projection.source as usize][projection.trip as usize];
            *slot = *slot | projection.dates;
        }
    }
    accepted
}

#[allow(clippy::too_many_arguments)]
pub fn reports(
    context: &Context<'_, '_>,
    built: &Built,
    headsigns: &Headsigns,
) -> (Vec<ReportTable>, Vec<Vec<DateSet>>) {
    let accepted = accepted(context);
    let rows = keys::mappings(context);
    let reports = vec![
        source_trips(context, &rows),
        source_trip_additions(context),
        headsign_rows(headsigns),
        post_pruning(context.prepared, context.stops, built),
        unmatched(context, &accepted),
    ];
    (reports, accepted)
}

/// One row per F# `source_to_output_trips` row: source, trip, date run, base trip, output trip,
/// and how (`bound`: the base trip itself; `bound_slice`: a part of it; `added`).
fn source_trips(context: &Context<'_, '_>, rows: &[Mapping]) -> ReportTable {
    let prepared = context.prepared;
    let table = rows
        .iter()
        .map(|row| {
            let source = &prepared.sources[row.source as usize];
            let trip = &source.trips[row.trip as usize];
            let output = &context.outputs[row.output];
            let (base, how) = match row.binding {
                Some(binding) => {
                    let base = context.matches.bindings[binding].candidate.base_trip;
                    let id = prepared.base.timetable.trips[prepared.base.trips[base as usize].trip]
                        .id
                        .to_string();
                    let how = if id == output.id {
                        "bound"
                    } else {
                        "bound_slice"
                    };
                    (id, how)
                }
                None => (String::new(), "added"),
            };
            vec![
                source.source_id().to_owned(),
                source.feed.trips[trip.feed_index as usize]
                    .trip_id
                    .to_string(),
                date(prepared, row.from),
                date(prepared, row.to),
                base,
                output.id.clone(),
                how.to_owned(),
            ]
        })
        .collect();
    ReportTable {
        name: "source_trips",
        header: vec![
            "source_id",
            "source_trip_id",
            "valid_from",
            "valid_to",
            "base_trip_id",
            "output_trip_id",
            "how",
        ],
        rows: table,
    }
}

fn line_text(context: &Context<'_, '_>, addition: &Addition) -> String {
    match &addition.line {
        AuthorityLine::National(line) => line.to_string(),
        AuthorityLine::SourceRoute { source, route } => {
            let s = &context.prepared.sources[*source as usize];
            format!(
                "source:{}:{}",
                s.source_id(),
                s.feed.routes[s.routes[*route as usize].feed_index as usize].route_id
            )
        }
    }
}

fn source_trip_additions(context: &Context<'_, '_>) -> ReportTable {
    let prepared = context.prepared;
    let mut rows = Vec::new();
    for (index, addition) in context.additions.iter().enumerate() {
        let source = &prepared.sources[addition.source as usize];
        let output = &context.outputs[context.of_addition[index]];
        let route = match addition.target {
            crate::authority::TargetRoute::Base(route) => {
                prepared.base.timetable.routes[route].id.to_string()
            }
            crate::authority::TargetRoute::Derived(d) => {
                context.authority.derived_routes[d as usize].id.to_string()
            }
        };
        for (from, to) in keys::runs(addition.dates) {
            rows.push(vec![
                trip_subject(source, &source.trips[addition.trip as usize]),
                output.id.clone(),
                line_text(context, addition),
                route.clone(),
                date(prepared, from),
                date(prepared, to),
                "authoritative_source_trip_set".to_owned(),
            ]);
        }
    }
    ReportTable {
        name: "source_trip_additions",
        header: vec![
            "source_trip_id",
            "output_trip_id",
            "cis_line_id",
            "output_route_id",
            "valid_from",
            "valid_to",
            "reason",
        ],
        rows,
    }
}

fn headsign_rows(headsigns: &Headsigns) -> ReportTable {
    ReportTable {
        name: "headsigns",
        header: vec![
            "source_trip_id",
            "stop_sequence",
            "raw_headsign",
            "output_headsign",
            "status",
            "reason",
            "destination_stop_id",
            "audit_day",
        ],
        rows: headsigns.rows.iter().map(|row| row.to_vec()).collect(),
    }
}

/// One row per accepted stop published as a post, in namespaced ID order.
fn post_pruning(prepared: &Prepared<'_>, stops: &Stops, built: &Built) -> ReportTable {
    let mut rows = Vec::new();
    for (index, post) in stops.posts.iter().enumerate() {
        for &(source, stop) in &post.members {
            let s = &prepared.sources[source as usize];
            rows.push(vec![
                format!("{}:{}", s.source_id(), s.feed.stops[stop as usize].stop_id),
                post.id.to_string(),
                if built.used_posts[index] {
                    "retained_used"
                } else {
                    "pruned_unused"
                }
                .to_owned(),
            ]);
        }
    }
    rows.sort();
    ReportTable {
        name: "post_pruning",
        header: vec!["source_stop_id", "output_post_id", "disposition"],
        rows,
    }
}

fn unmatched(context: &Context<'_, '_>, accepted: &[Vec<DateSet>]) -> ReportTable {
    let prepared = context.prepared;
    let flags = || -> Vec<Vec<bool>> {
        prepared
            .sources
            .iter()
            .map(|s| vec![false; s.trips.len()])
            .collect()
    };
    let (mut bound, mut projected, mut native, mut diagnosed) =
        (flags(), flags(), flags(), flags());
    for binding in &context.matches.bindings {
        bound[binding.source as usize][binding.trip as usize] = true;
    }
    for projection in &context.authority.projections {
        projected[projection.source as usize][projection.trip as usize] = true;
    }
    for addition in context.additions {
        if matches!(addition.line, AuthorityLine::SourceRoute { .. }) {
            native[addition.source as usize][addition.trip as usize] = true;
        }
    }
    let row = |s: usize, t: usize, code: &str, disposition: &str, message: &str| {
        let source = &prepared.sources[s];
        let trip = &source.trips[t];
        let feed_route =
            &source.feed.routes[source.routes[trip.route as usize].feed_index as usize];
        vec![
            source.source_id().to_owned(),
            source.feed.trips[trip.feed_index as usize]
                .trip_id
                .to_string(),
            feed_route.route_id.to_string(),
            mode_name(source, trip.route),
            code.to_owned(),
            disposition.to_owned(),
            message.to_owned(),
        ]
    };
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    for reason in &context.matches.unmatched {
        let (s, t) = (reason.source as usize, reason.trip as usize);
        diagnosed[s][t] = true;
        let disposition = if native[s][t] {
            "source_native_addition"
        } else if projected[s][t] {
            "authoritative_projection"
        } else if bound[s][t] {
            "matched_on_other_dates"
        } else {
            "withheld"
        };
        let values = row(s, t, reason.code, disposition, &reason.message);
        if seen.insert(values.clone()) {
            rows.push(values);
        }
    }
    for (s, source) in prepared.sources.iter().enumerate() {
        for (t, trip) in source.trips.iter().enumerate() {
            if trip.dates.is_empty() || !accepted[s][t].is_empty() || diagnosed[s][t] {
                continue;
            }
            let values = if bound[s][t] || projected[s][t] {
                row(
                    s,
                    t,
                    "trip_projection_unrepresented",
                    "withheld",
                    "Matching or authority evidence existed, but no active source date survived resolution and projection",
                )
            } else {
                row(
                    s,
                    t,
                    "trip_unclassified_unmatched",
                    "withheld",
                    "The active trip produced neither matching evidence nor a more specific matching diagnostic",
                )
            };
            if seen.insert(values.clone()) {
                rows.push(values);
            }
        }
    }
    ReportTable {
        name: "unmatched_trip_reasons",
        header: vec![
            "source_id",
            "source_trip_id",
            "source_route_id",
            "mode",
            "reason_code",
            "disposition",
            "message",
        ],
        rows,
    }
}

/// R120: the diagnostics that stand, in their order.
pub fn effective(
    prepared: &Prepared<'_>,
    matches: &Matches,
    authority: &Authority,
    accepted: &[Vec<DateSet>],
    diagnostics: Vec<Diagnostic>,
) -> Vec<Diagnostic> {
    let _ = authority;
    let mut placed: HashSet<String> = HashSet::new();
    for source in &prepared.sources {
        for group in source.groups.iter().filter(|g| g.matched.is_some()) {
            placed.insert(group.registry_key(source.source_id()));
        }
    }
    let mut withdrawn: HashSet<(&str, String, String)> = HashSet::new();
    let bound: HashSet<(u32, u32)> = matches
        .bindings
        .iter()
        .map(|b| (b.source, b.trip))
        .collect();
    let resolved: HashSet<(u32, u32, u32)> = matches
        .bindings
        .iter()
        .filter(|b| b.by_availability)
        .filter_map(|b| b.dates.first().map(|d| (b.source, b.trip, d as u32)))
        .collect();
    let covered = |s: u32, t: u32| {
        let trip = &prepared.sources[s as usize].trips[t as usize];
        !trip.dates.is_empty() && trip.dates.is_subset(&accepted[s as usize][t as usize])
    };
    for reason in &matches.unmatched {
        let source = &prepared.sources[reason.source as usize];
        let subject = trip_subject(source, &source.trips[reason.trip as usize]);
        let drop = (reason.code == "trip_validity_unresolved"
            && bound.contains(&(reason.source, reason.trip)))
            || (reason.code == "trip_same_date_ambiguous"
                && reason
                    .date
                    .is_some_and(|d| resolved.contains(&(reason.source, reason.trip, d))))
            || (covered(reason.source, reason.trip)
                && matches!(
                    reason.code,
                    "trip_route_candidate_unresolved"
                        | "trip_signature_unresolved"
                        | "trip_validity_unresolved"
                        | "trip_same_date_ambiguous"
                        | "base_snapshot_gap"
                        | "base_snapshot_capacity_gap"
                ));
        if drop {
            withdrawn.insert((reason.code, subject, reason.message.clone()));
        }
    }
    let by_key: HashMap<(&str, &str, &str), ()> = withdrawn
        .iter()
        .map(|(c, s, m)| ((*c, s.as_str(), m.as_str()), ()))
        .collect();
    diagnostics
        .into_iter()
        .filter(|d| {
            let stop = (d.code == "stop_match_unresolved" || d.code == "stop_match_ambiguous")
                && placed.contains(&d.subject);
            !stop
                && !by_key.contains_key(&(d.code.as_ref(), d.subject.as_str(), d.message.as_str()))
        })
        .collect()
}
