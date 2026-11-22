//! `trip` and `trip_call`.

use jr_model::timetable::{Boarding, Call, Trip};

use super::PublishError;
use super::assemble::{self, boolean, double, int16, int32, parallel, text};
use super::calendar::Services;
use super::places::tristate;
use super::scope::Published;
use crate::table::Table;

pub fn trip(p: &Published, services: &Services) -> Result<Table, PublishError> {
    let t = p.t;
    let rows: Vec<&Trip> = p.trips.iter().map(|&id| &t.trips[id]).collect();
    let run_parts = rows
        .iter()
        .map(|trip| {
            trip.run
                .map(|run| {
                    i16::try_from(run.part).map_err(|_| PublishError::OutOfRange {
                        relation: "trip",
                        field: "run_part",
                        value: run.part.to_string(),
                    })
                })
                .transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;
    let sym = |value| t.strings.get_opt(value);
    assemble::finish(
        "trip",
        vec![
            ("trip_id", text(rows.iter().map(|r| Some(r.id.as_str())))),
            (
                "route_id",
                text(rows.iter().map(|r| Some(t.routes[r.route].id.as_str()))),
            ),
            (
                "service_id",
                text(rows.iter().map(|r| services.id(r.service))),
            ),
            (
                "direction",
                int16(rows.iter().map(|r| r.direction.map(i16::from))),
            ),
            ("headsign", text(rows.iter().map(|r| sym(r.headsign)))),
            ("short_name", text(rows.iter().map(|r| sym(r.short_name)))),
            ("block_key", text(rows.iter().map(|r| sym(r.block)))),
            (
                "run_key",
                text(rows.iter().map(|r| r.run.map(|run| t.strings.get(run.key)))),
            ),
            ("run_part", int16(run_parts)),
            (
                "wheelchair_accessible",
                int16(rows.iter().map(|r| tristate(r.wheelchair_accessible))),
            ),
            (
                "bikes_allowed",
                int16(rows.iter().map(|r| tristate(r.bikes_allowed))),
            ),
            (
                "shape_id",
                text(
                    rows.iter()
                        .map(|r| r.shape.as_ref().map(|s| t.shapes[s.shape].id.as_str())),
                ),
            ),
        ],
    )
}

/// GTFS `pickup_type` / `drop_off_type`.
pub fn boarding(value: Boarding) -> i16 {
    match value {
        Boarding::Regular => 0,
        Boarding::None => 1,
        Boarding::PhoneAgency => 2,
        Boarding::AskDriver => 3,
    }
}

/// One row per call of a published trip, trips in ID order: already the primary-key order,
/// since calls are in sequence order within a trip.
pub fn trip_call(p: &Published) -> Result<Table, PublishError> {
    let t = p.t;
    let mut trips: Vec<&Trip> = p.trips.iter().map(|&id| &t.trips[id]).collect();
    trips.sort_by(|a, b| a.id.cmp(&b.id));
    let trips = &trips;
    let calls = move || {
        trips.iter().flat_map(|trip| {
            trip.calls
                .iter()
                .enumerate()
                .map(move |(index, call)| (*trip, index, call))
        })
    };
    let point = |id: Option<jr_model::BoardingPointId>| id.map(|id| &t.boarding_points[id]);
    type Build<'a> = assemble::ColumnBuilder<'a>;
    let each = |f: fn(&Call) -> Option<i32>| -> Build {
        Box::new(move || int32(calls().map(|(_, _, c)| f(c))))
    };
    let builders: Vec<(&'static str, Build)> = vec![
        (
            "trip_id",
            Box::new(move || text(calls().map(|(trip, _, _)| Some(trip.id.as_str())))),
        ),
        ("sequence", each(|c| Some(c.sequence))),
        (
            "location_id",
            Box::new(move || text(calls().map(|(_, _, c)| Some(t.places[c.place].id.as_str())))),
        ),
        (
            "passenger_service",
            Box::new(move || boolean(calls().map(|(_, _, c)| Some(c.passenger)))),
        ),
        (
            "boarding_point_id",
            Box::new(move || {
                text(calls().map(|(_, _, c)| point(c.boarding_point).map(|b| b.id.as_str())))
            }),
        ),
        (
            "route_stop_id",
            Box::new(move || {
                text(calls().map(|(_, _, c)| c.route_stop.map(|s| t.route_stops[s].id.as_str())))
            }),
        ),
        (
            "scheduled_arrival",
            each(|c| c.arrival.get().map(|time| time.seconds())),
        ),
        (
            "scheduled_departure",
            each(|c| c.departure.get().map(|time| time.seconds())),
        ),
        (
            "pickup_type",
            Box::new(move || int16(calls().map(|(_, _, c)| Some(boarding(c.pickup))))),
        ),
        (
            "dropoff_type",
            Box::new(move || int16(calls().map(|(_, _, c)| Some(boarding(c.dropoff))))),
        ),
        (
            "timepoint",
            Box::new(move || boolean(calls().map(|(_, _, c)| Some(c.timepoint)))),
        ),
        (
            "stop_headsign",
            Box::new(move || text(calls().map(|(_, _, c)| t.strings.get_opt(c.stop_headsign)))),
        ),
        (
            "shape_distance_traveled",
            Box::new(move || {
                double(calls().map(|(trip, index, _)| {
                    trip.shape
                        .as_ref()
                        .and_then(|s| s.offsets.get(index).copied().flatten())
                }))
            }),
        ),
        (
            "subsidiary_code",
            Box::new(move || {
                text(calls().map(|(_, _, c)| point(c.track).and_then(|b| b.source_code.as_deref())))
            }),
        ),
        (
            "subsidiary_name",
            Box::new(move || {
                text(calls().map(|(_, _, c)| point(c.track).and_then(|b| b.source_name.as_deref())))
            }),
        ),
        (
            "active_line_code",
            Box::new(move || text(calls().map(|(_, _, c)| t.strings.get_opt(c.line)))),
        ),
        (
            "source_kilometre",
            Box::new(move || double(calls().map(|(_, _, c)| c.kilometre))),
        ),
    ];
    assemble::finish("trip_call", parallel(builders))
}
