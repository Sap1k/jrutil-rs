//! Placing stop groups from the trips that call them (OVL-R044, R045), the approximate-stop
//! correction (R046) and call places (R054).

use std::collections::{BTreeMap, HashMap};

use jr_model::{Diagnostic, Diagnostics, PlaceId, Rule, Severity};
use rayon::prelude::*;

use super::base::{BaseView, point_set_distance};
use super::names::Names;
use super::signature::{self, Rank, Times};
use super::source::{ContextInference, MatchMethod, PlaceRef, PreparedSource, StopMatch};

/// Repeated until a pass accepts nothing: an eligible trip running in the horizon whose calls
/// leave between one and the profile's maximum of stop groups unplaced, each called once, with at
/// least the profile's minimum of other calls, is compared with the published base trips of its
/// route's primary and relaxed candidate routes (R049–R051) that have as many calls, share a
/// running day and agree at every position placed by R040–R043 (positions placed by context in
/// an earlier pass are not compared, as in the reference). The best-ranked of them (R069, R070,
/// identity alignment) propose, for each unplaced group, the place at its position when they
/// all agree. Proposals go to R045; after an accepting pass the trips are read again.
///
/// Names and coordinates leave some groups unplaced that the trips calling them place
/// unambiguously.
pub const OVL_R044: Rule = Rule {
    id: "OVL-R044",
    summary: "unplaced stop groups placed from the best base trips of the trips calling them",
};

/// A group is placed when all its proposals name one base place whose name ranks with the
/// group's (R033) or which is approximate (R026): method `trip_context_gtfs_authoritative` for
/// an approximate place, else `trip_context_unique`. No distance limit applies; beyond the
/// profile's distance from a place that is not approximate it is diagnosed. A place of another
/// name, or several places, leave the group unplaced (diagnosed).
pub const OVL_R045: Rule = Rule {
    id: "OVL-R045",
    summary: "a context proposal is accepted for one place of a compatible name or approximate",
};

/// After context inference, an approximate base place that exactly one source group (of any
/// source) is placed at is corrected by it: the source names and places it (`+gtfs_authoritative`).
/// When several source groups are placed at one approximate place, none of them stays placed.
///
/// A provisional JDF row cannot collapse multiple distinct source places.
pub const OVL_R046: Rule = Rule {
    id: "OVL-R046",
    summary: "one source group corrects an approximate base place; several are unplaced",
};

/// A source call's place is where its stop's group is placed, or none.
pub const OVL_R054: Rule = Rule {
    id: "OVL-R054",
    summary: "a source call's place is its stop group's placement",
};

pub const RULES: &[Rule] = &[OVL_R044, OVL_R045, OVL_R046, OVL_R054];

/// One proposal: group, trip, 0-based ordinal, place.
type Proposal = (u32, u32, u32, PlaceId);

