//! Source keys and call keys (OVL-R112–R116).

use std::borrow::Cow;
use std::collections::{BTreeSet, HashSet};

use jr_model::identity::escape;
use jr_model::timetable::{BindingMethod, CallKey, Entity, Namespace, SourceKey};
use jr_model::{Date, DateSet, EditBuilder, Rule};

use super::batch::Context;
use super::stops::{OutPlace, OutStop};
use super::summary::Summary;
use crate::profile::RouteTier;

/// A source trip's ID resolves to each output trip it ended on, per contiguous run of the dates
/// it is represented there: a part bound to it (`companion_assertion` or `structural_match` by
/// its route tier), or its addition (`authoritative_source_trip_set`). The namespace is the
/// profile's `key_namespace` with `:gtfs_trip_id`.
pub const OVL_R112: Rule = Rule {
    id: "OVL-R112",
    summary: "source trip keys per output trip and contiguous date run",
};

/// The operator's line and course (R019) resolve to the same output trips and runs
/// (`<namespace>:line_course`, `line/course`, `operator_crosswalk`).
pub const OVL_R113: Rule = Rule {
    id: "OVL-R113",
    summary: "operator line/course keys follow the source trip's keys",
};

/// A source route resolves to the routes its output trips run on (B7 fixed: the part's own
/// route, only where a part survives), a source stop to the post or place it is published as
/// when an output call uses it; over the package's dates, `source_native` for what the overlay
/// added, else `structural_match`.
pub const OVL_R114: Rule = Rule {
    id: "OVL-R114",
    summary: "source route and stop keys resolve to the published routes and posts",
};

/// Keys of a base trip follow its parts (`SplitTrip`), unclipped to the part's dates.
pub const OVL_R115: Rule = Rule {
    id: "OVL-R115",
    summary: "base keys follow their trips' parts",
};

/// A source call whose source sequence differs from the published sequence of the call it
/// became resolves by a call key; base call keys follow their calls (`ReplaceTripCalls`).
pub const OVL_R116: Rule = Rule {
    id: "OVL-R116",
    summary: "call keys where the source sequence differs from the published one",
};

pub const RULES: &[Rule] = &[OVL_R112, OVL_R113, OVL_R114, OVL_R115, OVL_R116];

/// Contiguous runs of a date set, as day offsets.
pub fn runs(dates: DateSet) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for day in dates.iter() {
        match runs.last_mut() {
            Some((_, last)) if *last + 1 == day => *last = day,
            _ => runs.push((day, day)),
        }
    }
    runs
}

/// One row of F#'s `source_to_output_trips` (and of the `source_trips` review table).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Mapping {
    pub source: u32,
    pub trip: u32,
    /// Index into the outputs.
    pub output: usize,
    pub from: usize,
    pub to: usize,
    /// The binding, or `None` for an addition.
    pub binding: Option<usize>,
    pub method: BindingMethod,
}

/// R112's rows: bindings × agreeing parts × runs, then additions × members × runs; distinct.
pub fn mappings(context: &Context<'_, '_>) -> Vec<Mapping> {
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    for (index, binding) in context.matches.bindings.iter().enumerate() {
        let method = match binding.route_tier {
            RouteTier::CompanionAssertion => BindingMethod::CompanionAssertion,
            RouteTier::StructuralTripEvidence => BindingMethod::StructuralMatch,
        };
        for (output, dates) in context.slices_for(index, &context.selections[index]) {
            for (from, to) in runs(dates) {
                let row = Mapping {
                    source: binding.source,
                    trip: binding.trip,
                    output,
                    from,
                    to,
                    binding: Some(index),
                    method,
                };
                let key = (
                    row.source,
                    row.trip,
                    row.output,
                    from,
                    to,
                    binding.route_tier,
                    binding.candidate.tier,
                    binding.candidate.rank,
                );
                if seen.insert(format!("{key:?}")) {
                    rows.push(row);
                }
            }
        }
    }
    for (index, addition) in context.additions.iter().enumerate() {
        for &(source, trip) in &addition.members {
            for (from, to) in runs(addition.dates) {
                rows.push(Mapping {
                    source,
                    trip,
                    output: context.of_addition[index],
                    from,
                    to,
                    binding: None,
                    method: BindingMethod::AuthoritativeSourceTripSet,
                });
            }
        }
    }
    rows
}

fn namespace(prefix: &str, name: &str) -> Namespace {
    Namespace(Cow::Owned(format!("{prefix}:{name}")))
}

