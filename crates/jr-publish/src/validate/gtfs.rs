//! `gtfs.zip`: flat, ordinal entry order, standard columns only; and `stop_times`, `trips`,
//! `calendar` and `calendar_dates` equal the projection of the relations, as multisets of
//! typed rows (times as seconds, dates as dates, blank pickup/drop-off as 0).

use rayon::prelude::*;

use super::keys::{self, ValueHashes};
use super::structure::Tables;
use super::{Findings, code};
use crate::error::PackageError;
use crate::package::{GTFS, Package};
use crate::table::{Column, Table};

/// Columns the projection reads.
pub const COLUMNS: &[(&str, &[&str])] = &[
    (
        "trip_call",
        &[
            "trip_id",
            "sequence",
            "location_id",
            "boarding_point_id",
            "scheduled_arrival",
            "scheduled_departure",
            "pickup_type",
            "dropoff_type",
            "passenger_service",
        ],
    ),
    ("trip", &["trip_id", "route_id", "service_id"]),
    (
        "service_calendar",
        &["service_id", "valid_from", "valid_to", "weekday_mask"],
    ),
    (
        "service_exception",
        &["service_id", "service_date", "added"],
    ),
];

pub fn check_archive(findings: &mut Findings, package: &Package) {
    let names = match package.gtfs_files() {
        Ok(names) => names,
        Err(error) => {
            findings.add(code::GTFS_UNREADABLE, GTFS, error.to_string());
            return;
        }
    };
    if names
        .iter()
        .any(|name| name.contains('/') || name.contains('\\'))
    {
        findings.add(
            code::GTFS_ENTRY_NOT_FLAT,
            GTFS,
            "entries must be flat GTFS table names",
        );
    }
    if !names.is_sorted() {
        findings.add(
            code::GTFS_ENTRY_ORDER,
            GTFS,
            "entries are not in ordinal order",
        );
    }
    let header = package.gtfs_csv("transfers.txt", |mut reader| {
        Ok(reader
            .byte_headers()?
            .iter()
            .any(|column| column == b"max_waiting_time"))
    });
    match header {
        Ok(Some(true)) => findings.add(
            code::GTFS_NONSTANDARD_COLUMN,
            "gtfs.zip/transfers.txt",
            "max_waiting_time is not a standard GTFS column",
        ),
        Ok(_) => {}
        Err(error) => findings.add(
            code::GTFS_UNREADABLE,
            "gtfs.zip/transfers.txt",
            error.to_string(),
        ),
    }
}

/// The most columns a projection reads.
const MAX_COLUMNS: usize = 10;

/// One projected GTFS file: how to hash the serving rows and the GTFS rows.
struct Projection {
    member: &'static str,
    serving: fn(&Tables) -> Option<Vec<u64>>,
    columns: &'static [&'static str],
    gtfs: fn(&[&[u8]]) -> u64,
}

const PROJECTIONS: [Projection; 4] = [
    Projection {
        member: "stop_times.txt",
        serving: stop_times,
        columns: &[
            "trip_id",
            "stop_sequence",
            "stop_id",
            "arrival_time",
            "departure_time",
            "pickup_type",
            "drop_off_type",
        ],
        gtfs: |row| {
            keys::combine([
                keys::text(row[0]),
                integer(row[1]),
                keys::text(row[2]),
                time(row[3]),
                time(row[4]),
                blank_zero(row[5]),
                blank_zero(row[6]),
            ])
        },
    },
    Projection {
        member: "trips.txt",
        serving: |tables| plain(tables.get("trip")?, &["trip_id", "route_id", "service_id"]),
        columns: &["trip_id", "route_id", "service_id"],
        gtfs: |row| keys::combine(row.iter().map(|cell| keys::text(cell))),
    },
    Projection {
        member: "calendar.txt",
        serving: |tables| {
            plain(
                tables.get("service_calendar")?,
                &["service_id", "valid_from", "valid_to", "weekday_mask"],
            )
        },
        columns: &[
            "service_id",
            "start_date",
            "end_date",
            "monday",
            "tuesday",
            "wednesday",
            "thursday",
            "friday",
            "saturday",
            "sunday",
        ],
        gtfs: |row| {
            let mask = row[3..]
                .iter()
                .enumerate()
                .filter(|(_, day)| **day == b"1")
                .map(|(bit, _)| 1 << bit)
                .sum();
            keys::combine([
                keys::text(row[0]),
                date(row[1]),
                date(row[2]),
                keys::int(mask),
            ])
        },
    },
    Projection {
        member: "calendar_dates.txt",
        serving: calendar_dates,
        columns: &["service_id", "date", "exception_type"],
        gtfs: |row| keys::combine([keys::text(row[0]), date(row[1]), integer(row[2])]),
    },
];

