use super::*;
use crate::geo::Coord;
use crate::identity::{Feed, PublicId};
use crate::time::{Horizon, OptTime, ServiceTime};
use crate::timetable::{Boarding, BoardingPointKind, Call, Mode, PlaceKind, Tristate};

const RULE: Rule = Rule {
    id: "TEST-R001",
    summary: "test",
};

fn place(code: &str) -> Place {
    Place {
        id: PublicId::build("czptt:stop", &["CZ", code]),
        kind: PlaceKind::StopPlace,
        name: code.into(),
        source_code: Some(code.into()),
        description: None,
        municipality: None,
        district_name: None,
        district_code: None,
        nearby_place: None,
        country: Some("CZ".into()),
        position: None,
        url: None,
        timezone: None,
        wheelchair_boarding: Tristate::Unknown,
    }
}

fn track(place: PlaceId, code: &str) -> BoardingPoint {
    BoardingPoint {
        id: PublicId::build("czptt:stop", &["CZ", code, "platform", "1"]),
        place,
        kind: BoardingPointKind::Track,
        public_code: Some("1".into()),
        source_code: None,
        source_name: None,
        description: None,
        position: None,
        wheelchair_boarding: Tristate::Unknown,
    }
}

fn call(place: PlaceId, sequence: i32, minute: i32) -> Call {
    let time = OptTime::some(ServiceTime::from_hms(8, minute, 0));
    Call {
        place,
        boarding_point: None,
        track: None,
        route_stop: None,
        arrival: time,
        departure: time,
        sequence,
        stop_headsign: None,
        line: None,
        zones: None,
        kilometre: None,
        pickup: Boarding::Regular,
        dropoff: Boarding::Regular,
        passenger: true,
        timepoint: true,
    }
}

/// Two stations, one agency, one route and one trip between them.
fn timetable() -> Timetable {
    let mut t = Timetable::new(Feed::Czptt, Horizon::gvd(2026).unwrap());
    let a = t.places.push(place("11111"));
    let b = t.places.push(place("22222"));
    let agency = t.agencies.push(Agency {
        id: PublicId::build("czptt:agency", &["0054"]),
        name: "ČD".into(),
        url: None,
        timezone: "Europe/Prague".into(),
        language: None,
        phone: None,
        fare_url: None,
        registration: None,
        email: None,
    });
    let route = t.routes.push(Route {
        id: PublicId::build("czptt:route", &["line", "S1"]),
        agency,
        mode: Mode::Rail,
        gtfs_route_type: 2,
        short_name: Some("S1".into()),
        long_name: None,
        description: None,
        url: None,
        color: None,
        text_color: None,
        sort_order: None,
        timetable_kind: Default::default(),
    });
    let service = t.services.intern(DateSet::from_indices([0, 1]));
    t.trips.push(Trip {
        id: PublicId::build("czptt:trip", &["PA", "1"]),
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
        calls: vec![call(a, 1, 0), call(b, 2, 10)],
    });
    assert_eq!(t.check(), []);
    t
}

fn precedence() -> Precedence {
    Precedence {
        position: vec![CoordSource::Sr70, CoordSource::Osm],
    }
}

#[test]
fn new_rows_and_values_get_the_handles_the_builder_promised() {
    let mut t = timetable();
    let trip = t.trips.handles().next().unwrap();
    let station = t.places.handles().next().unwrap();
    let mut edits = EditBuilder::new(&t, "test");
    let point = edits.add_boarding_point(RULE, track(station, "11111"));
    edits.push(
        RULE,
        Edit::SetCallBoardingPoint {
            trip,
            call: 0,
            boarding_point: Some(point),
        },
    );
    let service = edits.service(DateSet::from_indices([5]));
    edits.push(RULE, Edit::SetTripService { trip, service });
    let edits = edits.finish();

    apply(&mut t, edits, &precedence(), &mut Diagnostics::new()).unwrap();
    assert_eq!(t.trips[trip].calls[0].boarding_point, Some(point));
    assert_eq!(t.boarding_points[point].public_code.as_deref(), Some("1"));
    assert_eq!(
        t.services.get(t.trips[trip].service),
        DateSet::from_indices([5])
    );
    let who: Vec<_> = t.history.of(Target::Trip(trip)).collect();
    assert_eq!(who, [("test", "TEST-R001")]);
}

#[test]
fn a_batch_built_against_another_shape_is_refused() {
    let mut t = timetable();
    let stale = EditBuilder::new(&t, "first").finish();
    t.places.push(place("33333"));
    let result = apply(&mut t, stale, &precedence(), &mut Diagnostics::new());
    assert!(matches!(result, Err(ApplyError::Stale { .. })));
}

#[test]
fn a_weaker_coordinate_source_does_not_replace_a_stronger_one() {
    let mut t = timetable();
    let station = t.places.handles().next().unwrap();
    let sr70 = Position::exact(Coord::new(14.0, 50.0), CoordSource::Sr70);
    let osm = Position::exact(Coord::new(14.1, 50.1), CoordSource::Osm);
    let guess = Position::estimated(Coord::new(14.2, 50.2), CoordSource::RouteTime);
    let mut diagnostics = Diagnostics::new();
    let offers = [
        (guess, guess),
        (osm, osm),
        (sr70, sr70),
        (osm, sr70),
        (guess, sr70),
    ];
    for (offered, expected) in offers {
        let mut edits = EditBuilder::new(&t, "coordinates");
        edits.push(
            RULE,
            Edit::SetPlacePosition {
                place: station,
                position: offered,
            },
        );
        let edits = edits.finish();
        apply(&mut t, edits, &precedence(), &mut diagnostics).unwrap();
        assert_eq!(t.places[station].position, Some(expected));
    }
    assert_eq!(
        diagnostics.counts_by_code().get("edit_superseded"),
        Some(&2)
    );
}

#[test]
fn an_edit_that_breaks_an_invariant_fails_the_batch() {
    let mut t = timetable();
    let trip = t.trips.handles().next().unwrap();
    let other = t.places.handles().nth(1).unwrap();
    let mut edits = EditBuilder::new(&t, "test");
    // A track of the second station assigned to the call at the first.
    let point = edits.add_boarding_point(RULE, track(other, "22222"));
    edits.push(
        RULE,
        Edit::SetCallBoardingPoint {
            trip,
            call: 0,
            boarding_point: Some(point),
        },
    );
    let edits = edits.finish();
    let result = apply(&mut t, edits, &precedence(), &mut Diagnostics::new());
    let Err(ApplyError::Invariant { violations, .. }) = result else {
        panic!("{result:?}")
    };
    assert!(
        violations[0].problem.contains("another place"),
        "{violations:?}"
    );
}
