//! The typed rows of the standard tables other than `stop_times.txt`, and their decoders.
//!
//! Every row carries its 1-based `line` and `extra`: the cells of the columns
//! [`super::Options::extra_columns`] asks for, in that order.

use jr_model::Date;

use super::sheet::Sheet;
use super::{GtfsError, Problem};

type Extra = Box<[Option<Box<str>>]>;

/// `agency.txt`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agency {
    pub agency_id: Option<Box<str>>,
    pub agency_name: Option<Box<str>>,
    pub agency_url: Option<Box<str>>,
    pub agency_timezone: Option<Box<str>>,
    pub agency_lang: Option<Box<str>>,
    pub agency_phone: Option<Box<str>>,
    pub agency_fare_url: Option<Box<str>>,
    pub agency_email: Option<Box<str>>,
    pub line: u64,
    pub extra: Extra,
}

/// `routes.txt`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub route_id: Box<str>,
    pub agency_id: Option<Box<str>>,
    pub route_short_name: Option<Box<str>>,
    pub route_long_name: Option<Box<str>>,
    pub route_desc: Option<Box<str>>,
    pub route_type: Option<i32>,
    pub route_url: Option<Box<str>>,
    pub route_color: Option<Box<str>>,
    pub route_text_color: Option<Box<str>>,
    pub route_sort_order: Option<i32>,
    pub continuous_pickup: Option<i32>,
    pub continuous_drop_off: Option<i32>,
    pub network_id: Option<Box<str>>,
    pub line: u64,
    pub extra: Extra,
}

/// `trips.txt`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trip {
    pub route_id: Box<str>,
    pub service_id: Box<str>,
    pub trip_id: Box<str>,
    pub trip_headsign: Option<Box<str>>,
    pub trip_short_name: Option<Box<str>>,
    pub direction_id: Option<i32>,
    pub block_id: Option<Box<str>>,
    pub shape_id: Option<Box<str>>,
    pub wheelchair_accessible: Option<i32>,
    pub bikes_allowed: Option<i32>,
    pub cars_allowed: Option<i32>,
    pub line: u64,
    pub extra: Extra,
}

/// `stops.txt`.
#[derive(Clone, Debug, PartialEq)]
pub struct Stop {
    pub stop_id: Box<str>,
    pub stop_code: Option<Box<str>>,
    pub stop_name: Option<Box<str>>,
    pub tts_stop_name: Option<Box<str>>,
    pub stop_desc: Option<Box<str>>,
    pub stop_lat: Option<f64>,
    pub stop_lon: Option<f64>,
    pub zone_id: Option<Box<str>>,
    pub stop_url: Option<Box<str>>,
    pub location_type: Option<i32>,
    pub parent_station: Option<Box<str>>,
    pub stop_timezone: Option<Box<str>>,
    pub wheelchair_boarding: Option<i32>,
    pub level_id: Option<Box<str>>,
    pub platform_code: Option<Box<str>>,
    pub line: u64,
    pub extra: Extra,
}

/// `calendar.txt`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Calendar {
    pub service_id: Box<str>,
    /// `monday` … `sunday`, Monday first, as the raw integers.
    pub weekdays: [Option<i32>; 7],
    pub start_date: Option<Date>,
    pub end_date: Option<Date>,
    pub line: u64,
    pub extra: Extra,
}

/// `calendar_dates.txt`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarDate {
    pub service_id: Box<str>,
    pub date: Option<Date>,
    pub exception_type: Option<i32>,
    pub line: u64,
    pub extra: Extra,
}

/// `transfers.txt`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transfer {
    pub from_stop_id: Option<Box<str>>,
    pub to_stop_id: Option<Box<str>>,
    pub from_route_id: Option<Box<str>>,
    pub to_route_id: Option<Box<str>>,
    pub from_trip_id: Option<Box<str>>,
    pub to_trip_id: Option<Box<str>>,
    pub transfer_type: Option<i32>,
    pub min_transfer_time: Option<i32>,
    pub line: u64,
    pub extra: Extra,
}

