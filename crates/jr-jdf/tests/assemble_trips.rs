//! Places, routes, trips and calls of the assembled timetable (R405–R446, R448–R451, R466,
//! R469, R473, R478), ported from the F# `JdfToGtfsTests` and the non-post `JdfBundleTests`
//! cases (named in each test). Every timetable passes `check()` (the helper asserts it).

mod support;

use jiff::civil::date;
use jr_formats::geodata::route_rules::{LicencePattern, TransportModeRule};
use jr_formats::jdf::{Km, RouteType, TimeCell, TransportMode};
use jr_jdf::assemble::AssembleError;
use jr_jdf::batch::RouteIntegration;
use jr_jdf::fixed::FixedBatch;
use jr_model::timetable::{
    Boarding, BoardingPointKind, CoordSource, Mode, Precision, TimetableKind, Tristate,
};
use jr_model::{ServiceTime, Timetable};
use support::merge::*;

const LICENCE: &str = "100001";

fn two_stops(name: &str) -> B {
    batch(name)
        .stop(at(stop("Alfa", None, Some("náves")), 50.0, 14.0))
        .stop(at(stop("Beta", Some("střed"), None), 50.01, 14.0))
}

fn simple(trip: u64, codes: &[&str]) -> FixedBatch {
    year_route(two_stops("a"), LICENCE)
        .trip(LICENCE, trip, codes)
        .build()
}

fn trip_ids(timetable: &Timetable) -> Vec<&str> {
    timetable
        .trips
        .rows()
        .iter()
        .map(|t| t.id.as_str())
        .collect()
}

fn place<'t>(timetable: &'t Timetable, name: &str) -> &'t jr_model::timetable::Place {
    timetable
        .places
        .rows()
        .iter()
        .find(|p| &*p.name == name)
        .unwrap_or_else(|| panic!("no place {name}"))
}

fn sorted_names(timetable: &Timetable) -> Vec<&str> {
    let mut names: Vec<&str> = timetable.places.rows().iter().map(|p| &*p.name).collect();
    names.sort();
    names
}

fn text(timetable: &Timetable, sym: Option<jr_model::Sym>) -> Option<&str> {
    timetable.strings.get_opt(sym)
}

fn set_call(
    batch: &mut FixedBatch,
    index: usize,
    arrival: Option<TimeCell>,
    departure: Option<TimeCell>,
) {
    batch.batch.trip_stops[index].arrival = arrival;
    batch.batch.trip_stops[index].departure = departure;
}

fn at_minutes(hour: u16, minute: u16) -> Option<TimeCell> {
    Some(TimeCell::At {
        minutes: hour * 60 + minute,
    })
}

#[test]
fn r415_r418_r433_r469_ids_names_and_unspecified_points() {
    // "JDF conversion emits Oběhy identities and public line numbers".
    let (assembled, _) = assemble(vec![simple(1, &[])]);
    let t = &assembled.timetable;
    assert_eq!(trip_ids(t), ["jdf:trip:100001:251214:1"]);
    assert_eq!(sorted_names(t), ["Alfa,náves", "Beta,střed"]);
    let place = place(t, "Alfa,náves");
    assert_eq!(
        (
            place.municipality.as_deref(),
            place.nearby_place.as_deref(),
            place.district_code.as_deref()
        ),
        (Some("Alfa"), Some("náves"), Some("UL"))
    );
    assert!(place.id.as_str().starts_with("jdf:stop:"));
    let point = &t.boarding_points.rows()[0];
    assert_eq!(point.kind, BoardingPointKind::Unspecified);
    assert!(point.id.as_str().ends_with(":unspecified"));
    assert_eq!(t.source_version.as_deref(), Some("jdf-1.11:20251214"));
}

