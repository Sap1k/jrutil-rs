//! Transfers (OVL-R107–R109).

use std::collections::{BTreeMap, HashMap};

use jr_model::timetable::{LocationRef, Transfer, TransferKind};
use jr_model::{Diagnostic, Diagnostics, EditBuilder, Rule, Severity, TripId};

use super::batch::Context;
use super::capability;
use super::stops::OutStop;
use super::summary::Summary;
use crate::profile::{Capability, TRANSFER_WAIT};

/// A base transfer between trips follows their parts: every part of the one to every part of
/// the other (`SplitTrip`; no date intersection, as in the reference: B15 recorded).
pub const OVL_R107: Rule = Rule {
    id: "OVL-R107",
    summary: "base transfers follow their trips' parts",
};

/// With `transfers`, a source transfer between two trips is projected onto every pair of their
/// outputs (parts of bindings eligible for `transfers` that agree with them, and additions)
/// sharing a day, between the published boarding points of its stops. A transfer between stops
/// only is not projected (GTFS cannot bound it by date); one whose trips have no output is
/// `transfer_reference_unresolved`.
pub const OVL_R108: Rule = Rule {
    id: "OVL-R108",
    summary: "source trip-to-trip transfers are projected onto the trips' outputs",
};

/// Projected transfers with one selector (stops and trips) reconcile: one value set is kept;
/// different transfer types drop it (`transfer_conflict_quarantined`); otherwise the longest
/// minimum time and the shortest maximum wait (`transfer_date_variants_merged`).
///
/// "So no published wait is longer than every source promises."
pub const OVL_R109: Rule = Rule {
    id: "OVL-R109",
    summary: "transfers with one selector merge conservatively or are dropped",
};

pub const RULES: &[Rule] = &[OVL_R107, OVL_R108, OVL_R109];

type Selector = (LocationRef, LocationRef, TripId, TripId);
/// A transfer's values: kind, minimum time, maximum wait.
type Values = (TransferKind, Option<i32>, Option<i32>);

fn kind(code: Option<i32>) -> Option<TransferKind> {
    Some(match code? {
        0 => TransferKind::Recommended,
        1 => TransferKind::Timed,
        2 => TransferKind::MinimumTime,
        3 => TransferKind::NotPossible,
        4 => TransferKind::InSeat,
        5 => TransferKind::InSeatNotAllowed,
        _ => return None,
    })
}

