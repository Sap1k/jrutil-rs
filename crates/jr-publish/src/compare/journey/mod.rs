//! Pairing the trips of two packages whose trip IDs differ for some trips
//! (`TripIdentity::Journey`).
//!
//! 1. Trips with the same `trip_id` on both sides pair by ID.
//! 2. The others pair by content: every trip column but `trip_id` (references to generated
//!    identifiers by what they refer to) and the calls in `sequence` order (every `trip_call`
//!    column but `trip_id`), with the profile's tolerances and labels. Equal contents pair one
//!    to one in `trip_id` order on each side, first come first served.
//! 3. Of what is left, an A and a B trip are a near pair when they have the same `route_id`,
//!    the same service dates, as many calls and the same first call (location and scheduled
//!    departure), and neither has another candidate under that key.
//! 4. Of what is still left, an A and a B trip pair by dates when they have the same
//!    `route_id`, as many calls and the same first call, at least one service date in common,
//!    and each is the other's best partner: among A's candidates under that key B has strictly
//!    the most dates in common with A, and among B's candidates A has strictly the most in
//!    common with B. A tie on either side pairs neither. (Step 3 runs first, so a candidate
//!    with equal dates is taken before this step sees it.)
//! 5. The rest are unpaired, described so that a person can find the journey.
//!
//! Pairs from steps 3 and 4 are compared like pairs by ID: a date difference is
//! `trip.changed.service_id`, other columns and calls as usual.
//!
//! The pairs whose IDs differ rename A's trip IDs to B's wherever a column names a trip
//! (`Pair::renamed`), so every relation compares through the pairing. Only the trips unpaired
//! by ID are ever hashed by content.

mod near;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use rayon::prelude::*;

use super::cells::{CellHasher, Pair, cmp_rows};
use super::profile::Profile;
use super::rows::{Columns, match_rows};
use super::targets::Target;
use super::{Builder, Plan, load_targets, read_tables, target_pairs, typed};
use crate::contract::contract;
use crate::error::PackageError;
use crate::package::Package;
use crate::table::{Column, StrColumn, Table};
use crate::validate::keys;

/// Contract relations and columns the pairing reads.
pub(crate) const TRIP: &str = "trip";
pub(crate) const TRIP_ID: &str = "trip_id";
const TRIP_CALL: &str = "trip_call";
const SEQUENCE: &str = "sequence";
const ROUTE_ID: &str = "route_id";
const SERVICE_ID: &str = "service_id";
const LOCATION_ID: &str = "location_id";
const DEPARTURE: &str = "scheduled_departure";
const ARRIVAL: &str = "scheduled_arrival";

/// A's trip ID -> B's, for the trips paired although their IDs differ. Only looked up.
pub(crate) type Renames = HashMap<Box<str>, Box<str>>;

/// The trip pairing.
pub(crate) struct Journeys {
    pub renames: Renames,
    /// Trip row -> its description, for the trips left unpaired, per side. Only looked up.
    pub unpaired: [HashMap<u32, String>; 2],
    pub counts: TripPairing,
}

/// How many trips each step of `TripIdentity::Journey` paired.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TripPairing {
    pub by_id: u64,
    pub by_content: u64,
    pub near: u64,
    pub by_dates: u64,
    /// Left unpaired in A and in B.
    pub unpaired: [u64; 2],
}

/// `(relation, field)` of every field naming a trip: `trip.trip_id`, every foreign-key field
/// whose target names a trip (so through `trip_call` too), and `Profile::trip_columns`.
pub(super) fn trip_references(profile: &Profile) -> BTreeSet<(&'static str, &str)> {
    let contract = contract();
    let mut refs: BTreeSet<(&str, &str)> = BTreeSet::from([(TRIP, TRIP_ID)]);
    loop {
        let known = refs.len();
        for spec in &contract.relations {
            for key in &spec.foreign_keys {
                for (field, target) in key.fields.iter().zip(&key.target_fields) {
                    if refs.contains(&(key.relation.as_str(), target.as_str())) {
                        refs.insert((spec.name.as_str(), field.as_str()));
                    }
                }
            }
        }
        if refs.len() == known {
            break;
        }
    }
    for (relation, column) in &profile.trip_columns {
        if let Some(spec) = contract.relation(relation)
            && let Some(field) = spec.field(column)
        {
            refs.insert((spec.name.as_str(), field.name.as_str()));
        }
    }
    refs
}