#[test]
fn r423_r441_unserved_calls_are_not_published_nor_their_stops() {
    let mut batch = year_route(two_stops("a").stop(stop("Gama", None, None)), LICENCE)
        .trip(LICENCE, 1, &[])
        .build();
    set_call(&mut batch, 1, None, Some(TimeCell::OtherRoute));
    let (assembled, _) = assemble(vec![batch]);
    let t = &assembled.timetable;
    assert_eq!(sorted_names(t), ["Alfa,náves", "Gama"]);
    let sequences: Vec<i32> = t.trips.rows()[0].calls.iter().map(|c| c.sequence).collect();
    assert_eq!(sequences, [0, 2]);
}

#[test]
fn r441_a_passing_arrival_beside_a_departure_is_published_with_the_departure() {
    let mut batch = simple(1, &[]);
    set_call(&mut batch, 1, Some(TimeCell::Passes), at_minutes(9, 0));
    let (assembled, diagnostics) = assemble(vec![batch]);
    let call = assembled.timetable.trips.rows()[0].calls[1];
    assert_eq!(call.arrival.get(), Some(ServiceTime::from_hms(9, 0, 0)));
    assert_eq!(count(&diagnostics, "jdf_inconsistent_pass"), 1);
}

#[test]
fn r434_r443_even_trips_run_in_reverse() {
    let mut batch = simple(2, &[]);
    set_call(&mut batch, 0, None, at_minutes(9, 0));
    set_call(&mut batch, 1, None, at_minutes(8, 0));
    let (assembled, _) = assemble(vec![batch]);
    let trip = &assembled.timetable.trips.rows()[0];
    assert_eq!(trip.direction, Some(0));
    let places: Vec<&str> = trip
        .calls
        .iter()
        .map(|c| &*assembled.timetable.places[c.place].name)
        .collect();
    assert_eq!(places, ["Beta,střed", "Alfa,náves"]);
    assert_eq!(trip.calls[0].sequence, 0);
}

#[test]
fn r442_a_short_backwards_step_crosses_midnight_and_a_long_one_drops_the_trip() {
    let mut night = simple(1, &[]);
    set_call(&mut night, 0, None, at_minutes(23, 50));
    set_call(&mut night, 1, at_minutes(0, 10), None);
    let (assembled, _) = assemble(vec![night]);
    let calls = &assembled.timetable.trips.rows()[0].calls;
    assert_eq!(
        calls[1].arrival.get(),
        Some(ServiceTime::from_seconds(86_400 + 600))
    );

    let mut broken = simple(1, &[]);
    set_call(&mut broken, 0, None, at_minutes(10, 0));
    set_call(&mut broken, 1, at_minutes(9, 0), None);
    let (assembled, diagnostics) = assemble(vec![broken]);
    assert!(assembled.timetable.trips.is_empty());
    assert_eq!(count(&diagnostics, "jdf_time_backwards"), 1);
}

#[test]
fn r473_r430_a_trip_with_one_published_call_is_dropped_and_its_route_too() {
    let mut batch = simple(1, &[]);
    set_call(&mut batch, 1, None, Some(TimeCell::Passes));
    let (assembled, diagnostics) = assemble(vec![batch]);
    assert!(assembled.timetable.trips.is_empty());
    assert!(assembled.timetable.routes.is_empty());
    assert_eq!(count(&diagnostics, "jdf_short_trip"), 1);
    assert_eq!(count(&diagnostics, "jdf_route_without_trips"), 1);
    assert_eq!(assembled.summary.dropped.get("jdf_short_trip"), Some(&1));
}

#[test]
fn r440_a_repeated_route_stop_keeps_file_order_and_is_diagnosed() {
    let mut batch = simple(1, &[]);
    batch.batch.trip_stops[1].route_stop_id = 1;
    set_call(&mut batch, 1, None, at_minutes(9, 0));
    let (assembled, diagnostics) = assemble(vec![batch]);
    assert_eq!(assembled.timetable.trips.len(), 1);
    assert_eq!(count(&diagnostics, "jdf_route_stop_repeated"), 1);
}