/// `feed_info.txt` (its first row).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedInfo {
    pub feed_publisher_name: Option<Box<str>>,
    pub feed_publisher_url: Option<Box<str>>,
    pub feed_lang: Option<Box<str>>,
    pub default_lang: Option<Box<str>>,
    pub feed_start_date: Option<Date>,
    pub feed_end_date: Option<Date>,
    pub feed_version: Option<Box<str>>,
    pub feed_contact_email: Option<Box<str>>,
    pub feed_contact_url: Option<Box<str>>,
    pub line: u64,
    pub extra: Extra,
}

/// `shapes.txt`: points in file order, shape IDs interned.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shapes {
    /// Distinct `shape_id`s in order of first appearance; [`ShapePoint::shape`] indexes here.
    pub ids: Vec<Box<str>>,
    pub points: Vec<ShapePoint>,
}

/// One row of `shapes.txt`.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapePoint {
    /// Index into [`Shapes::ids`].
    pub shape: u32,
    pub shape_pt_lat: Option<f64>,
    pub shape_pt_lon: Option<f64>,
    pub shape_pt_sequence: Option<i32>,
    pub shape_dist_traveled: Option<f64>,
    pub line: u64,
    pub extra: Extra,
}

pub(super) fn agencies(
    sheet: Sheet<'_>,
    extra: &[&str],
    problems: &mut Vec<Problem>,
) -> Result<Vec<Agency>, GtfsError> {
    let [id, name, url, timezone, lang, phone, fare_url, email] = sheet.columns([
        "agency_id",
        "agency_name",
        "agency_url",
        "agency_timezone",
        "agency_lang",
        "agency_phone",
        "agency_fare_url",
        "agency_email",
    ]);
    let extra = sheet.columns_of(extra);
    let mut rows = Vec::new();
    sheet.rows(problems, |row| {
        rows.push(Agency {
            agency_id: row.text(id),
            agency_name: row.text(name),
            agency_url: row.text(url),
            agency_timezone: row.text(timezone),
            agency_lang: row.text(lang),
            agency_phone: row.text(phone),
            agency_fare_url: row.text(fare_url),
            agency_email: row.text(email),
            line: row.line,
            extra: row.extras(&extra),
        });
    })?;
    Ok(rows)
}

pub(super) fn routes(
    sheet: Sheet<'_>,
    extra: &[&str],
    problems: &mut Vec<Problem>,
) -> Result<Vec<Route>, GtfsError> {
    sheet.require(&["route_id"])?;
    let [
        id,
        agency,
        short_name,
        long_name,
        desc,
        kind,
        url,
        color,
        text_color,
        sort_order,
        pickup,
        drop_off,
        network,
    ] = sheet.columns([
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
        "continuous_pickup",
        "continuous_drop_off",
        "network_id",
    ]);
    let extra = sheet.columns_of(extra);
    let mut rows = Vec::new();
    sheet.rows(problems, |row| {
        rows.push(Route {
            route_id: row.key(id),
            agency_id: row.text(agency),
            route_short_name: row.text(short_name),
            route_long_name: row.text(long_name),
            route_desc: row.text(desc),
            route_type: row.int(kind),
            route_url: row.text(url),
            route_color: row.text(color),
            route_text_color: row.text(text_color),
            route_sort_order: row.int(sort_order),
            continuous_pickup: row.int(pickup),
            continuous_drop_off: row.int(drop_off),
            network_id: row.text(network),
            line: row.line,
            extra: row.extras(&extra),
        });
    })?;
    Ok(rows)
}

pub(super) fn trips(
    sheet: Sheet<'_>,
    extra: &[&str],
    problems: &mut Vec<Problem>,
) -> Result<Vec<Trip>, GtfsError> {
    sheet.require(&["route_id", "service_id", "trip_id"])?;
    let [
        route,
        service,
        id,
        headsign,
        short_name,
        direction,
        block,
        shape,
        wheelchair,
        bikes,
        cars,
    ] = sheet.columns([
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
        "cars_allowed",
    ]);
    let extra = sheet.columns_of(extra);
    let mut rows = Vec::new();
    sheet.rows(problems, |row| {
        rows.push(Trip {
            route_id: row.key(route),
            service_id: row.key(service),
            trip_id: row.key(id),
            trip_headsign: row.text(headsign),
            trip_short_name: row.text(short_name),
            direction_id: row.int(direction),
            block_id: row.text(block),
            shape_id: row.text(shape),
            wheelchair_accessible: row.int(wheelchair),
            bikes_allowed: row.int(bikes),
            cars_allowed: row.int(cars),
            line: row.line,
            extra: row.extras(&extra),
        });
    })?;
    Ok(rows)
}

