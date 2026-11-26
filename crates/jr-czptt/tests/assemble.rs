//! Places, boarding points, agencies, routes, trips, calls, keys and in-path transfers
//! (R052–R069, R080, R082, R118).

mod support;

use jiff::civil::date;
use jr_czptt::PointNames;
use jr_formats::kadr::Line;
use jr_model::Timetable;
use jr_model::timetable::{
    Boarding, BoardingPoint, BoardingPointKind, Entity, LocationRef, Mode, Namespace, PlaceKind,
    TransferKind,
};
use support::*;

fn point<'a>(timetable: &'a Timetable, id: &str) -> &'a BoardingPoint {
    timetable
        .boarding_points
        .rows()
        .iter()
        .find(|point| point.id.as_str() == id)
        .unwrap_or_else(|| panic!("no boarding point {id}"))
}

fn short_names(timetable: &Timetable) -> Vec<&str> {
    timetable
        .routes
        .rows()
        .iter()
        .filter_map(|route| route.short_name.as_deref())
        .collect()
}

fn bus_fixture() -> jr_formats::czptt::TimetableMessage {
    support::timetable(vec![
        line(stop("57076", "08:00:00"), "101"),
        bus(
            line(
                track(
                    location("57050", "Poříčany", Some("08:10:00"), &["0001", "0030"]),
                    "2",
                ),
                "101",
            ),
            "1",
        ),
        bus(
            line(track(point_location("93001", "08:15:00"), "99"), "101"),
            "1",
        ),
        bus(line(track(stop("57016", "08:20:00"), "3"), "101"), "0"),
        line(stop("54357", "09:00:00"), "101"),
    ])
}

fn point_location(code: &str, time: &str) -> jr_formats::czptt::Location {
    support::point(code, time)
}

#[test]
fn r052_points_become_places_and_calls_board_at_their_track() {
    let (timetable, _) = timetable_of(support::timetable(vec![
        track(
            location("57076", "Praha hl.n.", Some("08:00:00"), &["0001"]),
            "12",
        ),
        support::point("57050", "08:10:00"),
        stop("57016", "08:20:00"),
    ]));
    let places: Vec<(&str, PlaceKind, Option<&str>)> = timetable
        .places
        .rows()
        .iter()
        .map(|p| (p.id.as_str(), p.kind, p.timezone.as_deref()))
        .collect();
    assert_eq!(
        places,
        [
            (
                "czptt:stop:CZ:57076",
                PlaceKind::StopPlace,
                Some("Europe/Prague")
            ),
            ("czptt:stop:CZ:57050", PlaceKind::OperationalPoint, None),
            (
                "czptt:stop:CZ:57016",
                PlaceKind::StopPlace,
                Some("Europe/Prague")
            ),
        ]
    );
    assert_eq!(
        timetable.places.rows()[0].source_code.as_deref(),
        Some("57076")
    );
    let calls = &timetable.trips.rows()[0].calls;
    let track = point(&timetable, "czptt:stop:CZ:57076:platform:12");
    assert_eq!(track.kind, BoardingPointKind::Track);
    assert_eq!(calls[0].boarding_point, calls[0].track);
    assert_eq!(calls[1].boarding_point, None);
    let unspecified = point(&timetable, "czptt:stop:CZ:57016:unspecified");
    assert_eq!(unspecified.kind, BoardingPointKind::Unspecified);
    assert_eq!(calls[2].track, None);
}

