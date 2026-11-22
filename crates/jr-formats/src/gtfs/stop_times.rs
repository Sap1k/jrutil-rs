//! `stop_times.txt`, column-wise: 39 bytes a row for the standard columns, plus 4 for the
//! per-trip order when the file does not already list each trip's rows together and in order.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use super::sheet::Sheet;
use super::{GtfsError, Problem, Stop, Trip};

/// The high bit marks an ID that `trips.txt` or `stops.txt` does not have.
const UNKNOWN: u32 = 1 << 31;
const NO_I32: i32 = i32::MIN;
const NO_I8: i8 = i8::MIN;
const NO_STRING: u32 = u32::MAX;
/// A NaN bit pattern `parse_number` never yields.
const NO_F64: u64 = u64::MAX;

/// What a row's `trip_id` or `stop_id` refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IdRef<'a> {
    /// An index into [`super::Feed::trips`] or [`super::Feed::stops`] (the first row with the
    /// ID, if it is there more than once).
    Index(u32),
    /// An ID that table does not have (possibly empty), as written.
    Unknown(&'a str),
}

/// The rows of `stop_times.txt` in file order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StopTimes {
    trip: Vec<u32>,
    stop: Vec<u32>,
    stop_sequence: Vec<i32>,
    arrival: Vec<i32>,
    departure: Vec<i32>,
    headsign: Vec<u32>,
    pickup: Vec<i8>,
    drop_off: Vec<i8>,
    timepoint: Vec<i8>,
    distance: Vec<u64>,
    line: Vec<u32>,
    /// Per requested extra column, an index into `strings` per row.
    extra: Vec<Vec<u32>>,
    /// Interned headsigns and extra cells.
    strings: Vec<Box<str>>,
    unknown_trips: Vec<Box<str>>,
    unknown_stops: Vec<Box<str>>,
    /// Per distinct trip, by trip code: its rows, as a range of `order` (or of the rows when
    /// `order` is `None`).
    groups: Vec<Group>,
    /// Row indices sorted by (trip, stop_sequence, file order); `None` when that is file order
    /// within every trip and each trip's rows are contiguous.
    order: Option<Vec<u32>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Group {
    trip: u32,
    rows: Range<u32>,
}

/// One row of `stop_times.txt`.
#[derive(Clone, Copy, Debug)]
pub struct StopTime<'a> {
    pub trip: IdRef<'a>,
    pub stop: IdRef<'a>,
    pub stop_sequence: Option<i32>,
    /// Seconds from the service day's midnight.
    pub arrival_time: Option<i32>,
    /// Seconds from the service day's midnight.
    pub departure_time: Option<i32>,
    pub stop_headsign: Option<&'a str>,
    pub pickup_type: Option<i32>,
    pub drop_off_type: Option<i32>,
    pub timepoint: Option<i32>,
    pub shape_dist_traveled: Option<f64>,
    /// The 1-based line where the row starts.
    pub line: u64,
    owner: &'a StopTimes,
    row: usize,
}

impl<'a> StopTime<'a> {
    /// The cell of the `column`-th requested extra column (in [`super::Options`] order).
    pub fn extra(&self, column: usize) -> Option<&'a str> {
        let owner: &'a StopTimes = self.owner;
        owner.string(*owner.extra.get(column)?.get(self.row)?)
    }
}

/// The row indices of one trip, by (stop_sequence, file order); a missing `stop_sequence` sorts
/// first.
#[derive(Clone, Debug)]
pub struct TripRows<'a>(Rows<'a>);

#[derive(Clone, Debug)]
enum Rows<'a> {
    Contiguous(Range<u32>),
    Ordered(std::slice::Iter<'a, u32>),
}

impl Iterator for TripRows<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        match &mut self.0 {
            Rows::Contiguous(range) => range.next(),
            Rows::Ordered(rows) => rows.next().copied(),
        }
        .map(|row| row as usize)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match &self.0 {
            Rows::Contiguous(range) => range.size_hint(),
            Rows::Ordered(rows) => rows.size_hint(),
        }
    }
}

impl ExactSizeIterator for TripRows<'_> {}

impl StopTimes {
    pub fn len(&self) -> usize {
        self.trip.len()
    }

    pub fn is_empty(&self) -> bool {
        self.trip.is_empty()
    }

