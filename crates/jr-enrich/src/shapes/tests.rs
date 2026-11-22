use jr_model::identity::czptt;
use jr_model::timetable::{
    Agency, Boarding, BoardingPoint, BoardingPointKind, Call, CoordSource, Mode, Place, PlaceKind,
    Position, Route, TimetableKind, Trip, Tristate,
};
use jr_model::{
    BoardingPointId, DateSet, Feed, Horizon, OptTime, PlaceId, Precedence, RouteId, TripId, apply,
};
use serde_json::json;

use super::*;

fn exact(lat: f64, lon: f64) -> Option<Position> {
    Some(Position::exact(Coord::new(lon, lat), CoordSource::Osm))
}

fn estimated(lat: f64, lon: f64) -> Option<Position> {
    Some(Position::estimated(
        Coord::new(lon, lat),
        CoordSource::RouteTime,
    ))
}

/// One calling point of a fixture trip: place, boarding point, passenger service.
type Stop = (PlaceId, Option<BoardingPointId>, bool);

struct Fixture {
    t: Timetable,
    rail: RouteId,
    cable: RouteId,
}

impl Fixture {
    fn new() -> Self {
        let mut t = Timetable::new(Feed::Czptt, Horizon::gvd(2026).unwrap());
        let agency = t.agencies.push(Agency {
            id: czptt::agency("0054"),
            name: "ČD".into(),
            url: None,
            timezone: "Europe/Prague".into(),
            language: None,
            phone: None,
            fare_url: None,
            email: None,
        });
        let mut route = |line: &str, mode: Mode| {
            t.routes.push(Route {
                id: czptt::line_route(line, "0054", 106),
                agency,
                mode,
                gtfs_route_type: 106,
                short_name: None,
                long_name: None,
                description: None,
                url: None,
                color: None,
                text_color: None,
                sort_order: None,
                timetable_kind: TimetableKind::Regular,
            })
        };
        let rail = route("S1", Mode::Rail);
        let cable = route("LD", Mode::Cable);
        Self { t, rail, cable }
    }

    fn place(&mut self, code: &str, kind: PlaceKind, position: Option<Position>) -> PlaceId {
        self.t.places.push(Place {
            id: czptt::station("CZ", code),
            kind,
            name: code.into(),
            source_code: Some(code.into()),
            description: None,
            municipality: None,
            district_name: None,
            district_code: None,
            nearby_place: None,
            country: Some("CZ".into()),
            position,
            url: None,
            timezone: None,
            wheelchair_boarding: Tristate::Unknown,
        })
    }

    fn station(&mut self, code: &str, position: Option<Position>) -> PlaceId {
        self.place(code, PlaceKind::StopPlace, position)
    }

    fn track(&mut self, place: PlaceId, position: Option<Position>) -> BoardingPointId {
        let station = self.t.places[place].id.clone();
        self.t.boarding_points.push(BoardingPoint {
            id: czptt::track(&station, "1"),
            place,
            kind: BoardingPointKind::Track,
            public_code: Some("1".into()),
            source_code: Some("1".into()),
            source_name: None,
            description: None,
            position,
            wheelchair_boarding: Tristate::Unknown,
        })
    }

    fn trip(&mut self, id: &str, route: RouteId, stops: &[Stop], days: &[usize]) -> TripId {
        let calls = stops
            .iter()
            .enumerate()
            .map(|(index, &(place, point, passenger))| Call {
                place,
                boarding_point: point,
                track: point,
                route_stop: None,
                arrival: OptTime::NONE,
                departure: OptTime::NONE,
                sequence: index as i32 + 1,
                stop_headsign: None,
                line: None,
                zones: None,
                kilometre: None,
                pickup: Boarding::Regular,
                dropoff: Boarding::Regular,
                passenger,
                timepoint: true,
            })
            .collect();
        let service = self
            .t
            .services
            .intern(DateSet::from_indices(days.iter().copied()));
        self.t.trips.push(Trip {
            id: czptt::trip(id, 1),
            route,
            service,
            direction: None,
            headsign: None,
            short_name: None,
            block: None,
            run: None,
            wheelchair_accessible: Tristate::Unknown,
            bikes_allowed: Tristate::Unknown,
            shape: None,
            calls,
        })
    }

    fn rail_trip(&mut self, id: &str, stops: &[Stop]) -> TripId {
        let rail = self.rail;
        self.trip(id, rail, stops, &[0, 1])
    }

    /// Runs the mapping, applies its edits and checks the invariants.
    fn apply(
        &mut self,
        plan: &Plan,
        output: Result<&CompanionOutput, &str>,
    ) -> (Summary, Diagnostics) {
        let mut diagnostics = Diagnostics::new();
        let (edits, summary) = shapes(&self.t, plan, output, &mut diagnostics);
        apply(
            &mut self.t,
            edits,
            &Precedence { position: vec![] },
            &mut diagnostics,
        )
        .unwrap();
        assert_eq!(self.t.check(), []);
        (summary, diagnostics)
    }
}

