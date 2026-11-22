//! Paths (runs) and the railway points on them.

use std::collections::BTreeMap;

use jr_model::handle::Handle;
use jr_model::timetable::Call;
use jr_model::{PlaceId, Timetable, TripId};

/// The published paths and their railway points, points in resolution order (CZPTT-R098).
pub(super) struct Network<'t> {
    pub points: Vec<Point<'t>>,
    pub paths: Vec<Path<'t>>,
}

/// A place with a country and a source code on at least one published path.
pub(super) struct Point<'t> {
    pub place: PlaceId,
    /// The place's public ID, the subject of its diagnostics.
    pub id: &'t str,
    pub name: &'t str,
    pub country: &'t str,
    pub code: &'t str,
    /// Has a passenger call in a published trip (CZPTT-R099).
    pub passenger: bool,
    /// `(path, position)` of every call at the point.
    pub occurrences: Vec<(usize, usize)>,
}

pub(super) struct Path<'t> {
    /// The run key, or the public ID of a trip without a run: the `paId` of CZPTT-R100.
    pub key: &'t str,
    pub calls: Vec<PathCall>,
}

#[derive(Clone, Copy)]
pub(super) struct PathCall {
    /// Index into [`Network::points`]; `None` for a place that is not a railway point.
    pub point: Option<usize>,
    pub arrival: Option<i32>,
    pub departure: Option<i32>,
    pub passenger: bool,
    pub sequence: i32,
    /// Days of the trip the call belongs to (the earlier part's for a shared boundary).
    pub service_days: usize,
}

impl PathCall {
    /// When the train leaves: departure, else arrival.
    pub fn leaves(self) -> Option<i32> {
        self.departure.or(self.arrival)
    }

    /// When the train is there: arrival, else departure.
    pub fn reaches(self) -> Option<i32> {
        self.arrival.or(self.departure)
    }
}

impl<'t> Network<'t> {
    pub fn build(timetable: &'t Timetable) -> Self {
        // Paths by key; a run's key sorts before an equal trip ID (`false < true`).
        let mut runs: BTreeMap<(&str, bool), Vec<(u16, TripId)>> = BTreeMap::new();
        for (handle, trip) in timetable.trips.iter() {
            if timetable.services.get(trip.service).is_empty() {
                continue;
            }
            match trip.run {
                Some(run) => runs
                    .entry((timetable.strings.get(run.key), false))
                    .or_default()
                    .push((run.part, handle)),
                None => runs
                    .entry((trip.id.as_str(), true))
                    .or_default()
                    .push((0, handle)),
            }
        }

        let mut network = Network {
            points: Vec::new(),
            paths: Vec::with_capacity(runs.len()),
        };
        let mut point_of_place: Vec<Option<usize>> = vec![None; timetable.places.len()];
        for ((key, _), mut parts) in runs {
            parts.sort();
            let mut calls: Vec<(PathCall, PlaceId)> = Vec::new();
            for (_, handle) in parts {
                let trip = &timetable.trips[handle];
                let service_days = timetable.services.get(trip.service).count();
                for (position, call) in trip.calls.iter().enumerate() {
                    let next = path_call(call, service_days);
                    if position == 0
                        && let Some((last, place)) = calls.last_mut()
                        && *place == call.place
                        && last.sequence == call.sequence
                    {
                        // The boundary call two parts share: one occurrence.
                        last.arrival = last.arrival.or(next.arrival);
                        last.departure = next.departure.or(last.departure);
                        last.passenger |= next.passenger;
                        continue;
                    }
                    calls.push((next, call.place));
                }
            }
            let path_index = network.paths.len();
            let mut path = Path {
                key,
                calls: Vec::with_capacity(calls.len()),
            };
            for (position, (mut call, place)) in calls.into_iter().enumerate() {
                call.point = network.point_of(timetable, &mut point_of_place, place);
                if let Some(point) = call.point {
                    let point = &mut network.points[point];
                    point.passenger |= call.passenger;
                    point.occurrences.push((path_index, position));
                }
                path.calls.push(call);
            }
            network.paths.push(path);
        }
        network
    }

    /// The point of a place, created on first appearance; `None` unless the place has both a
    /// country and a source code.
    fn point_of(
        &mut self,
        timetable: &'t Timetable,
        point_of_place: &mut [Option<usize>],
        place: PlaceId,
    ) -> Option<usize> {
        if let Some(point) = point_of_place[place.index()] {
            return Some(point);
        }
        let row = &timetable.places[place];
        let (Some(country), Some(code)) = (row.country.as_deref(), row.source_code.as_deref())
        else {
            return None;
        };
        let point = self.points.len();
        self.points.push(Point {
            place,
            id: row.id.as_str(),
            name: &row.name,
            country,
            code,
            passenger: false,
            occurrences: Vec::new(),
        });
        point_of_place[place.index()] = Some(point);
        Some(point)
    }
}

fn path_call(call: &Call, service_days: usize) -> PathCall {
    PathCall {
        point: None,
        arrival: call.arrival.get().map(|time| time.seconds()),
        departure: call.departure.get().map(|time| time.seconds()),
        passenger: call.passenger,
        sequence: call.sequence,
        service_days,
    }
}