    /// The row at a file-order index. Panics when `row >= len()`, like slice indexing.
    pub fn get(&self, row: usize) -> StopTime<'_> {
        StopTime {
            trip: id_ref(self.trip[row], &self.unknown_trips),
            stop: id_ref(self.stop[row], &self.unknown_stops),
            stop_sequence: optional_i32(self.stop_sequence[row]),
            arrival_time: optional_i32(self.arrival[row]),
            departure_time: optional_i32(self.departure[row]),
            stop_headsign: self.string(self.headsign[row]),
            pickup_type: optional_i8(self.pickup[row]),
            drop_off_type: optional_i8(self.drop_off[row]),
            timepoint: optional_i8(self.timepoint[row]),
            shape_dist_traveled: (self.distance[row] != NO_F64)
                .then(|| f64::from_bits(self.distance[row])),
            line: u64::from(self.line[row]),
            owner: self,
            row,
        }
    }

    /// Every row in file order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = StopTime<'_>> {
        (0..self.len()).map(|row| self.get(row))
    }

    /// The rows of the trip at an index of [`super::Feed::trips`]; empty when it has none.
    pub fn trip_rows(&self, trip: usize) -> TripRows<'_> {
        let found = u32::try_from(trip)
            .ok()
            .filter(|&trip| trip < UNKNOWN)
            .and_then(|trip| {
                self.groups
                    .binary_search_by_key(&trip, |group| group.trip)
                    .ok()
            });
        match found {
            Some(group) => self.rows_of(&self.groups[group]),
            None => TripRows(Rows::Contiguous(0..0)),
        }
    }

    /// Every trip with rows and its rows: known trips in [`super::Feed::trips`] order, then
    /// unknown `trip_id`s in order of first appearance.
    pub fn trips(&self) -> impl Iterator<Item = (IdRef<'_>, TripRows<'_>)> {
        self.groups
            .iter()
            .map(|group| (id_ref(group.trip, &self.unknown_trips), self.rows_of(group)))
    }

    /// The `trip_id`s rows refer to that `trips.txt` does not have, in order of first appearance.
    pub fn unknown_trip_ids(&self) -> &[Box<str>] {
        &self.unknown_trips
    }

    /// The `stop_id`s rows refer to that `stops.txt` does not have, in order of first appearance.
    pub fn unknown_stop_ids(&self) -> &[Box<str>] {
        &self.unknown_stops
    }

    fn rows_of(&self, group: &Group) -> TripRows<'_> {
        TripRows(match &self.order {
            None => Rows::Contiguous(group.rows.clone()),
            Some(order) => {
                Rows::Ordered(order[group.rows.start as usize..group.rows.end as usize].iter())
            }
        })
    }

    fn string(&self, index: u32) -> Option<&str> {
        (index != NO_STRING).then(|| &*self.strings[index as usize])
    }
}

fn id_ref(code: u32, unknown: &[Box<str>]) -> IdRef<'_> {
    if code & UNKNOWN == 0 {
        IdRef::Index(code)
    } else {
        IdRef::Unknown(&unknown[(code & !UNKNOWN) as usize])
    }
}

fn optional_i32(value: i32) -> Option<i32> {
    (value != NO_I32).then_some(value)
}

fn optional_i8(value: i8) -> Option<i32> {
    (value != NO_I8).then_some(i32::from(value))
}

/// IDs of a table by their first row, with a problem for each repeat.
fn first_index<'t>(
    file: &str,
    column: &str,
    ids: impl Iterator<Item = (&'t str, u64)>,
    problems: &mut Vec<Problem>,
) -> HashMap<&'t str, u32> {
    let mut index: HashMap<&str, (u32, u64)> = HashMap::new();
    for (position, (id, line)) in ids.enumerate() {
        let next = (u32::try_from(position).unwrap_or(u32::MAX), line);
        if let Some(&(_, first)) = index.get(id) {
            problems.push(Problem {
                file: file.into(),
                line,
                column: Some(column.into()),
                message: format!(
                    "duplicate {column} \"{id}\", first at line {first}; stop_times.txt refers to the first"
                )
                .into(),
            });
        } else {
            index.insert(id, next);
        }
    }
    index.into_iter().map(|(id, (at, _))| (id, at)).collect()
}

#[derive(Default)]
struct Interner {
    index: HashMap<Box<str>, u32>,
    strings: Vec<Box<str>>,
}

impl Interner {
    fn intern(&mut self, text: Option<&str>) -> u32 {
        let Some(text) = text else { return NO_STRING };
        if let Some(&at) = self.index.get(text) {
            return at;
        }
        let at = u32::try_from(self.strings.len()).unwrap_or(NO_STRING);
        self.strings.push(text.into());
        self.index.insert(text.into(), at);
        at
    }
}

/// Codes of IDs: known ones by index, unknown ones numbered in order of first appearance with
/// the high bit set (one problem each, at the first row).
struct Resolver<'t> {
    known: HashMap<&'t str, u32>,
    unknown: HashMap<Box<str>, u32>,
    names: Vec<Box<str>>,
}

impl<'t> Resolver<'t> {
    fn new(known: HashMap<&'t str, u32>) -> Self {
        Resolver {
            known,
            unknown: HashMap::new(),
            names: Vec::new(),
        }
    }

    /// The code, and whether the ID is unknown and new.
    fn resolve(&mut self, id: &str) -> (u32, bool) {
        if let Some(&at) = self.known.get(id) {
            return (at, false);
        }
        if let Some(&code) = self.unknown.get(id) {
            return (code, false);
        }
        let code = UNKNOWN | u32::try_from(self.names.len()).unwrap_or(!UNKNOWN);
        self.names.push(id.into());
        self.unknown.insert(id.into(), code);
        (code, true)
    }
}

