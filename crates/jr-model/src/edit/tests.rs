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

#[test]
fn removing_calls_takes_what_was_stated_for_them_and_needs_two_left() {
    use crate::timetable::{
        ClaimDerivation, ClaimDirection, ClaimResolution, ConnectionClaim, Restriction,
        RestrictionGroup, RestrictionScope,
    };
    let mut t = timetable();
    let trip = t.trips.handles().next().unwrap();
    let route = t.routes.handles().next().unwrap();
    let third = t.places.push(place("33333"));
    t.trips
        .get_mut(trip)
        .unwrap()
        .calls
        .push(call(third, 3, 20));
    let claim = |sequence| ConnectionClaim {
        direction: ClaimDirection::WaitsFor,
        origin_trip: trip,
        origin_sequence: sequence,
        dates: None,
        target_source_route: None,
        target_source_trip: None,
        target_source_stop: None,
        target_source_post: None,
        target_source_end_stop: None,
        target_source_end_post: None,
        wait_minutes: None,
        note: None,
        target_public_line: None,
        target_destination_text: None,
        derivation: ClaimDerivation::Structured,
        resolution: ClaimResolution::Unresolved,
        target_route: None,
        target_trip: None,
        target_location: None,
        source_object: "claim".into(),
    };
    t.connection_claims = vec![claim(2), claim(3)];
    t.restrictions = vec![Restriction {
        group: RestrictionGroup::A,
        scope: RestrictionScope::RouteStop {
            route,
            calls: Box::new([(trip, 1), (trip, 2)]),
        },
        source_route_stop: "1".into(),
        source_object: "restriction".into(),
    }];
    assert_eq!(t.check(), []);

    let remove = |t: &mut Timetable, sequences: Vec<i32>| {
        let mut edits = EditBuilder::new(t, "test");
        edits.push(RULE, Edit::RemoveCalls { trip, sequences });
        let edits = edits.finish();
        apply(t, edits, &precedence(), &mut Diagnostics::new())
    };
    remove(&mut t, vec![2]).unwrap();
    let sequences: Vec<i32> = t.trips[trip]
        .calls
        .iter()
        .map(|call| call.sequence)
        .collect();
    assert_eq!(sequences, [1, 3]);
    assert_eq!(t.connection_claims.len(), 1);
    assert_eq!(t.connection_claims[0].origin_sequence, 3);
    let RestrictionScope::RouteStop { calls, .. } = &t.restrictions[0].scope else {
        panic!()
    };
    assert_eq!(**calls, [(trip, 1)]);
    // One call left is no trip.
    assert!(remove(&mut t, vec![3]).is_err());
}

#[test]
fn a_split_trip_takes_a_copy_of_what_is_stated_for_the_trip() {
    use crate::timetable::{
        AssignmentKind, BindingMethod, Entity, Namespace, Scope, SourceKey, TransferKind,
    };
    let mut t = timetable();
    let trip = t.trips.handles().next().unwrap();
    t.keys.push(SourceKey {
        namespace: Namespace::CZPTT_TRAIN_NUMBER,
        identifier: "1234".into(),
        entity: Entity::Trip(trip),
        valid_from: t.horizon.start(),
        valid_to: t.horizon.end(),
        binding: BindingMethod::Identity,
    });
    t.assignments.push(Assignment {
        scope: Scope::Call { trip, sequence: 2 },
        kind: AssignmentKind::CalendarDesignation,
        source_code: Some("X".into()),
        note: None,
        source_object: None,
    });
    t.transfers.push(Transfer {
        from: None,
        to: None,
        from_route: None,
        to_route: None,
        from_trip: Some(trip),
        to_trip: Some(trip),
        kind: TransferKind::Timed,
        minimum_time: None,
        maximum_wait: None,
    });

    let mut edits = EditBuilder::new(&t, "test");
    let first = edits.service(DateSet::from_indices([0]));
    let second = edits.service(DateSet::from_indices([1]));
    let id = crate::identity::overlay::slice(&t.trips[trip].id, &DateSet::from_indices([1]));
    let copy = edits.split_trip(RULE, trip, id.clone(), second);
    edits.push(
        RULE,
        Edit::SetTripService {
            trip,
            service: first,
        },
    );
    let edits = edits.finish();
    apply(&mut t, edits, &precedence(), &mut Diagnostics::new()).unwrap();

    assert_eq!(t.trips[copy].id, id);
    assert_eq!(t.trips[copy].calls, t.trips[trip].calls);
    assert_eq!(
        t.services.get(t.trips[copy].service),
        DateSet::from_indices([1])
    );
    assert_eq!(
        t.services.get(t.trips[trip].service),
        DateSet::from_indices([0])
    );
    let on = |entity| t.keys.iter().filter(|key| key.entity == entity).count();
    assert_eq!((on(Entity::Trip(trip)), on(Entity::Trip(copy))), (1, 1));
    assert!(t.assignments.iter().any(|a| a.scope
        == Scope::Call {
            trip: copy,
            sequence: 2
        }));
    // A transfer from the trip to itself now holds between the copy and itself too.
    let ends: Vec<_> = t
        .transfers
        .iter()
        .map(|x| (x.from_trip, x.to_trip))
        .collect();
    assert_eq!(ends, [(Some(trip), Some(trip)), (Some(copy), Some(copy))]);
}

#[test]
fn replaced_calls_keep_what_was_stated_for_the_calls_that_continue() {
    use crate::timetable::{AssignmentKind, Scope};
    let mut t = timetable();
    let trip = t.trips.handles().next().unwrap();
    let (a, b) = (t.trips[trip].calls[0].place, t.trips[trip].calls[1].place);
    let between = t.places.push(place("33333"));
    for sequence in [1, 2] {
        t.assignments.push(Assignment {
            scope: Scope::Call { trip, sequence },
            kind: AssignmentKind::CalendarDesignation,
            source_code: Some(format!("at {sequence}").into()),
            note: None,
            source_object: None,
        });
    }
    let mut edits = EditBuilder::new(&t, "test");
    // A call inserted between the two; the first call does not continue.
    edits.push(
        RULE,
        Edit::ReplaceTripCalls {
            trip,
            calls: vec![call(a, 1, 1), call(between, 2, 5), call(b, 3, 11)],
            kept: vec![(2, 3)],
        },
    );
    let edits = edits.finish();
    apply(&mut t, edits, &precedence(), &mut Diagnostics::new()).unwrap();
    let stated: Vec<_> = t
        .assignments
        .iter()
        .map(|a| (a.scope, a.source_code.as_deref()))
        .collect();
    assert_eq!(stated, [(Scope::Call { trip, sequence: 3 }, Some("at 2"))]);
}