#[test]
fn r444_pickup_and_drop_off_come_from_the_stop_and_the_call() {
    // "Timed calls at border-only stops are kept without boarding or alighting".
    let mut builder = batch("a")
        .stop_with(stop("Alfa", None, None), &["x"])
        .stop_with(stop("Beta", None, None), &["$"])
        .stop(stop("Gama", None, None))
        .stop(stop("Delta", None, None));
    builder = year_route(builder, LICENCE).trip(LICENCE, 1, &["T"]);
    let opening = builder.code("(");
    let phone = builder.code("!");
    let mut batch = builder.build();
    batch.batch.trip_stops[2].attributes[0] = Some(opening);
    batch.batch.trip_stops[3].attributes[0] = Some(phone);
    let (assembled, _) = assemble(vec![batch]);
    let calls: Vec<(Boarding, Boarding)> = assembled.timetable.trips.rows()[0]
        .calls
        .iter()
        .map(|c| (c.pickup, c.dropoff))
        .collect();
    assert_eq!(
        calls,
        [
            (Boarding::AskDriver, Boarding::AskDriver),
            (Boarding::None, Boarding::None),
            (Boarding::None, Boarding::Regular),
            (Boarding::PhoneAgency, Boarding::PhoneAgency),
        ]
    );
}

#[test]
fn r419_r420_r421_r422_posts_authored_textual_unknown_and_conflicting() {
    // "JDF conversion normalizes zones and preserves both post forms".
    let builder = batch("a")
        .stop(stop("Alfa", None, None))
        .stop(stop("Beta", None, None))
        .post(1, 4, "B")
        .post(2, 9, "");
    let mut batch = year_route(builder, LICENCE)
        .trip(LICENCE, 1, &[])
        .trip(LICENCE, 3, &[])
        .trip(LICENCE, 5, &[])
        .build();
    let texts = ["A", "7", "1", "7", "X", "Y"];
    let posts = [Some(4), Some(9), Some(4), Some(9), Some(77), None];
    for (index, call) in batch.batch.trip_stops.iter_mut().enumerate() {
        call.stop_post_id = posts[index];
        call.stop_post_number = Some(texts[index].into());
    }
    let (assembled, diagnostics) = assemble(vec![batch]);
    let t = &assembled.timetable;
    let mut points: Vec<(&str, Option<&str>)> = t
        .boarding_points
        .rows()
        .iter()
        .map(|p| (p.id.as_str(), p.public_code.as_deref()))
        .collect();
    let stop1 = &place(t, "Alfa").id;
    let stop2 = &place(t, "Beta").id;
    points.sort();
    let expected = [
        (format!("{stop1}:unspecified"), None),
        (format!("{stop1}:post:id:4"), Some("B")),
        (format!("{stop2}:post:id:9"), Some("7")),
        (format!("{stop2}:post:Y"), Some("Y")),
    ];
    let mut expected: Vec<(&str, Option<&str>)> = expected
        .iter()
        .map(|(id, code)| (id.as_str(), *code))
        .collect();
    expected.sort();
    assert_eq!(points, expected);
    assert_eq!(count(&diagnostics, "jdf_unknown_post"), 1);
    assert_eq!(count(&diagnostics, "jdf_post_numbers_conflict"), 1);
}