fn pattern_of(plan: &Plan, trip: TripId) -> &str {
    let planned = plan.trips.iter().find(|planned| planned.trip == trip);
    &plan.patterns[planned.unwrap().pattern].id
}

/// Companion output files from their data rows.
fn companion(status: &[String], shapes: &[String], offsets: &[String]) -> CompanionOutput {
    let file = |header: &str, rows: &[String]| {
        let mut text = format!("{header}\n");
        for row in rows {
            text.push_str(row);
            text.push('\n');
        }
        text
    };
    CompanionOutput {
        pattern_status: file("pattern_id,class,status,points,routed_m,beeline_m", status),
        shapes: file("pattern_id,seq,lat,lon,dist_m", shapes),
        offsets: file("pattern_id,match_seq,point_index,dist_m", offsets),
        manifest: Some(json!({"tool": "obehy-motis-shapes", "osr_commit": "test"})),
    }
}

/// A routed pattern's rows: one shape point per `(lat, lon, dist_m)` and the given offsets
/// `(match_seq, point_index, dist_m)`.
fn routed(
    id: &str,
    points: &[(f64, f64, f64)],
    offsets: &[(usize, usize, f64)],
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let status = vec![format!("{id},rail,routed,{},0.0,0.0", points.len())];
    let shapes = points
        .iter()
        .enumerate()
        .map(|(seq, (lat, lon, metres))| format!("{id},{seq},{lat:.7},{lon:.7},{metres:.1}"))
        .collect();
    let offsets = offsets
        .iter()
        .map(|(seq, index, metres)| format!("{id},{seq},{index},{metres:.1}"))
        .collect();
    (status, shapes, offsets)
}

const BEND: [(f64, f64, f64); 3] = [
    (50.0, 14.0, 0.0),
    (50.05, 14.08, 7000.0),
    (50.1, 14.1, 14000.0),
];

#[test]
fn serving_r030_r031_patterns_csv_keeps_railway_points_and_skips_estimated_calls() {
    let mut f = Fixture::new();
    // Rounded to seven decimals.
    let a = f.station("A", exact(50.000_000_04, 14.0));
    let point = f.place("R", PlaceKind::OperationalPoint, exact(50.05, 14.05));
    let unplaced = f.place("X", PlaceKind::OperationalPoint, None);
    let guessed = f.station("E", estimated(50.07, 14.07));
    // The exact track wins over the estimated station.
    let b = f.station("B", estimated(50.2, 14.2));
    let track = f.track(b, exact(50.1, 14.1));
    f.rail_trip(
        "t1",
        &[
            (a, None, true),
            (point, None, false),
            (unplaced, None, false),
            (guessed, None, true),
            (b, Some(track), true),
        ],
    );
    let cable = f.cable;
    f.trip("lift", cable, &[(a, None, true), (b, None, true)], &[0]);
    // Not published: not counted.
    let rail = f.rail;
    f.trip("retired", rail, &[(a, None, true), (b, None, true)], &[]);

    let plan = Plan::of(&f.t);
    assert_eq!(
        plan.patterns_csv(),
        "pattern_id,class,seq,lat,lon,waypoint\n\
         15c3b1db59acc231308e,rail,0,50.0000000,14.0000000,0\n\
         15c3b1db59acc231308e,rail,1,50.0500000,14.0500000,1\n\
         15c3b1db59acc231308e,rail,2,50.1000000,14.1000000,0\n"
    );
    let summary = plan.summary();
    assert_eq!(summary.trips, 2);
    assert_eq!(summary.routed_trips, 1);
    assert_eq!(summary.estimated_calls_skipped, 1);
    assert_eq!(
        summary.not_routed.unsupported_mode,
        BTreeMap::from([("cable".to_owned(), 1)])
    );
}

#[test]
fn serving_r031_trips_with_the_same_match_points_share_a_pattern() {
    let mut f = Fixture::new();
    let a = f.station("A", exact(50.0, 14.0));
    let b = f.station("B", exact(50.1, 14.1));
    let first = f.rail_trip("t1", &[(a, None, true), (b, None, true)]);
    let second = f.rail_trip("t2", &[(a, None, true), (b, None, true)]);
    // Same coordinates, but passing B without service: a waypoint, another pattern.
    let passing = f.rail_trip("t3", &[(a, None, true), (b, None, false)]);
    let plan = Plan::of(&f.t);
    assert_eq!(plan.patterns.len(), 2);
    assert_eq!(pattern_of(&plan, first), pattern_of(&plan, second));
    assert_ne!(pattern_of(&plan, first), pattern_of(&plan, passing));
    assert_eq!(plan.summary().routed_trips, 3);
}

