//! GTFS feeds: a regional or foreign feed as it says itself.
//!
//! [`read_zip`] and [`read_files`] return the standard tables as typed rows in file order, every
//! standard column as written: IDs and texts trimmed of surrounding whitespace, enumerations as
//! the raw integer, coordinates as `f64`, dates as [`Date`], times as seconds from the service
//! day's midnight. A missing or empty cell is `None`; nothing is defaulted, repaired or resolved.
//! `stop_times.txt` is stored column-wise ([`StopTimes`]). Other CSV members and verbatim members
//! are returned only when [`Options`] asks for them.
//!
//! # Errors and problems
//!
//! A [`GtfsError`] stops the feed from being read: the bytes are not a ZIP; two members have one
//! name; a required member is missing (`agency.txt`, `routes.txt`, `trips.txt`, `stops.txt`,
//! `stop_times.txt`, and at least one of `calendar.txt` and `calendar_dates.txt`), or sits only in
//! a directory ([`GtfsError::Nested`]); a member read has invalid UTF-8; a read table lacks a
//! required column; `stop_times.txt` has more rows or lines than its compact storage addresses.
//!
//! The required columns are the keys a row cannot be used without:
//!
//! | File | Required columns |
//! |---|---|
//! | `agency.txt` | none (`agency_id` is optional for a single agency) |
//! | `routes.txt` | `route_id` |
//! | `trips.txt` | `route_id`, `service_id`, `trip_id` |
//! | `stops.txt` | `stop_id` |
//! | `stop_times.txt` | `trip_id`, `stop_id` |
//! | `calendar.txt` | `service_id`, `monday` … `sunday`, `start_date`, `end_date` |
//! | `calendar_dates.txt` | `service_id`, `date`, `exception_type` |
//! | `transfers.txt` | `transfer_type` (stops and trips are each conditionally required) |
//! | `feed_info.txt` | none |
//! | `shapes.txt` | `shape_id`, `shape_pt_lat`, `shape_pt_lon`, `shape_pt_sequence` |
//!
//! Everything else is a [`Problem`] next to the feed, and the feed is still read:
//! - a row with another number of fields than the header: the row is **skipped**;
//! - a cell that does not parse (time, date, integer, number): the value is `None`, the row kept;
//! - an empty cell in a required column: the ID is the empty string, the row kept;
//! - an enumeration of `stop_times.txt` outside `-127..=127`: `None`, the row kept;
//! - a `trip_id` or `stop_id` of `stop_times.txt` that `trips.txt` or `stops.txt` does not
//!   have: one problem per distinct ID, at its first row; the rows are kept ([`IdRef::Unknown`]);
//! - a duplicate `trip_id` or `stop_id`: `stop_times.txt` refers to the first; the row is kept;
//! - a duplicate header column: the first is read;
//! - an optional member without even a header line: read as empty;
//! - a feed whose members all sit in one top directory: read from there, said by a problem
//!   naming the directory.
//!
//! Header names and cells are trimmed; column order is free; unknown columns are ignored unless
//! [`Options::extra_columns`] asks for them. A UTF-8 byte order mark is skipped. Quoting is
//! RFC 4180 as the `csv` crate reads it. Member names are matched exactly (GTFS names are lower
//! case). Reading is sequential and the result depends on the bytes alone.

mod members;
mod records;
mod sheet;
mod stop_times;

use std::collections::BTreeMap;
use std::fmt;
use std::io::{Read, Seek};

use jr_model::Date;

pub use records::{
    Agency, Calendar, CalendarDate, FeedInfo, Route, ShapePoint, Shapes, Stop, Transfer, Trip,
};
pub use sheet::Table;
pub use stop_times::{IdRef, StopTime, StopTimes, TripRows};

/// One feed: the standard tables in file order, and what [`Options`] asked for besides.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Feed {
    pub agencies: Vec<Agency>,
    pub routes: Vec<Route>,
    pub trips: Vec<Trip>,
    pub stops: Vec<Stop>,
    pub stop_times: StopTimes,
    /// `calendar.txt`; empty when absent.
    pub calendars: Vec<Calendar>,
    /// `calendar_dates.txt`; empty when absent.
    pub calendar_dates: Vec<CalendarDate>,
    /// `transfers.txt`; empty when absent.
    pub transfers: Vec<Transfer>,
    /// The first row of `feed_info.txt`.
    pub feed_info: Option<FeedInfo>,
    /// `shapes.txt`, when [`Options::shapes`] asked for it and the feed has it.
    pub shapes: Option<Shapes>,
    /// The CSV members [`Options::tables`] asked for and the feed has, as text.
    pub other: BTreeMap<Box<str>, Table>,
    /// The members [`Options::raw`] asked for and the feed has, verbatim.
    pub raw: BTreeMap<Box<str>, Vec<u8>>,
    /// Member names present in the feed (relative to the feed's directory, directories
    /// excluded), sorted.
    pub members: Vec<Box<str>>,
    pub problems: Vec<Problem>,
}

