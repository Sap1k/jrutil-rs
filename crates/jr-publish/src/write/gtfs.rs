//! `gtfs.zip`: the projection of the serving relations (serving.md section 9), plus
//! `feed_info.txt` from the options.
//!
//! Entries flat and in ordinal name order, Deflate at the fastest level, every entry dated
//! 1980-01-01 00:00:00 and marked as made on Unix; CSV in UTF-8 with LF line ends and every
//! field quoted. Cells: text verbatim, integers in decimal, doubles in their shortest
//! round-trip form, booleans `1`/`0`, dates `yyyymmdd`, times `[-]HH:MM:SS`; null is empty.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use jiff::civil::Date;

use super::{PublishError, PublishOptions};
use crate::table::{Column, Table};

/// Writes one cell of a row into the buffer.
type Cell<'a> = Box<dyn Fn(usize, &mut String) + 'a>;

fn table<'a>(tables: &'a [Table], name: &str) -> Result<&'a Table, PublishError> {
    tables
        .iter()
        .find(|table| table.name == name)
        .ok_or_else(|| PublishError::Contract(format!("no relation {name}")))
}

fn column<'a>(table: &'a Table, name: &str) -> Result<&'a Column, PublishError> {
    table
        .column(name)
        .ok_or_else(|| PublishError::Contract(format!("no column {}.{name}", table.name)))
}

pub fn date_text(date: Date) -> String {
    format!("{:04}{:02}{:02}", date.year(), date.month(), date.day())
}

fn time_text(out: &mut String, seconds: i32) {
    let sign = if seconds < 0 { "-" } else { "" };
    let s = seconds.unsigned_abs();
    let _ = write!(
        out,
        "{sign}{:02}:{:02}:{:02}",
        s / 3600,
        s / 60 % 60,
        s % 60
    );
}

/// A cell copied from a column.
fn copy<'a>(table: &'a Table, name: &str) -> Result<Cell<'a>, PublishError> {
    let column = column(table, name)?;
    Ok(Box::new(move |row, out| match column {
        Column::Str(values) => out.push_str(values.get(row).unwrap_or("")),
        Column::Int16(values) => {
            if let Some(v) = values[row] {
                let _ = write!(out, "{v}");
            }
        }
        Column::Int32(values) => {
            if let Some(v) = values[row] {
                let _ = write!(out, "{v}");
            }
        }
        Column::Double(values) => {
            if let Some(v) = values[row] {
                let _ = write!(out, "{v}");
            }
        }
        Column::Bool(values) => {
            if let Some(v) = values[row] {
                out.push(if v { '1' } else { '0' });
            }
        }
        Column::Date(values) => {
            if let Some(v) = values[row] {
                out.push_str(&date_text(v));
            }
        }
    }))
}

/// A text cell copied from a column, or `fallback` where the column holds none.
fn copy_or<'a>(table: &'a Table, name: &str, fallback: &'a str) -> Result<Cell<'a>, PublishError> {
    let Column::Str(values) = column(table, name)? else {
        return Err(PublishError::Contract(format!("{name} is not text")));
    };
    Ok(Box::new(move |row, out| {
        out.push_str(values.get(row).unwrap_or(fallback));
    }))
}

/// A time column as `[-]HH:MM:SS`.
fn time<'a>(table: &'a Table, name: &str) -> Result<Cell<'a>, PublishError> {
    let Column::Int32(values) = column(table, name)? else {
        return Err(PublishError::Contract(format!("{name} is not int32")));
    };
    Ok(Box::new(move |row, out| {
        if let Some(seconds) = values[row] {
            time_text(out, seconds);
        }
    }))
}

struct Archive {
    path: std::path::PathBuf,
    zip: zip::ZipWriter<BufWriter<File>>,
}