#[test]
fn r053_a_place_takes_the_catalogue_name_else_its_first_source_name() {
    let mut names = PointNames::new();
    names.insert(("CZ".into(), "57076".into()), "Praha hlavní nádraží".into());
    let first = support::timetable(vec![
        location("57076", "Praha hl.n.", Some("08:00:00"), &["0001"]),
        location("57016", "Kolín", Some("08:20:00"), &["0001"]),
    ]);
    let second = numbered(
        support::timetable(vec![
            location("57016", "Kolin", Some("09:00:00"), &["0001"]),
            location("57076", "Praha", Some("09:20:00"), &["0001"]),
        ]),
        "2",
        "2",
    );
    let (compiled, diagnostics) = compile_named(vec![annual(1, first), annual(2, second)], &names);
    let names: Vec<&str> = compiled
        .timetable
        .places
        .rows()
        .iter()
        .map(|p| &*p.name)
        .collect();
    assert_eq!(names, ["Praha hlavní nádraží", "Kolín"]);
    let differ: Vec<&str> = diagnostics
        .iter()
        .filter(|d| d.code == "czptt_place_names_differ")
        .map(|d| d.subject.as_str())
        .collect();
    assert_eq!(differ, ["czptt:stop:CZ:57076", "czptt:stop:CZ:57016"]);
}

#[test]
fn r054_a_track_shows_its_name_only_when_published_and_a_bus_boards_at_the_bus_point() {
    let message = || {
        let mut named = track(stop("57076", "08:00:00"), "12");
        named.subsidiary.as_mut().expect("track").name = Some("12S".into());
        support::timetable(vec![named, stop("57016", "08:20:00")])
    };
    let (timetable, _) = timetable_of(message());
    let track = point(&timetable, "czptt:stop:CZ:57076:platform:12");
    assert_eq!(
        track.public_code, None,
        "track codes are not published by default"
    );
    assert_eq!(track.source_code.as_deref(), Some("12"));
    assert_eq!(track.source_name.as_deref(), Some("12S"));

    let mut policy = jr_czptt::Policy::builtin();
    policy.tracks.public_code = true;
    let mut diagnostics = jr_model::Diagnostics::new();
    let compiled = jr_czptt::compile(
        vec![annual(1, message())],
        &support::catalog(),
        &PointNames::new(),
        2026,
        &policy,
        &mut diagnostics,
    )
    .expect("compiles");
    let track = point(&compiled.timetable, "czptt:stop:CZ:57076:platform:12");
    assert_eq!(track.public_code.as_deref(), Some("12S"));

    let (timetable, _) = timetable_of(bus_fixture());
    let bus_point = point(&timetable, "czptt:stop:CZ:57050:platform:BUS");
    assert_eq!(bus_point.kind, BoardingPointKind::Platform);
    assert_eq!(bus_point.public_code.as_deref(), Some("BUS"));
    assert!(
        bus_point
            .description
            .as_deref()
            .is_some_and(|d| d.starts_with("Pro přesné"))
    );
    let bus_trip = &timetable.trips.rows()[1];
    let boundary = &bus_trip.calls[0];
    let track_2 = timetable
        .boarding_points
        .iter()
        .find(|(_, p)| p.id.as_str() == "czptt:stop:CZ:57050:platform:2")
        .map(|(handle, _)| handle);
    assert_eq!(boundary.track, track_2);
    assert_ne!(boundary.boarding_point, boundary.track);
}

#[test]
fn r055_a_track_coded_bus_shares_the_bus_point() {
    let (timetable, diagnostics) = timetable_of(support::timetable(vec![
        bus(stop("57076", "08:00:00"), "1"),
        bus(track(stop("57050", "08:10:00"), "BUS"), "1"),
        bus(untimed(track(stop("57050", "08:10:00"), "1")), "0"),
        stop("57016", "08:20:00"),
    ]));
    let shared = point(&timetable, "czptt:stop:CZ:57050:platform:BUS");
    assert_eq!(shared.kind, BoardingPointKind::Track);
    assert!(has(&diagnostics, "czptt_track_coded_bus"));
}