pub(super) fn stops(
    sheet: Sheet<'_>,
    extra: &[&str],
    problems: &mut Vec<Problem>,
) -> Result<Vec<Stop>, GtfsError> {
    sheet.require(&["stop_id"])?;
    let [
        id,
        code,
        name,
        tts_name,
        desc,
        lat,
        lon,
        zone,
        url,
        location_type,
        parent,
        timezone,
        wheelchair,
        level,
        platform,
    ] = sheet.columns([
        "stop_id",
        "stop_code",
        "stop_name",
        "tts_stop_name",
        "stop_desc",
        "stop_lat",
        "stop_lon",
        "zone_id",
        "stop_url",
        "location_type",
        "parent_station",
        "stop_timezone",
        "wheelchair_boarding",
        "level_id",
        "platform_code",
    ]);
    let extra = sheet.columns_of(extra);
    let mut rows = Vec::new();
    sheet.rows(problems, |row| {
        rows.push(Stop {
            stop_id: row.key(id),
            stop_code: row.text(code),
            stop_name: row.text(name),
            tts_stop_name: row.text(tts_name),
            stop_desc: row.text(desc),
            stop_lat: row.number(lat),
            stop_lon: row.number(lon),
            zone_id: row.text(zone),
            stop_url: row.text(url),
            location_type: row.int(location_type),
            parent_station: row.text(parent),
            stop_timezone: row.text(timezone),
            wheelchair_boarding: row.int(wheelchair),
            level_id: row.text(level),
            platform_code: row.text(platform),
            line: row.line,
            extra: row.extras(&extra),
        });
    })?;
    Ok(rows)
}

const WEEKDAYS: [&str; 7] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];

pub(super) fn calendars(
    sheet: Sheet<'_>,
    extra: &[&str],
    problems: &mut Vec<Problem>,
) -> Result<Vec<Calendar>, GtfsError> {
    if !sheet.present(problems) {
        return Ok(Vec::new());
    }
    sheet.require(&["service_id"])?;
    sheet.require(&WEEKDAYS)?;
    sheet.require(&["start_date", "end_date"])?;
    let [service, start, end] = sheet.columns(["service_id", "start_date", "end_date"]);
    let weekdays = sheet.columns(WEEKDAYS);
    let extra = sheet.columns_of(extra);
    let mut rows = Vec::new();
    sheet.rows(problems, |row| {
        rows.push(Calendar {
            service_id: row.key(service),
            weekdays: weekdays.map(|day| row.int(day)),
            start_date: row.date(start),
            end_date: row.date(end),
            line: row.line,
            extra: row.extras(&extra),
        });
    })?;
    Ok(rows)
}

pub(super) fn calendar_dates(
    sheet: Sheet<'_>,
    extra: &[&str],
    problems: &mut Vec<Problem>,
) -> Result<Vec<CalendarDate>, GtfsError> {
    if !sheet.present(problems) {
        return Ok(Vec::new());
    }
    sheet.require(&["service_id", "date", "exception_type"])?;
    let [service, date, exception] = sheet.columns(["service_id", "date", "exception_type"]);
    let extra = sheet.columns_of(extra);
    let mut rows = Vec::new();
    sheet.rows(problems, |row| {
        rows.push(CalendarDate {
            service_id: row.key(service),
            date: row.date(date),
            exception_type: row.int(exception),
            line: row.line,
            extra: row.extras(&extra),
        });
    })?;
    Ok(rows)
}

