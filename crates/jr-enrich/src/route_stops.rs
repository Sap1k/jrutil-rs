//! Route-stop order (`docs/rules/serving.md`, section 7.1–7.4).
//!
//! A route's route stops are the ordered stop list of each of its directions, as a line
//! timetable prints it: one slot per visit of a place. They are derived by merging the stop
//! patterns of the route's trips, and every passenger call then points at its slot.
//!
//! Only published trips take part: a trip whose date set is empty does not run, is not
//! published and its calls get no slot. Non-passenger calls (railway points passed without
//! service) are not part of any pattern and get no slot either.
//!
//! Groups are `(route, direction)`; a trip without a direction is in the route's "no direction"
//! group (verdict 1: CZPTT trips have none, so a rail line's list holds both travel directions).
//! Groups are emitted in order of the route's public ID, then direction with "none" first, so
//! the slot handles are stable. Groups are merged in parallel; the result does not depend on it.

use std::collections::BTreeMap;

use jr_model::handle::Handle;
use jr_model::identity;
use jr_model::timetable::{RouteStop, Trip};
use jr_model::{Edit, EditBuilder, Edits, PlaceId, RouteId, RouteStopId, Rule, Timetable, TripId};
use rayon::prelude::*;

mod merge;

use merge::{Pattern, merge};

/// The pass name recorded in the timetable's history.
pub const PASS: &str = "route_stops";

/// Slot ID `<route>/<direction or ->/<place>/<visit>`, where `visit` counts the place's
/// occurrences in the group's merged list from 1.
///
/// Route-stop IDs are foreign-key targets in Oběhy and the contract specifies their readable
/// form; a loop line visits a place twice, so the visit number keeps the two slots apart.
/// Built only by `jr_model::identity::route_stop`.
pub const SERVING_R003: Rule = Rule {
    id: "SERVING-R003",
    summary: "route-stop slot ID is route/direction/place/visit, visit counting the place's slots",
};

/// The merge of a route direction's stop patterns into one ordered slot list.
///
/// Trips of one line run different stop patterns (short turns, a stop served only by some
/// trips, versions of the route), but a line timetable prints one list. A pattern is the
/// sequence of places of a trip's passenger calls; its weight is the number of trips running
/// it. Patterns are merged heaviest first, then longest first, then by the tie-break below.
/// Each is aligned to the list built so far by longest common subsequence, preferring the
/// earliest slot on a tie; aligned stops reuse their slot, the others become new slots, and
/// within each gap the new slots go before an existing one only when both relative times are
/// known and the new one is strictly earlier. Every visit of a place is its own slot.
///
/// Tie-break between patterns of equal weight and length: the sequences of the places' public
/// IDs, compared element by element as strings (byte order), the shorter first on a common
/// prefix. (The F# compared the joined, escaped composite key ordinally instead.)
pub const SERVING_R028: Rule = Rule {
    id: "SERVING-R028",
    summary: "route-stop order merges patterns by weight, length, place IDs; LCS, strict-earlier insert",
};

/// A pattern's relative times come from its representative trip: the one with the smallest
/// public trip ID (byte order). Each passenger call's time (departure, else arrival) is its
/// fraction of the span between the trip's earliest and latest known time; unknown when the
/// trip has fewer than two known times or no span.
///
/// Trips of one pattern differ in their running times; taking one trip chosen by ID keeps the
/// result independent of input order.
pub const SERVING_R029: Rule = Rule {
    id: "SERVING-R029",
    summary: "route-stop pattern times are fractions of the smallest-ID trip's span",
};

pub const RULES: &[Rule] = &[SERVING_R003, SERVING_R028, SERVING_R029];

/// The counts published in diagnostics.json under `route_stop_order`.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Summary {
    pub route_directions: usize,
    pub patterns: usize,
    pub slots: usize,
    pub maximum_patterns_per_route_direction: usize,
}

/// Derives the route-stop table and every call's slot. Pure: returns one
/// [`Edit::SetRouteStops`] that replaces the whole table.
pub fn route_stops(timetable: &Timetable) -> (Edits, Summary) {
    let groups = groups(timetable);
    let merged: Vec<GroupSlots> = groups
        .par_iter()
        .map(|((route, direction), trips)| order_group(timetable, *route, *direction, trips))
        .collect();

    let mut summary = Summary {
        route_directions: merged.len(),
        ..Summary::default()
    };
    let mut rows = Vec::new();
    let mut calls = Vec::new();
    for group in merged {
        summary.patterns += group.patterns;
        summary.maximum_patterns_per_route_direction = summary
            .maximum_patterns_per_route_direction
            .max(group.patterns);
        let first = rows.len();
        let route_id = &timetable.routes[group.route].id;
        let mut visits: BTreeMap<PlaceId, u32> = BTreeMap::new();
        for (position, &place) in group.places.iter().enumerate() {
            let visit = visits.entry(place).or_default();
            *visit += 1;
            rows.push(RouteStop {
                id: identity::route_stop(
                    route_id,
                    group.direction,
                    &timetable.places[place].id,
                    *visit,
                ),
                route: group.route,
                direction: group.direction,
                sequence: i32::try_from(position + 1).expect("fewer than 2^31 slots"),
                place,
            });
        }
        for (trip, slots) in group.trips {
            let mut passenger_slots = slots.into_iter();
            let row = timetable.trips[trip]
                .calls
                .iter()
                .map(|call| {
                    call.passenger.then(|| {
                        let slot = passenger_slots.next().expect("one slot per passenger call");
                        RouteStopId::from_index(first + slot)
                    })
                })
                .collect();
            calls.push((trip, row));
        }
    }
    summary.slots = rows.len();
    calls.sort_by_key(|(trip, _)| *trip);

    let mut edits = EditBuilder::new(timetable, PASS);
    edits.push(
        SERVING_R028,
        Edit::SetRouteStops {
            route_stops: rows,
            calls,
        },
    );
    (edits.finish(), summary)
}

