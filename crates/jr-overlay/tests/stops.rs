//! Stop groups and their matching (OVL-R016, R017, R039–R043, R047, R048).

mod support;

use jr_formats::geodata::registry::{OverlayPlace, Status};
use jr_formats::geodata::stop_overrides;
use jr_model::identity::overlay;
use jr_model::{Diagnostics, Timetable};
use jr_overlay::prepare::{MatchMethod, PlaceRef, Prepared};
use jr_overlay::profile::{BaseProfile, SourceProfile};
use support::*;

/// A feed with route `1` (`route_type`), one trip calling every listed stop in order.
fn stop_feed(profile: &SourceProfile, route_type: u32, stops: &str) -> jr_formats::gtfs::Feed {
    let mut stop_times =
        String::from("trip_id,arrival_time,departure_time,stop_id,stop_sequence\n");
    for (index, line) in stops.lines().skip(1).enumerate() {
        let id = line.split(',').next().unwrap();
        let location_type = line.split(',').nth(4).unwrap_or("");
        if location_type == "1" {
            continue;
        }
        stop_times.push_str(&format!(
            "t1,10:{index:02}:00,10:{index:02}:00,{id},{}\n",
            index + 1
        ));
    }
    let routes = format!("route_id,agency_id,route_short_name,route_type\nr1,a,1,{route_type}\n");
    feed(
        profile,
        &[
            ("agency.txt", AGENCY),
            ("routes.txt", &routes),
            ("trips.txt", "route_id,service_id,trip_id\nr1,s,t1\n"),
            ("stops.txt", stops),
            ("stop_times.txt", &stop_times),
            ("calendar.txt", CALENDAR),
        ],
    )
}

fn prepared<'a>(
    base: &'a Timetable,
    base_profile: &'a BaseProfile,
    profile: &'a SourceProfile,
    feed: jr_formats::gtfs::Feed,
    overrides: Vec<stop_overrides::Row>,
    places: &'a [OverlayPlace],
    synonyms: &'a [jr_formats::geodata::synonyms::Line],
) -> (Prepared<'a>, Diagnostics) {
    let mut diagnostics = Diagnostics::new();
    let prepared = run(
        base,
        base_profile,
        profile,
        feed,
        overrides,
        places,
        synonyms,
        &mut diagnostics,
    );
    (prepared, diagnostics)
}

fn codes(diagnostics: &Diagnostics) -> Vec<&str> {
    diagnostics.iter().map(|d| &*d.code).collect()
}

const PID_HOOKS: &str = r#"
[hooks.stop_group]
key_column = "asw_node_id"
[hooks.post_key]
column = "asw_stop_id"
"#;

#[test]
fn ovl_r016_r039_groups_by_parent_then_key_column_and_name() {
    let profile = generic_profile(PID_HOOKS);
    let stops = "stop_id,stop_name,stop_lat,stop_lon,location_type,parent_station,asw_node_id,asw_stop_id\n\
U1,Alfa,50.0,14.0,1,,,\n\
U1Z1,Alfa nástupiště,50.0001,14.0,0,U1,1,101\n\
U1Z2,Alfa,50.0003,14.0,0,U1,1,102\n\
x1,Beta,50.01,14.0,0,,7,701\n\
x2,Beta,50.0102,14.0,0,,7,\n\
x3,Gamma,50.02,14.0,0,,7,703\n\
y1,Delta,50.03,14.0,0,,,\n";
    let feed = stop_feed(&profile, 3, stops);
    let base = Base::new();
    let base_profile = BaseProfile::road().unwrap();
    let synonyms = synonym_lines();
    let (prepared, _) = prepared(
        &base.t,
        &base_profile,
        &profile,
        feed,
        Vec::new(),
        &[],
        &synonyms,
    );
    let source = &prepared.sources[0];
    let keys: Vec<String> = source
        .groups
        .iter()
        .map(|g| g.registry_key("test-gtfs"))
        .collect();
    assert_eq!(
        keys,
        [
            "test-gtfs:U1",
            "group:test-gtfs:group:7:beta",
            "group:test-gtfs:group:7:gamma",
            "group:test-gtfs:group:y1:delta"
        ]
    );
    let alfa = &source.groups[0];
    assert_eq!(
        (&*alfa.name, alfa.members.len(), alfa.points.len()),
        ("Alfa", 2, 2)
    );
    assert!((alfa.position.unwrap().lat - 50.0002).abs() < 1e-9);
    // R017: the post key column, else the stop ID; a station has none.
    let keys: Vec<Option<&str>> = source.post_keys.iter().map(|k| k.as_deref()).collect();
    assert_eq!(
        keys[..5],
        [None, Some("101"), Some("102"), Some("701"), Some("x2")]
    );
}

