//! A batch's trips as the rules walk them: the trip matrices (JDF-R035) and the time-ordered
//! trips (JDF-R068).

use std::collections::{BTreeMap, HashMap};

use jr_formats::jdf::{Km, TimeCell};

use super::policy::Policy;
use crate::batch::{Batch, TripStop};

/// One row of a trip matrix: a route stop in travel order, its stop, and per representative
/// trip the call there.
#[derive(Clone, Debug)]
pub(crate) struct Row {
    /// Position in `batch.stops`.
    pub stop: usize,
    /// Per column, the call (index into `batch.trip_stops`).
    pub calls: Vec<Option<usize>>,
}

/// Rows in travel order; `columns` representative trips.
#[derive(Clone, Debug, Default)]
pub(crate) struct Matrix {
    pub rows: Vec<Row>,
    pub columns: usize,
}

impl Matrix {
    pub(crate) fn reversed(&self) -> Matrix {
        let mut rows = self.rows.clone();
        rows.reverse();
        Matrix {
            rows,
            columns: self.columns,
        }
    }
}

/// A kilometre pattern cell: no call, or a call with or without kilometres.
type KmCell = Option<Option<(u64, u8)>>;

/// A trip's key (R034 verdict: route, distinction, trip).
type TripKey<'a> = (&'a str, u32, u64);

/// A kilometre value compared by value (`1.0` = `1.00`, as F# decimals compare).
pub(crate) fn km_key(km: Km) -> (u64, u8) {
    let (mut units, mut scale) = (km.units, km.scale);
    while scale > 0 && units % 10 == 0 {
        units /= 10;
        scale -= 1;
    }
    (units, scale)
}

/// The position of the first stop with each ID.
pub(crate) fn stop_positions(batch: &Batch) -> HashMap<u64, usize> {
    let mut positions = HashMap::new();
    for (index, stop) in batch.stops.iter().enumerate() {
        positions.entry(stop.id).or_insert(index);
    }
    positions
}

/// Calls grouped by trip in first-appearance order.
fn trips(batch: &Batch) -> Vec<(TripKey<'_>, Vec<usize>)> {
    let mut order: Vec<(TripKey<'_>, Vec<usize>)> = Vec::new();
    let mut index: HashMap<TripKey<'_>, usize> = HashMap::new();
    for (position, call) in batch.trip_stops.iter().enumerate() {
        let key = (&*call.route_id, call.route_distinction, call.trip_id);
        let at = *index.entry(key).or_insert_with(|| {
            order.push((key, Vec::new()));
            order.len() - 1
        });
        order[at].1.push(position);
    }
    order
}

/// R035: matrix 1 (odd trips, forward) and matrix 2 (even trips, in travel order). Route stops
/// are the sorted `Zaslinky` keys (duplicates kept); one representative trip per kilometre
/// pattern; rows without any call dropped; a row's stop is its first call's. A row whose stop
/// is not in `Zastavky` is left out and returned (F# aborted the batch).
pub(crate) fn matrices(
    batch: &Batch,
    positions: &HashMap<u64, usize>,
) -> (Matrix, Matrix, Vec<u64>) {
    let mut keys: Vec<(&str, u32, u64)> = batch
        .route_stops
        .iter()
        .map(|rs| (&*rs.route_id, rs.route_distinction, rs.route_stop_id))
        .collect();
    keys.sort_unstable();
    let trips = trips(batch);
    let mut unknown = Vec::new();
    let mut build = |remainder: u64| {
        let mut seen: std::collections::HashSet<Vec<KmCell>> = std::collections::HashSet::new();
        let mut columns: Vec<Vec<Option<usize>>> = Vec::new();
        for ((route, distinction, trip), calls) in &trips {
            if trip % 2 != remainder {
                continue;
            }
            let mut at: HashMap<(&str, u32, u64), usize> = HashMap::new();
            for &call in calls {
                let c = &batch.trip_stops[call];
                at.insert((route, *distinction, c.route_stop_id), call);
            }
            let array: Vec<Option<usize>> = keys.iter().map(|key| at.get(key).copied()).collect();
            // A missing call and a call without kilometres are different patterns.
            let pattern: Vec<KmCell> = array
                .iter()
                .map(|call| call.map(|c| batch.trip_stops[c].km.map(km_key)))
                .collect();
            if seen.insert(pattern) {
                columns.push(array);
            }
        }
        if remainder == 0 {
            for column in &mut columns {
                column.reverse();
            }
        }
        let mut rows = Vec::new();
        for row in 0..keys.len() {
            let calls: Vec<Option<usize>> = columns.iter().map(|column| column[row]).collect();
            let Some(first) = calls.iter().flatten().next() else {
                continue;
            };
            let stop_id = batch.trip_stops[*first].stop_id;
            match positions.get(&stop_id) {
                Some(&stop) => rows.push(Row { stop, calls }),
                None => {
                    if !unknown.contains(&stop_id) {
                        unknown.push(stop_id);
                    }
                }
            }
        }
        Matrix {
            rows,
            columns: columns.len(),
        }
    };
    let forward = build(1);
    let backward = build(0);
    (forward, backward, unknown)
}

/// The clock time of a call: arrival, else departure, in minutes.
fn clock_minutes(call: &TripStop) -> Option<f64> {
    let at = |cell: &Option<TimeCell>| match cell {
        Some(TimeCell::At { minutes }) => Some(f64::from(*minutes)),
        _ => None,
    };
    at(&call.arrival).or_else(|| at(&call.departure))
}

/// Whether a call has a scheduled time (R033, R073).
pub(crate) fn is_called(call: &TripStop) -> bool {
    clock_minutes(call).is_some()
}

/// R068: every trip's calls in travel order (by route stop, descending for even trips), each
/// with its time after the day rollover; untimed calls stay with no time.
pub(crate) fn ordered_timed_trips(
    batch: &Batch,
    policy: &Policy,
) -> Vec<Vec<(usize, Option<f64>)>> {
    let backstep = policy.file.plausibility.rollover_backstep_min;
    let day = policy.file.plausibility.day_min;
    trips(batch)
        .into_iter()
        .map(|((_, _, trip), mut calls)| {
            let reverse = trip % 2 == 0;
            calls.sort_by_key(|&call| {
                let id = batch.trip_stops[call].route_stop_id as i128;
                if reverse { -id } else { id }
            });
            let mut offset = 0.0;
            let mut previous: Option<f64> = None;
            calls
                .into_iter()
                .map(|call| {
                    let time = clock_minutes(&batch.trip_stops[call]).map(|raw| {
                        let mut value = raw + offset;
                        while previous.is_some_and(|old| value < old - backstep) {
                            offset += day;
                            value = raw + offset;
                        }
                        previous = Some(value);
                        value
                    });
                    (call, time)
                })
                .collect()
        })
        .collect()
}

/// R072: the calls of each trip in file order.
pub(crate) fn trips_in_file_order(batch: &Batch) -> BTreeMap<usize, Vec<usize>> {
    trips(batch)
        .into_iter()
        .enumerate()
        .map(|(index, (_, calls))| (index, calls))
        .collect()
}
