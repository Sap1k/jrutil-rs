//! Small fixtures for stop matching: a batch from stops and trips, and tiny evidence (okres
//! squares, a few municipalities, catalogue rows and OSM nodes).

#![allow(dead_code)]

use jiff::civil::date;
use jr_formats::geodata::{catalogue, gazetteer, synonyms};
use jr_formats::jdf::{Header, Km, TimeCell, Version};
use jr_formats::osm::Node;
use jr_geo::laea;
use jr_jdf::batch::{Batch, Route, RouteStop, Stop, TripStop};
use jr_jdf::fix::{Evidence, EvidenceInputs, Policy};
use jr_jdf::fixed::FixedBatch;
use jr_model::{Coord, Diagnostics};

pub const SYNONYMS: &str = include_str!("../../../../data/names/stop_synonyms.txt");

pub fn policy() -> Policy {
    Policy::builtin()
}

pub fn codes(diagnostics: &Diagnostics) -> Vec<&str> {
    diagnostics.iter().map(|d| d.code.as_ref()).collect()
}

// --- batches ---

pub fn stop(id: u64, town: &str, district: Option<&str>, nearby: Option<&str>) -> Stop {
    Stop {
        id,
        town: town.into(),
        district: district.map(Into::into),
        nearby_place: nearby.map(Into::into),
        okres: None,
        country: None,
        attributes: [None; 6],
    }
}

pub fn located(mut stop: Stop, okres: Option<&str>, country: Option<&str>) -> Stop {
    stop.okres = okres.map(Into::into);
    stop.country = country.map(Into::into);
    stop
}

/// A call of a trip: stop, minute after 08:00 (`None`: passes without a time), kilometre.
#[derive(Clone, Copy, Debug)]
pub struct Call {
    pub stop: u64,
    pub minute: Option<u16>,
    pub km: Option<f64>,
}

pub fn at(stop: u64, minute: u16) -> Call {
    Call {
        stop,
        minute: Some(minute),
        km: None,
    }
}

pub fn at_km(stop: u64, minute: u16, km: f64) -> Call {
    Call {
        stop,
        minute: Some(minute),
        km: Some(km),
    }
}

pub fn passes(stop: u64) -> Call {
    Call {
        stop,
        minute: None,
        km: None,
    }
}

/// A one-route batch; each trip lists its calls in route-stop order (route stops 1, 2, …).
pub fn batch(name: &str, stops: Vec<Stop>, trips: &[(u64, Vec<Call>)]) -> Batch {
    let route_stop_count = trips
        .iter()
        .map(|(_, calls)| calls.len())
        .max()
        .unwrap_or(0);
    let mut trip_stops = Vec::new();
    for (trip, calls) in trips {
        for (index, call) in calls.iter().enumerate() {
            let time = match call.minute {
                Some(minute) => TimeCell::At {
                    minutes: 8 * 60 + minute,
                },
                None => TimeCell::Passes,
            };
            trip_stops.push(TripStop {
                route_id: "100001".into(),
                trip_id: *trip,
                route_stop_id: index as u64 + 1,
                stop_id: call.stop,
                stop_post_id: None,
                stop_post_number: None,
                attributes: [None; 3],
                km: call.km.map(|km| Km {
                    units: (km * 10.0).round() as u64,
                    scale: 1,
                }),
                arrival: Some(time),
                departure: Some(time),
                earliest_arrival: None,
                latest_departure: None,
                route_distinction: 1,
            });
        }
    }
    Batch {
        name: name.into(),
        version: Version::V1_11,
        header: Header {
            version: "1.11".into(),
            du_number: None,
            region: None,
            batch_id: None,
            created: None,
            generator: None,
        },
        stops,
        stop_posts: Vec::new(),
        agencies: Vec::new(),
        routes: vec![Route {
            id: "100001".into(),
            name: "Test line".into(),
            agency_id: "12345678".into(),
            route_type: None,
            transport_mode: None,
            detour: false,
            grouped: false,
            uses_stop_posts: None,
            one_way: None,
            reserved1: None,
            licence_number: None,
            licence_valid_from: None,
            licence_valid_to: None,
            timetable_valid_from: date(2025, 12, 14),
            timetable_valid_to: None,
            agency_distinction: 1,
            distinction: 1,
        }],
        route_integrations: Vec::new(),
        route_stops: (1..=route_stop_count as u64)
            .map(|id| RouteStop {
                route_id: "100001".into(),
                route_stop_id: id,
                zone: None,
                stop_id: 0,
                approximate_minutes: None,
                attributes: [None; 3],
                route_distinction: 1,
            })
            .collect(),
        trips: Vec::new(),
        trip_groups: Vec::new(),
        trip_stops,
        route_info: Vec::new(),
        attribute_refs: Vec::new(),
        service_notes: Vec::new(),
        transfers: Vec::new(),
        agency_alternations: Vec::new(),
        alternate_route_names: Vec::new(),
        reservations: Vec::new(),
    }
}

// --- evidence ---