/// Reads `trip` and `trip_call` of both packages and pairs the trips; the tables go to
/// `preread` for their own comparison.
pub(super) fn pair_trips(
    plans: &[Plan<'static>],
    packages: [&Package; 2],
    profile: &Profile,
    targets: &mut BTreeMap<usize, Target>,
    preread: &mut HashMap<&str, [Table; 2]>,
) -> Result<Journeys, PackageError> {
    let find = |name: &str| {
        plans
            .iter()
            .find(|plan| plan.spec.name == name)
            .ok_or_else(|| PackageError::MissingMember {
                path: "serving".into(),
                member: format!("{name} (not in the contract)"),
            })
    };
    let (trip_plan, call_plan) = (find(TRIP)?, find(TRIP_CALL)?);
    load_targets(trip_plan, packages, profile, targets)?;
    load_targets(call_plan, packages, profile, targets)?;
    let (trips, calls) = rayon::join(
        || read_tables(trip_plan, packages),
        || read_tables(call_plan, packages),
    );
    let (trips, calls) = (trips?, calls?);
    let journeys = {
        let targets: &BTreeMap<usize, Target> = targets;
        let trip_targets = target_pairs(trip_plan, targets, packages, profile)?;
        let call_targets = target_pairs(call_plan, targets, packages, profile)?;
        let values = Builder {
            plan: trip_plan,
            packages,
            tables: &trips,
            profile,
            target_pairs: &trip_targets,
            renames: None,
        }
        .columns(&trip_plan.value_fields)?;
        let call_fields: Vec<&'static str> = call_plan
            .key_fields
            .iter()
            .chain(&call_plan.value_fields)
            .copied()
            .filter(|field| *field != TRIP_ID)
            .collect();
        let call_columns = Builder {
            plan: call_plan,
            packages,
            tables: &calls,
            profile,
            target_pairs: &call_targets,
            renames: None,
        }
        .columns(&call_fields)?;
        let (trip_ids, call_trips) = (trip_ids(packages, &trips)?, trip_ids(packages, &calls)?);
        let sequence = |side: usize| typed(packages[side], &calls[side], SEQUENCE, Some);
        pair(&Input {
            ids: trip_ids,
            values: &values,
            call_trips,
            calls: &call_columns,
            sequences: [sequence(0)?, sequence(1)?],
        })
    };
    preread.insert(TRIP, trips);
    preread.insert(TRIP_CALL, calls);
    Ok(journeys)
}

/// The `trip_id` column of both sides' tables.
fn trip_ids<'t>(
    packages: [&Package; 2],
    tables: &'t [Table; 2],
) -> Result<[&'t StrColumn; 2], PackageError> {
    Ok([
        typed(packages[0], &tables[0], TRIP_ID, Column::as_str)?,
        typed(packages[1], &tables[1], TRIP_ID, Column::as_str)?,
    ])
}