pub fn check_projection(findings: &mut Findings, package: &Package, tables: &Tables) {
    let results: Vec<Findings> = PROJECTIONS
        .par_iter()
        .map(|projection| {
            let mut local = Findings::default();
            let subject = format!("{GTFS}/{}", projection.member);
            // The relations were loaded with `COLUMNS`; a missing one was already reported.
            let Some(mut serving) = (projection.serving)(tables) else { return local };
            let mut gtfs = match gtfs_hashes(package, projection) {
                Ok(hashes) => hashes,
                Err(error) => {
                    local.add(code::GTFS_UNREADABLE, subject, error.to_string());
                    return local;
                }
            };
            serving.par_sort_unstable();
            gtfs.par_sort_unstable();
            let (missing, extra) = multiset_difference(&serving, &gtfs);
            if missing + extra > 0 {
                local.add(
                    code::GTFS_PROJECTION_MISMATCH,
                    subject,
                    format!(
                        "{} rows are not the projection of {} serving rows ({missing} projected rows missing, {extra} rows not projected)",
                        gtfs.len(),
                        serving.len()
                    ),
                );
            }
            local
        })
        .collect();
    for local in results {
        findings.extend(local);
    }
}

/// Row hashes of a GTFS member; an absent member has no rows.
fn gtfs_hashes(package: &Package, projection: &Projection) -> Result<Vec<u64>, PackageError> {
    let hashes = package.gtfs_csv(projection.member, |mut reader| {
        let header = reader.byte_headers()?.clone();
        let indexes: Vec<Option<usize>> = projection
            .columns
            .iter()
            .map(|column| header.iter().position(|name| name == column.as_bytes()))
            .collect();
        let mut record = csv::ByteRecord::new();
        let mut hashes = Vec::new();
        while reader.read_byte_record(&mut record)? {
            let mut cells: [&[u8]; MAX_COLUMNS] = [&[]; MAX_COLUMNS];
            for (cell, index) in cells.iter_mut().zip(&indexes) {
                *cell = index.and_then(|i| record.get(i)).unwrap_or_default();
            }
            hashes.push((projection.gtfs)(&cells[..indexes.len()]));
        }
        Ok(hashes)
    })?;
    Ok(hashes.unwrap_or_default())
}

/// Counts of rows only in `serving` and only in `gtfs` (both sorted).
fn multiset_difference(serving: &[u64], gtfs: &[u64]) -> (usize, usize) {
    let (mut i, mut j, mut missing, mut extra) = (0, 0, 0, 0);
    while i < serving.len() && j < gtfs.len() {
        match serving[i].cmp(&gtfs[j]) {
            std::cmp::Ordering::Equal => (i, j) = (i + 1, j + 1),
            std::cmp::Ordering::Less => (i, missing) = (i + 1, missing + 1),
            std::cmp::Ordering::Greater => (j, extra) = (j + 1, extra + 1),
        }
    }
    (missing + serving.len() - i, extra + gtfs.len() - j)
}

/// Row hashes of the named columns, nulls as values.
fn plain(table: &Table, fields: &[&str]) -> Option<Vec<u64>> {
    let columns: Vec<ValueHashes> = fields
        .iter()
        .map(|field| table.column(field).map(ValueHashes::new))
        .collect::<Option<_>>()?;
    Some(
        (0..table.rows())
            .map(|row| keys::combine(columns.iter().map(|column| column.get_or_null(row))))
            .collect(),
    )
}