#[test]
fn r056_a_line_version_is_chosen_at_the_first_date() {
    let mut message = support::timetable(vec![
        line(stop("57076", "08:00:00"), "101"),
        line(stop("57016", "08:20:00"), "101"),
    ]);
    message.calendar = calendar(date(2026, 3, 2), None, "1");
    let mut catalog = catalog();
    catalog.lines[0].valid_to = Some(date(2026, 3, 1));
    catalog.lines.push(Line {
        valid_from: Some(date(2026, 3, 2)),
        valid_to: None,
        mark: Some("S1X".into()),
        ..catalog.lines[0].clone()
    });
    let mut diagnostics = jr_model::Diagnostics::new();
    let compiled = jr_czptt::compile(
        vec![annual(1, message)],
        &catalog,
        &PointNames::new(),
        2026,
        &jr_czptt::Policy::builtin(),
        &mut diagnostics,
    )
    .expect("compiles");
    assert_eq!(short_names(&compiled.timetable), ["S1X"]);
}

#[test]
fn r057_the_train_number_is_operational_else_the_tr_core() {
    let (timetable, _) = timetable_of(simple("1"));
    let numbers: Vec<&str> = timetable
        .keys
        .iter()
        .filter(|k| k.namespace == Namespace::CZPTT_TRAIN_NUMBER)
        .map(|k| &*k.identifier)
        .collect();
    assert_eq!(numbers, ["01234"]);

    let mut message = simple("00000000AB12");
    for location in &mut message.locations {
        location.operational_train_number = None;
    }
    let (timetable, diagnostics) = timetable_of(message);
    assert!(has(&diagnostics, "czptt_train_number_from_tr_core"));
    let trip = &timetable.trips.rows()[0];
    assert_eq!(text(&timetable, trip.short_name), Some("Vlak 00000000AB12"));
    assert!(
        !timetable
            .keys
            .iter()
            .any(|k| k.namespace == Namespace::CZPTT_TRAIN_NUMBER)
    );
}

#[test]
fn r058_routes_are_labelled_by_mark_else_category_and_number() {
    let (timetable, _) = timetable_of(support::timetable(vec![
        line(stop("57076", "08:00:00"), "101"),
        line(stop("57050", "08:10:00"), "101"),
        stop("57016", "08:20:00"),
        stop("54357", "09:00:00"),
    ]));
    assert_eq!(short_names(&timetable), ["S1", "Vlak 01234"]);
    assert_eq!(
        timetable.routes.rows()[0].long_name.as_deref(),
        Some("Praha – Kolín")
    );

    let (timetable, _) = timetable_of(numbered(
        support::timetable(vec![
            commercial(bus(stop("53001", "08:00:00"), "1"), "84"),
            commercial(bus(stop("53003", "08:10:00"), "1"), "84"),
        ]),
        "1",
        "363784",
    ));
    assert_eq!(short_names(&timetable), ["Os 363784 (NAD)"]);
    assert_eq!(timetable.routes.rows()[0].mode, Mode::Bus);
    assert_eq!(timetable.routes.rows()[0].gtfs_route_type, 714);

    let (timetable, _) = timetable_of(bus_fixture());
    assert_eq!(short_names(&timetable), ["S1", "S1 (NAD)"]);
}

#[test]
fn r059_fallback_routes_are_one_per_operator_category_and_number() {
    let outbound = numbered(
        support::timetable(vec![
            commercial(stop("57076", "08:00:00"), "RJ"),
            commercial(stop("54357", "10:00:00"), "RJ"),
        ]),
        "1",
        "01234",
    );
    let inbound = numbered(
        support::timetable(vec![
            commercial(stop("54357", "11:00:00"), "RJ"),
            commercial(stop("57076", "13:00:00"), "RJ"),
        ]),
        "2",
        "01234",
    );
    let (compiled, _) = compile_timetable(vec![annual(1, outbound), annual(2, inbound)]);
    let timetable = &compiled.timetable;
    let ids: Vec<&str> = timetable
        .routes
        .rows()
        .iter()
        .map(|r| r.id.as_str())
        .collect();
    assert_eq!(ids, ["czptt:route:fallback:54:RJ:102:01234"]);
    assert_eq!(timetable.trips.len(), 2);

    let (timetable, _) = timetable_of(support::timetable(vec![
        line(stop("57076", "08:00:00"), "101"),
        line(stop("57016", "08:20:00"), "101"),
    ]));
    assert_eq!(
        timetable.routes.rows()[0].id.as_str(),
        "czptt:route:line:101:54:100"
    );
}