fn single(
    base: &Timetable,
    profile: &SourceProfile,
    stops: &str,
    overrides: Vec<stop_overrides::Row>,
) -> (String, String, String, Vec<String>) {
    let base_profile = BaseProfile::road().unwrap();
    let synonyms = synonym_lines();
    let feed = stop_feed(profile, 3, stops);
    let (prepared, diagnostics) = prepared(
        base,
        &base_profile,
        profile,
        feed,
        overrides,
        &[],
        &synonyms,
    );
    let group = prepared.sources[0].groups[0].registry_key("test-gtfs");
    let (target, method, status) = group_row(&prepared, &group);
    let codes = codes(&diagnostics).into_iter().map(str::to_owned).collect();
    (target, method, status, codes)
}

/// Latitude `metres` north of 50°.
fn north(metres: f64) -> f64 {
    50.0 + metres / 111_195.0
}

#[test]
fn ovl_r042_a_unique_name_within_the_radius_places_a_group() {
    let mut base = Base::new();
    let beta = base.place(1, "Beta", Some((50.0, 14.0)));
    let far = base.place(2, "Beta", Some((north(2000.0), 14.0)));
    base.serve(&[beta, far]);
    let profile = generic_profile("");
    let stops = format!(
        "stop_id,stop_name,stop_lat,stop_lon\nsb,Beta,{},14.0\n",
        north(250.0)
    );
    let (target, method, status, _) = single(&base.t, &profile, &stops, Vec::new());
    assert_eq!(
        (target.as_str(), method.as_str(), status.as_str()),
        ("jdf:stop:1", "name_geo_unique", "matched")
    );
    // Beyond the radius the name does not count.
    let stops = format!(
        "stop_id,stop_name,stop_lat,stop_lon\nsb,Beta,{},14.0\n",
        north(1000.0)
    );
    let (_, _, status, codes) = single(&base.t, &profile, &stops, Vec::new());
    assert_eq!(status, "quarantined");
    assert!(codes.contains(&"stop_match_unresolved".to_owned()));
}

#[test]
fn ovl_r042_a_different_facility_prefix_is_not_merged() {
    let mut base = Base::new();
    let clinic = base.place(4, "Poliklinika Budějovická", Some((50.0005, 14.0005)));
    base.serve(&[clinic]);
    let profile = generic_profile("");
    let stops = "stop_id,stop_name,stop_lat,stop_lon\nsg,Budějovická,50.0003,14.0003\n";
    let (target, _, status, _) = single(&base.t, &profile, stops, Vec::new());
    assert_eq!((target.as_str(), status.as_str()), ("", "quarantined"));
}

#[test]
fn ovl_r042_two_best_names_are_ambiguous() {
    let mut base = Base::new();
    let one = base.place(1, "Beta", Some((50.0, 14.0)));
    let two = base.place(2, "Beta", Some((north(100.0), 14.0)));
    base.serve(&[one, two]);
    let profile = generic_profile("");
    let stops = format!(
        "stop_id,stop_name,stop_lat,stop_lon\nsb,Beta,{},14.0\n",
        north(50.0)
    );
    let (_, _, status, codes) = single(&base.t, &profile, &stops, Vec::new());
    assert_eq!(status, "quarantined");
    assert!(codes.contains(&"stop_match_ambiguous".to_owned()));
}

#[test]
fn ovl_r043_coordinate_identity_needs_its_margin() {
    let mut base = Base::new();
    let near = base.place(1, "Úplně jiné jméno", Some((50.0, 14.0)));
    let other = base.place(2, "Další jméno", Some((north(30.0), 14.0)));
    base.serve(&[near, other]);
    let profile = generic_profile("");
    let stops = format!(
        "stop_id,stop_name,stop_lat,stop_lon\nsc,Hlavní,{},14.0\n",
        north(5.0)
    );
    let (target, method, _, codes) = single(&base.t, &profile, &stops, Vec::new());
    assert_eq!(
        (target.as_str(), method.as_str()),
        ("jdf:stop:1", "coordinate_identity_unique")
    );
    assert!(codes.contains(&"stop_coordinate_identity_selected".to_owned()));
    // 12 m and 18 m: the runner-up is not 10 m farther.
    let stops = format!(
        "stop_id,stop_name,stop_lat,stop_lon\nsc,Hlavní,{},14.0\n",
        north(12.0)
    );
    let (_, _, status, codes) = single(&base.t, &profile, &stops, Vec::new());
    assert_eq!(status, "quarantined");
    assert!(codes.contains(&"stop_match_ambiguous".to_owned()));
}