pub fn build(context: &Context<'_, '_>, b: &mut EditBuilder<'_>, summary: &mut Summary) {
    let prepared = context.prepared;
    let horizon = prepared.base.timetable.horizon;
    let date = |day: usize| -> Date { horizon.date_at(day) };
    let mut all = DateSet::EMPTY;
    for output in context.outputs {
        all = all | output.dates;
    }
    let (Some(first), Some(last)) = (all.first(), all.last()) else {
        return;
    };
    let mut keys: Vec<SourceKey> = Vec::new();
    let mut seen: BTreeSet<SourceKey> = BTreeSet::new();
    let mut push = |key: SourceKey, keys: &mut Vec<SourceKey>| {
        if seen.insert(key.clone()) {
            keys.push(key);
        }
    };
    let rows = mappings(context);
    // R112, R113.
    for row in &rows {
        let source = &prepared.sources[row.source as usize];
        let trip = &source.trips[row.trip as usize];
        let ns = &source.profile.key_namespace;
        let output = &context.outputs[row.output];
        push(
            SourceKey {
                namespace: namespace(ns, "gtfs_trip_id"),
                identifier: source.feed.trips[trip.feed_index as usize].trip_id.clone(),
                entity: Entity::Trip(output.trip),
                valid_from: date(row.from),
                valid_to: date(row.to),
                binding: row.method,
            },
            &mut keys,
        );
        if let Some(crosswalk) = trip.operational {
            let entry = &source.crosswalk[crosswalk as usize];
            push(
                SourceKey {
                    namespace: namespace(ns, "line_course"),
                    identifier: format!(
                        "{}/{}",
                        escape(&entry.line).replace("%3A", ":"),
                        escape(&entry.course).replace("%3A", ":")
                    )
                    .into(),
                    entity: Entity::Trip(output.trip),
                    valid_from: date(row.from),
                    valid_to: date(row.to),
                    binding: BindingMethod::OperatorCrosswalk,
                },
                &mut keys,
            );
        }
    }
    // R114: routes.
    for row in &rows {
        let source = &prepared.sources[row.source as usize];
        let trip = &source.trips[row.trip as usize];
        let feed_route =
            &source.feed.routes[source.routes[trip.route as usize].feed_index as usize];
        let output = &context.outputs[row.output];
        let added = context.derived.contains(&Some(output.route));
        push(
            SourceKey {
                namespace: namespace(&source.profile.key_namespace, "gtfs_route_id"),
                identifier: feed_route.route_id.clone(),
                entity: Entity::Route(output.route),
                valid_from: date(first),
                valid_to: date(last),
                binding: if added {
                    BindingMethod::SourceNative
                } else {
                    BindingMethod::StructuralMatch
                },
            },
            &mut keys,
        );
    }
    // R114: stops whose post or place an output call uses.
    let used_points: HashSet<jr_model::BoardingPointId> = context
        .outputs
        .iter()
        .flat_map(|o| o.points.iter().flatten().copied())
        .collect();
    for (source_index, source) in prepared.sources.iter().enumerate() {
        for (stop, row) in source.feed.stops.iter().enumerate() {
            if !context.stops.accepted[source_index][stop] {
                continue;
            }
            let (entity, binding) = match context.stops.out(source_index as u32, stop as u32) {
                Some(OutStop::Post(post)) => match context.handles[post as usize] {
                    Some(handle) if used_points.contains(&handle) => {
                        (Entity::BoardingPoint(handle), BindingMethod::SourceNative)
                    }
                    _ => continue,
                },
                Some(OutStop::Place(OutPlace::Base(place))) => {
                    (Entity::Place(place), BindingMethod::StructuralMatch)
                }
                _ => continue,
            };
            push(
                SourceKey {
                    namespace: namespace(&source.profile.key_namespace, "gtfs_stop_id"),
                    identifier: row.stop_id.clone(),
                    entity,
                    valid_from: date(first),
                    valid_to: date(last),
                    binding,
                },
                &mut keys,
            );
        }
    }
    for key in keys {
        b.push(OVL_R112, jr_model::Edit::AddKey(key));
        summary.keys_added += 1;
    }
    // R116.
    let mut call_seen: HashSet<(u32, u32, usize, usize, u32)> = HashSet::new();
    for row in &rows {
        let source = &prepared.sources[row.source as usize];
        let trip = &source.trips[row.trip as usize];
        let calls = source.trip_calls(trip);
        let output = &context.outputs[row.output];
        let mut sequences = output.sequences.clone();
        sequences.sort_unstable();
        let positions: Vec<(usize, usize)> = match row.binding {
            Some(binding) => {
                let selection = &context.selections[binding];
                let candidate = &context.matches.bindings[binding].candidate;
                let mut positions = Vec::new();
                let mut before = 0;
                for (target, aligned) in candidate.alignment.iter().enumerate() {
                    let kept: Vec<u32> = selection
                        .facts
                        .repeats
                        .iter()
                        .zip(selection.repeat_ordinals.iter())
                        .filter(|(r, _)| r.target as usize == target)
                        .map(|(_, &o)| o)
                        .collect();
                    let position = target + 1 + before;
                    if let Some(first_call) = aligned {
                        let length = if candidate.run_lengths.is_empty() {
                            1
                        } else {
                            candidate.run_lengths[target].max(1)
                        };
                        for ordinal in *first_call..*first_call + length {
                            if !kept.contains(&ordinal) {
                                positions.push((ordinal as usize, position));
                            }
                        }
                    }
                    for (index, &ordinal) in kept.iter().enumerate() {
                        positions.push((ordinal as usize, position + 1 + index));
                    }
                    before += kept.len();
                }
                positions
            }
            None => (0..calls.len()).map(|i| (i, i + 1)).collect(),
        };
        for (ordinal, position) in positions {
            let Some(&sequence) = sequences.get(position - 1) else {
                continue;
            };
            let Some(source_sequence) = calls.get(ordinal).and_then(|c| c.sequence) else {
                continue;
            };
            if source_sequence == sequence
                || !call_seen.insert((row.source, row.trip, ordinal, row.output, position as u32))
            {
                continue;
            }
            b.push(
                OVL_R116,
                jr_model::Edit::AddCallKey(CallKey {
                    namespace: namespace(&source.profile.key_namespace, "gtfs_trip_id"),
                    identifier: source.feed.trips[trip.feed_index as usize].trip_id.clone(),
                    source_sequence: source_sequence.to_string().into(),
                    trip: output.trip,
                    sequence,
                }),
            );
            summary.call_keys_added += 1;
        }
    }
}