#[test]
fn r060_the_first_part_defines_a_shared_route() {
    let first = support::timetable(vec![
        line(stop("57076", "08:00:00"), "101"),
        line(stop("57016", "08:20:00"), "101"),
    ]);
    let mut second = numbered(first.clone(), "2", "2");
    second.calendar = calendar(date(2026, 3, 2), None, "1");
    let mut catalog = catalog();
    catalog.lines[0].valid_to = Some(date(2026, 3, 1));
    catalog.lines.push(Line {
        valid_from: Some(date(2026, 3, 2)),
        valid_to: None,
        name: Some("Praha – Kolín – Pardubice".into()),
        ..catalog.lines[0].clone()
    });
    let mut diagnostics = jr_model::Diagnostics::new();
    let compiled = jr_czptt::compile(
        vec![annual(1, first), annual(2, second)],
        &catalog,
        &PointNames::new(),
        2026,
        &jr_czptt::Policy::builtin(),
        &mut diagnostics,
    )
    .expect("compiles");
    let routes = compiled.timetable.routes.rows();
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].long_name.as_deref(), Some("Praha – Kolín"));
    assert!(has(&diagnostics, "czptt_route_attributes_differ"));
}

#[test]
fn r061_route_colours_follow_the_route_type() {
    let cases = [
        ("Os", 106, "1c1745"),
        ("RJ", 102, "b91c1c"),
        ("R", 103, "b45309"),
    ];
    for (category, route_type, color) in cases {
        let (timetable, _) = timetable_of(support::timetable(vec![
            commercial(stop("57076", "08:00:00"), category),
            commercial(stop("57016", "08:20:00"), category),
        ]));
        let route = &timetable.routes.rows()[0];
        assert_eq!(route.gtfs_route_type, route_type);
        assert_eq!(route.color.as_deref(), Some(color));
        assert_eq!(route.text_color.as_deref(), Some("ffffff"));
    }
    let (timetable, _) = timetable_of(simple("1"));
    assert_eq!(timetable.routes.rows()[0].color.as_deref(), Some("475569"));
}

#[test]
fn r062_r063_parts_are_named_by_category_and_number_and_head_for_the_terminus() {
    let (timetable, _) = timetable_of(support::timetable(vec![
        commercial(
            location("57076", "Praha hl.n.", Some("08:00:00"), &["0001"]),
            "Os",
        ),
        commercial(
            location("57050", "Poříčany", Some("08:10:00"), &["0001"]),
            "RJ",
        ),
        commercial(
            location("57016", "Nymburk", Some("08:20:00"), &["0001"]),
            "RJ",
        ),
        location("57017", "Deadhead", Some("08:30:00"), &[]),
    ]));
    let trips = timetable.trips.rows();
    let names: Vec<Option<&str>> = trips
        .iter()
        .map(|t| text(&timetable, t.short_name))
        .collect();
    assert_eq!(names, [Some("Os 01234"), Some("RJ 01234")]);
    assert!(
        trips
            .iter()
            .all(|t| text(&timetable, t.headsign) == Some("Nymburk"))
    );

    let mut nameless = stop("57016", "08:20:00");
    nameless.primary_name = None;
    let (timetable, _) = timetable_of(support::timetable(vec![
        stop("57076", "08:00:00"),
        nameless,
    ]));
    let trip = &timetable.trips.rows()[0];
    assert_eq!(text(&timetable, trip.headsign), Some("CZ 57016"));
}

