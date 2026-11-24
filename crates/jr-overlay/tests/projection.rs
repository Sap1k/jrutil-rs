//! Projection (OVL-R077–R121): the overlay's edits applied to small timetables.

mod support;

use jr_formats::geodata::synonyms;
use jr_model::edit::Precedence;
use jr_model::identity::overlay;
use jr_model::timetable::{CoordSource, Entity, Mode, Precision, SourceKey};
use jr_model::{DateSet, Diagnostics, PlaceId, RouteId, Timetable};
use jr_overlay::profile::{BaseProfile, SourceProfile};
use jr_overlay::{Overlaid, OverlayInput, SourceInput};
use support::*;

/// The overlay over the sources, applied; the timetable passes `check()` (apply checks it).
fn applied(
    base: &Timetable,
    sources: Vec<(&SourceProfile, jr_formats::gtfs::Feed)>,
) -> (Timetable, Overlaid, Vec<String>) {
    let base_profile = BaseProfile::road().unwrap();
    let synonyms: Vec<synonyms::Line> = synonym_lines();
    let mut diagnostics = Diagnostics::new();
    let overlaid = jr_overlay::overlay(
        OverlayInput {
            base,
            base_profile: &base_profile,
            sources: sources
                .into_iter()
                .map(|(profile, feed)| SourceInput {
                    profile,
                    feed,
                    overrides: Vec::new(),
                })
                .collect(),
            places: &[],
            synonyms: &synonyms,
        },
        &mut diagnostics,
    )
    .unwrap();
    let mut result = base.clone();
    let precedence = Precedence {
        position: vec![
            CoordSource::Sr70,
            CoordSource::Overlay,
            CoordSource::Catalogue,
        ],
    };
    let mut applying = Diagnostics::new();
    jr_model::apply(
        &mut result,
        overlaid.edits.clone(),
        &precedence,
        &mut applying,
    )
    .unwrap();
    assert!(result.check().is_empty());
    let codes = diagnostics.iter().map(|d| d.code.to_string()).collect();
    (result, overlaid, codes)
}

/// Line `100005` (`5`, bus): A–B–C at 10:00, 10:05, 10:10 every day.
fn line_five() -> (Base, Vec<PlaceId>, RouteId, jr_model::TripId) {
    let mut base = Base::new();
    let places = four_places(&mut base);
    let route = base.route("100005", "5", Mode::Bus);
    let trip = base.trip(
        route,
        days(0..364),
        &[
            (places[0], "10:00"),
            (places[1], "10:05"),
            (places[2], "10:10"),
        ],
    );
    (base, places, route, trip)
}

fn abc<'t>(trip: &'t str, times: [&'t str; 3]) -> Vec<(&'t str, &'t str, &'t str)> {
    vec![
        (trip, "a", times[0]),
        (trip, "b", times[1]),
        (trip, "c", times[2]),
    ]
}

fn trip_by_id<'t>(t: &'t Timetable, id: &str) -> Option<&'t jr_model::timetable::Trip> {
    t.trips.rows().iter().find(|trip| trip.id.as_str() == id)
}

fn running(t: &Timetable, trip: &jr_model::timetable::Trip) -> DateSet {
    t.services.get(trip.service)
}

#[test]
fn ovl_r080_r093_a_bound_trip_takes_posts_and_times() {
    let (base, places, _, trip) = line_five();
    let profile = generic_profile("");
    let feed = trip_feed(
        &profile,
        &[("r", "5", 3)],
        &[("r", "s1", "")],
        &abc("s1", ["10:00", "10:06", "10:10"]),
        "",
    );
    let (t, overlaid, _) = applied(&base.t, vec![(&profile, feed)]);
    let trip = &t.trips[trip];
    assert_eq!(
        trip.calls[1].departure.get().unwrap().seconds(),
        10 * 3600 + 6 * 60
    );
    let point = trip.calls[1].boarding_point.unwrap();
    assert_eq!(t.boarding_points[point].id, overlay::post("test-gtfs", "b"));
    assert_eq!(t.boarding_points[point].place, places[1]);
    assert_eq!(overlaid.summary.trips_bound, 1);
    // R091: the bound stop's centroid places its base place.
    let position = t.places[places[1]].position.unwrap();
    assert_eq!(
        (position.source, position.precision),
        (Some(CoordSource::Overlay), Precision::Exact)
    );
}