pub(super) fn decode(
    sheet: Sheet<'_>,
    trips: &[Trip],
    stops: &[Stop],
    extra: &[&str],
    problems: &mut Vec<Problem>,
) -> Result<StopTimes, GtfsError> {
    const FILE: &str = "stop_times.txt";
    sheet.require(&["trip_id", "stop_id"])?;
    let mut trip_ids = Resolver::new(first_index(
        "trips.txt",
        "trip_id",
        trips.iter().map(|trip| (&*trip.trip_id, trip.line)),
        problems,
    ));
    let mut stop_ids = Resolver::new(first_index(
        "stops.txt",
        "stop_id",
        stops.iter().map(|stop| (&*stop.stop_id, stop.line)),
        problems,
    ));
    let [
        trip,
        stop,
        sequence,
        arrival,
        departure,
        headsign,
        pickup,
        drop_off,
        timepoint,
        distance,
    ] = sheet.columns([
        "trip_id",
        "stop_id",
        "stop_sequence",
        "arrival_time",
        "departure_time",
        "stop_headsign",
        "pickup_type",
        "drop_off_type",
        "timepoint",
        "shape_dist_traveled",
    ]);
    let extra_columns = sheet.columns_of(extra);
    let mut out = StopTimes {
        extra: vec![Vec::new(); extra_columns.len()],
        ..StopTimes::default()
    };
    let mut strings = Interner::default();
    let mut too_large = None;
    sheet.rows(problems, |row| {
        if out.trip.len() >= UNKNOWN as usize {
            too_large.get_or_insert("more than 2^31 rows");
            return;
        }
        let Ok(line) = u32::try_from(row.line) else {
            too_large.get_or_insert("a line past 2^32");
            return;
        };
        for (column, resolver, codes, name) in [
            (trip, &mut trip_ids, &mut out.trip, "trip_id"),
            (stop, &mut stop_ids, &mut out.stop, "stop_id"),
        ] {
            let id = row.key(column);
            let (code, new) = resolver.resolve(&id);
            if new {
                row.problem(
                    column,
                    format!("unknown {name} \"{id}\"; this and later rows kept").into(),
                );
            }
            codes.push(code);
        }
        let small = |row: &mut super::sheet::Row<'_>, column| {
            row.parse(column, "an integer in -127..=127", |cell| {
                cell.parse::<i8>().ok().filter(|&value| value != NO_I8)
            })
            .unwrap_or(NO_I8)
        };
        out.stop_sequence.push(
            row.parse(sequence, "an integer", |cell| {
                cell.parse::<i32>().ok().filter(|&value| value != NO_I32)
            })
            .unwrap_or(NO_I32),
        );
        out.arrival.push(row.time(arrival).unwrap_or(NO_I32));
        out.departure.push(row.time(departure).unwrap_or(NO_I32));
        out.headsign.push(strings.intern(row.cell(headsign)));
        out.pickup.push(small(row, pickup));
        out.drop_off.push(small(row, drop_off));
        out.timepoint.push(small(row, timepoint));
        out.distance
            .push(row.number(distance).map_or(NO_F64, f64::to_bits));
        out.line.push(line);
        for (cells, &column) in out.extra.iter_mut().zip(&extra_columns) {
            cells.push(strings.intern(row.cell(column)));
        }
    })?;
    if let Some(what) = too_large {
        return Err(GtfsError::TooLarge {
            file: FILE.into(),
            what,
        });
    }
    out.strings = strings.strings;
    out.unknown_trips = trip_ids.names;
    out.unknown_stops = stop_ids.names;
    (out.groups, out.order) = index(&out.trip, &out.stop_sequence);
    Ok(out)
}

/// The per-trip groups, and the sorted row order unless file order already is it.
fn index(trip: &[u32], sequence: &[i32]) -> (Vec<Group>, Option<Vec<u32>>) {
    let rows = trip.len() as u32;
    let mut seen = HashSet::new();
    let mut groups: Vec<Group> = Vec::new();
    let mut in_order = true;
    for row in 0..rows {
        let at = row as usize;
        match groups.last_mut() {
            Some(group) if group.trip == trip[at] && sequence[at - 1] <= sequence[at] => {
                group.rows.end = row + 1;
            }
            Some(group) if group.trip == trip[at] => {
                in_order = false;
                break;
            }
            _ => {
                if !seen.insert(trip[at]) {
                    in_order = false;
                    break;
                }
                groups.push(Group {
                    trip: trip[at],
                    rows: row..row + 1,
                });
            }
        }
    }
    if in_order {
        groups.sort_unstable_by_key(|group| group.trip);
        return (groups, None);
    }
    let mut order: Vec<u32> = (0..rows).collect();
    order.sort_unstable_by_key(|&row| (trip[row as usize], sequence[row as usize], row));
    let mut groups: Vec<Group> = Vec::new();
    for (position, &row) in order.iter().enumerate() {
        let position = position as u32;
        match groups.last_mut() {
            Some(group) if group.trip == trip[row as usize] => group.rows.end = position + 1,
            _ => groups.push(Group {
                trip: trip[row as usize],
                rows: position..position + 1,
            }),
        }
    }
    (groups, Some(order))
}