#[test]
fn r064_a_path_alternating_modes_gets_one_block_per_mode_block() {
    let (timetable, _) = timetable_of(bus_fixture());
    let blocks: Vec<&str> = timetable
        .trips
        .rows()
        .iter()
        .filter_map(|t| text(&timetable, t.block))
        .collect();
    assert_eq!(
        blocks,
        [
            "czptt:block:Pa%3A54%3A000000001234%3A00%3A2026:mode:1",
            "czptt:block:Pa%3A54%3A000000001234%3A00%3A2026:mode:2",
            "czptt:block:Pa%3A54%3A000000001234%3A00%3A2026:mode:3",
        ]
    );
    let runs: Vec<u16> = timetable
        .trips
        .rows()
        .iter()
        .filter_map(|t| t.run.map(|r| r.part))
        .collect();
    assert_eq!(runs, [1, 2, 3]);

    let (timetable, _) = timetable_of(support::timetable(vec![
        commercial(stop("57076", "08:00:00"), "Os"),
        commercial(stop("57050", "08:10:00"), "RJ"),
        commercial(stop("57016", "08:20:00"), "RJ"),
    ]));
    let blocks: Vec<&str> = timetable
        .trips
        .rows()
        .iter()
        .filter_map(|t| text(&timetable, t.block))
        .collect();
    assert_eq!(
        blocks,
        ["czptt:block:Pa%3A54%3A000000001234%3A00%3A2026"; 2]
    );
}

#[test]
fn r067_pickup_and_dropoff_follow_the_activities() {
    let (timetable, _) = timetable_of(support::timetable(vec![
        location(
            "57076",
            "Praha",
            Some("08:00:00"),
            &["0001", "0028", "0030"],
        ),
        support::point("57060", "08:05:00"),
        location("57050", "Poříčany", Some("08:10:00"), &["0001", "0030"]),
        location(
            "57016",
            "Kolín",
            Some("08:20:00"),
            &["0001", "0029", "0030"],
        ),
    ]));
    let sides: Vec<(Boarding, Boarding)> = timetable.trips.rows()[0]
        .calls
        .iter()
        .map(|c| (c.pickup, c.dropoff))
        .collect();
    assert_eq!(
        sides,
        [
            (Boarding::AskDriver, Boarding::None),
            (Boarding::None, Boarding::None),
            (Boarding::AskDriver, Boarding::AskDriver),
            (Boarding::None, Boarding::AskDriver),
        ]
    );
}

#[test]
fn r068_r069_timepoints_and_source_sequences() {
    let (timetable, _) = timetable_of(support::timetable(vec![
        bus(stop("57076", "08:00:00"), "1"),
        bus(track(stop("57050", "08:10:00"), "2"), "1"),
        untimed(support::point("57055", "08:12:00")),
        bus(untimed(track(stop("57060", "08:15:00"), "1")), "0"),
        stop("57016", "08:20:00"),
    ]));
    let trips = timetable.trips.rows();
    let first: Vec<(i32, bool)> = trips[0]
        .calls
        .iter()
        .map(|c| (c.sequence, c.timepoint))
        .collect();
    assert_eq!(first, [(1, true), (2, true), (4, false)]);
    let second: Vec<(i32, bool)> = trips[1]
        .calls
        .iter()
        .map(|c| (c.sequence, c.timepoint))
        .collect();
    assert_eq!(second, [(4, false), (5, true)]);
    // The untimed railway point between a bus and a rail part belongs to neither.
    assert!(
        !trips
            .iter()
            .any(|t| t.calls.iter().any(|c| c.sequence == 3))
    );
}

