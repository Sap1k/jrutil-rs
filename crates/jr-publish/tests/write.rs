//! The package writer on a small rail timetable: what it publishes, the decisions the contract
//! fixes (calendar masks, zone placement, the GTFS stop-pair form), and that the package it
//! writes is valid and the same bytes every time.

use std::path::{Path, PathBuf};

use jiff::civil::{Date, date};
use jr_model::geo::Coord;
use jr_model::identity::{self, czptt};
use jr_model::timetable::*;
use jr_model::{
    DateSet, Diagnostic, Diagnostics, Feed, Horizon, OptTime, PublicId, ServiceTime, Severity,
    Timetable,
};
use jr_publish::{
    Column, FeedInfo, Package, PublishError, PublishOptions, SourceEntry, Table, relations,
    validate, write_package,
};

const PATH: &str = "Pa:0054:--KADR012345:00:2026";

fn options() -> PublishOptions {
    PublishOptions {
        compiler_version: "test".into(),
        build_spec_sha256: "2".repeat(64),
        sources: vec![SourceEntry {
            source_id: "national-czptt".into(),
            payload_sha256: "1".repeat(64),
            payload_kind: Some("file".into()),
        }],
        feed_info: FeedInfo {
            publisher_name: "Oběhy".into(),
            publisher_url: "https://obehy.cz".into(),
            lang: "cs".into(),
            contact_email: None,
        },
        approximate_suffix: " [?]".into(),
        gtfs_timed_transfers_as_stop_pairs: false,
        extra_sections: vec![("route_stop_order".into(), serde_json::json!({ "slots": 3 }))],
        publication_eligible: true,
    }
}

fn days(horizon: Horizon, dates: impl IntoIterator<Item = Date>) -> DateSet {
    DateSet::from_dates(horizon, dates).0
}

/// Every weekday from Monday 2026-01-05 to Friday 2026-01-30, except Tuesdays 6 and 20 (two
/// holidays), plus Saturday 17.
fn weekdays(horizon: Horizon) -> DateSet {
    let mut dates = Vec::new();
    let mut day = date(2026, 1, 5);
    while day <= date(2026, 1, 30) {
        let weekend = day.weekday().to_monday_zero_offset() >= 5;
        if !weekend && day != date(2026, 1, 6) && day != date(2026, 1, 20) {
            dates.push(day);
        }
        day = day.tomorrow().unwrap();
    }
    dates.push(date(2026, 1, 17));
    days(horizon, dates)
}

fn place(id: PublicId, kind: PlaceKind, name: &str, position: Option<Position>) -> Place {
    Place {
        id,
        kind,
        name: name.into(),
        source_code: None,
        description: None,
        municipality: Some("Obec".into()),
        district_name: None,
        district_code: None,
        nearby_place: None,
        country: Some("CZ".into()),
        position,
        url: None,
        timezone: (kind == PlaceKind::StopPlace).then(|| "Europe/Prague".into()),
        wheelchair_boarding: Tristate::Unknown,
    }
}

fn track(t: &mut Timetable, place: jr_model::PlaceId, code: &str) -> jr_model::BoardingPointId {
    let id = czptt::track(&t.places[place].id, code);
    t.boarding_points.push(BoardingPoint {
        id,
        place,
        kind: BoardingPointKind::Track,
        public_code: Some(code.into()),
        source_code: Some(code.into()),
        source_name: None,
        description: None,
        position: None,
        wheelchair_boarding: Tristate::Unknown,
    })
}

fn time(hours: i32, minutes: i32) -> OptTime {
    OptTime::some(ServiceTime::from_hms(hours, minutes, 0))
}