pub(super) fn transfers(
    sheet: Sheet<'_>,
    extra: &[&str],
    problems: &mut Vec<Problem>,
) -> Result<Vec<Transfer>, GtfsError> {
    if !sheet.present(problems) {
        return Ok(Vec::new());
    }
    sheet.require(&["transfer_type"])?;
    let [
        from_stop,
        to_stop,
        from_route,
        to_route,
        from_trip,
        to_trip,
        kind,
        min_time,
    ] = sheet.columns([
        "from_stop_id",
        "to_stop_id",
        "from_route_id",
        "to_route_id",
        "from_trip_id",
        "to_trip_id",
        "transfer_type",
        "min_transfer_time",
    ]);
    let extra = sheet.columns_of(extra);
    let mut rows = Vec::new();
    sheet.rows(problems, |row| {
        rows.push(Transfer {
            from_stop_id: row.text(from_stop),
            to_stop_id: row.text(to_stop),
            from_route_id: row.text(from_route),
            to_route_id: row.text(to_route),
            from_trip_id: row.text(from_trip),
            to_trip_id: row.text(to_trip),
            transfer_type: row.int(kind),
            min_transfer_time: row.int(min_time),
            line: row.line,
            extra: row.extras(&extra),
        });
    })?;
    Ok(rows)
}

pub(super) fn feed_info(
    sheet: Sheet<'_>,
    extra: &[&str],
    problems: &mut Vec<Problem>,
) -> Result<Option<FeedInfo>, GtfsError> {
    if !sheet.present(problems) {
        return Ok(None);
    }
    let [
        name,
        url,
        lang,
        default_lang,
        start,
        end,
        version,
        email,
        contact_url,
    ] = sheet.columns([
        "feed_publisher_name",
        "feed_publisher_url",
        "feed_lang",
        "default_lang",
        "feed_start_date",
        "feed_end_date",
        "feed_version",
        "feed_contact_email",
        "feed_contact_url",
    ]);
    let extra = sheet.columns_of(extra);
    let mut first = None;
    sheet.rows(problems, |row| {
        if first.is_some() {
            return;
        }
        first = Some(FeedInfo {
            feed_publisher_name: row.text(name),
            feed_publisher_url: row.text(url),
            feed_lang: row.text(lang),
            default_lang: row.text(default_lang),
            feed_start_date: row.date(start),
            feed_end_date: row.date(end),
            feed_version: row.text(version),
            feed_contact_email: row.text(email),
            feed_contact_url: row.text(contact_url),
            line: row.line,
            extra: row.extras(&extra),
        });
    })?;
    Ok(first)
}

pub(super) fn shapes(
    sheet: Sheet<'_>,
    extra: &[&str],
    problems: &mut Vec<Problem>,
) -> Result<Shapes, GtfsError> {
    if !sheet.present(problems) {
        return Ok(Shapes::default());
    }
    sheet.require(&[
        "shape_id",
        "shape_pt_lat",
        "shape_pt_lon",
        "shape_pt_sequence",
    ])?;
    let [id, lat, lon, sequence, dist] = sheet.columns([
        "shape_id",
        "shape_pt_lat",
        "shape_pt_lon",
        "shape_pt_sequence",
        "shape_dist_traveled",
    ]);
    let extra = sheet.columns_of(extra);
    let mut shapes = Shapes::default();
    let mut index = std::collections::HashMap::<Box<str>, u32>::new();
    let mut overflow = false;
    sheet.rows(problems, |row| {
        let key = row.key(id);
        let next = shapes.ids.len();
        let shape = *index.entry(key.clone()).or_insert_with(|| {
            shapes.ids.push(key);
            u32::try_from(next).unwrap_or_else(|_| {
                overflow = true;
                u32::MAX
            })
        });
        shapes.points.push(ShapePoint {
            shape,
            shape_pt_lat: row.number(lat),
            shape_pt_lon: row.number(lon),
            shape_pt_sequence: row.int(sequence),
            shape_dist_traveled: row.number(dist),
            line: row.line,
            extra: row.extras(&extra),
        });
    })?;
    if overflow {
        return Err(GtfsError::TooLarge {
            file: "shapes.txt".into(),
            what: "more than 2^32 shapes",
        });
    }
    Ok(shapes)
}
