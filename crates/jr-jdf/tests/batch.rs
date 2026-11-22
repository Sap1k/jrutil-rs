//! Version defaults and code checks of a batch (R025, R026, R028, R029, R030, R283).

mod support;

use jiff::civil::date;
use jr_formats::jdf::{NoteType, RouteType, TransportMode, Version};
use jr_jdf::Policy;
use jr_jdf::batch::normalise;
use jr_jdf::calendar::TripCalendarInput;
use jr_model::Diagnostics;
use support::*;

#[test]
fn r028_a_1_9_route_takes_the_version_defaults() {
    let mut source = source(Version::V1_9);
    source.routes.push(old_route(
        "100001",
        date(2026, 1, 1),
        Some(date(2026, 6, 30)),
    ));
    source.trips.push(trip("100001", 1, &[], None));
    let mut diagnostics = Diagnostics::new();
    let batch = normalise(
        "vld-100001.zip",
        source,
        &Policy::builtin(),
        &mut diagnostics,
    );

    let route = &batch.routes[0];
    assert_eq!(
        (route.distinction, route.agency_distinction),
        (0, 0),
        "distinctions"
    );
    assert_eq!(route.transport_mode, Some(TransportMode::Bus));
    assert!(!route.detour && !route.grouped);
    // Defaults nobody reads are not applied.
    assert_eq!((route.uses_stop_posts, route.one_way), (None, None));
    assert_eq!(batch.trips[0].route_distinction, 0);
    assert_eq!(&*batch.name, "vld-100001.zip");
    assert_eq!(codes(&diagnostics), ["jdf_version_defaults"]);
}

#[test]
fn r028_an_empty_flag_of_a_1_11_route_is_false_and_an_empty_mode_stays_absent() {
    let mut source = source(Version::V1_11);
    let mut route = old_route("100001", date(2026, 1, 1), Some(date(2026, 6, 30)));
    route.distinction = Some(3);
    route.agency_distinction = Some(1);
    route.detour = Some(true);
    source.routes.push(route);
    let mut diagnostics = Diagnostics::new();
    let batch = normalise("b", source, &Policy::builtin(), &mut diagnostics);

    let route = &batch.routes[0];
    assert_eq!((route.distinction, route.agency_distinction), (3, 1));
    assert!(route.detour && !route.grouped);
    assert_eq!(route.transport_mode, None);
    assert_eq!(codes(&diagnostics), ["jdf_route_flag_empty"]);
}

#[test]
fn r030_an_open_ended_1_10_validity_stays_open() {
    let mut source = source(Version::V1_10);
    let mut route = old_route("100001", date(2026, 1, 1), None);
    route.distinction = Some(1);
    route.agency_distinction = Some(0);
    route.detour = Some(false);
    route.grouped = Some(false);
    source.routes.push(route);
    let mut diagnostics = Diagnostics::new();
    let batch = normalise("b", source, &Policy::builtin(), &mut diagnostics);

    assert_eq!(batch.routes[0].timetable_valid_to, None);
    assert_eq!(codes(&diagnostics), ["jdf_validity_open_end"]);
}

#[test]
fn r029_a_1_9_note_type_reaches_the_calendar() {
    let mut source = source(Version::V1_9);
    source
        .routes
        .push(old_route("100001", MONDAY, Some(SUNDAY_AFTER_NEXT)));
    source.trips.push(trip("100001", 1, &[], None));
    source.service_notes.push(note(
        "100001",
        1,
        Some(NoteType::ServiceOnly),
        Some(MONDAY),
        None,
        None,
    ));
    let mut diagnostics = Diagnostics::new();
    let batch = normalise("b", source, &Policy::builtin(), &mut diagnostics);

    let codes = batch.attribute_codes();
    let input = TripCalendarInput::for_trip(
        "100001:0",
        &batch.routes[0],
        &batch.trips[0],
        &codes,
        &batch.service_notes,
    );
    let (days, _) = run(&input);
    assert_eq!(days, [MONDAY]);
}

#[test]
fn r025_unknown_fixed_codes_bad_day_numbers_and_duplicates_are_diagnosed() {
    let mut source = source(Version::V1_11);
    source.attribute_refs = vec![
        attribute(1, "X"),
        attribute(2, "Q"),
        attribute(3, "9"),
        attribute(4, "7"),
        attribute(1, "+"),
    ];
    let mut diagnostics = Diagnostics::new();
    let batch = normalise("b", source, &Policy::builtin(), &mut diagnostics);

    assert_eq!(
        support::codes(&diagnostics),
        [
            "jdf_fixed_code_duplicate",
            "jdf_fixed_code_unknown",
            "jdf_day_code_invalid"
        ]
    );
    assert_eq!(batch.attribute_codes().get(1), Some("X"));
}

#[test]
fn r026_an_unlisted_route_type_is_diagnosed() {
    let mut source = source(Version::V1_9);
    let mut route = old_route("100001", MONDAY, Some(SUNDAY_AFTER_NEXT));
    route.route_type = Some(RouteType::Other("Q".into()));
    source.routes.push(route);
    let mut diagnostics = Diagnostics::new();
    normalise("b", source, &Policy::builtin(), &mut diagnostics);
    assert!(support::codes(&diagnostics).contains(&"jdf_code_unknown"));
}

#[test]
fn r283_an_attribute_number_without_a_row_is_ignored() {
    let mut source = source(Version::V1_11);
    let mut route = old_route("100001", MONDAY, Some(SUNDAY_AFTER_NEXT));
    route.distinction = Some(0);
    route.agency_distinction = Some(0);
    route.detour = Some(false);
    route.grouped = Some(false);
    source.routes.push(route);
    source.trips.push(trip("100001", 1, &[1, 5], Some(0)));
    source.attribute_refs.push(attribute(1, "6"));
    let mut diagnostics = Diagnostics::new();
    let batch = normalise("b", source, &Policy::builtin(), &mut diagnostics);
    assert_eq!(support::codes(&diagnostics), ["jdf_attribute_dangling"]);

    let codes = batch.attribute_codes();
    let input =
        TripCalendarInput::for_trip("100001:0", &batch.routes[0], &batch.trips[0], &codes, []);
    assert_eq!(input.codes, ["6"]);
    let (days, _) = run(&input);
    assert_eq!(days, [date(2026, 3, 7), date(2026, 3, 14)]);
}