fn call(
    place: jr_model::PlaceId,
    point: Option<jr_model::BoardingPointId>,
    sequence: i32,
    arrival: OptTime,
    departure: OptTime,
) -> Call {
    Call {
        place,
        boarding_point: point,
        track: point,
        route_stop: None,
        arrival,
        departure,
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

struct Fixture {
    t: Timetable,
    weekdays: DateSet,
}

/// Three stations with tracks, a railway point, one route; a path in two parts sharing their
/// boundary call (part 1 passes the railway point), another trip, and a trip that no longer
/// runs. A note on part 1's first three calls, a timed transfer, keys, an estimated station.
fn fixture() -> Fixture {
    let horizon = Horizon::gvd(2026).unwrap();
    let mut t = Timetable::new(Feed::Czptt, horizon);
    t.source_version = Some("czptt:20261120T140940".into());
    let agency = t.agencies.push(Agency {
        id: czptt::agency("54"),
        name: "České dráhy, a.s.".into(),
        url: None,
        timezone: "Europe/Prague".into(),
        language: Some("cs".into()),
        phone: None,
        fare_url: None,
        registration: None,
        email: None,
    });
    let exact = |lon, lat| Some(Position::exact(Coord::new(lon, lat), CoordSource::Sr70));
    let alfa = t.places.push(place(
        czptt::station("CZ", "54321"),
        PlaceKind::StopPlace,
        "Alfa",
        exact(14.40, 50.00),
    ));
    let beta = t.places.push(place(
        czptt::station("CZ", "54322"),
        PlaceKind::StopPlace,
        "Beta",
        Some(Position::estimated(
            Coord::new(14.50, 50.05),
            CoordSource::RouteTime,
        )),
    ));
    let gama = t.places.push(place(
        czptt::station("CZ", "54323"),
        PlaceKind::StopPlace,
        "Gama",
        exact(14.60, 50.10),
    ));
    let junction = t.places.push(place(
        czptt::station("CZ", "54399"),
        PlaceKind::OperationalPoint,
        "Odbočka",
        exact(14.45, 50.02),
    ));
    let delta = t.places.push(place(
        czptt::station("CZ", "54324"),
        PlaceKind::StopPlace,
        "Delta",
        exact(14.70, 50.20),
    ));
    let alfa_1 = track(&mut t, alfa, "1");
    let _alfa_9 = track(&mut t, alfa, "9");
    let beta_2 = track(&mut t, beta, "2");
    let gama_1 = track(&mut t, gama, "1");
    let delta_1 = track(&mut t, delta, "1");

    let route = t.routes.push(Route {
        id: czptt::line_route("S1", "54", 106),
        agency,
        mode: Mode::Rail,
        gtfs_route_type: 106,
        short_name: Some("S1".into()),
        long_name: None,
        description: None,
        url: None,
        color: Some("1c1745".into()),
        text_color: Some("ffffff".into()),
        sort_order: None,
        timetable_kind: TimetableKind::Regular,
    });
    let slot = |t: &mut Timetable, sequence, place| {
        let id = identity::route_stop(&t.routes[route].id, None, &t.places[place].id, 1);
        t.route_stops.push(RouteStop {
            id,
            route,
            direction: None,
            sequence,
            place,
        })
    };
    let (slot_alfa, slot_beta, slot_gama) = (
        slot(&mut t, 1, alfa),
        slot(&mut t, 2, beta),
        slot(&mut t, 3, gama),
    );

    let weekdays = weekdays(horizon);
    let running = t.services.intern(weekdays);
    let once = t.services.intern(days(horizon, [date(2026, 2, 7)]));
    let cancelled = t.services.intern(DateSet::EMPTY);
    let zones = |t: &mut Timetable, codes: &[&str]| {
        let list: Vec<Zone> = codes
            .iter()
            .map(|code| Zone {
                code: (*code).into(),
                system: Some("PID".into()),
            })
            .collect();
        t.zone_sets.intern(&list)
    };
    let (zones_0b, zones_0, zones_p) = (
        zones(&mut t, &["0", "B"]),
        zones(&mut t, &["0"]),
        zones(&mut t, &["P"]),
    );
    let run = Some(Run {
        key: t.strings.intern(PATH),
        part: 1,
    });
    let headsign = t.strings.intern_opt(Some("Gama"));
    let trip = |id: PublicId, service, calls: Vec<Call>| Trip {
        id,
        route,
        service,
        direction: None,
        headsign,
        short_name: None,
        block: None,
        run: None,
        wheelchair_accessible: Tristate::Unknown,
        bikes_allowed: Tristate::Yes,
        shape: None,
        calls,
    };

    let boundary = Call {
        route_stop: Some(slot_beta),
        zones: zones_p,
        ..call(beta, Some(beta_2), 3, time(8, 10), time(8, 12))
    };
    let part_1 = t.trips.push(Trip {
        run,
        ..trip(
            czptt::trip(PATH, 1),
            running,
            vec![
                Call {
                    route_stop: Some(slot_alfa),
                    zones: zones_0b,
                    ..call(alfa, Some(alfa_1), 1, OptTime::NONE, time(8, 0))
                },
                Call {
                    passenger: false,
                    pickup: Boarding::None,
                    dropoff: Boarding::None,
                    ..call(junction, None, 2, time(8, 5), time(8, 5))
                },
                boundary,
            ],
        )
    });
    let part_2 = t.trips.push(Trip {
        run: run.map(|run| Run { part: 2, ..run }),
        ..trip(
            czptt::trip(PATH, 2),
            running,
            vec![
                boundary,
                Call {
                    route_stop: Some(slot_gama),
                    ..call(gama, Some(gama_1), 4, time(8, 30), OptTime::NONE)
                },
            ],
        )
    });
    let shape = t.shapes.push(Shape {
        id: identity::content(Feed::Czptt, "shape", &[("test", "1")]),
        generation: ShapeGeneration::Motis,
        points: vec![
            Coord::new(14.40, 50.00),
            Coord::new(14.50, 50.05),
            Coord::new(14.60, 50.10),
        ],
    });
    let other_path = "Pa:0054:--KADR099999:00:2026";
    t.trips.push(Trip {
        shape: Some(TripShape {
            shape,
            offsets: vec![Some(0.0), Some(15_000.0)].into(),
        }),
        ..trip(
            czptt::trip(other_path, 1),
            once,
            vec![
                Call {
                    route_stop: Some(slot_alfa),
                    zones: zones_0,
                    ..call(alfa, Some(alfa_1), 1, OptTime::NONE, time(9, 0))
                },
                Call {
                    route_stop: Some(slot_gama),
                    ..call(gama, Some(gama_1), 2, time(9, 30), OptTime::NONE)
                },
            ],
        )
    });
    let gone = t.trips.push(trip(
        czptt::trip("Pa:0054:--KADR000001:00:2026", 1),
        cancelled,
        vec![
            call(delta, Some(delta_1), 1, OptTime::NONE, time(10, 0)),
            call(alfa, Some(alfa_1), 2, time(10, 30), OptTime::NONE),
        ],
    ));

    let note = t.notes.push(Note {
        id: czptt::note(PATH, "central", 1),
        kind: NoteKind::CzpttCentral,
        label: Some("Bistro".into()),
        text: Some("44|CZ54321||CZ54322||0|".into()),
        valid_from: None,
        valid_to: None,
        note_type: Some("44".into()),
        source_object: czptt::note(PATH, "central", 1).to_string().into(),
    });
    t.assignments.push(Assignment {
        scope: Scope::CallRange {
            trip: part_1,
            from: 1,
            to: 3,
        },
        kind: AssignmentKind::Note,
        source_code: Some("44".into()),
        note: Some(note),
        source_object: None,
    });
    t.transfers.push(Transfer {
        from: Some(LocationRef::BoardingPoint(beta_2)),
        to: Some(LocationRef::BoardingPoint(gama_1)),
        from_route: None,
        to_route: None,
        from_trip: Some(part_1),
        to_trip: Some(part_2),
        kind: TransferKind::Timed,
        minimum_time: Some(0),
        maximum_wait: Some(300),
    });
    let key = |namespace, identifier: &str, entity| SourceKey {
        namespace,
        identifier: identifier.into(),
        entity,
        valid_from: date(2026, 1, 5),
        valid_to: date(2026, 1, 30),
        binding: BindingMethod::Identity,
    };
    t.keys.extend([
        key(Namespace::CZPTT_TRAIN_NUMBER, "1234", Entity::Trip(part_1)),
        key(Namespace::CZPTT_TRAIN_NUMBER, "1234", Entity::Trip(part_2)),
        key(Namespace::CZPTT_TRAIN_NUMBER, "9", Entity::Trip(gone)),
        key(Namespace::SR70, "54321", Entity::Place(alfa)),
        key(
            Namespace::SR70_TRACK,
            "54321:1",
            Entity::BoardingPoint(alfa_1),
        ),
    ]);
    // A JDF-style claim at part 1's first call on one day only, and one on the cancelled trip.
    let claim_day = t.services.intern(days(horizon, [date(2026, 1, 12)]));
    let claim = |origin_trip, dates, source_object: &str| ConnectionClaim {
        direction: ClaimDirection::WaitsFor,
        origin_trip,
        origin_sequence: 1,
        dates,
        target_source_route: Some("100001".into()),
        target_source_trip: None,
        target_source_stop: Some("12".into()),
        target_source_post: None,
        target_source_end_stop: None,
        target_source_end_post: None,
        wait_minutes: Some(5),
        note: Some("čeká na bus".into()),
        target_public_line: None,
        target_destination_text: None,
        derivation: ClaimDerivation::Structured,
        resolution: ClaimResolution::Unresolved,
        target_route: None,
        target_trip: None,
        target_location: None,
        source_object: source_object.into(),
    };
    t.connection_claims.extend([
        claim(part_1, Some(claim_day), "claim:1"),
        claim(gone, None, "claim:2"),
    ]);
    // One restriction on a call, one on a route stop whose calls reach Alfa's and Gama's slots.
    t.restrictions.extend([
        Restriction {
            group: RestrictionGroup::A,
            scope: RestrictionScope::Call {
                trip: part_1,
                sequence: 3,
            },
            source_route_stop: "2".into(),
            source_object: "restriction:1".into(),
        },
        Restriction {
            group: RestrictionGroup::Section,
            scope: RestrictionScope::RouteStop {
                route,
                calls: vec![(part_1, 1), (part_2, 4), (gone, 2)].into(),
            },
            source_route_stop: "1".into(),
            source_object: "restriction:2".into(),
        },
    ]);
    assert_eq!(t.check(), [], "the fixture is a well-formed timetable");
    Fixture { t, weekdays }
}

fn relation<'a>(tables: &'a [Table], name: &str) -> &'a Table {
    tables.iter().find(|table| table.name == name).unwrap()
}