impl Archive {
    fn member(
        &mut self,
        name: &str,
        header: &[&str],
        rows: impl Iterator<Item = usize>,
        cells: &[Cell],
    ) -> Result<(), PublishError> {
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .compression_level(Some(1))
            .last_modified_time(zip::DateTime::default())
            .system(zip::System::Unix)
            .unix_permissions(0o644);
        let path = &self.path;
        self.zip
            .start_file(name, options)
            .map_err(|source| PublishError::Zip {
                path: path.clone(),
                source,
            })?;
        let csv_error = |source| PublishError::Csv {
            path: path.join(name),
            source,
        };
        let mut writer = csv::WriterBuilder::new()
            .quote_style(csv::QuoteStyle::Always)
            .terminator(csv::Terminator::Any(b'\n'))
            .buffer_capacity(1 << 16)
            .from_writer(&mut self.zip);
        writer.write_record(header).map_err(csv_error)?;
        let mut buffer = String::new();
        for row in rows {
            for cell in cells {
                buffer.clear();
                cell(row, &mut buffer);
                writer.write_field(&buffer).map_err(csv_error)?;
            }
            writer.write_record(None::<&[u8]>).map_err(csv_error)?;
        }
        writer
            .flush()
            .map_err(|source| PublishError::io(path.join(name))(source))?;
        Ok(())
    }

    fn rows(
        &mut self,
        name: &str,
        header: &[&str],
        rows: &[Vec<String>],
    ) -> Result<(), PublishError> {
        let cells: Vec<Cell> = (0..header.len())
            .map(|i| -> Cell {
                Box::new(move |row: usize, out: &mut String| out.push_str(&rows[row][i]))
            })
            .collect();
        self.member(name, header, 0..rows.len(), &cells)
    }
}