#[test]
fn ovl_r087_r112_r115_a_trip_bound_on_some_days_is_sliced_and_its_keys_follow() {
    let (mut base, _, _, trip) = line_five();
    base.t.keys.push(SourceKey {
        namespace: jr_model::timetable::Namespace::CIS_LINE_TRIP,
        identifier: "100005:1".into(),
        entity: Entity::Trip(trip),
        valid_from: horizon().start(),
        valid_to: horizon().end(),
        binding: jr_model::timetable::BindingMethod::Identity,
    });
    let profile = generic_profile("");
    let mut feed = trip_feed(
        &profile,
        &[("r", "5", 3)],
        &[("r", "s1", "")],
        &abc("s1", ["10:00", "10:06", "10:10"]),
        "",
    );
    feed.calendars[0].end_date = Some(day(6));
    let (t, overlaid, _) = applied(&base.t, vec![(&profile, feed)]);
    assert!(running(&t, &t.trips[trip]).is_empty());
    let base_id = base.t.trips[trip].id.clone();
    let week = days(0..7);
    let rest = days(0..364) - week;
    let bound = trip_by_id(&t, overlay::slice(&base_id, &week).as_str()).unwrap();
    let kept = trip_by_id(&t, overlay::slice(&base_id, &rest).as_str()).unwrap();
    assert_eq!((running(&t, bound), running(&t, kept)), (week, rest));
    assert_eq!(
        kept.calls[1].departure.get().unwrap().seconds(),
        10 * 3600 + 5 * 60
    );
    // Both parts keep the national key; the source trip key names the bound part over its run.
    let keyed = |id: &str| {
        t.keys
            .iter()
            .filter(|k| matches!(k.entity, Entity::Trip(x) if t.trips[x].id.as_str() == id))
            .count()
    };
    assert_eq!(keyed(kept.id.as_str()), 1);
    assert_eq!(keyed(bound.id.as_str()), 2);
    let source_key = t
        .keys
        .iter()
        .find(|k| k.namespace.as_str() == "test:gtfs_trip_id")
        .unwrap();
    assert_eq!(
        (source_key.valid_from, source_key.valid_to),
        (day(0), day(6))
    );
    assert_eq!(
        (overlaid.summary.trips_sliced, overlaid.summary.slices),
        (1, 2)
    );
}

#[test]
fn ovl_r086_r090_authority_retires_an_unclaimed_national_trip_and_adds_the_source_trip() {
    let (mut base, places, route, _) = line_five();
    let late = base.trip(
        route,
        days(0..364),
        &[
            (places[0], "12:00"),
            (places[1], "12:05"),
            (places[2], "12:10"),
        ],
    );
    base.serve(&[places[0], places[3]]);
    let profile = claiming_profile("");
    let calls: Vec<_> = abc("s1", ["10:00", "10:05", "10:10"])
        .into_iter()
        .chain([
            ("s2", "a", "14:00"),
            ("s2", "b", "14:05"),
            ("s2", "d", "14:15"),
        ])
        .chain([
            ("s3", "a", "14:00"),
            ("s3", "b", "14:05"),
            ("s3", "d", "14:15"),
        ])
        .collect();
    let feed = trip_feed(
        &profile,
        &[("r", "X", 3), ("q", "X", 3)],
        &[("r", "s1", "1"), ("r", "s2", "1"), ("q", "s3", "1")],
        &calls,
        "r,1,100005\nq,1,100005\n",
    );
    let (t, overlaid, codes) = applied(&base.t, vec![(&profile, feed)]);
    assert!(
        running(&t, &t.trips[late]).is_empty(),
        "{:?} {codes:?}",
        overlaid.summary
    );
    // s2 and s3 run alike: one added trip, named after the first.
    let added = trip_by_id(
        &t,
        overlay::added_trip("test-gtfs", "s2", &days(0..364)).as_str(),
    )
    .unwrap();
    assert_eq!(added.route, route);
    assert_eq!(t.strings.get_opt(added.headsign), Some("Delta"));
    assert_eq!(overlaid.summary.trips_added, 1);
    assert_eq!(overlaid.summary.trips_retired, 1);
    let additions = overlaid
        .reports
        .iter()
        .find(|r| r.name == "source_trip_additions")
        .unwrap();
    assert_eq!(additions.rows[0][2], "100005");
}

