//! Trip parts of one run (a CZPTT path): consecutive parts share exactly one boundary call,
//! the last of one part and the first of the next, with the same sequence, location and
//! times; a bus part carries no non-passenger calls. A part may be published as several trips
//! running on different days (CZPTT-R120: its IDS zones differ by day); each of them must then
//! share its boundary call with a trip of the previous part.

use std::collections::{BTreeMap, HashMap};

use super::structure::Tables;
use super::{Findings, code};
use crate::table::{Column, StrColumn};

pub const COLUMNS: &[(&str, &[&str])] = &[
    ("route", &["route_id", "mode"]),
    ("trip", &["trip_id", "route_id", "run_key", "run_part"]),
    (
        "trip_call",
        &[
            "trip_id",
            "sequence",
            "location_id",
            "scheduled_arrival",
            "scheduled_departure",
            "passenger_service",
        ],
    ),
];

fn str_column<'a>(tables: &'a Tables, relation: &str, column: &str) -> Option<&'a StrColumn> {
    match tables.get(relation)?.column(column)? {
        Column::Str(values) => Some(values),
        _ => None,
    }
}

fn int32<'a>(tables: &'a Tables, relation: &str, column: &str) -> Option<&'a [Option<i32>]> {
    match tables.get(relation)?.column(column)? {
        Column::Int32(values) => Some(values),
        _ => None,
    }
}

/// Needs `COLUMNS`, which the structure check loaded; without them there is nothing to check.
pub fn check(findings: &mut Findings, tables: &Tables) {
    check_parts(findings, tables);
}

struct Part<'a> {
    run: &'a str,
    number: i16,
    bus: bool,
    first: Option<usize>,
    last: Option<usize>,
}

fn check_parts(findings: &mut Findings, tables: &Tables) -> Option<()> {
    let route_ids = str_column(tables, "route", "route_id")?;
    let modes = str_column(tables, "route", "mode")?;
    let bus_routes: HashMap<&str, bool> = (0..route_ids.len())
        .filter_map(|row| Some((route_ids.get(row)?, modes.get(row)? == "bus")))
        .collect();

    let trip_ids = str_column(tables, "trip", "trip_id")?;
    let trip_routes = str_column(tables, "trip", "route_id")?;
    let run_keys = str_column(tables, "trip", "run_key")?;
    let Column::Int16(run_parts) = tables.get("trip")?.column("run_part")? else {
        return None;
    };
    let mut parts: HashMap<&str, Part> = HashMap::new();
    let mut unnumbered = Vec::new();
    for (row, &number) in run_parts.iter().enumerate() {
        let (Some(trip), Some(run)) = (trip_ids.get(row), run_keys.get(row)) else {
            continue;
        };
        let Some(number) = number else {
            unnumbered.push(trip);
            continue;
        };
        let bus = trip_routes
            .get(row)
            .and_then(|route| bus_routes.get(route))
            .copied()
            .unwrap_or(false);
        parts.insert(
            trip,
            Part {
                run,
                number,
                bus,
                first: None,
                last: None,
            },
        );
    }
    if let Some(example) = unnumbered.first() {
        findings.add(
            code::TRIP_PART_NUMBER_MISSING,
            "serving/trip.parquet",
            format!(
                "{} trips have a run_key but no run_part, e.g. {example}",
                unnumbered.len()
            ),
        );
    }
    if parts.is_empty() {
        return Some(());
    }

    let call_trips = str_column(tables, "trip_call", "trip_id")?;
    let sequences = int32(tables, "trip_call", "sequence")?;
    let locations = str_column(tables, "trip_call", "location_id")?;
    let arrivals = int32(tables, "trip_call", "scheduled_arrival")?;
    let departures = int32(tables, "trip_call", "scheduled_departure")?;
    let Column::Bool(passenger) = tables.get("trip_call")?.column("passenger_service")? else {
        return None;
    };
    let mut bus_calls = 0_u64;
    for row in 0..call_trips.len() {
        let Some(part) = call_trips.get(row).and_then(|trip| parts.get_mut(trip)) else {
            continue;
        };
        let sequence = sequences[row];
        if part.first.is_none_or(|first| sequences[first] > sequence) {
            part.first = Some(row);
        }
        if part.last.is_none_or(|last| sequences[last] < sequence) {
            part.last = Some(row);
        }
        if part.bus && passenger[row] == Some(false) {
            bus_calls += 1;
        }
    }
    if bus_calls > 0 {
        findings.add(
            code::TRIP_PART_BUS_CALLS,
            "serving/trip_call.parquet",
            format!("{bus_calls} non-passenger calls belong to bus trip parts"),
        );
    }

    let mut runs: BTreeMap<&str, Vec<(i16, &str)>> = BTreeMap::new();
    for (trip, part) in &parts {
        runs.entry(part.run).or_default().push((part.number, trip));
    }
    let same_call = |left: usize, right: usize| {
        sequences[left] == sequences[right]
            && locations.code(left) == locations.code(right)
            && arrivals[left] == arrivals[right]
            && departures[left] == departures[right]
    };
    let mut broken = 0_u64;
    let mut example: Option<&str> = None;
    for ordered in runs.values_mut() {
        ordered.sort_unstable();
        for &(number, trip) in ordered.iter() {
            let Some(previous_number) = ordered
                .iter()
                .map(|&(n, _)| n)
                .filter(|&n| n < number)
                .max()
            else {
                continue;
            };
            let next = &parts[trip];
            let shared = ordered
                .iter()
                .filter(|&&(n, _)| n == previous_number)
                .any(|&(_, previous)| {
                    matches!((parts[previous].last, next.first), (Some(l), Some(f)) if same_call(l, f))
                });
            if !shared {
                broken += 1;
                example = Some(example.map_or(trip, |current| current.min(trip)));
            }
        }
    }
    if let Some(example) = example {
        findings.add(
            code::TRIP_PART_BOUNDARY,
            "serving/trip.parquet",
            format!("{broken} trip parts do not share one boundary call with the previous part, e.g. {example}"),
        );
    }
    Some(())
}