struct Input<'a> {
    /// `trip.trip_id`.
    ids: [&'a StrColumn; 2],
    /// Every trip column but `trip_id`.
    values: &'a Columns<'a>,
    /// `trip_call.trip_id`.
    call_trips: [&'a StrColumn; 2],
    /// Every `trip_call` column but `trip_id`.
    calls: &'a Columns<'a>,
    sequences: [&'a Column; 2],
}

/// The calls of some trips: `trip_call` rows in `sequence` order per trip.
struct Calls {
    order: Vec<u32>,
    /// Trip (index into the trips grouped) -> range of `order`.
    ranges: Vec<(u32, u32)>,
}

impl Calls {
    fn of(&self, trip: usize) -> &[u32] {
        let (start, end) = self.ranges[trip];
        &self.order[start as usize..end as usize]
    }
}

fn pair(input: &Input) -> Journeys {
    // 1. By ID. Each side's other trips, in trip_id order.
    let present = |side: usize| -> HashSet<&str> {
        let ids = input.ids[side];
        (0..ids.len()).filter_map(|row| ids.get(row)).collect()
    };
    let unpaired_by_id = |side: usize, other: &HashSet<&str>| -> Vec<u32> {
        let ids = input.ids[side];
        let mut rows: Vec<u32> = (0..ids.len() as u32)
            .filter(|&row| ids.get(row as usize).is_none_or(|id| !other.contains(id)))
            .collect();
        rows.sort_by(|&x, &y| ids.get(x as usize).cmp(&ids.get(y as usize)));
        rows
    };
    let sides = [
        unpaired_by_id(0, &present(1)),
        unpaired_by_id(1, &present(0)),
    ];
    let by_id = (input.ids[0].len() - sides[0].len()) as u64;
    let calls = [0, 1].map(|side| {
        group_calls(
            input.call_trips[side],
            input.sequences[side],
            input.ids[side],
            &sides[side],
        )
    });

    // 2. By content.
    let content = [0, 1].map(|side| {
        let own = hashers(input.values, side);
        let per_call = hashers(input.calls, side);
        let mut hashes: Vec<(u64, u32)> = (0..sides[side].len())
            .into_par_iter()
            .map(|trip| {
                let row = sides[side][trip] as usize;
                let cells = own.iter().map(|hasher| hasher.get(row));
                let trip_calls = calls[side].of(trip).iter().map(|&call| {
                    keys::combine(per_call.iter().map(|hasher| hasher.get(call as usize)))
                });
                (keys::combine(cells.chain(trip_calls)), trip as u32)
            })
            .collect();
        hashes.par_sort_unstable();
        hashes
    });
    let same_content = |ta: u32, tb: u32| {
        let (ra, rb) = (sides[0][ta as usize], sides[1][tb as usize]);
        let (ca, cb) = (calls[0].of(ta as usize), calls[1].of(tb as usize));
        input.values.eq(ra, rb)
            && ca.len() == cb.len()
            && ca.iter().zip(cb).all(|(&p, &q)| input.calls.eq(p, q))
    };
    let matching = match_rows(&content[0], &content[1], same_content);
    drop(content);

    // 3. Near pairs, 4. pairs by dates, each over what the steps before left.
    let left = [matching.missing, matching.extra];
    let near = near::near_pairs(
        input,
        &near::Left {
            sides: &sides,
            calls: &calls,
            trips: &left,
        },
    );
    let left = without(left, &near);
    let by_dates = near::date_pairs(
        input,
        &near::Left {
            sides: &sides,
            calls: &calls,
            trips: &left,
        },
    );
    let left = without(left, &by_dates);

    // 5. What is left.
    let id = |side: usize, trip: u32| input.ids[side].get(sides[side][trip as usize] as usize);
    let mut renames = Renames::new();
    for &(ta, tb) in matching.pairs.iter().chain(&near).chain(&by_dates) {
        if let (Some(a), Some(b)) = (id(0, ta), id(1, tb)) {
            renames.insert(a.into(), b.into());
        }
    }
    let unpaired = [0, 1].map(|side| {
        left[side]
            .iter()
            .map(|&trip| {
                let row = sides[side][trip as usize];
                let text = describe(input, side, row, calls[side].of(trip as usize));
                (row, text)
            })
            .collect::<HashMap<u32, String>>()
    });
    let counts = TripPairing {
        by_id,
        by_content: matching.pairs.len() as u64,
        near: near.len() as u64,
        by_dates: by_dates.len() as u64,
        unpaired: [unpaired[0].len() as u64, unpaired[1].len() as u64],
    };
    Journeys {
        renames,
        unpaired,
        counts,
    }
}

/// Each side's trips without those `pairs` took.
fn without(trips: [Vec<u32>; 2], pairs: &[(u32, u32)]) -> [Vec<u32>; 2] {
    let taken: [HashSet<u32>; 2] = [
        pairs.iter().map(|pair| pair.0).collect(),
        pairs.iter().map(|pair| pair.1).collect(),
    ];
    let [a, b] = trips;
    [(a, 0), (b, 1)].map(|(trips, side)| {
        trips
            .into_iter()
            .filter(|trip| !taken[side].contains(trip))
            .collect()
    })
}

/// The `trip_call` rows of the given trips (trip rows), grouped by trip in `sequence` order.
fn group_calls(
    call_trips: &StrColumn,
    sequences: &Column,
    trip_ids: &StrColumn,
    trips: &[u32],
) -> Calls {
    let mut ranges = vec![(0, 0); trips.len()];
    if trips.is_empty() {
        return Calls {
            order: Vec::new(),
            ranges,
        };
    }
    let index: HashMap<&str, u32> = trips
        .iter()
        .enumerate()
        .filter_map(|(trip, &row)| Some((trip_ids.get(row as usize)?, trip as u32)))
        .collect();
    let trip_of_code: Vec<Option<u32>> = call_trips
        .pool()
        .iter()
        .map(|id| index.get(&**id).copied())
        .collect();
    let trip_of = |row: u32| {
        call_trips
            .code(row as usize)
            .and_then(|code| trip_of_code[code as usize])
    };
    let mut order: Vec<u32> = (0..call_trips.len() as u32)
        .into_par_iter()
        .filter(|&row| trip_of(row).is_some())
        .collect();
    order.par_sort_unstable_by(|&x, &y| {
        trip_of(x)
            .cmp(&trip_of(y))
            .then_with(|| cmp_rows(sequences, x as usize, y as usize))
    });
    let mut start = 0;
    while start < order.len() {
        let trip = trip_of(order[start]);
        let end = start + order[start..].partition_point(|&row| trip_of(row) == trip);
        if let Some(trip) = trip {
            ranges[trip as usize] = (start as u32, end as u32);
        }
        start = end;
    }
    Calls { order, ranges }
}

fn hashers<'c>(columns: &'c Columns, side: usize) -> Vec<CellHasher<'c>> {
    columns.pairs.iter().map(|pair| pair.hasher(side)).collect()
}

