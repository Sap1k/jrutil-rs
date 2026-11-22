//! `location`: one row per published place and per published boarding point.
//!
//! A boarding point shows its place's name, municipality, district, nearby place and country;
//! its public code, description and accessibility are its own. Without a position of its own
//! it is published at its place's. A place with an estimated position has the configured
//! suffix appended to its name, and so do its boarding points.

use std::borrow::Cow;

use jr_model::timetable::{CoordSource, Place, PlaceKind, Position, Precision, Tristate};
use jr_model::{BoardingPointId, PlaceId};

use super::assemble::{self, double, int16, text};
use super::scope::Published;
use super::{PublishError, PublishOptions};
use crate::table::Table;

enum Row {
    Place(PlaceId),
    Point(BoardingPointId),
}

pub fn location(p: &Published, options: &PublishOptions) -> Result<Table, PublishError> {
    let t = p.t;
    let rows: Vec<Row> = p
        .place_ids()
        .map(Row::Place)
        .chain(p.boarding_point_ids().map(Row::Point))
        .collect();
    let place_of = |row: &Row| -> &Place {
        match *row {
            Row::Place(id) => &t.places[id],
            Row::Point(id) => &t.places[t.boarding_points[id].place],
        }
    };
    let position = |row: &Row| -> Option<Position> {
        match *row {
            Row::Place(id) => t.places[id].position,
            Row::Point(id) => t.boarding_points[id].position.or(place_of(row).position),
        }
    };
    let names: Vec<Cow<str>> = rows
        .iter()
        .map(|row| {
            let place = place_of(row);
            let estimated = place
                .position
                .is_some_and(|position| position.precision == Precision::Estimated);
            if estimated {
                Cow::Owned(format!("{}{}", place.name, options.approximate_suffix))
            } else {
                Cow::Borrowed(&*place.name)
            }
        })
        .collect();
    let own =
        |row: &Row,
         place: fn(&Place) -> Option<&str>,
         point: fn(&jr_model::timetable::BoardingPoint) -> Option<&str>| match *row {
            Row::Place(id) => place(&t.places[id]),
            Row::Point(id) => point(&t.boarding_points[id]),
        };
    let inherited =
        |field: fn(&Place) -> Option<&str>| text(rows.iter().map(|row| field(place_of(row))));
    let domain = match t.feed {
        jr_model::Feed::Czptt => "heavy_rail",
        jr_model::Feed::Jdf => "surface",
    };
    let columns = vec![
        (
            "location_id",
            text(rows.iter().map(|row| {
                Some(match *row {
                    Row::Place(id) => t.places[id].id.as_str(),
                    Row::Point(id) => t.boarding_points[id].id.as_str(),
                })
            })),
        ),
        (
            "kind",
            text(rows.iter().map(|row| {
                Some(match *row {
                    Row::Place(id) => match t.places[id].kind {
                        PlaceKind::StopPlace => "stop_place",
                        PlaceKind::OperationalPoint => "operational_point",
                    },
                    Row::Point(_) => "boarding_point",
                })
            })),
        ),
        ("domain", text(rows.iter().map(|_| Some(domain)))),
        (
            "parent_location_id",
            text(rows.iter().map(|row| match *row {
                Row::Place(_) => None,
                Row::Point(_) => Some(place_of(row).id.as_str()),
            })),
        ),
        ("name", text(names.iter().map(|name| Some(name.as_ref())))),
        (
            "public_code",
            text(
                rows.iter()
                    .map(|row| own(row, |_| None, |b| b.public_code.as_deref())),
            ),
        ),
        (
            "description",
            text(rows.iter().map(|row| {
                own(
                    row,
                    |p| p.description.as_deref(),
                    |b| b.description.as_deref(),
                )
            })),
        ),
        (
            "municipality_name",
            inherited(|p| p.municipality.as_deref()),
        ),
        ("district_name", inherited(|p| p.district_name.as_deref())),
        ("district_code", inherited(|p| p.district_code.as_deref())),
        ("nearby_place", inherited(|p| p.nearby_place.as_deref())),
        ("country_code", inherited(|p| p.country.as_deref())),
        (
            "coordinate_precision",
            text(rows.iter().map(|row| {
                Some(match position(row).map(|position| position.precision) {
                    None => "missing",
                    Some(Precision::Exact) => "exact",
                    Some(Precision::Estimated) => "estimated",
                })
            })),
        ),
        (
            "coordinate_source",
            text(rows.iter().map(|row| {
                position(row)
                    .and_then(|position| position.source)
                    .map(coordinate_source)
            })),
        ),
        (
            "longitude",
            double(rows.iter().map(|row| position(row).map(|p| p.coord.lon))),
        ),
        (
            "latitude",
            double(rows.iter().map(|row| position(row).map(|p| p.coord.lat))),
        ),
        (
            "url",
            text(
                rows.iter()
                    .map(|row| own(row, |p| p.url.as_deref(), |_| None)),
            ),
        ),
        (
            "timezone",
            text(
                rows.iter()
                    .map(|row| own(row, |p| p.timezone.as_deref(), |_| None)),
            ),
        ),
        (
            "wheelchair_boarding",
            int16(rows.iter().map(|row| {
                tristate(match *row {
                    Row::Place(id) => t.places[id].wheelchair_boarding,
                    Row::Point(id) => t.boarding_points[id].wheelchair_boarding,
                })
            })),
        ),
    ];
    assemble::finish("location", columns)
}

pub fn coordinate_source(source: CoordSource) -> &'static str {
    match source {
        CoordSource::Sr70 => "sr70",
        CoordSource::Osm => "osm",
        CoordSource::Gapfill => "gapfill",
        CoordSource::Catalogue => "catalogue",
        CoordSource::Overlay => "overlay",
        CoordSource::RouteTime => "route_time",
        CoordSource::RouteEnd => "route_end",
    }
}

/// GTFS tri-state: unknown is null.
pub fn tristate(value: Tristate) -> Option<i16> {
    match value {
        Tristate::Unknown => None,
        Tristate::Yes => Some(1),
        Tristate::No => Some(2),
    }
}