#[test]
fn ovl_r082_a_conflict_on_an_owned_line_date_keeps_the_national_trip() {
    let (base, _, _, trip) = line_five();
    let profile = claiming_profile("");
    // Two source trips of one source claim the base trip with other times; neither is newer.
    let calls: Vec<_> = abc("s1", ["10:00", "10:06", "10:10"])
        .into_iter()
        .chain(abc("s2", ["10:00", "10:07", "10:10"]))
        .collect();
    let feed = trip_feed(
        &profile,
        &[("r", "X", 3)],
        &[("r", "s1", "1"), ("r", "s2", "1")],
        &calls,
        "r,1,100005\n",
    );
    let (t, overlaid, codes) = applied(&base.t, vec![(&profile, feed)]);
    assert!(codes.contains(&"overlay_fact_conflict".to_owned()));
    let kept = &t.trips[trip];
    assert_eq!(running(&t, kept), days(0..364));
    assert_eq!(
        kept.calls[1].departure.get().unwrap().seconds(),
        10 * 3600 + 5 * 60
    );
    assert_eq!(overlaid.summary.trips_added, 0);
}

#[test]
fn ovl_r082_the_newer_revision_wins() {
    let (base, _, _, trip) = line_five();
    let profile = generic_profile(
        "[hooks.revision]\ncolumn = \"trip_id\"\npattern = '_(?<revision>\\d{6})(?:_|$)'\ndate_format = \"%y%m%d\"\n",
    );
    let calls: Vec<_> = abc("x_260101", ["10:00", "10:06", "10:10"])
        .into_iter()
        .chain(abc("y_260201", ["10:00", "10:07", "10:10"]))
        .collect();
    let feed = trip_feed(
        &profile,
        &[("r", "5", 3)],
        &[("r", "x_260101", ""), ("r", "y_260201", "")],
        &calls,
        "",
    );
    let (t, _, codes) = applied(&base.t, vec![(&profile, feed)]);
    assert!(codes.contains(&"overlay_newer_source_selected".to_owned()));
    assert_eq!(
        t.trips[trip].calls[1].departure.get().unwrap().seconds(),
        10 * 3600 + 7 * 60
    );
}

#[test]
fn ovl_r079_r081_two_sources_coalesce_on_a_shared_post() {
    let (base, places, _, trip) = line_five();
    let mut first = generic_profile("");
    first.source_id = "one-gtfs".into();
    let mut second = generic_profile("");
    second.source_id = "two-gtfs".into();
    // The second states boarding points only: a different, compatible claim.
    second
        .capabilities
        .remove(&jr_overlay::profile::Capability::Schedules);
    let with_platform = |profile: &SourceProfile| {
        let mut feed = trip_feed(
            profile,
            &[("r", "5", 3)],
            &[("r", "s1", "")],
            &abc("s1", ["10:00", "10:06", "10:10"]),
            "",
        );
        for stop in &mut feed.stops {
            stop.platform_code = Some("a".into());
        }
        feed
    };
    let (t, _, codes) = applied(
        &base.t,
        vec![
            (&first, with_platform(&first)),
            (&second, with_platform(&second)),
        ],
    );
    assert!(codes.contains(&"cross_source_fact_coalesced".to_owned()));
    let point = t.trips[trip].calls[1].boarding_point.unwrap();
    assert_eq!(
        t.boarding_points[point].id,
        overlay::shared_post(&t.places[places[1]].id, "A")
    );
    assert_eq!(t.boarding_points[point].public_code.as_deref(), Some("a"));
}

#[test]
fn ovl_r059_r088_a_trolleybus_part_moves_to_its_derived_route() {
    let (base, _, route, trip) = line_five();
    let mut profile = claiming_profile("");
    profile.authority.modes.push(Mode::Trolleybus);
    let feed = trip_feed(
        &profile,
        &[("tb", "X", 11)],
        &[("tb", "s1", "1")],
        &abc("s1", ["10:00", "10:05", "10:10"]),
        "tb,1,100005\n",
    );
    let (t, overlaid, _) = applied(&base.t, vec![(&profile, feed)]);
    let moved = &t.trips[trip];
    assert_ne!(moved.route, route);
    assert_eq!(
        t.routes[moved.route].id,
        overlay::route("test-gtfs", "tb", "100005")
    );
    assert_eq!(t.routes[moved.route].mode, Mode::Trolleybus);
    assert_eq!(
        t.agencies[t.routes[moved.route].agency].id,
        overlay::agency("test-gtfs", "a")
    );
    assert!(
        t.keys
            .iter()
            .any(|k| k.namespace.as_str() == "cis:line" && k.entity == Entity::Route(moved.route))
    );
    assert_eq!(overlaid.summary.routes_added, 1);
}