pub fn build(
    context: &Context<'_, '_>,
    b: &mut EditBuilder<'_>,
    diagnostics: &mut Diagnostics,
    summary: &mut Summary,
) {
    let prepared = context.prepared;
    let t = prepared.base.timetable;
    let selections = context.selections;
    let trip_ids: HashMap<TripId, &str> =
        context.outputs.iter().map(|o| (o.trip, &*o.id)).collect();
    let point_ids: HashMap<jr_model::BoardingPointId, &str> = context
        .handles
        .iter()
        .zip(&context.stops.posts)
        .filter_map(|(handle, post)| handle.map(|h| (h, post.id.as_str())))
        .collect();
    let mut rows: BTreeMap<Selector, Vec<Values>> = BTreeMap::new();
    for (source_index, source) in prepared.sources.iter().enumerate() {
        let source_index = source_index as u32;
        if !capability::enabled(prepared, source_index, Capability::Transfers) {
            continue;
        }
        let trip_by_id: HashMap<&str, u32> = source
            .trips
            .iter()
            .enumerate()
            .map(|(index, trip)| {
                (
                    &*source.feed.trips[trip.feed_index as usize].trip_id,
                    index as u32,
                )
            })
            .collect();
        let mut bindings_of: HashMap<u32, Vec<usize>> = HashMap::new();
        for (index, binding) in context.matches.bindings.iter().enumerate() {
            if binding.source == source_index {
                bindings_of.entry(binding.trip).or_default().push(index);
            }
        }
        let mut addition_of: HashMap<u32, usize> = HashMap::new();
        for (index, addition) in context.additions.iter().enumerate() {
            for &(s, trip) in &addition.members {
                if s == source_index {
                    addition_of.insert(trip, index);
                }
            }
        }
        let outputs_of = |trip: u32| -> Vec<(TripId, jr_model::DateSet)> {
            let mut found = Vec::new();
            for &binding in bindings_of.get(&trip).into_iter().flatten() {
                if !capability::eligible(
                    prepared,
                    &context.matches.bindings[binding],
                    Capability::Transfers,
                ) {
                    continue;
                }
                for (output, dates) in context.slices_for(binding, &selections[binding]) {
                    found.push((context.outputs[output].trip, dates));
                }
            }
            if let Some(&addition) = addition_of.get(&trip) {
                let output = &context.outputs[context.of_addition[addition]];
                found.push((output.trip, context.additions[addition].dates));
            }
            found
        };
        let stop_index: HashMap<&str, u32> = source
            .feed
            .stops
            .iter()
            .enumerate()
            .rev()
            .map(|(index, stop)| (&*stop.stop_id, index as u32))
            .collect();
        let stop_of = |id: Option<&str>| -> Option<LocationRef> {
            let index = *stop_index.get(id?)?;
            match context.stops.out(source_index, index)? {
                OutStop::Post(post) if context.used_posts[post as usize] => {
                    context.handles[post as usize].map(LocationRef::BoardingPoint)
                }
                _ => None,
            }
        };
        let wait_column = source
            .profile
            .reader()
            .extra_columns
            .iter()
            .find(|(file, _)| &**file == "transfers.txt")
            .and_then(|(_, columns)| columns.iter().position(|c| &**c == TRANSFER_WAIT));
        for row in &source.feed.transfers {
            let (from_trip, to_trip) = (row.from_trip_id.as_deref(), row.to_trip_id.as_deref());
            let subject = || {
                format!(
                    "{src}:{}->{src}:{}",
                    from_trip.unwrap_or(""),
                    to_trip.unwrap_or(""),
                    src = source.source_id()
                )
            };
            let (Some(from_trip), Some(to_trip)) = (from_trip, to_trip) else {
                if from_trip.is_some() || to_trip.is_some() {
                    diagnostics.push(unresolved(subject()));
                }
                continue;
            };
            let from_outputs = trip_by_id
                .get(from_trip)
                .map_or_else(Vec::new, |&t| outputs_of(t));
            let to_outputs = trip_by_id
                .get(to_trip)
                .map_or_else(Vec::new, |&t| outputs_of(t));
            if from_outputs.is_empty() || to_outputs.is_empty() {
                diagnostics.push(unresolved(subject()));
                continue;
            }
            let (Some(from), Some(to), Some(transfer_kind)) = (
                stop_of(row.from_stop_id.as_deref()),
                stop_of(row.to_stop_id.as_deref()),
                kind(row.transfer_type),
            ) else {
                continue;
            };
            let wait = wait_column
                .and_then(|c| row.extra.get(c))
                .and_then(|v| v.as_deref())
                .and_then(|v| v.parse::<i32>().ok());
            for &(from_output, from_dates) in &from_outputs {
                for &(to_output, to_dates) in &to_outputs {
                    if from_dates.intersects(&to_dates) {
                        rows.entry((from, to, from_output, to_output))
                            .or_default()
                            .push((transfer_kind, row.min_transfer_time, wait));
                    }
                }
            }
        }
    }
    // R109.
    let mut ordered: Vec<(String, Transfer)> = Vec::new();
    for ((from, to, from_trip, to_trip), mut variants) in rows {
        variants.dedup();
        let mut distinct: Vec<Values> = Vec::new();
        for v in variants {
            if !distinct.contains(&v) {
                distinct.push(v);
            }
        }
        let location = |l: LocationRef| match l {
            LocationRef::Place(p) => t.places.get(p).map(|r| r.id.to_string()),
            LocationRef::BoardingPoint(p) => point_ids.get(&p).map(|id| (*id).to_owned()),
        };
        let (from_id, to_id) = (
            location(from).unwrap_or_default(),
            location(to).unwrap_or_default(),
        );
        let trip_id = |trip: TripId| {
            trip_ids
                .get(&trip)
                .map(|id| (*id).to_owned())
                .unwrap_or_default()
        };
        let object = format!(
            "{from_id}|{to_id}|||{}|{}",
            trip_id(from_trip),
            trip_id(to_trip)
        );
        let (transfer_kind, minimum, wait) = match distinct.as_slice() {
            [one] => *one,
            _ if distinct.iter().any(|v| v.0 != distinct[0].0) => {
                diagnostics.push(
                    Diagnostic::new(
                        "transfer_conflict_quarantined",
                        Severity::Warning,
                        object,
                        "Source transfer revisions disagree on transfer_type for one output selector; transfer dropped",
                    )
                    .rule(OVL_R109),
                );
                continue;
            }
            _ => {
                let minimum = distinct.iter().filter_map(|v| v.1).max();
                let wait = distinct.iter().filter_map(|v| v.2).min();
                diagnostics.push(
                    Diagnostic::new(
                        "transfer_date_variants_merged",
                        Severity::Info,
                        object,
                        format!(
                            "{} source transfer revisions merged conservatively (min_transfer_time={}, max_waiting_time={})",
                            distinct.len(),
                            minimum.map_or_else(String::new, |m| m.to_string()),
                            wait.map_or_else(String::new, |w| w.to_string())
                        ),
                    )
                    .rule(OVL_R109),
                );
                (distinct[0].0, minimum, wait)
            }
        };
        ordered.push((
            format!(
                "{}\u{1f}{}\u{1f}{from_id}\u{1f}{to_id}",
                trip_id(from_trip),
                trip_id(to_trip)
            ),
            Transfer {
                from: Some(from),
                to: Some(to),
                from_route: None,
                to_route: None,
                from_trip: Some(from_trip),
                to_trip: Some(to_trip),
                kind: transfer_kind,
                minimum_time: minimum,
                maximum_wait: wait,
            },
        ));
    }
    ordered.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, transfer) in ordered {
        b.push(OVL_R108, jr_model::Edit::AddTransfer(transfer));
        summary.transfers_added += 1;
    }
}

fn unresolved(subject: String) -> Diagnostic {
    Diagnostic::new(
        "transfer_reference_unresolved",
        Severity::Warning,
        subject,
        "Both non-rail source trips and their output slices must resolve",
    )
    .rule(OVL_R108)
}