/// R044, R045 for one source.
pub fn infer(
    source: &mut PreparedSource<'_>,
    base: &BaseView<'_>,
    names: &Names,
    diagnostics: &mut Diagnostics,
) {
    let policy = source.profile.stops.context;
    if !policy.enabled {
        return;
    }
    // The places R040–R043 gave, by group: the known positions of every pass.
    let initial: Vec<Option<PlaceId>> = source
        .groups
        .iter()
        .map(|group| match group.matched {
            Some(StopMatch {
                target: PlaceRef::Base(place),
                method,
                ..
            }) if method.is_initial() => Some(place),
            _ => None,
        })
        .collect();
    let mut pending: Vec<Diagnostic> = Vec::new();
    loop {
        let proposals = collect(source, base, &initial);
        let mut by_group: Vec<(u32, Vec<Proposal>)> = Vec::new();
        let mut slot: HashMap<u32, usize> = HashMap::new();
        for proposal in proposals {
            let at = *slot.entry(proposal.0).or_insert_with(|| {
                by_group.push((proposal.0, Vec::new()));
                by_group.len() - 1
            });
            by_group[at].1.push(proposal);
        }
        pending.clear();
        let mut accepted = 0;
        for (group, evidence) in by_group {
            let mut targets: Vec<PlaceId> = Vec::new();
            for proposal in &evidence {
                if !targets.contains(&proposal.3) {
                    targets.push(proposal.3);
                }
            }
            let source_id = source.profile.source_id.to_string();
            let subject = source.groups[group as usize].registry_key(&source_id);
            let place = match targets.as_slice() {
                [target] => base.place(*target),
                _ => {
                    pending.push(
                        Diagnostic::new(
                            "stop_context_conflict",
                            Severity::Warning,
                            subject,
                            format!("Context proposed {} target stop places", targets.len()),
                        )
                        .rule(OVL_R045),
                    );
                    continue;
                }
            };
            let Some(place) = place else { continue };
            let entry = &source.groups[group as usize];
            if !place.estimated && names.rank(&entry.name, &place.name).is_none() {
                pending.push(
                    Diagnostic::new(
                        "stop_context_name_conflict",
                        Severity::Warning,
                        subject,
                        format!(
                            "Context target {} is incompatible with {}",
                            place.name, entry.name
                        ),
                    )
                    .rule(OVL_R045),
                );
                continue;
            }
            let distance = point_set_distance(&entry.points, &place.points);
            if let Some(distance) = distance
                && distance > source.profile.stops.maximum_distance_metres
                && !place.estimated
            {
                diagnostics.push(
                    Diagnostic::new(
                        "stop_context_distance_warning",
                        Severity::Warning,
                        subject,
                        format!("Strong route/call context accepted a compatible established stop at {distance:.1} m"),
                    )
                    .rule(OVL_R045),
                );
            }
            let method = if place.estimated {
                MatchMethod::TripContextGtfsAuthoritative
            } else {
                MatchMethod::TripContextUnique
            };
            source.groups[group as usize].matched = Some(StopMatch {
                target: PlaceRef::Base(place.place),
                method,
                distance,
                corrected: false,
            });
            for (group, trip, ordinal, target) in evidence {
                source.inferences.push(ContextInference {
                    group,
                    trip,
                    ordinal: ordinal + 1,
                    target,
                    method: MatchMethod::TripContextUnique,
                });
            }
            accepted += 1;
        }
        if accepted == 0 {
            break;
        }
    }
    for diagnostic in pending {
        diagnostics.push(diagnostic);
    }
}

