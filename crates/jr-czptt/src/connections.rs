//! The `nad_connections` pass (R083): a replacement bus published as its own path connects to
//! the train it replaces part of.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use jr_model::timetable::{Boarding, Call, Entity, LocationRef, Namespace, Transfer, TransferKind};
use jr_model::{Diagnostics, Edit, EditBuilder, Edits, PlaceId, Rule, Sym, Timetable, TripId};

use crate::policy::Policy;

pub const PASS: &str = "nad_connections";

/// SŽ publishes many replacement buses as paths of their own. Their first or last stop meets a
/// train of another path on the same line (or numbered as the train plus 300000) within a
/// short gap on a common day; riders change there. The closest such train per calendar wins.
pub const R083: Rule = Rule {
    id: "CZPTT-R083",
    summary: "A separate replacement bus connects to the nearest train of its line or number at its ends",
};

/// A passenger call of a rail trip at a station where a separate replacement bus starts or ends.
#[derive(Clone, Copy)]
struct Candidate {
    trip: TripId,
    position: usize,
}

struct Context<'a> {
    timetable: &'a Timetable,
    policy: &'a Policy,
    by_line: HashMap<(PlaceId, Sym), Vec<Candidate>>,
    by_number: HashMap<(PlaceId, i64), Vec<Candidate>>,
}

fn passenger_positions(calls: &[Call]) -> impl Iterator<Item = usize> + '_ {
    (0..calls.len()).filter(|&position| calls[position].passenger)
}

fn location(call: &Call) -> LocationRef {
    call.boarding_point
        .map_or(LocationRef::Place(call.place), LocationRef::BoardingPoint)
}

fn reached(call: &Call) -> Option<i32> {
    call.arrival
        .get()
        .or(call.departure.get())
        .map(|t| t.seconds())
}

fn leaves(call: &Call) -> Option<i32> {
    call.departure
        .get()
        .or(call.arrival.get())
        .map(|t| t.seconds())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    /// The train arrives, then the bus departs from its first stop.
    Before,
    /// The bus arrives at its last stop, then the train departs.
    After,
}

impl Context<'_> {
    fn overlap(&self, a: TripId, b: TripId) -> bool {
        let t = self.timetable;
        t.services
            .get(t.trips[a].service)
            .intersects(&t.services.get(t.trips[b].service))
    }

    /// The candidates allowed at `side`, with their gaps.
    fn eligible(
        &self,
        bus: TripId,
        time: i32,
        side: Side,
        candidates: &[Candidate],
    ) -> Vec<(i32, Candidate)> {
        let t = self.timetable;
        let bus_run = t.trips[bus].run.map(|run| run.key);
        candidates
            .iter()
            .filter_map(|candidate| {
                let trip = &t.trips[candidate.trip];
                if bus_run.is_some() && trip.run.map(|run| run.key) == bus_run {
                    return None;
                }
                let call = &trip.calls[candidate.position];
                let mut passengers = passenger_positions(&trip.calls);
                let (allowed, gap) = match side {
                    Side::Before => (
                        passengers.next() != Some(candidate.position)
                            && call.dropoff != Boarding::None,
                        reached(call).map(|arrival| time - arrival),
                    ),
                    Side::After => (
                        passengers.last() != Some(candidate.position)
                            && call.pickup != Boarding::None,
                        leaves(call).map(|departure| departure - time),
                    ),
                };
                let gap = gap.filter(|gap| {
                    allowed && (0..=self.policy.connections.max_gap_seconds).contains(gap)
                })?;
                self.overlap(bus, candidate.trip)
                    .then_some((gap, *candidate))
            })
            .collect()
    }

    /// Per trip its smallest gap; a trip is kept unless another with an equal or smaller gap
    /// runs on one of its days.
    fn closest(&self, eligible: Vec<(i32, Candidate)>) -> Vec<Candidate> {
        let mut best: Vec<(i32, Candidate)> = Vec::new();
        for (gap, candidate) in eligible {
            match best
                .iter_mut()
                .find(|(_, kept)| kept.trip == candidate.trip)
            {
                Some(kept) if kept.0 <= gap => {}
                Some(kept) => *kept = (gap, candidate),
                None => best.push((gap, candidate)),
            }
        }
        let t = self.timetable;
        best.sort_by(|a, b| (a.0, &t.trips[a.1.trip].id).cmp(&(b.0, &t.trips[b.1.trip].id)));
        best.iter()
            .filter(|(gap, candidate)| {
                !best.iter().any(|(other_gap, other)| {
                    other.trip != candidate.trip
                        && other_gap <= gap
                        && self.overlap(candidate.trip, other.trip)
                })
            })
            .map(|(_, candidate)| *candidate)
            .collect()
    }

    fn matches(
        &self,
        bus: TripId,
        number: Option<i64>,
        call: &Call,
        time: i32,
        side: Side,
    ) -> Vec<Candidate> {
        let offset = self.policy.connections.train_number_offset;
        let numbered = number
            .filter(|number| *number >= offset)
            .and_then(|number| self.by_number.get(&(call.place, number - offset)))
            .map(|candidates| self.eligible(bus, time, side, candidates))
            .unwrap_or_default();
        if !numbered.is_empty() {
            return self.closest(numbered);
        }
        let lined = call
            .line
            .and_then(|line| self.by_line.get(&(call.place, line)))
            .map(|candidates| self.eligible(bus, time, side, candidates))
            .unwrap_or_default();
        self.closest(lined)
    }
}

