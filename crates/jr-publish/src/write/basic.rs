//! `agency`, `route`, `shape`, `shape_point`: direct copies of the published rows.

use jr_model::geo::distance_m;
use jr_model::handle::Handle;
use jr_model::timetable::{Mode, ShapeGeneration, TimetableKind};

use super::PublishError;
use super::assemble::{self, double, int32, text};
use super::scope::Published;
use crate::table::Table;

pub fn agency(p: &Published) -> Result<Table, PublishError> {
    let rows: Vec<_> =
        p.t.agencies
            .iter()
            .filter(|(id, _)| p.agency[id.index()])
            .map(|(_, row)| row)
            .collect();
    assemble::finish(
        "agency",
        vec![
            ("agency_id", text(rows.iter().map(|r| Some(r.id.as_str())))),
            ("name", text(rows.iter().map(|r| Some(&*r.name)))),
            ("url", text(rows.iter().map(|r| r.url.as_deref()))),
            ("timezone", text(rows.iter().map(|r| Some(&*r.timezone)))),
            ("language", text(rows.iter().map(|r| r.language.as_deref()))),
            ("phone", text(rows.iter().map(|r| r.phone.as_deref()))),
            ("fare_url", text(rows.iter().map(|r| r.fare_url.as_deref()))),
            ("email", text(rows.iter().map(|r| r.email.as_deref()))),
        ],
    )
}

pub fn route(p: &Published) -> Result<Table, PublishError> {
    let t = p.t;
    let rows: Vec<_> = t
        .routes
        .iter()
        .filter(|(id, _)| p.route[id.index()])
        .map(|(_, row)| row)
        .collect();
    assemble::finish(
        "route",
        vec![
            ("route_id", text(rows.iter().map(|r| Some(r.id.as_str())))),
            (
                "agency_id",
                text(rows.iter().map(|r| Some(t.agencies[r.agency].id.as_str()))),
            ),
            ("mode", text(rows.iter().map(|r| Some(mode(r.mode))))),
            (
                "gtfs_route_type",
                int32(rows.iter().map(|r| Some(r.gtfs_route_type))),
            ),
            (
                "short_name",
                text(rows.iter().map(|r| r.short_name.as_deref())),
            ),
            (
                "long_name",
                text(rows.iter().map(|r| r.long_name.as_deref())),
            ),
            (
                "description",
                text(rows.iter().map(|r| r.description.as_deref())),
            ),
            ("url", text(rows.iter().map(|r| r.url.as_deref()))),
            ("color", text(rows.iter().map(|r| r.color.as_deref()))),
            (
                "text_color",
                text(rows.iter().map(|r| r.text_color.as_deref())),
            ),
            ("sort_order", int32(rows.iter().map(|r| r.sort_order))),
            (
                "timetable_kind",
                text(rows.iter().map(|r| {
                    Some(match r.timetable_kind {
                        TimetableKind::Regular => "regular",
                        TimetableKind::Detour => "detour",
                    })
                })),
            ),
        ],
    )
}

pub fn mode(mode: Mode) -> &'static str {
    match mode {
        Mode::Bus => "bus",
        Mode::Trolleybus => "trolleybus",
        Mode::Tram => "tram",
        Mode::Metro => "metro",
        Mode::Rail => "rail",
        Mode::Water => "water",
        Mode::Cable => "cable",
        Mode::Funicular => "funicular",
    }
}

pub fn shape(p: &Published) -> Result<Table, PublishError> {
    let rows: Vec<_> =
        p.t.shapes
            .iter()
            .filter(|(id, _)| p.shape[id.index()])
            .map(|(_, row)| row)
            .collect();
    assemble::finish(
        "shape",
        vec![
            ("shape_id", text(rows.iter().map(|r| Some(r.id.as_str())))),
            (
                "generation_method",
                text(rows.iter().map(|r| {
                    Some(match r.generation {
                        ShapeGeneration::Source => "source",
                        ShapeGeneration::Compiler => "compiler",
                        ShapeGeneration::Motis => "motis",
                    })
                })),
            ),
        ],
    )
}

/// Points numbered from 0; `distance_traveled` is the running great-circle distance from the
/// first point.
pub fn shape_point(p: &Published) -> Result<Table, PublishError> {
    let mut rows: Vec<(&str, i32, f64, f64, f64)> = Vec::new();
    for (id, shape) in p.t.shapes.iter() {
        if !p.shape[id.index()] {
            continue;
        }
        let mut travelled = 0.0;
        for (index, point) in shape.points.iter().enumerate() {
            if index > 0 {
                travelled += distance_m(shape.points[index - 1], *point);
            }
            let sequence = i32::try_from(index).map_err(|_| PublishError::OutOfRange {
                relation: "shape_point",
                field: "sequence",
                value: index.to_string(),
            })?;
            rows.push((shape.id.as_str(), sequence, point.lon, point.lat, travelled));
        }
    }
    assemble::finish(
        "shape_point",
        vec![
            ("shape_id", text(rows.iter().map(|r| Some(r.0)))),
            ("sequence", int32(rows.iter().map(|r| Some(r.1)))),
            ("longitude", double(rows.iter().map(|r| Some(r.2)))),
            ("latitude", double(rows.iter().map(|r| Some(r.3)))),
            ("distance_traveled", double(rows.iter().map(|r| Some(r.4)))),
        ],
    )
}