/// A text column as owned values.
fn texts(table: &Table, column: &str) -> Vec<Option<String>> {
    let Some(Column::Str(values)) = table.column(column) else {
        panic!("{column}")
    };
    (0..values.len())
        .map(|row| values.get(row).map(str::to_owned))
        .collect()
}

fn row_of(table: &Table, column: &str, value: &str) -> usize {
    texts(table, column)
        .iter()
        .position(|cell| cell.as_deref() == Some(value))
        .unwrap_or_else(|| panic!("{} has no {column} {value}", table.name))
}

/// `<tmp>/jr-publish-write/<name>/package`, fresh: each test has its own parent directory.
fn out(name: &str) -> PathBuf {
    let parent = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("jr-publish-write")
        .join(name);
    if parent.exists() {
        std::fs::remove_dir_all(&parent).unwrap();
    }
    std::fs::create_dir_all(&parent).unwrap();
    parent.join("package")
}

fn diagnostics() -> Diagnostics {
    let mut diagnostics = Diagnostics::new();
    diagnostics.push(Diagnostic::new(
        "dropped_train",
        Severity::Warning,
        "Pa:0054:--KADR000002:00:2026",
        "no calls",
    ));
    diagnostics
}

#[test]
fn the_written_package_is_valid() {
    let fixture = fixture();
    let root = out("valid");
    let manifest = write_package(&fixture.t, &diagnostics(), &options(), &root).unwrap();
    let package = Package::open(&root).unwrap();
    assert_eq!(validate(&package).unwrap(), []);
    assert_eq!(package.manifest(), &manifest);
    assert_eq!(manifest.relation("trip_call").unwrap().row_count, 7);
    let siblings: Vec<_> = std::fs::read_dir(root.parent().unwrap())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name())
        .collect();
    assert_eq!(
        siblings,
        ["package"],
        "no temporary directory is left behind"
    );
    let diagnostics = package.diagnostics().unwrap();
    assert_eq!(diagnostics["counts_by_code"][0]["count"], 1);
    assert_eq!(
        diagnostics["examples_by_code"]["dropped_train"][0]["source_object_id"],
        "Pa:0054:--KADR000002:00:2026"
    );
    assert_eq!(diagnostics["route_stop_order"]["slots"], 3);
    let feed_info = package.gtfs("feed_info.txt").unwrap();
    assert_eq!(
        feed_info.rows,
        [[
            "Oběhy",
            "https://obehy.cz",
            "cs",
            "20260105",
            "20260207",
            "czptt:20261120T140940",
            ""
        ]]
    );
}

