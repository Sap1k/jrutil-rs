//! Placing stop groups from trip context (OVL-R044, R045), the approximate-stop correction
//! and split (R046), call places (R054), and independence from the thread count.

mod support;

use jr_model::timetable::Mode;
use jr_model::{Diagnostics, Timetable};
use jr_overlay::prepare::{MatchMethod, PlaceRef, Prepared};
use jr_overlay::profile::{BaseProfile, SourceProfile};
use support::*;

/// Latitude `metres` north of 50°.
fn north(metres: f64) -> f64 {
    50.0 + metres / 111_195.0
}

/// Base line `5`: A (0 m), B (1 km), C (2 km), one trip A 10:00, B 10:05, C 10:10 daily.
fn base(b_estimated: bool) -> Timetable {
    let mut base = Base::new();
    let a = base.place(1, "Alfa", Some((50.0, 14.0)));
    let b = if b_estimated {
        base.estimated_place(2, "Beta", (north(5000.0), 14.0))
    } else {
        base.place(2, "Beta", Some((north(1000.0), 14.0)))
    };
    let c = base.place(3, "Gama", Some((north(2000.0), 14.0)));
    let route = base.route("100005", "5", Mode::Bus);
    base.trip(
        route,
        days(0..364),
        &[(a, "10:00"), (b, "10:05"), (c, "10:10")],
    );
    base.t
}

/// A source trip of line `5` calling a, b, c; b's name and position as given.
fn source(profile: &SourceProfile, b_name: &str, b_metres: f64) -> jr_formats::gtfs::Feed {
    let stops = format!(
        "stop_id,stop_name,stop_lat,stop_lon\na,Alfa,50.0,14.0\nb,{b_name},{},14.0\nc,Gama,{},14.0\n",
        north(b_metres),
        north(2000.0)
    );
    feed(
        profile,
        &[
            ("agency.txt", AGENCY),
            (
                "routes.txt",
                "route_id,agency_id,route_short_name,route_type\nr5,a,5,3\n",
            ),
            ("trips.txt", "route_id,service_id,trip_id\nr5,s,t1\n"),
            ("stops.txt", &stops),
            (
                "stop_times.txt",
                "trip_id,arrival_time,departure_time,stop_id,stop_sequence\n\
t1,10:00:00,10:00:00,a,1\nt1,10:06:00,10:06:00,b,2\nt1,10:10:00,10:10:00,c,3\n",
            ),
            ("calendar.txt", CALENDAR),
        ],
    )
}

fn run_one<'a>(
    base: &'a Timetable,
    base_profile: &'a BaseProfile,
    profile: &'a SourceProfile,
    feed: jr_formats::gtfs::Feed,
    synonyms: &'a [jr_formats::geodata::synonyms::Line],
    diagnostics: &mut Diagnostics,
) -> Prepared<'a> {
    run(
        base,
        base_profile,
        profile,
        feed,
        Vec::new(),
        &[],
        synonyms,
        diagnostics,
    )
}

#[test]
fn ovl_r044_r045_a_trip_places_its_unplaced_stop_from_context() {
    let base = base(false);
    let base_profile = BaseProfile::road().unwrap();
    let synonyms = synonym_lines();
    let profile = generic_profile("");
    // b is 3 km from the base's Beta: names and coordinates leave it unplaced.
    let mut diagnostics = Diagnostics::new();
    let prepared = run_one(
        &base,
        &base_profile,
        &profile,
        source(&profile, "Beta", 4000.0),
        &synonyms,
        &mut diagnostics,
    );
    assert_eq!(
        group_row(&prepared, "group:test-gtfs:group:b:beta"),
        (
            "jdf:stop:2".into(),
            "trip_context_unique".into(),
            "matched".into()
        )
    );
    let inference = &prepared.reports()[1].rows;
    assert_eq!(
        inference,
        &vec![vec![
            "group:test-gtfs:group:b:beta".to_owned(),
            "test-gtfs:t1".into(),
            "2".into(),
            "jdf:stop:2".into(),
            "Beta".into(),
            "trip_context_unique".into(),
        ]]
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == "stop_context_distance_warning")
    );
    // R054: every call now has its place.
    let prepared_source = &prepared.sources[0];
    assert!(
        prepared_source
            .calls
            .iter()
            .all(|call| matches!(call.place, Some(PlaceRef::Base(_))))
    );

    // R045: another name is not accepted.
    let mut diagnostics = Diagnostics::new();
    let prepared = run_one(
        &base,
        &base_profile,
        &profile,
        source(&profile, "Omega", 4000.0),
        &synonyms,
        &mut diagnostics,
    );
    assert_eq!(
        group_row(&prepared, "group:test-gtfs:group:b:omega").2,
        "quarantined"
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == "stop_context_name_conflict")
    );
}