type GroupKey = (RouteId, Option<u8>);

/// Published trips by `(route, direction)`, in output order: route public ID, then direction
/// with "none" first. Trips within a group are in handle order.
fn groups(timetable: &Timetable) -> Vec<(GroupKey, Vec<TripId>)> {
    let mut by_key: BTreeMap<GroupKey, Vec<TripId>> = BTreeMap::new();
    for (handle, trip) in timetable.trips.iter() {
        if timetable.services.get(trip.service).is_empty() {
            continue;
        }
        by_key
            .entry((trip.route, trip.direction))
            .or_default()
            .push(handle);
    }
    let mut groups: Vec<_> = by_key.into_iter().collect();
    groups.sort_by(
        |((left, left_direction), _), ((right, right_direction), _)| {
            let left = timetable.routes[*left].id.as_str();
            let right = timetable.routes[*right].id.as_str();
            left.cmp(right)
                .then_with(|| left_direction.cmp(right_direction))
        },
    );
    groups
}

/// One merged group: its slots' places and each trip's slot per passenger call (indices into
/// `places`).
struct GroupSlots {
    route: RouteId,
    direction: Option<u8>,
    places: Vec<PlaceId>,
    trips: Vec<(TripId, Vec<usize>)>,
    patterns: usize,
}

/// A distinct pattern while collecting: weight and representative trip (SERVING-R029).
struct Collected {
    weight: usize,
    representative: TripId,
}

fn order_group(
    timetable: &Timetable,
    route: RouteId,
    direction: Option<u8>,
    trips: &[TripId],
) -> GroupSlots {
    let trip_id = |trip: TripId| timetable.trips[trip].id.as_str();
    let mut collected: BTreeMap<Vec<PlaceId>, Collected> = BTreeMap::new();
    let mut trip_patterns = Vec::with_capacity(trips.len());
    for &trip in trips {
        let places: Vec<PlaceId> = passenger_calls(&timetable.trips[trip])
            .map(|call| call.place)
            .collect();
        if places.is_empty() {
            continue;
        }
        let entry = collected.entry(places.clone()).or_insert(Collected {
            weight: 0,
            representative: trip,
        });
        entry.weight += 1;
        if trip_id(trip) < trip_id(entry.representative) {
            entry.representative = trip;
        }
        trip_patterns.push((trip, places));
    }

    // Merge order (SERVING-R028): weight descending, length descending, then the places'
    // public IDs element by element (a slice of strings orders exactly so).
    let mut ordered: Vec<(Vec<&str>, Vec<PlaceId>, Collected)> = collected
        .into_iter()
        .map(|(places, entry)| {
            let ids = places
                .iter()
                .map(|&place| timetable.places[place].id.as_str())
                .collect();
            (ids, places, entry)
        })
        .collect();
    ordered.sort_by(
        |(left_ids, left, left_entry), (right_ids, right, right_entry)| {
            right_entry
                .weight
                .cmp(&left_entry.weight)
                .then_with(|| right.len().cmp(&left.len()))
                .then_with(|| left_ids.cmp(right_ids))
        },
    );
    let patterns: Vec<Pattern<PlaceId>> = ordered
        .iter()
        .map(|(_, places, entry)| Pattern {
            places: places.clone(),
            times: relative_times(&timetable.trips[entry.representative]),
        })
        .collect();
    let merged = merge(&patterns);

    let slot_of: BTreeMap<&[PlaceId], &[usize]> = ordered
        .iter()
        .zip(&merged.assigned)
        .map(|((_, places, _), slots)| (places.as_slice(), slots.as_slice()))
        .collect();
    let trips = trip_patterns
        .iter()
        .map(|(trip, places)| (*trip, slot_of[places.as_slice()].to_vec()))
        .collect();
    GroupSlots {
        route,
        direction,
        places: merged.places,
        trips,
        patterns: ordered.len(),
    }
}

fn passenger_calls(trip: &Trip) -> impl Iterator<Item = &jr_model::timetable::Call> {
    trip.calls.iter().filter(|call| call.passenger)
}

/// SERVING-R029: per passenger call, its fraction of the trip's known time span.
fn relative_times(trip: &Trip) -> Vec<Option<f64>> {
    let times: Vec<Option<i32>> = passenger_calls(trip)
        .map(|call| {
            call.departure
                .get()
                .or(call.arrival.get())
                .map(|time| time.seconds())
        })
        .collect();
    let known: Vec<i32> = times.iter().flatten().copied().collect();
    let (Some(&first), Some(&last)) = (known.iter().min(), known.iter().max()) else {
        return vec![None; times.len()];
    };
    if known.len() < 2 || last <= first {
        return vec![None; times.len()];
    }
    let span = f64::from(last - first);
    times
        .into_iter()
        .map(|time| time.map(|time| f64::from(time - first) / span))
        .collect()
}

#[cfg(test)]
mod tests;