#[test]
fn only_running_trips_and_what_they_reference_are_published() {
    let fixture = fixture();
    let tables = relations(&fixture.t, &options()).unwrap();
    let trips = texts(relation(&tables, "trip"), "trip_id");
    assert_eq!(trips.len(), 3);
    assert!(!trips.contains(&Some(
        czptt::trip("Pa:0054:--KADR000001:00:2026", 1).to_string()
    )));
    let locations = texts(relation(&tables, "location"), "location_id");
    let alfa = czptt::station("CZ", "54321");
    for absent in [
        czptt::track(&alfa, "9"),
        czptt::station("CZ", "54324"),
        czptt::track(&czptt::station("CZ", "54324"), "1"),
    ] {
        assert!(!locations.contains(&Some(absent.to_string())), "{absent}");
    }
    assert_eq!(locations.len(), 4 + 3, "four places, three boarding points");
    let keys = texts(relation(&tables, "source_key"), "identifier");
    assert_eq!(keys.len(), 4, "the cancelled trip's key is not published");
    let calls = relation(&tables, "trip_call");
    let row = row_of(
        calls,
        "location_id",
        &czptt::station("CZ", "54399").to_string(),
    );
    assert_eq!(
        calls.column("passenger_service").unwrap().display(row),
        "false"
    );
    assert_eq!(texts(calls, "boarding_point_id")[row], None);
}

