//! A hand-built road timetable, its post facts and a street graph for the estimator's tests.
#![allow(dead_code)]

use std::collections::HashMap;

use jr_formats::geodata::registry::Post;
use jr_formats::post_evidence::{PackIdentity, RouterEvidence};
use jr_geo::routing::{GraphBuilder, OsmNode, OsmWay, RoutingGraph, RoutingPolicy};
use jr_jdf::assemble::{PostFacts, StopFacts, TripFacts};
use jr_jdf::merge::PostCandidate;
use jr_model::identity::{self, jdf};
use jr_model::timetable::{
    Agency, Boarding, BoardingPoint, BoardingPointKind, Call, CoordSource, Mode, Place, PlaceKind,
    Position, Route, TimetableKind, Trip, Tristate,
};
use jr_model::{
    BoardingPointId, Coord, DateSet, Diagnostics, Feed, Horizon, OptTime, PlaceId, Precedence,
    RouteId, ServiceId, ServiceTime, Timetable,
};
use jr_posts::evaluate::EvaluationPolicy;
use jr_posts::{EstimateInput, Estimated};

/// A position `north` and `east` metres from 50°N 14°E.
pub fn at(north: f64, east: f64) -> Coord {
    Coord::new(14.0 + east / 71_700.0, 50.0 + north / 111_250.0)
}

/// A two-way street along the east axis from 3 km west to 3 km east, nodes every 100 m.
pub fn street() -> RoutingGraph {
    let policy = RoutingPolicy::builtin();
    let mut builder = GraphBuilder::new(&policy.graph);
    let ids: Vec<i64> = (1..=61).collect();
    for &id in &ids {
        let coord = at(0.0, (id - 31) as f64 * 100.0);
        builder
            .node(&OsmNode {
                id,
                coord,
                tags: &[],
            })
            .unwrap();
    }
    builder
        .way(&OsmWay {
            id: 1,
            nodes: &ids,
            tags: &[("highway", "residential")],
        })
        .unwrap();
    builder.finish().unwrap()
}

/// An OSM candidate `north`/`east` metres off the origin.
pub fn osm(node: u64, north: f64, east: f64) -> PostCandidate {
    PostCandidate {
        coord: at(north, east),
        observation: format!("osm:node:{node}").into(),
        facts: vec![
            ("source_kind".into(), "osm".into()),
            ("explicit_modes".into(), "".into()),
            ("denied_modes".into(), "".into()),
            ("lifecycle".into(), "active".into()),
            ("tag:highway".into(), "bus_stop".into()),
        ],
    }
}

pub struct World {
    pub timetable: Timetable,
    pub facts: PostFacts,
    pub graph: RoutingGraph,
    pub places: HashMap<u64, PlaceId>,
    pub unspecified: HashMap<u64, BoardingPointId>,
    route: RouteId,
    service: ServiceId,
}

impl World {
    pub fn new() -> Self {
        let horizon = Horizon::gvd(2026).unwrap();
        let mut timetable = Timetable::new(Feed::Jdf, horizon);
        let agency = timetable.agencies.push(Agency {
            id: jdf::agency("12345678", 1),
            name: "Dopravce".into(),
            url: None,
            timezone: "Europe/Prague".into(),
            language: None,
            phone: None,
            fare_url: None,
            registration: None,
            email: None,
        });
        let route = timetable.routes.push(Route {
            id: identity::content(Feed::Jdf, "route", &[("test", "1")]),
            agency,
            mode: Mode::Bus,
            gtfs_route_type: 3,
            short_name: Some("1".into()),
            long_name: None,
            description: None,
            url: None,
            color: None,
            text_color: None,
            sort_order: None,
            timetable_kind: TimetableKind::Regular,
        });
        let service = timetable.services.intern(DateSet::full(horizon.days()));
        World {
            timetable,
            facts: PostFacts::default(),
            graph: street(),
            places: HashMap::new(),
            unspecified: HashMap::new(),
            route,
            service,
        }
    }

    /// A stop at `east` metres on the street with its candidates; `precise` gives it a precise
    /// position.
    pub fn stop(&mut self, number: u64, east: f64, precise: bool, candidates: Vec<PostCandidate>) {
        let coord = at(0.0, east);
        let id = jdf::stop(number);
        let place = self.timetable.places.push(Place {
            id: id.clone(),
            kind: PlaceKind::StopPlace,
            name: format!("Zastávka {number}").into(),
            source_code: None,
            description: None,
            municipality: None,
            district_name: None,
            district_code: None,
            nearby_place: None,
            country: Some("CZ".into()),
            position: Some(Position::exact(coord, CoordSource::Osm)),
            url: None,
            timezone: None,
            wheelchair_boarding: Tristate::Unknown,
        });
        let point = self.timetable.boarding_points.push(BoardingPoint {
            id: jdf::unspecified_point(&id),
            place,
            kind: BoardingPointKind::Unspecified,
            public_code: None,
            source_code: None,
            source_name: None,
            description: None,
            position: None,
            wheelchair_boarding: Tristate::Unknown,
        });
        self.places.insert(number, place);
        self.unspecified.insert(number, point);
        self.facts.stops.push(StopFacts {
            place,
            number,
            precise: precise.then_some(coord),
            candidates,
        });
        self.facts.stops.sort_by_key(|stop| stop.number);
    }