pub fn write(
    path: &Path,
    tables: &[Table],
    source_version: Option<&str>,
    options: &PublishOptions,
) -> Result<(), PublishError> {
    let file = File::create(path).map_err(PublishError::io(path))?;
    let mut archive = Archive {
        path: path.to_path_buf(),
        zip: zip::ZipWriter::new(BufWriter::new(file)).set_auto_large_file(),
    };
    let all = |t: &Table| 0..t.rows();

    let agency = table(tables, "agency")?;
    archive.member(
        "agency.txt",
        &[
            "agency_id",
            "agency_name",
            "agency_url",
            "agency_timezone",
            "agency_lang",
            "agency_phone",
            "agency_fare_url",
            "agency_email",
        ],
        all(agency),
        &[
            copy(agency, "agency_id")?,
            copy(agency, "name")?,
            // GTFS requires `agency_url`. An operator whose source names no website gets the
            // feed publisher's, where the feed itself is explained; the serving relation keeps
            // the absence (serving verdict on SERVING-R046).
            copy_or(agency, "url", &options.feed_info.publisher_url)?,
            copy(agency, "timezone")?,
            copy(agency, "language")?,
            copy(agency, "phone")?,
            copy(agency, "fare_url")?,
            copy(agency, "email")?,
        ],
    )?;

    let calendar = table(tables, "service_calendar")?;
    let Column::Int16(masks) = column(calendar, "weekday_mask")? else {
        return Err(PublishError::Contract("weekday_mask is not int16".into()));
    };
    let mut calendar_cells = vec![copy(calendar, "service_id")?];
    for day in 0..7 {
        calendar_cells.push(Box::new(move |row: usize, out: &mut String| {
            let set = masks[row].is_some_and(|mask| mask & (1 << day) != 0);
            out.push(if set { '1' } else { '0' });
        }));
    }
    calendar_cells.push(copy(calendar, "valid_from")?);
    calendar_cells.push(copy(calendar, "valid_to")?);
    archive.member(
        "calendar.txt",
        &[
            "service_id",
            "monday",
            "tuesday",
            "wednesday",
            "thursday",
            "friday",
            "saturday",
            "sunday",
            "start_date",
            "end_date",
        ],
        all(calendar),
        &calendar_cells,
    )?;

    let exceptions = table(tables, "service_exception")?;
    let Column::Bool(added) = column(exceptions, "added")? else {
        return Err(PublishError::Contract("added is not bool".into()));
    };
    archive.member(
        "calendar_dates.txt",
        &["service_id", "date", "exception_type"],
        all(exceptions),
        &[
            copy(exceptions, "service_id")?,
            copy(exceptions, "service_date")?,
            Box::new(move |row, out| out.push(if added[row] == Some(true) { '1' } else { '2' })),
        ],
    )?;

    let trips = table(tables, "trip")?;
    if trips.rows() > 0 {
        let Column::Date(starts) = column(calendar, "valid_from")? else {
            return Err(PublishError::Contract("valid_from is not date32".into()));
        };
        let Column::Date(ends) = column(calendar, "valid_to")? else {
            return Err(PublishError::Contract("valid_to is not date32".into()));
        };
        let start = starts.iter().flatten().min().map(|d| date_text(*d));
        let end = ends.iter().flatten().max().map(|d| date_text(*d));
        let info = &options.feed_info;
        archive.rows(
            "feed_info.txt",
            &[
                "feed_publisher_name",
                "feed_publisher_url",
                "feed_lang",
                "feed_start_date",
                "feed_end_date",
                "feed_version",
                "feed_contact_email",
            ],
            &[vec![
                info.publisher_name.clone(),
                info.publisher_url.clone(),
                info.lang.clone(),
                start.unwrap_or_default(),
                end.unwrap_or_default(),
                source_version.unwrap_or_default().to_owned(),
                info.contact_email.clone().unwrap_or_default(),
            ]],
        )?;
    }

    let routes = table(tables, "route")?;
    archive.member(
        "routes.txt",
        &[
            "route_id",
            "agency_id",
            "route_short_name",
            "route_long_name",
            "route_desc",
            "route_type",
            "route_url",
            "route_color",
            "route_text_color",
            "route_sort_order",
        ],
        all(routes),
        &[
            copy(routes, "route_id")?,
            copy(routes, "agency_id")?,
            copy(routes, "short_name")?,
            copy(routes, "long_name")?,
            copy(routes, "description")?,
            copy(routes, "gtfs_route_type")?,
            copy(routes, "url")?,
            copy(routes, "color")?,
            copy(routes, "text_color")?,
            copy(routes, "sort_order")?,
        ],
    )?;

    let points = table(tables, "shape_point")?;
    if points.rows() > 0 {
        archive.member(
            "shapes.txt",
            &[
                "shape_id",
                "shape_pt_lat",
                "shape_pt_lon",
                "shape_pt_sequence",
                "shape_dist_traveled",
            ],
            all(points),
            &[
                copy(points, "shape_id")?,
                copy(points, "latitude")?,
                copy(points, "longitude")?,
                copy(points, "sequence")?,
                copy(points, "distance_traveled")?,
            ],
        )?;
    }

    let calls = table(tables, "trip_call")?;
    let Column::Bool(passenger) = column(calls, "passenger_service")? else {
        return Err(PublishError::Contract(
            "passenger_service is not bool".into(),
        ));
    };
    let Column::Str(boarding) = column(calls, "boarding_point_id")? else {
        return Err(PublishError::Contract(
            "boarding_point_id is not text".into(),
        ));
    };
    let Column::Str(location) = column(calls, "location_id")? else {
        return Err(PublishError::Contract("location_id is not text".into()));
    };
    archive.member(
        "stop_times.txt",
        &[
            "trip_id",
            "arrival_time",
            "departure_time",
            "stop_id",
            "stop_sequence",
            "stop_headsign",
            "pickup_type",
            "drop_off_type",
            "shape_dist_traveled",
            "timepoint",
        ],
        (0..calls.rows()).filter(|&row| passenger[row] == Some(true)),
        &[
            copy(calls, "trip_id")?,
            time(calls, "scheduled_arrival")?,
            time(calls, "scheduled_departure")?,
            Box::new(move |row, out| {
                out.push_str(boarding.get(row).or(location.get(row)).unwrap_or(""));
            }),
            copy(calls, "sequence")?,
            copy(calls, "stop_headsign")?,
            copy(calls, "pickup_type")?,
            copy(calls, "dropoff_type")?,
            copy(calls, "shape_distance_traveled")?,
            copy(calls, "timepoint")?,
        ],
    )?;

    let locations = table(tables, "location")?;
    let Column::Str(kinds) = column(locations, "kind")? else {
        return Err(PublishError::Contract("kind is not text".into()));
    };
    let Column::Str(ids) = column(locations, "location_id")? else {
        return Err(PublishError::Contract("location_id is not text".into()));
    };
    let Column::Str(parents) = column(locations, "parent_location_id")? else {
        return Err(PublishError::Contract(
            "parent_location_id is not text".into(),
        ));
    };
    let stations: HashSet<&str> = (0..parents.len())
        .filter_map(|row| parents.get(row))
        .collect();
    archive.member(
        "stops.txt",
        &[
            "stop_id",
            "stop_name",
            "stop_desc",
            "stop_lat",
            "stop_lon",
            "stop_url",
            "location_type",
            "parent_station",
            "stop_timezone",
            "wheelchair_boarding",
            "platform_code",
        ],
        (0..locations.rows()).filter(|&row| kinds.get(row) != Some("operational_point")),
        &[
            copy(locations, "location_id")?,
            copy(locations, "name")?,
            copy(locations, "description")?,
            copy(locations, "latitude")?,
            copy(locations, "longitude")?,
            copy(locations, "url")?,
            Box::new(move |row, out| {
                let station = kinds.get(row) == Some("stop_place")
                    && ids.get(row).is_some_and(|id| stations.contains(id));
                out.push(if station { '1' } else { '0' });
            }),
            copy(locations, "parent_location_id")?,
            copy(locations, "timezone")?,
            copy(locations, "wheelchair_boarding")?,
            copy(locations, "public_code")?,
        ],
    )?;

    let transfers = table(tables, "transfer")?;
    if transfers.rows() > 0 {
        let rows = transfer_rows(transfers, options.gtfs_timed_transfers_as_stop_pairs)?;
        archive.rows(
            "transfers.txt",
            &[
                "from_stop_id",
                "to_stop_id",
                "from_route_id",
                "to_route_id",
                "from_trip_id",
                "to_trip_id",
                "transfer_type",
                "min_transfer_time",
            ],
            &rows,
        )?;
    }

    archive.member(
        "trips.txt",
        &[
            "route_id",
            "service_id",
            "trip_id",
            "trip_headsign",
            "trip_short_name",
            "direction_id",
            "block_id",
            "shape_id",
            "wheelchair_accessible",
            "bikes_allowed",
        ],
        all(trips),
        &[
            copy(trips, "route_id")?,
            copy(trips, "service_id")?,
            copy(trips, "trip_id")?,
            copy(trips, "headsign")?,
            copy(trips, "short_name")?,
            copy(trips, "direction")?,
            copy(trips, "block_key")?,
            copy(trips, "shape_id")?,
            copy(trips, "wheelchair_accessible")?,
            copy(trips, "bikes_allowed")?,
        ],
    )?;

    let Archive { path, zip } = archive;
    let mut inner = zip.finish().map_err(|source| PublishError::Zip {
        path: path.clone(),
        source,
    })?;
    inner.flush().map_err(PublishError::io(&path))?;
    Ok(())
}