fn override_row(source_id: &str, namespace: &str, target: &str) -> stop_overrides::Row {
    stop_overrides::Row {
        source_namespace: "test-stop-group".into(),
        source_id: source_id.into(),
        target_namespace: namespace.into(),
        target_id: target.into(),
        valid_from: Some(day(0)),
        valid_to: None,
        review_note: "reviewed".into(),
        line: 2,
    }
}

#[test]
fn ovl_r040_r041_reviewed_overrides_win_and_failures_quarantine() {
    let mut base = Base::new();
    let station = base.place(1, "Praha,Letňany", Some((north(1000.0), 14.0)));
    let near = base.place(2, "Letňany", Some((50.0, 14.0)));
    base.serve(&[station, near]);
    let profile = generic_profile("");
    let stops = "stop_id,stop_name,stop_lat,stop_lon,location_type,parent_station\n\
U1000S1,Letňany,50.0,14.0,1,\nU1000Z1,Letňany,50.0001,14.0,0,U1000S1\n";
    let rows = vec![
        override_row("U1000S1", "jdf-stop-place-name", "Praha,Letňany"),
        override_row("U9", "jdf-stop-place-name", "Praha,Letňany"),
    ];
    let (target, method, _, codes) = single(&base.t, &profile, stops, rows);
    assert_eq!(
        (target.as_str(), method.as_str()),
        ("jdf:stop:1", "reviewed_override")
    );
    assert!(codes.contains(&"stop_override_source_missing".to_owned()));
    // B11: an override that does not resolve leaves the group unplaced although a name matches.
    let rows = vec![override_row(
        "U1000S1",
        "jdf-stop-place-name",
        "Praha,Neznámá",
    )];
    let (_, _, status, codes) = single(&base.t, &profile, stops, rows);
    assert_eq!(status, "quarantined");
    assert!(codes.contains(&"stop_override_target_missing".to_owned()));
    // By ID.
    let rows = vec![override_row("U1000S1", "jdf-stop-place", "jdf:stop:2")];
    let (target, method, _, _) = single(&base.t, &profile, stops, rows);
    assert_eq!(
        (target.as_str(), method.as_str()),
        ("jdf:stop:2", "reviewed_override")
    );
}

#[test]
fn ovl_r047_r048_unplaced_stops_of_native_trips_become_source_places() {
    let base = Base::new();
    let base_profile = BaseProfile::road().unwrap();
    let synonyms = synonym_lines();
    let profile = generic_profile("");
    let stops = "stop_id,stop_name,stop_lat,stop_lon\nn1,Nová,50.0,14.0\nn2,Jiná,50.01,14.0\n";
    let pinned = vec![OverlayPlace {
        source_id: "test-gtfs".into(),
        group_key: "group:test-gtfs:group:n1:nová".into(),
        place_id: "overlay:test-gtfs:stop-place:reviewed".into(),
        status: Status::Active,
        note: None,
        line: 2,
    }];
    // Tram (route type 0) is source-native in the test profile.
    let feed = stop_feed(&profile, 0, stops);
    let (tram, _) = prepared(
        &base.t,
        &base_profile,
        &profile,
        feed,
        Vec::new(),
        &pinned,
        &synonyms,
    );
    let (target, method, status) = group_row(&tram, "group:test-gtfs:group:n1:nová");
    assert_eq!(
        (target.as_str(), method.as_str(), status.as_str()),
        (
            "overlay:test-gtfs:stop-place:reviewed",
            "source_native",
            "native"
        )
    );
    let (target, _, _) = group_row(&tram, "group:test-gtfs:group:n2:jiná");
    assert_eq!(
        target,
        overlay::stop_place("test-gtfs", "group:test-gtfs:group:n2:jiná").as_str()
    );
    let source = &tram.sources[0];
    assert!(
        source
            .calls
            .iter()
            .all(|call| matches!(call.place, Some(PlaceRef::Native(_))))
    );
    assert!(
        source
            .groups
            .iter()
            .all(|g| g.matched.unwrap().method == MatchMethod::SourceNative)
    );
    let candidates = &tram.reports()[2];
    assert_eq!(candidates.rows.len(), 1);
    assert_eq!(candidates.rows[0][1], "group:test-gtfs:group:n2:jiná");
    // A bus trip is not source-native: its stops stay unplaced.
    let feed = stop_feed(&profile, 3, stops);
    let (bus, _) = prepared(
        &base.t,
        &base_profile,
        &profile,
        feed,
        Vec::new(),
        &pinned,
        &synonyms,
    );
    assert_eq!(
        group_row(&bus, "group:test-gtfs:group:n1:nová").2,
        "quarantined"
    );
}