/// Passenger calls as stop_times: the boarding point, else the location, is the stop.
fn stop_times(tables: &Tables) -> Option<Vec<u64>> {
    let table = tables.get("trip_call")?;
    let column = |name: &str| table.column(name).map(ValueHashes::new);
    let (trip, sequence, location, boarding) = (
        column("trip_id")?,
        column("sequence")?,
        column("location_id")?,
        column("boarding_point_id")?,
    );
    let (arrival, departure, pickup, dropoff) = (
        column("scheduled_arrival")?,
        column("scheduled_departure")?,
        column("pickup_type")?,
        column("dropoff_type")?,
    );
    let Column::Bool(passenger) = table.column("passenger_service")? else {
        return None;
    };
    Some(
        (0..table.rows())
            .filter(|&row| passenger[row] == Some(true))
            .map(|row| {
                keys::combine([
                    trip.get_or_null(row),
                    sequence.get_or_null(row),
                    boarding
                        .get(row)
                        .unwrap_or_else(|| location.get_or_null(row)),
                    arrival.get_or_null(row),
                    departure.get_or_null(row),
                    pickup.get_or_null(row),
                    dropoff.get_or_null(row),
                ])
            })
            .collect(),
    )
}

/// Exceptions as calendar_dates: added is exception type 1, removed 2.
fn calendar_dates(tables: &Tables) -> Option<Vec<u64>> {
    let table = tables.get("service_exception")?;
    let service = ValueHashes::new(table.column("service_id")?);
    let date = ValueHashes::new(table.column("service_date")?);
    let Column::Bool(added) = table.column("added")? else {
        return None;
    };
    Some(
        (0..table.rows())
            .map(|row| {
                let kind = if added[row] == Some(true) { 1 } else { 2 };
                keys::combine([
                    service.get_or_null(row),
                    date.get_or_null(row),
                    keys::int(kind),
                ])
            })
            .collect(),
    )
}

/// A cell that cannot be parsed: never equal to a projected value.
fn unparsable(cell: &[u8]) -> u64 {
    keys::combine([keys::text(b"\0unparsable"), keys::text(cell)])
}

fn parse_int(cell: &[u8]) -> Option<i64> {
    let (negative, digits) = match cell.split_first() {
        Some((b'-', rest)) => (true, rest),
        _ => (false, cell),
    };
    if digits.is_empty() || digits.len() > 12 || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let value = digits
        .iter()
        .fold(0_i64, |value, digit| value * 10 + i64::from(digit - b'0'));
    Some(if negative { -value } else { value })
}

fn integer(cell: &[u8]) -> u64 {
    parse_int(cell).map_or_else(|| unparsable(cell), keys::int)
}

fn blank_zero(cell: &[u8]) -> u64 {
    if cell.is_empty() {
        keys::int(0)
    } else {
        integer(cell)
    }
}

/// `[-]H:MM:SS` as signed seconds; blank is null.
fn time(cell: &[u8]) -> u64 {
    if cell.iter().all(u8::is_ascii_whitespace) {
        return keys::null();
    }
    let (negative, body) = match cell.split_first() {
        Some((b'-', rest)) => (true, rest),
        _ => (false, cell),
    };
    let mut parts = body.split(|&b| b == b':');
    let seconds = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(h), Some(m), Some(s), None) if !h.starts_with(b"-") => {
            match (parse_int(h), parse_int(m), parse_int(s)) {
                (Some(h), Some(m), Some(s)) if m >= 0 && s >= 0 => h * 3600 + m * 60 + s,
                _ => return unparsable(cell),
            }
        }
        _ => return unparsable(cell),
    };
    keys::int(if negative { -seconds } else { seconds })
}

/// `yyyymmdd`.
fn date(cell: &[u8]) -> u64 {
    let parsed = (cell.len() == 8)
        .then(|| parse_int(cell))
        .flatten()
        .and_then(|value| {
            let (year, month, day) = (value / 10_000, value / 100 % 100, value % 100);
            jiff::civil::Date::new(year as i16, month as i8, day as i8).ok()
        });
    parsed.map_or_else(
        || unparsable(cell),
        |d| keys::date(d.year(), d.month(), d.day()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gtfs_times_parse_to_signed_seconds_after_noon_minus_12h() {
        assert_eq!(time(b"25:10:05"), keys::int(25 * 3600 + 10 * 60 + 5));
        assert_eq!(time(b"-01:00:00"), keys::int(-3600));
        // F# read "-00:30:00" as +1800; the sign belongs to the whole value.
        assert_eq!(time(b"-00:30:00"), keys::int(-1800));
        assert_eq!(time(b""), keys::null());
        assert_ne!(time(b"1:2"), keys::int(3720));
    }
}