#[test]
fn r416_r417_r425_positions_wheelchair_boarding_and_location_features() {
    let builder = batch("a")
        .stop_with(at(stop("Alfa", None, None), 50.0, 14.0), &["@", "~"])
        .stop(estimated(stop("Beta", None, None), 50.1, 14.0))
        .stop(stop("Gama", None, None));
    let (assembled, diagnostics) = assemble(vec![
        year_route(builder, LICENCE).trip(LICENCE, 1, &[]).build(),
    ]);
    let t = &assembled.timetable;
    let places = [place(t, "Alfa"), place(t, "Beta"), place(t, "Gama")];
    let first = places[0].position.expect("exact");
    assert_eq!(
        (first.precision, first.source),
        (Precision::Exact, Some(CoordSource::Osm))
    );
    assert_eq!(
        places[1].position.map(|p| p.precision),
        Some(Precision::Estimated)
    );
    assert_eq!(places[2].position, None);
    assert_eq!(count(&diagnostics, "jdf_stop_without_position"), 1);
    assert_eq!(places[0].wheelchair_boarding, Tristate::Yes);
    assert_eq!(places[1].wheelchair_boarding, Tristate::Unknown);
    // The stop's code speaks for its boarding points.
    let accessible: Vec<bool> = t
        .boarding_points
        .rows()
        .iter()
        .map(|point| point.wheelchair_boarding == Tristate::Yes)
        .collect();
    assert_eq!(accessible.iter().filter(|yes| **yes).count(), 1);
    let kinds: Vec<&str> = t.assignments.iter().map(|a| a.kind.as_str()).collect();
    assert_eq!(
        kinds,
        ["wheelchair_accessible", "urban_transport_interchange"]
    );
}

#[test]
fn r426_agencies_carry_website_phone_and_ico() {
    let mut batch = simple(1, &[]);
    batch.batch.agencies[0].website = Some("www.example.cz".into());
    batch.batch.agencies[0].office_phone = "123 456 789".into();
    let (assembled, _) = assemble(vec![batch]);
    let agency = &assembled.timetable.agencies.rows()[0];
    assert_eq!(agency.id.as_str(), "jdf:agency:12345678:1");
    assert_eq!(agency.url.as_deref(), Some("http://www.example.cz"));
    assert_eq!(agency.phone.as_deref(), Some("123 456 789"));
    assert_eq!(agency.registration.as_deref(), Some("12345678"));
}

fn version(
    name: &str,
    licence: &str,
    detour: bool,
    from: jiff::civil::Date,
    long_name: &str,
) -> FixedBatch {
    let mut batch = two_stops(name)
        .route(licence, from, Some(date(2026, 12, 12)), detour)
        .trip(licence, 1, &[])
        .build();
    batch.batch.routes[0].name = long_name.into();
    batch
}

#[test]
fn r427_r428_r429_r430_versions_collapse_into_a_regular_and_a_detour_route() {
    // "Route versions collapse into one regular and one detour route", "A later version with
    // different semantics gets a hashed route id".
    let (assembled, _) = assemble(vec![
        version("a", LICENCE, false, date(2025, 12, 14), "Alfa - Beta"),
        version("b", LICENCE, true, date(2026, 3, 1), "Alfa - Beta"),
        version("c", "100002", false, date(2025, 12, 14), "Gama"),
        version("d", "100002", false, date(2026, 6, 1), "Gama nově"),
    ]);
    let t = &assembled.timetable;
    let routes: Vec<(&str, TimetableKind, Option<&str>)> = t
        .routes
        .rows()
        .iter()
        .map(|r| (r.id.as_str(), r.timetable_kind, r.description.as_deref()))
        .collect();
    assert_eq!(
        routes[0],
        ("jdf:route:100001", TimetableKind::Regular, None)
    );
    assert_eq!(
        routes[1],
        (
            "jdf:route:100001:detour",
            TimetableKind::Detour,
            Some("Výlukový jízdní řád")
        )
    );
    assert_eq!(routes[2].0, "jdf:route:100002");
    assert!(
        routes[3].0.starts_with("jdf:route:100002:")
            && routes[3].0.len() == "jdf:route:100002:".len() + 8
    );
    let route = &t.routes.rows()[0];
    assert_eq!((route.mode, route.gtfs_route_type), (Mode::Bus, 701));
    assert_eq!(
        (route.color.as_deref(), route.text_color.as_deref()),
        (Some("0076a3"), Some("ffffff"))
    );
}