#[test]
fn r080_agencies_take_the_catalogue_name_up_to_its_qualifier() {
    let (timetable, diagnostics) = timetable_of(support::timetable(vec![
        stop("57076", "08:00:00"),
        operator(stop("57050", "08:10:00"), Some("80")),
        operator(stop("57060", "08:15:00"), Some("99")),
        operator(stop("57016", "08:20:00"), Some("99")),
    ]));
    let agencies: Vec<(&str, &str, Option<&str>)> = timetable
        .agencies
        .rows()
        .iter()
        .map(|a| (a.id.as_str(), &*a.name, a.url.as_deref()))
        .collect();
    assert_eq!(
        agencies,
        [
            (
                "czptt:agency:54",
                "České dráhy, a.s.",
                Some("https://www.cd.cz")
            ),
            (
                "czptt:agency:80",
                "Česko-německá dráha",
                Some("https://portal.cisjr.cz/")
            ),
            (
                "czptt:agency:99",
                "Unknown 99",
                Some("https://portal.cisjr.cz/")
            ),
        ]
    );
    assert!(has(&diagnostics, "czptt_unknown_operator"));
    assert_eq!(
        timetable.agencies.rows()[0].timezone.as_ref(),
        "Europe/Prague"
    );
}

#[test]
fn r082_parts_sharing_their_boundary_are_linked() {
    let (timetable, _) = timetable_of(support::timetable(vec![
        commercial(stop("57076", "08:00:00"), "Os"),
        commercial(stop("57050", "08:10:00"), "RJ"),
        commercial(stop("57016", "08:20:00"), "RJ"),
    ]));
    assert_eq!(timetable.transfers.len(), 1);
    let transfer = &timetable.transfers[0];
    assert_eq!(transfer.kind, TransferKind::InSeat);
    assert_eq!((transfer.from, transfer.to), (None, None));

    let (timetable, _) = timetable_of(bus_fixture());
    let timed: Vec<(&str, &str)> = timetable
        .transfers
        .iter()
        .map(|t| {
            assert_eq!((t.kind, t.minimum_time), (TransferKind::Timed, Some(0)));
            let id = |location| match location {
                Some(LocationRef::BoardingPoint(point)) => {
                    timetable.boarding_points[point].id.as_str()
                }
                _ => "",
            };
            (id(t.from), id(t.to))
        })
        .collect();
    assert_eq!(
        timed,
        [
            (
                "czptt:stop:CZ:57050:platform:2",
                "czptt:stop:CZ:57050:platform:BUS"
            ),
            (
                "czptt:stop:CZ:57016:platform:BUS",
                "czptt:stop:CZ:57016:platform:3"
            ),
        ]
    );
}

#[test]
fn r118_trips_and_czech_points_carry_national_keys() {
    let mut foreign = track(stop("01003", "08:30:00"), "1");
    foreign.country = "AT".into();
    let (timetable, _) = timetable_of(support::timetable(vec![
        track(stop("57076", "08:00:00"), "12"),
        stop("57016", "08:20:00"),
        foreign,
    ]));
    let keys: Vec<(&str, &str, Entity)> = timetable
        .keys
        .iter()
        .map(|k| (k.namespace.as_str(), &*k.identifier, k.entity))
        .collect();
    let trip = Entity::Trip(timetable.trips.handles().next().expect("trip"));
    let place = |i: usize| Entity::Place(timetable.places.handles().nth(i).expect("place"));
    let track = timetable
        .boarding_points
        .iter()
        .find(|(_, p)| p.id.as_str() == "czptt:stop:CZ:57076:platform:12")
        .map(|(h, _)| Entity::BoardingPoint(h))
        .expect("track");
    assert_eq!(
        keys,
        [
            ("czptt:pa", PA, trip),
            ("czptt:tr", "Tr:54:000000001234:00:2026", trip),
            ("czptt:train_number", "01234", trip),
            ("sr70", "57076", place(0)),
            ("sr70", "57016", place(1)),
            ("sr70:track", "57076:12", track),
        ]
    );
    assert!(
        timetable
            .keys
            .iter()
            .all(|k| k.valid_from == date(2025, 12, 14) && k.valid_to == date(2025, 12, 14))
    );
}