pub fn nad_connections(
    timetable: &Timetable,
    policy: &Policy,
    _diagnostics: &mut Diagnostics,
) -> Edits {
    let bus_type = policy.route_types.replacement_bus;
    let is_bus =
        |trip: TripId| timetable.routes[timetable.trips[trip].route].gtfs_route_type == bus_type;
    let numbers: HashMap<TripId, i64> = timetable
        .keys
        .iter()
        .filter(|key| key.namespace == Namespace::CZPTT_TRAIN_NUMBER)
        .filter_map(|key| match key.entity {
            Entity::Trip(trip) => key.identifier.parse().ok().map(|number| (trip, number)),
            _ => None,
        })
        .collect();
    let mut runs: BTreeMap<Sym, Vec<(u16, TripId)>> = BTreeMap::new();
    for (handle, trip) in timetable.trips.iter() {
        if let Some(run) = trip.run {
            runs.entry(run.key).or_default().push((run.part, handle));
        }
    }
    let mut opens = HashSet::new();
    let mut closes = HashSet::new();
    for parts in runs.values_mut() {
        parts.sort();
        if let (Some(first), Some(last)) = (parts.first(), parts.last()) {
            opens.insert(first.1);
            closes.insert(last.1);
        }
    }

    // Stations where a separate replacement bus starts or ends.
    let mut stations: BTreeSet<PlaceId> = BTreeSet::new();
    for (handle, trip) in timetable.trips.iter().filter(|(handle, _)| is_bus(*handle)) {
        let mut passengers = passenger_positions(&trip.calls);
        let first = passengers.next();
        let last = passengers.last().or(first);
        if let (true, Some(first)) = (opens.contains(&handle), first) {
            stations.insert(trip.calls[first].place);
        }
        if let (true, Some(last)) = (closes.contains(&handle), last) {
            stations.insert(trip.calls[last].place);
        }
    }

    let mut context = Context {
        timetable,
        policy,
        by_line: HashMap::new(),
        by_number: HashMap::new(),
    };
    for (handle, trip) in timetable
        .trips
        .iter()
        .filter(|(handle, _)| !is_bus(*handle))
    {
        for position in passenger_positions(&trip.calls) {
            let call = &trip.calls[position];
            if !stations.contains(&call.place) {
                continue;
            }
            let candidate = Candidate {
                trip: handle,
                position,
            };
            if let Some(line) = call.line {
                context
                    .by_line
                    .entry((call.place, line))
                    .or_default()
                    .push(candidate);
            }
            if let Some(&number) = numbers.get(&handle) {
                context
                    .by_number
                    .entry((call.place, number))
                    .or_default()
                    .push(candidate);
            }
        }
    }

    let mut found: Vec<Transfer> = Vec::new();
    for (bus, trip) in timetable.trips.iter().filter(|(handle, _)| is_bus(*handle)) {
        let mut passengers = passenger_positions(&trip.calls);
        let Some(first) = passengers.next() else {
            continue;
        };
        let last = passengers.last().unwrap_or(first);
        let number = numbers.get(&bus).copied();
        let first_call = &trip.calls[first];
        let last_call = &trip.calls[last];
        if opens.contains(&bus)
            && first_call.pickup != Boarding::None
            && let Some(time) = leaves(first_call)
        {
            for candidate in context.matches(bus, number, first_call, time, Side::Before) {
                let train = &timetable.trips[candidate.trip].calls[candidate.position];
                found.push(timed(
                    candidate.trip,
                    location(train),
                    bus,
                    location(first_call),
                ));
            }
        }
        if closes.contains(&bus)
            && last_call.dropoff != Boarding::None
            && let Some(time) = reached(last_call)
        {
            for candidate in context.matches(bus, number, last_call, time, Side::After) {
                let train = &timetable.trips[candidate.trip].calls[candidate.position];
                found.push(timed(
                    bus,
                    location(last_call),
                    candidate.trip,
                    location(train),
                ));
            }
        }
    }

    let order = |transfer: &Transfer| {
        (
            transfer.from_trip.map(|trip| &timetable.trips[trip].id),
            transfer.to_trip.map(|trip| &timetable.trips[trip].id),
            transfer.from,
            transfer.to,
        )
    };
    found.sort_by(|a, b| order(a).cmp(&order(b)));
    found.dedup();
    let mut edits = EditBuilder::new(timetable, PASS);
    for transfer in found {
        edits.push(R083, Edit::AddTransfer(transfer));
    }
    edits.finish()
}

fn timed(from_trip: TripId, from: LocationRef, to_trip: TripId, to: LocationRef) -> Transfer {
    Transfer {
        from: Some(from),
        to: Some(to),
        from_route: None,
        to_route: None,
        from_trip: Some(from_trip),
        to_trip: Some(to_trip),
        kind: TransferKind::Timed,
        minimum_time: Some(0),
        maximum_wait: None,
    }
}