    /// An authored post of a stop with its key.
    pub fn authored(&mut self, number: u64, post: u64) -> BoardingPointId {
        let place = self.places[&number];
        let point = self.timetable.boarding_points.push(BoardingPoint {
            id: jdf::authored_post(&jdf::stop(number), post),
            place,
            kind: BoardingPointKind::Post,
            public_code: None,
            source_code: Some(post.to_string().into()),
            source_name: None,
            description: None,
            position: None,
            wheelchair_boarding: Tristate::Unknown,
        });
        self.facts
            .authored_keys
            .push((point, format!("id:{post}").into()));
        point
    }

    /// A trip calling at `stops` (stop number, authored point or the unspecified one).
    pub fn trip(
        &mut self,
        number: u64,
        mode: &str,
        line: &str,
        stops: &[(u64, Option<BoardingPointId>)],
    ) {
        let calls = stops
            .iter()
            .enumerate()
            .map(|(index, &(stop, point))| {
                let time = OptTime::some(ServiceTime::from_hms(8, index as i32, 0));
                Call {
                    place: self.places[&stop],
                    boarding_point: Some(point.unwrap_or(self.unspecified[&stop])),
                    track: None,
                    route_stop: None,
                    arrival: time,
                    departure: time,
                    sequence: index as i32 + 1,
                    stop_headsign: None,
                    line: None,
                    zones: None,
                    kilometre: None,
                    pickup: Boarding::Regular,
                    dropoff: Boarding::Regular,
                    passenger: true,
                    timepoint: true,
                }
            })
            .collect();
        let trip = self.timetable.trips.push(Trip {
            id: identity::content(Feed::Jdf, "trip", &[("trip", &number.to_string())]),
            route: self.route,
            service: self.service,
            direction: Some(u8::from(number % 2 == 1)),
            headsign: None,
            short_name: None,
            block: None,
            run: None,
            wheelchair_accessible: Tristate::Unknown,
            bikes_allowed: Tristate::Unknown,
            shape: None,
            calls,
        });
        self.facts.trips.push(TripFacts {
            trip,
            line: line.into(),
            distinction: 1,
            mode: mode.into(),
            number,
        });
    }

    pub fn estimate(&self, policy: &EvaluationPolicy, registry: &[Post]) -> Estimated {
        jr_posts::estimate(
            EstimateInput {
                timetable: &self.timetable,
                facts: &self.facts,
                graph: &self.graph,
                routing: &RoutingPolicy::builtin(),
                policy,
                registry,
                identity: PackIdentity {
                    capture_tool_version: "test".into(),
                    merged_jdf_sha256: "0".repeat(64),
                    routing_pbf_sha256: "1".repeat(64),
                    source_id: "test".into(),
                    router: RouterEvidence::Rust,
                },
            },
            &mut Diagnostics::new(),
        )
        .expect("estimates")
    }

    /// The timetable with the estimate's edits applied.
    pub fn applied(&self, estimated: Estimated) -> Timetable {
        let mut timetable = self.timetable.clone();
        jr_model::apply(
            &mut timetable,
            estimated.edits,
            &Precedence { position: vec![] },
            &mut Diagnostics::new(),
        )
        .expect("applies");
        timetable
    }
}

/// The street with stops 1 (500 m west, one candidate), 2 (at the origin, a candidate 4 m
/// either side) and 3 (500 m east, one candidate), and a trip each way along it. Traffic keeps
/// right: eastbound buses stop at the south post (node 21), westbound at the north one (22).
pub fn crossing() -> World {
    let mut world = World::new();
    world.stop(1, -500.0, true, vec![osm(11, -4.0, -500.0)]);
    world.stop(2, 0.0, true, vec![osm(21, -4.0, 0.0), osm(22, 4.0, 0.0)]);
    world.stop(3, 500.0, true, vec![osm(31, 4.0, 500.0)]);
    world.trip(1, "A", "100001", &[(1, None), (2, None), (3, None)]);
    world.trip(2, "A", "100001", &[(3, None), (2, None), (1, None)]);
    world
}