/// What to read beyond the standard tables.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options<'a> {
    /// Columns to keep in a typed row's `extra`, per standard file, in this order (e.g.
    /// `("stops.txt", &["asw_node_id", "asw_stop_id"])`). A column the file lacks is `None` in
    /// every row. Entries for a file without typed rows are ignored; ask for it in `tables`.
    pub extra_columns: &'a [(&'a str, &'a [&'a str])],
    /// Other CSV members to read as text tables (e.g. `route_sub_agencies.txt`).
    pub tables: &'a [&'a str],
    /// Members to return verbatim (e.g. IDS JMK `api.txt`).
    pub raw: &'a [&'a str],
    /// Whether to read `shapes.txt`.
    pub shapes: bool,
}

impl Options<'_> {
    fn extra(&self, file: &str) -> Vec<&str> {
        self.extra_columns
            .iter()
            .filter(|(name, _)| *name == file)
            .flat_map(|(_, columns)| columns.iter().copied())
            .collect()
    }
}

/// Something that did not stop the feed from being read (see the module documentation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// The member's name (`stop_times.txt`), or the feed's directory.
    pub file: Box<str>,
    /// The 1-based line in the file where the row starts; 0 for the file as a whole.
    pub line: u64,
    /// The column, where one applies.
    pub column: Option<Box<str>>,
    pub message: Box<str>,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.file)?;
        if self.line > 0 {
            write!(f, " line {}", self.line)?;
        }
        if let Some(column) = &self.column {
            write!(f, " {column}")?;
        }
        write!(f, ": {}", self.message)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GtfsError {
    #[error("ZIP: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("ZIP member {name}: {source}")]
    Member {
        name: String,
        #[source]
        source: std::io::Error,
    },
    #[error("two members named {0}")]
    DuplicateMember(Box<str>),
    #[error("feed has no {0}")]
    MissingMember(&'static str),
    #[error("feed has neither calendar.txt nor calendar_dates.txt")]
    NoCalendar,
    #[error("feed is nested in directory {directory}")]
    Nested { directory: Box<str> },
    #[error("{file} has no column {column}")]
    MissingColumn {
        file: Box<str>,
        column: &'static str,
    },
    #[error("{file} line {line}: invalid UTF-8")]
    Utf8 { file: Box<str>, line: u64 },
    #[error("{file}: {source}")]
    Csv {
        file: Box<str>,
        #[source]
        source: csv::Error,
    },
    #[error("{file} is too large to address: {what}")]
    TooLarge { file: Box<str>, what: &'static str },
}

/// The standard members read into typed rows.
const STANDARD: [&str; 9] = [
    "agency.txt",
    "routes.txt",
    "trips.txt",
    "stops.txt",
    "stop_times.txt",
    "calendar.txt",
    "calendar_dates.txt",
    "transfers.txt",
    "feed_info.txt",
];
const SHAPES: &str = "shapes.txt";

/// Reads a feed from a ZIP archive. Only the members the feed is read from are decompressed.
pub fn read_zip<R: Read + Seek>(zip: R, options: &Options) -> Result<Feed, GtfsError> {
    let mut archive = zip::ZipArchive::new(zip)?;
    let mut names = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let name = archive
            .name_for_index(index)
            .transpose()?
            .map_or_else(String::new, |name| name.into_owned());
        names.push(name);
    }
    let layout = members::locate(names.iter().map(String::as_str))?;
    let mut contents: BTreeMap<&str, Vec<u8>> = BTreeMap::new();
    for (member, index) in &layout.entries {
        if !wanted(member, options) {
            continue;
        }
        let mut file = archive.by_index(*index)?;
        let mut bytes = Vec::with_capacity(usize::try_from(file.size()).unwrap_or(0));
        if let Err(source) = file.read_to_end(&mut bytes) {
            return Err(GtfsError::Member {
                name: names[*index].clone(),
                source,
            });
        }
        contents.insert(member, bytes);
    }
    let get = |name: &str| contents.get(name).map(Vec::as_slice);
    assemble(&layout, &get, options)
}

/// Reads a feed from named member contents (name, bytes), for files the caller has read. Names
/// may carry a directory part (`/` or `\`), with the same rules as ZIP member names.
pub fn read_files<'a>(
    files: impl IntoIterator<Item = (&'a str, &'a [u8])>,
    options: &Options,
) -> Result<Feed, GtfsError> {
    let files: Vec<(String, &[u8])> = files
        .into_iter()
        .map(|(name, bytes)| (name.replace('\\', "/"), bytes))
        .collect();
    let layout = members::locate(files.iter().map(|(name, _)| name.as_str()))?;
    let contents: BTreeMap<&str, &[u8]> = layout
        .entries
        .iter()
        .map(|(member, index)| (&**member, files[*index].1))
        .collect();
    let get = |name: &str| contents.get(name).copied();
    assemble(&layout, &get, options)
}