#[test]
fn ovl_r101_source_zones_apply_to_every_unzoned_call_at_the_place() {
    let (mut base, places, _, trip) = line_five();
    let other_route = base.route("100009", "9", Mode::Bus);
    let other = base.trip(
        other_route,
        days(0..364),
        &[(places[0], "08:00"), (places[3], "08:30")],
    );
    let profile =
        generic_profile("[capabilities.stop_zones]\n[hooks.zones]\nsystem = \"ids-jmk\"\n");
    let mut feed = trip_feed(
        &profile,
        &[("r", "5", 3)],
        &[("r", "s1", "")],
        &abc("s1", ["10:00", "10:05", "10:10"]),
        "",
    );
    feed.stops[0].zone_id = Some("100".into());
    let (t, _, _) = applied(&base.t, vec![(&profile, feed)]);
    for trip in [trip, other] {
        let zones = t.trips[trip].calls[0]
            .zones
            .map(|z| t.zone_sets.get(z).to_vec())
            .unwrap_or_default();
        assert_eq!(zones.len(), 1);
        assert_eq!(
            (&*zones[0].code, zones[0].system.as_deref()),
            ("100", Some("ids-jmk"))
        );
    }
    assert!(t.trips[other].calls[1].zones.is_none());
}

#[test]
fn ovl_r113_r116_line_course_keys_and_call_keys_where_sequences_differ() {
    let (base, _, _, trip) = line_five();
    let profile = generic_profile(
        "[hooks.operational_crosswalk]\nmember = \"api.txt\"\npattern = '^Linka/CVlaku = trip_id: (?<line>[^/]+)/(?<course>.+) = (?<trip>[^=\\s]+)$'\n",
    );
    let mut feed = trip_feed(
        &profile,
        &[("r", "5", 3)],
        &[("r", "s1", "")],
        &abc("s1", ["10:00", "10:06", "10:10"]),
        "",
    );
    feed.raw.insert(
        "api.txt".into(),
        b"Linka/CVlaku = trip_id: 10/42 = s1\n".to_vec(),
    );
    let (t, _, _) = applied(&base.t, vec![(&profile, feed)]);
    let course = t
        .keys
        .iter()
        .find(|k| k.namespace.as_str() == "test:line_course")
        .unwrap();
    assert_eq!(
        (&*course.identifier, course.entity),
        ("10/42", Entity::Trip(trip))
    );
    // Source sequences 1, 2, 3 against base sequences 1, 2, 3: no call keys.
    assert!(t.call_keys.is_empty());
}

#[test]
fn ovl_r131_a_crosswalked_trip_id_keys_every_trip_of_its_line_and_course() {
    let (base, _, _, _) = line_five();
    let profile = generic_profile(
        "[hooks.operational_crosswalk]\nmember = \"api.txt\"\npattern = '^Linka/CVlaku = trip_id: (?<line>[^/]+)/(?<course>.+) = (?<trip>[^=\\s]+)$'\n",
    );
    // Two calendar variants of course 10/42 (the first week, the second week) and course 10/43.
    let calls: Vec<_> = abc("s1", ["10:00", "10:06", "10:10"])
        .into_iter()
        .chain(abc("s2", ["10:00", "10:06", "10:10"]))
        .chain(abc("s3", ["10:00", "10:06", "10:10"]))
        .collect();
    let mut feed = trip_feed(
        &profile,
        &[("r", "5", 3)],
        &[("r", "s1", ""), ("r", "s2", ""), ("r", "s3", "")],
        &calls,
        "",
    );
    let mut second = feed.calendars[0].clone();
    second.service_id = "w".into();
    second.start_date = Some(day(7));
    second.end_date = Some(day(13));
    let mut third = second.clone();
    third.service_id = "x".into();
    third.start_date = Some(day(14));
    third.end_date = Some(day(20));
    feed.calendars[0].end_date = Some(day(6));
    feed.calendars.extend([second, third]);
    feed.trips[1].service_id = "w".into();
    feed.trips[2].service_id = "x".into();
    feed.raw.insert(
        "api.txt".into(),
        concat!(
            "Linka/CVlaku = trip_id: 10/42 = s1\n",
            "Linka/CVlaku = trip_id: 10/42 = s2\n",
            "Linka/CVlaku = trip_id: 10/43 = s3\n",
        )
        .as_bytes()
        .to_vec(),
    );
    let (t, _, _) = applied(&base.t, vec![(&profile, feed)]);
    let keys = |namespace: &str, id: &str| {
        let mut found: Vec<_> = t
            .keys
            .iter()
            .filter(|k| k.namespace.as_str() == namespace && &*k.identifier == id)
            .map(|k| (k.valid_from, k.valid_to))
            .collect();
        found.sort();
        found
    };
    let (first_week, second_week) = ((day(0), day(6)), (day(7), day(13)));
    assert_eq!(keys("test:gtfs_trip_id", "s1"), [first_week]);
    // Either variant's ID names both weeks' trips; the other course stays its own.
    for id in ["s1", "s2"] {
        assert_eq!(
            keys("test:gtfs_trip_line_course", id),
            [first_week, second_week],
            "{id}"
        );
    }
    assert_eq!(
        keys("test:gtfs_trip_line_course", "s3"),
        [(day(14), day(20))]
    );
    let entity = |id: &str, from| {
        t.keys
            .iter()
            .find(|k| {
                k.namespace.as_str() == "test:gtfs_trip_line_course"
                    && &*k.identifier == id
                    && k.valid_from == from
            })
            .map(|k| k.entity)
    };
    assert_eq!(entity("s1", day(7)), entity("s2", day(7)));
    assert_eq!(
        entity("s2", day(7)),
        t.keys
            .iter()
            .find(|k| k.namespace.as_str() == "test:gtfs_trip_id" && &*k.identifier == "s2")
            .map(|k| k.entity)
    );
}