fn column<'c, 'a>(columns: &'c Columns<'a>, name: &str) -> Option<&'c Pair<'a>> {
    let at = columns.names.iter().position(|known| *known == name)?;
    Some(&columns.pairs[at])
}

/// `trip_id | route_id | first..last (n days) | first stop@departure → last stop@arrival |
/// n calls`.
fn describe(input: &Input, side: usize, row: u32, calls: &[u32]) -> String {
    let row = row as usize;
    let text = |pair: Option<&Pair>, row: usize| {
        pair.and_then(|pair| pair.text(side, row))
            .unwrap_or_else(|| "null".to_owned())
    };
    let trip = input.ids[side].get(row).unwrap_or("null");
    let route = text(column(input.values, ROUTE_ID), row);
    let dates = column(input.values, SERVICE_ID)
        .and_then(|pair| pair.date_span(side, row))
        .unwrap_or_else(|| "no dates".to_owned());
    let stop = |call: u32, time: &str| {
        let call = call as usize;
        let time = column(input.calls, time).map_or_else(
            || "null".to_owned(),
            |pair| match pair.column(side) {
                Column::Int32(values) => values[call].map_or_else(|| "null".to_owned(), clock),
                _ => text(Some(pair), call),
            },
        );
        format!("{}@{time}", text(column(input.calls, LOCATION_ID), call))
    };
    let journey = match (calls.first(), calls.last()) {
        (Some(&first), Some(&last)) => {
            format!("{} → {}", stop(first, DEPARTURE), stop(last, ARRIVAL))
        }
        _ => "no calls".to_owned(),
    };
    format!(
        "{trip} | {route} | {dates} | {journey} | {} calls",
        calls.len()
    )
}

/// Seconds from midnight as `HH:MM:SS` (past 24 hours like GTFS).
fn clock(seconds: i32) -> String {
    if seconds < 0 {
        return seconds.to_string();
    }
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}