#[test]
fn an_estimated_place_and_its_points_carry_the_suffix() {
    let fixture = fixture();
    let tables = relations(&fixture.t, &options()).unwrap();
    let location = relation(&tables, "location");
    let beta = czptt::station("CZ", "54322");
    for id in [beta.clone(), czptt::track(&beta, "2")] {
        let row = row_of(location, "location_id", id.as_str());
        assert_eq!(texts(location, "name")[row].as_deref(), Some("Beta [?]"));
        assert_eq!(
            texts(location, "coordinate_precision")[row].as_deref(),
            Some("estimated")
        );
        assert_eq!(
            texts(location, "coordinate_source")[row].as_deref(),
            Some("route_time")
        );
    }
    let alfa = row_of(
        location,
        "location_id",
        czptt::station("CZ", "54321").as_str(),
    );
    assert_eq!(texts(location, "name")[alfa].as_deref(), Some("Alfa"));
}

#[test]
fn a_calendar_is_one_weekday_mask_and_its_exceptions() {
    let fixture = fixture();
    let tables = relations(&fixture.t, &options()).unwrap();
    let service = identity::service(Feed::Czptt, &fixture.weekdays).to_string();
    let calendar = relation(&tables, "service_calendar");
    let row = row_of(calendar, "service_id", &service);
    assert_eq!(calendar.column("weekday_mask").unwrap().display(row), "31");
    assert_eq!(
        calendar.column("valid_from").unwrap().display(row),
        "2026-01-05"
    );
    assert_eq!(
        calendar.column("valid_to").unwrap().display(row),
        "2026-01-30"
    );
    let exceptions = relation(&tables, "service_exception");
    let found: Vec<(String, String)> = texts(exceptions, "service_id")
        .iter()
        .enumerate()
        .filter(|(_, id)| id.as_deref() == Some(&service))
        .map(|(row, _)| {
            (
                exceptions.column("service_date").unwrap().display(row),
                exceptions.column("added").unwrap().display(row),
            )
        })
        .collect();
    let expected = [
        ("2026-01-06", "false"),
        ("2026-01-17", "true"),
        ("2026-01-20", "false"),
    ];
    assert_eq!(
        found,
        expected.map(|(day, added)| (day.to_owned(), added.to_owned()))
    );
}