#[test]
fn r427_r428_route_types_and_colours_follow_mode_and_type() {
    // "Extended bus types reserve coach for long-distance routes", "JDF routes receive default
    // colors by transport mode".
    let typed = |name: &str, licence: &str, route_type: RouteType, mode: TransportMode| {
        let mut batch = year_route(two_stops(name), licence)
            .route_type(route_type)
            .trip(licence, 1, &[])
            .build();
        batch.batch.routes[0].transport_mode = Some(mode);
        batch
    };
    let (assembled, _) = assemble(vec![
        typed("a", "100001", RouteType::City, TransportMode::Bus),
        typed(
            "b",
            "100002",
            RouteType::LongDistanceNational,
            TransportMode::Bus,
        ),
        typed("c", "100003", RouteType::City, TransportMode::Tram),
    ]);
    let types: Vec<(i32, Option<&str>)> = assembled
        .timetable
        .routes
        .rows()
        .iter()
        .map(|r| (r.gtfs_route_type, r.color.as_deref()))
        .collect();
    assert_eq!(
        types,
        [
            (704, Some("0076a3")),
            (202, Some("004f71")),
            (900, Some("7a0200"))
        ]
    );
}

#[test]
fn r408_the_public_line_number() {
    // "Ambiguous or malformed public line identity is not guessed".
    let integration = |route: &str, name: &str| RouteIntegration {
        route_id: route.into(),
        entry_number: 1,
        transport_system_id: 30001,
        route_name: name.into(),
        preferential: Some(true),
        reserved1: "".into(),
        route_distinction: 1,
    };
    let mut designated = simple(1, &[]);
    designated
        .batch
        .route_integrations
        .push(integration(LICENCE, "015"));
    let (assembled, _) = assemble(vec![designated]);
    assert_eq!(
        assembled.timetable.routes.rows()[0].short_name.as_deref(),
        Some("15")
    );

    let (assembled, _) = assemble(vec![version("a", "100123", false, date(2025, 12, 14), "x")]);
    assert_eq!(
        assembled.timetable.routes.rows()[0].short_name.as_deref(),
        Some("123")
    );

    let (assembled, diagnostics) =
        assemble(vec![version("a", "X12", false, date(2025, 12, 14), "x")]);
    assert_eq!(assembled.timetable.routes.rows()[0].short_name, None);
    assert_eq!(count(&diagnostics, "jdf_public_line_missing"), 1);
}

fn rule(licence: LicencePattern, public: Option<(i32, i32)>, line: u64) -> TransportModeRule {
    TransportModeRule {
        agency_ico: Some(AGENCY.into()),
        licence: "x".into(),
        licence_pattern: licence,
        public_lines: public
            .map(|(low, high)| jr_formats::geodata::route_rules::PublicLines { low, high }),
        expected_mode: "A".into(),
        effective_mode: "E".into(),
        reason: "Tramvaj".into(),
        line,
    }
}

#[test]
fn r405_r406_r407_reviewed_transport_mode_rules() {
    // "Reviewed transport mode rule requires every guard", "The most specific transport mode
    // rule wins".
    let range = LicencePattern::Range {
        low: "100000".into(),
        high: "100009".into(),
    };
    let (assembled, diagnostics) = assemble_with(
        vec![simple(1, &[])],
        &[rule(range.clone(), Some((1, 1)), 2)],
    );
    let assembled = assembled.expect("assembles");
    let route = &assembled.timetable.routes.rows()[0];
    assert_eq!((route.mode, route.gtfs_route_type), (Mode::Tram, 900));
    assert_eq!(count(&diagnostics, "jdf_transport_mode_corrected"), 1);

    let (_, diagnostics) = assemble_with(
        vec![simple(1, &[])],
        &[rule(range.clone(), Some((2, 9)), 2)],
    );
    assert_eq!(count(&diagnostics, "jdf_transport_mode_rule_mismatch"), 1);

    let (tie, _) = assemble_with(
        vec![simple(1, &[])],
        &[rule(range.clone(), None, 2), rule(range, None, 3)],
    );
    assert!(matches!(tie, Err(AssembleError::RuleTie { lines }) if lines == [2, 3]));
}