fn wanted(member: &str, options: &Options) -> bool {
    STANDARD.contains(&member)
        || (options.shapes && member == SHAPES)
        || options.tables.contains(&member)
        || options.raw.contains(&member)
}

fn assemble<'b>(
    layout: &members::Layout,
    get: &dyn Fn(&str) -> Option<&'b [u8]>,
    options: &Options,
) -> Result<Feed, GtfsError> {
    let mut problems = Vec::new();
    if let Some(directory) = &layout.directory {
        problems.push(Problem {
            file: directory.clone(),
            line: 0,
            column: None,
            message: "every member sits in this directory; the feed is read from it".into(),
        });
    }
    // `members::locate` has checked that the required members exist.
    let required = |name: &'static str| get(name).ok_or(GtfsError::MissingMember(name));
    let open = |name: &'static str, problems: &mut Vec<Problem>| {
        get(name)
            .map(|bytes| sheet::Sheet::open(name, bytes, problems))
            .transpose()
    };

    let mut feed = Feed {
        members: layout
            .entries
            .iter()
            .map(|(name, _)| name.clone())
            .collect(),
        ..Feed::default()
    };
    let p = &mut problems;
    let file = "agency.txt";
    feed.agencies = records::agencies(
        sheet::Sheet::open(file, required(file)?, p)?,
        &options.extra(file),
        p,
    )?;
    let file = "routes.txt";
    feed.routes = records::routes(
        sheet::Sheet::open(file, required(file)?, p)?,
        &options.extra(file),
        p,
    )?;
    let file = "trips.txt";
    feed.trips = records::trips(
        sheet::Sheet::open(file, required(file)?, p)?,
        &options.extra(file),
        p,
    )?;
    let file = "stops.txt";
    feed.stops = records::stops(
        sheet::Sheet::open(file, required(file)?, p)?,
        &options.extra(file),
        p,
    )?;
    let file = "stop_times.txt";
    feed.stop_times = stop_times::decode(
        sheet::Sheet::open(file, required(file)?, p)?,
        &feed.trips,
        &feed.stops,
        &options.extra(file),
        p,
    )?;
    let file = "calendar.txt";
    if let Some(sheet) = open(file, p)? {
        feed.calendars = records::calendars(sheet, &options.extra(file), p)?;
    }
    let file = "calendar_dates.txt";
    if let Some(sheet) = open(file, p)? {
        feed.calendar_dates = records::calendar_dates(sheet, &options.extra(file), p)?;
    }
    let file = "transfers.txt";
    if let Some(sheet) = open(file, p)? {
        feed.transfers = records::transfers(sheet, &options.extra(file), p)?;
    }
    let file = "feed_info.txt";
    if let Some(sheet) = open(file, p)? {
        feed.feed_info = records::feed_info(sheet, &options.extra(file), p)?;
    }
    if options.shapes
        && let Some(sheet) = open(SHAPES, p)?
    {
        feed.shapes = Some(records::shapes(sheet, &options.extra(SHAPES), p)?);
    }
    for &name in options.tables {
        if let Some(bytes) = get(name) {
            let table = sheet::Sheet::open(name, bytes, p)?.table(p)?;
            feed.other.insert(name.into(), table);
        }
    }
    for &name in options.raw {
        if let Some(bytes) = get(name) {
            feed.raw.insert(name.into(), bytes.to_vec());
        }
    }
    feed.problems = problems;
    Ok(feed)
}

/// `YYYYMMDD`.
fn parse_date(text: &str) -> Option<Date> {
    if text.len() != 8 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let year = text[..4].parse().ok()?;
    let month = text[4..6].parse().ok()?;
    let day = text[6..].parse().ok()?;
    Date::new(year, month, day).ok()
}

/// `H:MM:SS` or `HH:MM:SS` (any number of hour digits, hours past 24 allowed) as seconds.
fn parse_time(text: &str) -> Option<i32> {
    let mut parts = text.split(':');
    let (hours, minutes, seconds) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some()
        || hours.is_empty()
        || hours.len() > 6
        || minutes.len() != 2
        || seconds.len() != 2
        || ![hours, minutes, seconds]
            .iter()
            .all(|part| part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    let (hours, minutes, seconds): (i32, i32, i32) = (
        hours.parse().ok()?,
        minutes.parse().ok()?,
        seconds.parse().ok()?,
    );
    (minutes < 60 && seconds < 60).then_some(hours * 3600 + minutes * 60 + seconds)
}

/// A decimal number; `None` for anything else, including `NaN` and infinities.
fn parse_number(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|value| value.is_finite())
}