#[test]
fn zones_belong_to_the_route_stop_only_when_its_calls_agree() {
    let fixture = fixture();
    let tables = relations(&fixture.t, &options()).unwrap();
    let route_stop_zone = relation(&tables, "route_stop_zone");
    assert_eq!(route_stop_zone.rows(), 1);
    assert_eq!(texts(route_stop_zone, "zone_code"), [Some("P".to_owned())]);
    assert!(
        texts(route_stop_zone, "route_stop_id")[0]
            .as_deref()
            .unwrap()
            .contains("54322")
    );
    // Alfa's calls disagree (0+B against 0): each call keeps its own zones.
    let call_zone = relation(&tables, "call_zone");
    assert_eq!(
        texts(call_zone, "zone_code"),
        [Some("0"), Some("B"), Some("0")].map(|z| z.map(str::to_owned))
    );
}

#[test]
fn timed_transfers_become_stop_pairs_only_in_gtfs_and_only_on_request() {
    let fixture = fixture();
    let plain = out("transfers-plain");
    write_package(&fixture.t, &Diagnostics::new(), &options(), &plain).unwrap();
    let pairs = out("transfers-pairs");
    let mut stop_pairs = options();
    stop_pairs.gtfs_timed_transfers_as_stop_pairs = true;
    write_package(&fixture.t, &Diagnostics::new(), &stop_pairs, &pairs).unwrap();

    let beta_2 = czptt::track(&czptt::station("CZ", "54322"), "2").to_string();
    let gama_1 = czptt::track(&czptt::station("CZ", "54323"), "1").to_string();
    let read = |root: &Path| {
        Package::open(root)
            .unwrap()
            .gtfs("transfers.txt")
            .unwrap()
            .rows
    };
    assert_eq!(
        read(&plain),
        [[
            beta_2.clone(),
            gama_1.clone(),
            String::new(),
            String::new(),
            czptt::trip(PATH, 1).to_string(),
            czptt::trip(PATH, 2).to_string(),
            "1".into(),
            "0".into()
        ]]
    );
    assert_eq!(
        read(&pairs),
        [[
            beta_2,
            gama_1,
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            "2".into(),
            "0".into()
        ]]
    );
    let serving = Package::open(&pairs).unwrap().relation("transfer").unwrap();
    assert_eq!(serving.column("transfer_type").unwrap().display(0), "1");
    assert_eq!(validate(&Package::open(&pairs).unwrap()).unwrap(), []);
}