#[test]
fn serving_r033_an_accepted_pattern_shapes_its_trips_with_offsets_on_match_points() {
    let mut f = Fixture::new();
    let a = f.station("A", exact(50.0, 14.0));
    let guessed = f.station("E", estimated(50.05, 14.05));
    let b = f.station("B", exact(50.1, 14.1));
    let trip = f.rail_trip(
        "t1",
        &[(a, None, true), (guessed, None, true), (b, None, true)],
    );
    let plan = Plan::of(&f.t);
    let id = pattern_of(&plan, trip).to_owned();
    // Offsets are taken by match_seq, not file order.
    let (status, shapes, offsets) = routed(&id, &BEND, &[(1, 2, 14000.0), (0, 0, 0.0)]);
    let output = companion(&status, &shapes, &offsets);

    let (summary, diagnostics) = f.apply(&plan, Ok(&output));
    assert!(diagnostics.is_empty());
    let shape = f.t.trips[trip].shape.as_ref().unwrap();
    assert_eq!(&*shape.offsets, [Some(0.0), None, Some(14000.0)]);
    let row = &f.t.shapes[shape.shape];
    assert!(row.id.as_str().starts_with("czptt:shape:motis:"));
    assert_eq!(row.generation, ShapeGeneration::Motis);
    let points: Vec<(f64, f64)> = row.points.iter().map(|p| (p.lat, p.lon)).collect();
    assert_eq!(points, [(50.0, 14.0), (50.05, 14.08), (50.1, 14.1)]);
    assert_eq!(
        (
            summary.shaped_patterns,
            summary.shapes,
            summary.shaped_trips
        ),
        (1, 1, 1)
    );
    assert!(summary.unshaped_patterns.is_empty());
    assert_eq!(summary.companion, output.manifest);
}

#[test]
fn serving_r032_patterns_with_identical_geometry_share_one_shape() {
    let mut f = Fixture::new();
    let a = f.station("A", exact(50.0, 14.0));
    let b = f.station("B", exact(50.1, 14.1));
    let stopping = f.rail_trip("t1", &[(a, None, true), (b, None, true)]);
    let passing = f.rail_trip("t2", &[(a, None, true), (b, None, false)]);
    let plan = Plan::of(&f.t);
    let (mut status, mut shapes, mut offsets) = (Vec::new(), Vec::new(), Vec::new());
    for trip in [stopping, passing] {
        let rows = routed(
            pattern_of(&plan, trip),
            &BEND,
            &[(0, 0, 0.0), (1, 2, 14000.0)],
        );
        status.extend(rows.0);
        shapes.extend(rows.1);
        offsets.extend(rows.2);
    }
    let (summary, _) = f.apply(&plan, Ok(&companion(&status, &shapes, &offsets)));
    assert_eq!(f.t.shapes.len(), 1);
    let shape_of = |trip: TripId| f.t.trips[trip].shape.as_ref().unwrap().shape;
    assert_eq!(shape_of(stopping), shape_of(passing));
    assert_eq!((summary.shaped_patterns, summary.shapes), (2, 1));
}

#[test]
fn serving_r032_unaccepted_patterns_are_unshaped_with_their_reason() {
    let mut f = Fixture::new();
    let a = f.station("A", exact(50.0, 14.0));
    let b = f.station("B", exact(50.1, 14.1));
    let c = f.station("C", exact(50.2, 14.2));
    let failed = f.rail_trip("t1", &[(a, None, true), (b, None, true)]);
    f.rail_trip("t2", &[(a, None, true), (c, None, true)]);
    let misaligned = f.rail_trip("t3", &[(b, None, true), (c, None, true)]);
    let plan = Plan::of(&f.t);
    // t2's pattern has no status row; t3's has one offset for two match points.
    let mut status = vec![format!(
        "{},rail,failed,0,0.0,0.0",
        pattern_of(&plan, failed)
    )];
    let (more, shapes, offsets) = routed(pattern_of(&plan, misaligned), &BEND, &[(0, 0, 0.0)]);
    status.extend(more);

    let (summary, diagnostics) = f.apply(&plan, Ok(&companion(&status, &shapes, &offsets)));
    assert!(f.t.trips.rows().iter().all(|trip| trip.shape.is_none()));
    assert!(f.t.shapes.is_empty());
    assert_eq!(
        diagnostics.counts_by_code(),
        BTreeMap::from([("route_shapes_misaligned", 1)])
    );
    assert_eq!(
        serde_json::to_value(&summary).unwrap(),
        json!({
            "trips": 3,
            "already_shaped": 0,
            "routed_trips": 3,
            "patterns": 3,
            "not_routed": {"unsupported_mode": {}},
            "estimated_calls_skipped": 0,
            "shaped_patterns": 0,
            "shapes": 0,
            "shaped_trips": 0,
            "unshaped_patterns": {"failed": 1, "misaligned": 1, "missing": 1},
            "companion": {"tool": "obehy-motis-shapes", "osr_commit": "test"},
        })
    );
}