pub fn rectangle(west: f64, south: f64, east: f64, north: f64) -> Vec<Coord> {
    vec![
        Coord::new(west, south),
        Coord::new(east, south),
        Coord::new(east, north),
        Coord::new(west, north),
        Coord::new(west, south),
    ]
}

pub fn okres(code: &str, polygons: Vec<Vec<Coord>>) -> gazetteer::Okres {
    gazetteer::Okres {
        code: code.into(),
        name: None,
        polygons: polygons
            .into_iter()
            .map(|exterior| gazetteer::Polygon {
                exterior,
                holes: Vec::new(),
            })
            .collect(),
    }
}

/// `AA` and `BB` share the meridian 14.5; `CC` is far east; `OC` (Olomouc) further; `LT` has two
/// polygons, the first adjacent to `UL`.
pub fn okresy() -> Vec<gazetteer::Okres> {
    vec![
        okres("AA", vec![rectangle(14.0, 50.0, 14.5, 50.3)]),
        okres("BB", vec![rectangle(14.5, 50.0, 15.0, 50.3)]),
        okres("CC", vec![rectangle(16.0, 50.0, 16.5, 50.3)]),
        okres("OC", vec![rectangle(17.0, 49.5, 17.5, 49.8)]),
        okres(
            "LT",
            vec![
                rectangle(13.0, 50.5, 13.3, 50.7),
                rectangle(12.0, 48.0, 12.1, 48.1),
            ],
        ),
        okres("UL", vec![rectangle(13.3, 50.5, 13.6, 50.7)]),
    ]
}

pub fn town(name: &str, okres: &str, area: Vec<Coord>, feature: usize) -> gazetteer::Town {
    let ring = area
        .into_iter()
        .map(|coord| {
            let projected = laea::from_wgs84(coord);
            gazetteer::Laea {
                easting: projected.easting,
                northing: projected.northing,
            }
        })
        .collect();
    gazetteer::Town {
        name: name.into(),
        okres: Some(okres.into()),
        polygons: vec![gazetteer::Polygon {
            exterior: ring,
            holes: Vec::new(),
        }],
        feature,
    }
}

pub fn towns() -> Vec<gazetteer::Town> {
    vec![
        town("Lhota", "AA", rectangle(14.1, 50.1, 14.2, 50.2), 0),
        town("Adamov", "BB", rectangle(14.7, 50.1, 14.8, 50.2), 1),
        town("Praha", "AA", rectangle(14.3, 50.0, 14.45, 50.1), 2),
        town("Most", "CC", rectangle(16.1, 50.1, 16.2, 50.2), 3),
    ]
}

pub fn foreign(name: &str, country: &str) -> gazetteer::ForeignTown {
    gazetteer::ForeignTown {
        name: name.into(),
        country: country.into(),
        latitude: None,
        longitude: None,
        line: 1,
    }
}

pub fn row(
    name: &str,
    lat: f64,
    lon: f64,
    okres: Option<&str>,
    country: Option<&str>,
) -> catalogue::Row {
    catalogue::Row {
        name: name.into(),
        latitude: Some(lat),
        longitude: Some(lon),
        okres: okres.map(Into::into),
        country: country.map(Into::into),
        line: 1,
    }
}

pub fn bus_stop(id: i64, name: &str, lat: f64, lon: f64) -> Node {
    Node {
        id,
        coord: Coord::new(lon, lat),
        tags: vec![
            ("highway".into(), "bus_stop".into()),
            ("name".into(), name.into()),
        ],
    }
}

pub fn evidence_with(
    catalogues: Vec<(&str, Vec<catalogue::Row>)>,
    nodes: Vec<Node>,
    foreign_towns: Vec<gazetteer::ForeignTown>,
) -> (Evidence, Diagnostics) {
    let mut diagnostics = Diagnostics::new();
    let mut rows = catalogues;
    for (_, rows) in &mut rows {
        for (line, row) in rows.iter_mut().enumerate() {
            row.line = line as u64 + 1;
        }
    }
    let evidence = Evidence::build(
        EvidenceInputs {
            catalogues: rows,
            osm_nodes: nodes,
            okresy: okresy(),
            towns: towns(),
            foreign_towns,
            synonyms: synonyms::parse(SYNONYMS),
        },
        &policy(),
        &mut diagnostics,
    );
    (evidence, diagnostics)
}

pub fn evidence(rows: Vec<catalogue::Row>) -> Evidence {
    evidence_with(vec![("manual.csv", rows)], Vec::new(), Vec::new()).0
}

pub fn fix(batch: Batch, evidence: &Evidence) -> (Option<FixedBatch>, Diagnostics) {
    let mut diagnostics = Diagnostics::new();
    let fixed = jr_jdf::fix::fix(batch, evidence, &policy(), &mut diagnostics);
    (fixed, diagnostics)
}

/// Metres of latitude per degree, by the one distance function.
pub fn degrees(metres: f64) -> f64 {
    metres / jr_model::geo::distance_m(Coord::new(14.0, 50.0), Coord::new(14.0, 51.0))
}