#[test]
fn an_agency_without_a_website_gets_the_publisher_url_in_gtfs_only() {
    let fixture = fixture();
    let root = out("agency-url");
    write_package(&fixture.t, &Diagnostics::new(), &options(), &root).unwrap();
    let package = Package::open(&root).unwrap();
    assert_eq!(
        package.gtfs("agency.txt").unwrap().rows[0][2],
        "https://obehy.cz"
    );
    let agency = package.relation("agency").unwrap();
    assert_eq!(texts(&agency, "url"), [None]);
}

#[test]
fn writing_twice_gives_the_same_bytes() {
    let fixture = fixture();
    let (first, second) = (out("twice-1"), out("twice-2"));
    write_package(&fixture.t, &diagnostics(), &options(), &first).unwrap();
    write_package(&fixture.t, &diagnostics(), &options(), &second).unwrap();
    let mut files = vec!["manifest.json".to_owned(), "diagnostics.json".to_owned()];
    files.push("gtfs.zip".to_owned());
    for entry in std::fs::read_dir(first.join("serving")).unwrap() {
        files.push(format!(
            "serving/{}",
            entry.unwrap().file_name().to_string_lossy()
        ));
    }
    assert_eq!(files.len(), 3 + 19);
    for file in files {
        let a = std::fs::read(first.join(&file)).unwrap();
        let b = std::fs::read(second.join(&file)).unwrap();
        assert!(a == b, "{file} differs");
    }
    let again = write_package(&fixture.t, &diagnostics(), &options(), &first);
    assert!(matches!(again, Err(PublishError::OutputExists(_))));
}

#[test]
fn a_duplicate_public_id_is_an_error() {
    let mut fixture = fixture();
    let copy = fixture.t.trips.rows()[0].clone();
    fixture.t.trips.push(copy);
    let error = relations(&fixture.t, &options()).unwrap_err();
    match error {
        PublishError::DuplicateKey { relation, key } => {
            assert_eq!(relation, "trip");
            assert_eq!(key, czptt::trip(PATH, 1).to_string());
        }
        other => panic!("{other}"),
    }
}

#[test]
fn claims_and_restrictions_follow_their_trips_and_route_stops() {
    let fixture = fixture();
    let tables = relations(&fixture.t, &options()).unwrap();
    let claims = relation(&tables, "connection_claim");
    assert_eq!(
        texts(claims, "source_object_id"),
        [Some("claim:1".to_owned())],
        "the claim on the cancelled trip is absent"
    );
    let day = identity::service(Feed::Czptt, &days(fixture.t.horizon, [date(2026, 1, 12)]));
    assert_eq!(texts(claims, "service_id"), [Some(day.to_string())]);
    let calendar = texts(relation(&tables, "service_calendar"), "service_id");
    assert!(calendar.contains(&Some(day.to_string())));

    let restrictions = relation(&tables, "travel_restriction");
    let scopes = texts(restrictions, "scope");
    let slots = texts(restrictions, "route_stop_id");
    let route_stop_rows: Vec<&str> = scopes
        .iter()
        .zip(&slots)
        .filter(|(scope, _)| scope.as_deref() == Some("route_stop"))
        .map(|(_, slot)| slot.as_deref().unwrap())
        .collect();
    assert_eq!(route_stop_rows.len(), 2, "one row per slot reached");
    assert!(route_stop_rows.iter().any(|slot| slot.contains("54321")));
    assert!(route_stop_rows.iter().any(|slot| slot.contains("54323")));
    let call = scopes
        .iter()
        .position(|scope| scope.as_deref() == Some("trip_call"))
        .unwrap();
    assert!(slots[call].as_deref().unwrap().contains("54322"));
    assert_eq!(
        restrictions.column("call_sequence").unwrap().display(call),
        "3"
    );
}