#[test]
fn ovl_r116_call_keys_only_where_the_source_sequence_differs() {
    let (mut base, places, route, first) = line_five();
    let second = base.trip(
        route,
        days(0..364),
        &[
            (places[0], "12:00"),
            (places[1], "12:05"),
            (places[2], "12:10"),
        ],
    );
    let profile = generic_profile("");
    // trip_feed numbers calls across the feed: s1 has 1, 2, 3, s2 has 4, 5, 6.
    let calls: Vec<_> = abc("s1", ["10:00", "10:05", "10:10"])
        .into_iter()
        .chain(abc("s2", ["12:00", "12:05", "12:10"]))
        .collect();
    let feed = trip_feed(
        &profile,
        &[("r", "5", 3)],
        &[("r", "s1", ""), ("r", "s2", "")],
        &calls,
        "",
    );
    let (t, _, _) = applied(&base.t, vec![(&profile, feed)]);
    assert!(
        t.call_keys
            .iter()
            .all(|k| k.trip == second && &*k.identifier == "s2")
    );
    let pairs: Vec<(&str, i32)> = t
        .call_keys
        .iter()
        .map(|k| (&*k.source_sequence, k.sequence))
        .collect();
    assert_eq!(pairs, [("4", 1), ("5", 2), ("6", 3)]);
    let _ = first;
}

#[test]
fn ovl_r094_a_kept_further_call_shifts_later_calls_and_their_claims() {
    let mut base = Base::new();
    let places = four_places(&mut base);
    base.serve(&[places[1], places[3]]);
    let route = base.route("100005", "5", Mode::Bus);
    let trip = base.trip(
        route,
        days(0..364),
        &[(places[0], "10:00"), (places[2], "10:10")],
    );
    base.t
        .connection_claims
        .push(jr_model::timetable::ConnectionClaim {
            direction: jr_model::timetable::ClaimDirection::WaitsFor,
            origin_trip: trip,
            origin_sequence: 2,
            dates: None,
            target_source_route: None,
            target_source_trip: None,
            target_source_stop: None,
            target_source_post: None,
            target_source_end_stop: None,
            target_source_end_post: None,
            wait_minutes: Some(3),
            note: None,
            target_public_line: None,
            target_destination_text: None,
            derivation: jr_model::timetable::ClaimDerivation::None,
            resolution: jr_model::timetable::ClaimResolution::Unresolved,
            target_route: None,
            target_trip: None,
            target_location: None,
            source_object: "test".into(),
        });
    let mut profile = generic_profile("");
    profile.trips.keep_source_calls = true;
    let feed = trip_feed(
        &profile,
        &[("r", "5", 3)],
        &[("r", "s1", "")],
        &abc("s1", ["10:00", "10:05", "10:10"]),
        "",
    );
    let (t, _, _) = applied(&base.t, vec![(&profile, feed)]);
    let calls = &t.trips[trip].calls;
    let at: Vec<(PlaceId, i32)> = calls.iter().map(|c| (c.place, c.sequence)).collect();
    assert_eq!(at, [(places[0], 1), (places[1], 2), (places[2], 3)]);
    assert_eq!(
        calls[1].arrival.get().unwrap().seconds(),
        10 * 3600 + 5 * 60
    );
    assert_eq!(t.connection_claims[0].origin_sequence, 3);
}