#[test]
fn r478_a_version_without_a_transport_mode_is_a_bus() {
    let mut batch = simple(1, &[]);
    batch.batch.routes[0].transport_mode = None;
    let (assembled, diagnostics) = assemble(vec![batch]);
    assert_eq!(assembled.timetable.routes.rows()[0].mode, Mode::Bus);
    assert_eq!(count(&diagnostics, "jdf_transport_mode_missing"), 1);
}

#[test]
fn r435_r436_r437_r438_headsign_short_name_and_closed_world_flags() {
    let (assembled, _) = assemble(vec![simple(1, &["@", "O"])]);
    let t = &assembled.timetable;
    let trip = &t.trips.rows()[0];
    assert_eq!(text(t, trip.headsign), Some("Beta,střed"));
    assert_eq!(text(t, trip.short_name), Some("100001 1"));
    assert_eq!(
        (trip.wheelchair_accessible, trip.bikes_allowed),
        (Tristate::Yes, Tristate::Yes)
    );
    let (assembled, _) = assemble(vec![simple(1, &[])]);
    let trip = &assembled.timetable.trips.rows()[0];
    assert_eq!(
        (trip.wheelchair_accessible, trip.bikes_allowed),
        (Tristate::No, Tristate::No)
    );
}

#[test]
fn r445_r446_r448_r449_r451_kilometres_timepoints_and_zones() {
    let mut batch = year_route(two_stops("a").stop(stop("Gama", None, None)), LICENCE)
        .trip(LICENCE, 1, &[])
        .build();
    batch.batch.route_stops[0].zone = Some(" 6, 193,6,".into());
    // Route stop 3 names a second stop: no zones there.
    let mut twin = batch.batch.route_stops[2].clone();
    twin.stop_id = 1;
    twin.zone = Some("9".into());
    batch.batch.route_stops.push(twin);
    batch.batch.route_stops[2].zone = Some("9".into());
    batch.batch.trip_stops[1].km = Some(Km {
        units: 125,
        scale: 1,
    });
    let (assembled, diagnostics) = assemble(vec![batch]);
    let t = &assembled.timetable;
    let calls = &t.trips.rows()[0].calls;
    let zones: Vec<&str> = t
        .zone_sets
        .get(calls[0].zones.expect("zones"))
        .iter()
        .map(|z| &*z.code)
        .collect();
    assert_eq!(zones, ["6", "193"]);
    assert!(
        t.zone_sets
            .get(calls[0].zones.expect("zones"))
            .iter()
            .all(|z| z.system.is_none())
    );
    assert_eq!(calls[2].zones, None);
    assert_eq!(count(&diagnostics, "jdf_route_stop_ambiguous"), 1);
    assert_eq!(calls[1].kilometre, Some(12.5));
    assert!(
        calls
            .iter()
            .all(|c| c.timepoint && c.passenger && c.track.is_none())
    );
}

#[test]
fn r466_cis_keys_name_lines_and_trips() {
    let (assembled, _) = assemble(vec![version("a", "1401", false, date(2025, 12, 14), "x")]);
    let keys: Vec<(&str, &str)> = assembled
        .timetable
        .keys
        .iter()
        .map(|k| (k.namespace.as_str(), &*k.identifier))
        .collect();
    assert_eq!(
        keys,
        [("cis:line_trip", "001401:1"), ("cis:line", "001401")]
    );
    let key = &assembled.timetable.keys[0];
    assert_eq!(
        (key.valid_from, key.valid_to),
        (date(2025, 12, 14), date(2026, 12, 12))
    );

    let (assembled, diagnostics) =
        assemble(vec![version("a", "X12", false, date(2025, 12, 14), "x")]);
    assert!(assembled.timetable.keys.is_empty());
    assert_eq!(count(&diagnostics, "jdf_no_cis_key"), 1);
}