#[test]
fn serving_r032_malformed_companion_output_unshapes_the_affected_patterns() {
    let mut f = Fixture::new();
    let a = f.station("A", exact(50.0, 14.0));
    let b = f.station("B", exact(50.1, 14.1));
    let good = f.rail_trip("t1", &[(a, None, true), (b, None, true)]);
    let bad = f.rail_trip("t2", &[(a, None, true), (b, None, false)]);
    let plan = Plan::of(&f.t);
    let (mut status, mut shapes, mut offsets) = routed(
        pattern_of(&plan, good),
        &BEND,
        &[(0, 0, 0.0), (1, 2, 14000.0)],
    );
    let rows = routed(
        pattern_of(&plan, bad),
        &BEND,
        &[(0, 0, 0.0), (1, 2, 14000.0)],
    );
    status.extend(rows.0);
    shapes.extend(rows.1);
    offsets.extend(rows.2);

    // A file with the wrong header: every pattern.
    let mut output = companion(&status, &shapes, &offsets);
    output.shapes = output.shapes.replacen("dist_m", "metres", 1);
    let (summary, diagnostics) = f.apply(&plan, Ok(&output));
    assert_eq!(
        summary.unshaped_patterns,
        BTreeMap::from([("malformed".to_owned(), 2)])
    );
    assert_eq!(diagnostics.len(), 1);

    // A bad row: its pattern only.
    let mut output = companion(&status, &shapes, &offsets);
    let bad_id = pattern_of(&plan, bad);
    output.shapes = output.shapes.replacen(
        &format!("{bad_id},1,50.0500000"),
        &format!("{bad_id},1,north"),
        1,
    );
    let (summary, diagnostics) = f.apply(&plan, Ok(&output));
    assert_eq!(
        summary.unshaped_patterns,
        BTreeMap::from([("malformed".to_owned(), 1)])
    );
    assert_eq!(
        diagnostics.counts_by_code(),
        BTreeMap::from([("route_shapes_malformed", 1)])
    );
    assert!(f.t.trips[good].shape.is_some());
    assert!(f.t.trips[bad].shape.is_none());
}

#[test]
fn serving_r032_a_failed_companion_unshapes_every_pattern() {
    let mut f = Fixture::new();
    let a = f.station("A", exact(50.0, 14.0));
    let b = f.station("B", exact(50.1, 14.1));
    f.rail_trip("t1", &[(a, None, true), (b, None, true)]);
    f.rail_trip("t2", &[(b, None, true), (a, None, true)]);
    let plan = Plan::of(&f.t);
    let (summary, diagnostics) = f.apply(&plan, Err("exited with code 3"));
    assert_eq!(
        summary.unshaped_patterns,
        BTreeMap::from([("companion_failed".to_owned(), 2)])
    );
    assert_eq!(summary.companion, None);
    assert!(
        !serde_json::to_value(&summary)
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("companion")
    );
    assert_eq!(
        diagnostics.counts_by_code(),
        BTreeMap::from([("route_shapes_companion_failed", 1)])
    );
    assert!(f.t.shapes.is_empty());
}

#[test]
fn serving_r031_a_trip_that_already_has_a_shape_is_left_alone() {
    let mut f = Fixture::new();
    let a = f.station("A", exact(50.0, 14.0));
    let b = f.station("B", exact(50.1, 14.1));
    let trip = f.rail_trip("t1", &[(a, None, true), (b, None, true)]);
    let shape = f.t.shapes.push(Shape {
        id: identity::generated_shape(Feed::Czptt, "motis", "rail", &[Coord::new(14.0, 50.0)]),
        generation: ShapeGeneration::Source,
        points: vec![Coord::new(14.0, 50.0), Coord::new(14.1, 50.1)],
    });
    let given = Some(TripShape {
        shape,
        offsets: vec![None, None].into_boxed_slice(),
    });
    f.t.trips[trip].shape = given.clone();

    let plan = Plan::of(&f.t);
    assert!(plan.is_empty());
    assert_eq!(
        plan.patterns_csv(),
        "pattern_id,class,seq,lat,lon,waypoint\n"
    );
    // With nothing to route the companion is not run and its absence is no failure.
    let (summary, diagnostics) = f.apply(&plan, Err("not run"));
    assert!(diagnostics.is_empty());
    assert_eq!((summary.trips, summary.already_shaped), (1, 1));
    assert_eq!(f.t.trips[trip].shape, given);
}