/// One pass of proposals, in trip order.
fn collect(
    source: &PreparedSource<'_>,
    base: &BaseView<'_>,
    initial: &[Option<PlaceId>],
) -> Vec<Proposal> {
    let policy = source.profile.stops.context;
    let maximum = policy.maximum_unresolved_groups_per_trip.max(1) as usize;
    let per_trip: Vec<Vec<Proposal>> = source
        .trips
        .par_iter()
        .enumerate()
        .map(|(trip_index, trip)| {
            let calls = source.trip_calls(trip);
            if trip.dates.is_empty() || calls.is_empty() {
                return Vec::new();
            }
            let groups: Vec<Option<u32>> = calls
                .iter()
                .map(|call| source.group_of_stop[call.stop as usize])
                .collect();
            let unplaced = |group: &Option<u32>| {
                group.is_some_and(|group| source.groups[group as usize].matched.is_none())
            };
            let unresolved_calls = groups.iter().filter(|group| unplaced(group)).count();
            let mut gaps: Vec<(u32, u32)> = Vec::new();
            for (ordinal, group) in groups.iter().enumerate() {
                if unplaced(group) {
                    let group = group.unwrap_or_default();
                    if gaps.iter().any(|(g, _)| *g == group) {
                        return Vec::new();
                    }
                    gaps.push((group, ordinal as u32));
                }
            }
            if gaps.is_empty()
                || gaps.len() > maximum
                || calls.len() - unresolved_calls < policy.minimum_mapped_calls as usize
            {
                return Vec::new();
            }
            let known: Vec<Option<PlaceId>> = groups
                .iter()
                .map(|group| group.and_then(|group| initial[group as usize]))
                .collect();
            let arrivals: Vec<Option<i32>> = calls.iter().map(|call| call.arrival).collect();
            let departures: Vec<Option<i32>> = calls.iter().map(|call| call.departure).collect();
            let source_times = Times {
                arrivals: &arrivals,
                departures: &departures,
            };
            let route = &source.routes[trip.route as usize];
            let mut seen: Vec<u32> = Vec::new();
            let alignment: Vec<Option<usize>> = (0..calls.len()).map(Some).collect();
            let mut best: Option<Rank> = None;
            let mut best_patterns: Vec<&[PlaceId]> = Vec::new();
            for &route in route.primary.iter().chain(&route.structural) {
                for &slot in base.trips_with_length(route, calls.len()) {
                    if seen.contains(&slot) {
                        continue;
                    }
                    seen.push(slot);
                    let candidate = &base.trips[slot as usize];
                    let pattern = base.pattern(candidate.pattern);
                    if !trip.dates.intersects(&candidate.dates)
                        || !known
                            .iter()
                            .zip(pattern)
                            .all(|(known, place)| known.is_none_or(|k| k == *place))
                    {
                        continue;
                    }
                    let target_times = Times {
                        arrivals: &candidate.arrivals,
                        departures: &candidate.departures,
                    };
                    let rank = signature::rank(0, source_times, target_times, &alignment);
                    match best {
                        Some(current) if rank > current => {}
                        Some(current) if rank == current => best_patterns.push(pattern),
                        _ => {
                            best = Some(rank);
                            best_patterns.clear();
                            best_patterns.push(pattern);
                        }
                    }
                }
            }
            let mut proposals = Vec::new();
            if best.is_none() {
                return proposals;
            }
            for (group, ordinal) in gaps {
                let first = best_patterns[0][ordinal as usize];
                if best_patterns
                    .iter()
                    .all(|pattern| pattern[ordinal as usize] == first)
                {
                    proposals.push((group, trip_index as u32, ordinal, first));
                }
            }
            proposals
        })
        .collect();
    per_trip.into_iter().flatten().collect()
}

/// R046 over every source.
pub fn correct_approximate(
    sources: &mut [PreparedSource<'_>],
    base: &BaseView<'_>,
    diagnostics: &mut Diagnostics,
) {
    let mut at: BTreeMap<PlaceId, Vec<(usize, usize)>> = BTreeMap::new();
    for (index, source) in sources.iter().enumerate() {
        for (group_index, group) in source.groups.iter().enumerate() {
            if let Some(StopMatch {
                target: PlaceRef::Base(place),
                ..
            }) = group.matched
                && base.place(place).is_some_and(|place| place.estimated)
            {
                at.entry(place).or_default().push((index, group_index));
            }
        }
    }
    for (place, groups) in at {
        let id = base.timetable.places[place].id.to_string();
        if let [(source, group)] = groups.as_slice() {
            let source = &mut sources[*source];
            let name = source.groups[*group].name.clone();
            if let Some(matched) = &mut source.groups[*group].matched {
                matched.corrected = true;
            }
            diagnostics.push(
                Diagnostic::new(
                    "stop_approximate_corrected",
                    Severity::Info,
                    id,
                    format!(
                        "{} GTFS supplied authoritative name and geodata: {name}",
                        source.profile.source_id
                    ),
                )
                .rule(OVL_R046),
            );
        } else {
            for &(source, group) in &groups {
                sources[source].groups[group].matched = None;
            }
            diagnostics.push(
                Diagnostic::new(
                    "stop_approximate_split",
                    Severity::Warning,
                    id,
                    format!("{} source places remain distinct instead of sharing one approximate JDF stop", groups.len()),
                )
                .rule(OVL_R046),
            );
        }
    }
}

/// R054: every call's place from its group's placement.
pub fn place_calls(source: &mut PreparedSource<'_>) {
    let PreparedSource {
        calls,
        groups,
        group_of_stop,
        ..
    } = source;
    for call in calls.iter_mut() {
        call.place = group_of_stop[call.stop as usize]
            .and_then(|group| groups[group as usize].matched)
            .map(|matched| matched.target);
    }
}