#[test]
fn ovl_r046_the_source_corrects_a_context_matched_approximate_stop() {
    let base = base(true);
    let base_profile = BaseProfile::road().unwrap();
    let synonyms = synonym_lines();
    let profile = generic_profile("");
    let mut diagnostics = Diagnostics::new();
    let prepared = run_one(
        &base,
        &base_profile,
        &profile,
        source(&profile, "Beta u hřiště", 1000.0),
        &synonyms,
        &mut diagnostics,
    );
    let (target, method, status) = group_row(&prepared, "group:test-gtfs:group:b:beta-u-hřiště");
    assert_eq!(
        (target.as_str(), method.as_str(), status.as_str()),
        ("jdf:stop:2", "trip_context_gtfs_authoritative", "corrected")
    );
    let report = &prepared.reports()[0];
    let row = report
        .rows
        .iter()
        .find(|row| row[0] == "group:test-gtfs:group:b:beta-u-hřiště")
        .unwrap();
    assert_eq!(
        (row[4].as_str(), row[9].as_str()),
        ("Beta [?]", "pid_name_and_coordinates")
    );
    let group = prepared.sources[0]
        .groups
        .iter()
        .find(|g| &*g.name == "Beta u hřiště")
        .unwrap();
    assert!(group.matched.unwrap().corrected);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == "stop_approximate_corrected")
    );
}

#[test]
fn ovl_r046_two_source_places_at_one_approximate_stop_stay_apart() {
    let mut base = Base::new();
    let b = base.estimated_place(2, "Beta", (50.0, 14.0));
    base.serve(&[b]);
    let base_profile = BaseProfile::road().unwrap();
    let synonyms = synonym_lines();
    let profile = generic_profile("");
    let stops = format!(
        "stop_id,stop_name,stop_lat,stop_lon,location_type,parent_station\n\
P1,Beta,50.0,14.0,1,\np1,Beta,50.0,14.0,0,P1\nP2,Beta,{lat},14.0,1,\np2,Beta,{lat},14.0,0,P2\n",
        lat = north(100.0)
    );
    let feed = feed(
        &profile,
        &[
            ("agency.txt", AGENCY),
            (
                "routes.txt",
                "route_id,agency_id,route_short_name,route_type\nr,a,7,3\n",
            ),
            ("trips.txt", "route_id,service_id,trip_id\nr,s,t\n"),
            ("stops.txt", &stops),
            (
                "stop_times.txt",
                "trip_id,arrival_time,departure_time,stop_id,stop_sequence\nt,10:00:00,10:00:00,p1,1\nt,10:05:00,10:05:00,p2,2\n",
            ),
            ("calendar.txt", CALENDAR),
        ],
    );
    let mut diagnostics = Diagnostics::new();
    let prepared = run_one(
        &base.t,
        &base_profile,
        &profile,
        feed,
        &synonyms,
        &mut diagnostics,
    );
    assert_eq!(group_row(&prepared, "test-gtfs:P1").2, "quarantined");
    assert_eq!(group_row(&prepared, "test-gtfs:P2").2, "quarantined");
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == "stop_approximate_split")
    );
}

#[test]
fn ovl_r044_results_do_not_depend_on_the_thread_count() {
    let base = base(false);
    let base_profile = BaseProfile::road().unwrap();
    let synonyms = synonym_lines();
    let profile = generic_profile("");
    let reports = |threads: usize| {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        pool.install(|| {
            let mut diagnostics = Diagnostics::new();
            let prepared = run_one(
                &base,
                &base_profile,
                &profile,
                source(&profile, "Beta", 4000.0),
                &synonyms,
                &mut diagnostics,
            );
            assert!(prepared.sources[0].groups.iter().any(|g| {
                g.matched
                    .is_some_and(|m| m.method == MatchMethod::TripContextUnique)
            }));
            (prepared.reports(), diagnostics)
        })
    };
    assert_eq!(reports(1), reports(4));
}