/// `transfers.txt` rows. With `stop_pairs`, a timed transfer between two locations becomes a
/// type-2, zero-minute rule between them without routes and trips; rows are then distinct,
/// first occurrence kept.
fn transfer_rows(transfers: &Table, stop_pairs: bool) -> Result<Vec<Vec<String>>, PublishError> {
    let names = [
        "from_location_id",
        "to_location_id",
        "from_route_id",
        "to_route_id",
        "from_trip_id",
        "to_trip_id",
        "transfer_type",
        "minimum_transfer_time",
    ];
    let cells: Vec<Cell> = names
        .iter()
        .map(|name| copy(transfers, name))
        .collect::<Result<_, _>>()?;
    let mut rows = Vec::with_capacity(transfers.rows());
    let mut seen = HashSet::new();
    for row in 0..transfers.rows() {
        let mut values: Vec<String> = cells
            .iter()
            .map(|cell| {
                let mut text = String::new();
                cell(row, &mut text);
                text
            })
            .collect();
        if stop_pairs && values[6] == "1" && !values[0].is_empty() && !values[1].is_empty() {
            for value in &mut values[2..6] {
                value.clear();
            }
            values[6] = "2".to_owned();
            values[7] = "0".to_owned();
        }
        if !stop_pairs || seen.insert(values.clone()) {
            rows.push(values);
        }
    }
    Ok(rows)
}
