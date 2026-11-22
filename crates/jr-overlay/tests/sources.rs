//! Reading a source against its profile: eligibility (OVL-R013, R028), calendars (R029), the
//! fold (R056), the hooks (R015, R018, R019), the feed window (R020), calls (R054) and route
//! candidates (R049–R051).

mod support;

use jr_model::timetable::Mode;
use jr_model::{Date, Diagnostics, Timetable};
use jr_overlay::prepare::Prepared;
use jr_overlay::profile::{BaseProfile, RouteTier, SourceProfile};
use support::*;

fn prepare_feed<'a>(
    base: &'a Timetable,
    base_profile: &'a BaseProfile,
    profile: &'a SourceProfile,
    feed: jr_formats::gtfs::Feed,
    synonyms: &'a [jr_formats::geodata::synonyms::Line],
) -> (Prepared<'a>, Diagnostics) {
    let mut diagnostics = Diagnostics::new();
    let prepared = run(
        base,
        base_profile,
        profile,
        feed,
        Vec::new(),
        &[],
        synonyms,
        &mut diagnostics,
    );
    (prepared, diagnostics)
}

const STOPS: &str = "stop_id,stop_name,stop_lat,stop_lon\na,Alfa,50.0,14.0\nb,Beta,50.01,14.0\n";

#[test]
fn ovl_r013_r028_r029_r056_eligible_trips_with_calendars_and_the_fold() {
    let base = Base::new();
    let base_profile = BaseProfile::road().unwrap();
    let synonyms = synonym_lines();
    let profile = generic_profile("");
    // Weekdays of the first week (Mon 15 – Fri 19 Dec 2025) less Wednesday, plus Sunday 21.
    let calendar = "service_id,monday,tuesday,wednesday,thursday,friday,saturday,sunday,start_date,end_date\n\
w,1,1,1,1,1,0,0,20251215,20251219\nlast,1,1,1,1,1,1,1,20261212,20261212\n";
    let dates = "service_id,date,exception_type\nw,20251217,2\nw,20251221,1\nw,20270101,1\n";
    let feed = feed(
        &profile,
        &[
            ("agency.txt", AGENCY),
            (
                "routes.txt",
                "route_id,agency_id,route_short_name,route_type\nbus,a,1,3\nrail,a,S1,2\ncoach,a,2,202\n",
            ),
            (
                "trips.txt",
                "route_id,service_id,trip_id\nbus,w,day\nbus,w,night\nrail,w,train\ncoach,last,late\nnone,w,orphan\n",
            ),
            ("stops.txt", STOPS),
            (
                "stop_times.txt",
                "trip_id,arrival_time,departure_time,stop_id,stop_sequence\n\
day,10:00:00,10:00:00,a,1\nday,10:05:00,10:05:00,b,2\n\
night,23:59:50,24:00:10,a,1\nnight,24:30:00,24:30:00,b,2\n\
train,10:00:00,10:00:00,a,1\ntrain,10:05:00,10:05:00,b,2\n\
late,24:10:00,24:10:00,a,1\nlate,24:20:00,24:20:00,b,2\n",
            ),
            ("calendar.txt", calendar),
            ("calendar_dates.txt", dates),
        ],
    );
    let (prepared, diagnostics) = prepare_feed(&base.t, &base_profile, &profile, feed, &synonyms);
    let source = &prepared.sources[0];
    let ids: Vec<&str> = source
        .trips
        .iter()
        .map(|trip| &*source.feed.trips[trip.feed_index as usize].trip_id)
        .collect();
    assert_eq!(ids, ["day", "night", "late"]);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == "overlay_trip_route_missing")
    );
    let day_trip = &source.trips[0];
    assert_eq!(day_trip.dates.iter().collect::<Vec<_>>(), [1, 2, 4, 5, 7]);
    // R056: the night trip runs a day later, 24 hours earlier, its first arrival before midnight.
    let night = &source.trips[1];
    assert!(night.folded);
    assert_eq!(night.dates.iter().collect::<Vec<_>>(), [2, 3, 5, 6, 8]);
    let calls = source.trip_calls(night);
    assert_eq!(
        (calls[0].arrival, calls[0].departure, calls[1].arrival),
        (Some(-10), Some(10), Some(1800))
    );
    // The last day's late trip leaves the horizon.
    assert!(source.trips[2].dates.is_empty());
    assert_eq!(source.folded_days_lost, 1);
}

#[test]
fn ovl_r015_r018_r019_r020_hooks_read_what_the_profile_names() {
    let base = Base::new();
    let base_profile = BaseProfile::road().unwrap();
    let synonyms = synonym_lines();
    let profile = generic_profile(
        r#"
[hooks.revision]
column = "trip_id"
pattern = '_(?<revision>\d{6})(?:_|$)'
date_format = "%y%m%d"
[hooks.cis_line_claim]
table = "route_sub_agencies.txt"
source_keys = ["route_id", "sub_agency_id"]
lookup_keys = ["route_id", "sub_agency_id"]
value_column = "route_licence_number"
[hooks.operational_crosswalk]
member = "api.txt"
pattern = '^Linka/CVlaku = trip_id: (?<line>[^/]+)/(?<course>.+) = (?<trip>[^=\s]+)$'
"#,
    );
    let api = "Linka/CVlaku = trip_id: 10/42 = 991_1_260207\nnonsense\n\n";
    let feed = feed(
        &profile,
        &[
            ("agency.txt", AGENCY),
            (
                "routes.txt",
                "route_id,agency_id,route_short_name,route_type\nL1,a,1,3\n",
            ),
            (
                "trips.txt",
                "route_id,service_id,trip_id,sub_agency_id\nL1,s,991_1_260207,23\nL1,s,991_2,24\nL1,s,991_3,25\n",
            ),
            (
                "route_sub_agencies.txt",
                "route_id,sub_agency_id,route_licence_number\nL1,23,100001\nL1,24,100001\nL1,24,100002\nL12,3,999\n",
            ),
            ("stops.txt", STOPS),
            (
                "stop_times.txt",
                "trip_id,arrival_time,departure_time,stop_id,stop_sequence\n",
            ),
            ("calendar.txt", CALENDAR),
            (
                "feed_info.txt",
                "feed_publisher_name,feed_publisher_url,feed_lang,feed_start_date,feed_end_date\nX,https://x,cs,20251201,20260131\n",
            ),
            ("api.txt", api),
        ],
    );
    let (prepared, diagnostics) = prepare_feed(&base.t, &base_profile, &profile, feed, &synonyms);
    let source = &prepared.sources[0];
    assert_eq!(
        source.trips[0].revision,
        Some(Date::new(2026, 2, 7).unwrap())
    );
    assert_eq!(source.trips[1].revision, None);
    assert_eq!(source.trips[0].line_claim.as_deref(), Some("100001"));
    // Two distinct licences assert nothing; a composite key does not collide (B10).
    assert_eq!(source.trips[1].line_claim, None);
    assert_eq!(source.trips[2].line_claim, None);
    let row = &source.crosswalk[source.trips[0].operational.unwrap() as usize];
    assert_eq!((&*row.line, &*row.course), ("10", "42"));
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == "overlay_crosswalk_line_rejected")
    );
    assert_eq!(
        source.window,
        Some((
            Date::new(2025, 12, 1).unwrap(),
            Date::new(2026, 1, 31).unwrap()
        ))
    );
}

#[test]
fn ovl_r049_r050_r051_route_candidates_by_line_claim_and_structure() {
    let mut base = Base::new();
    let a = base.place(1, "Alfa", Some((50.0, 14.0)));
    let b = base.place(2, "Beta", Some((50.01, 14.0)));
    let claimed = base.route("100777", "X", Mode::Bus);
    let detour = base.route_with("100777", "X", Mode::Bus, true);
    let same_number = base.route("100001", "1", Mode::Trolleybus);
    let variant = base.route("100004", "H4", Mode::Bus);
    let tram = base.route("100009", "1", Mode::Tram);
    for route in [claimed, detour, same_number, variant, tram] {
        base.trip(route, days(0..364), &[(a, "10:00"), (b, "10:05")]);
    }
    let base_profile = BaseProfile::road().unwrap();
    let synonyms = synonym_lines();
    let profile = generic_profile(
        r#"
[hooks.cis_line_claim]
table = "route_sub_agencies.txt"
source_keys = ["route_id", "sub_agency_id"]
lookup_keys = ["route_id", "sub_agency_id"]
value_column = "route_licence_number"
"#,
    );
    let feed = feed(
        &profile,
        &[
            ("agency.txt", AGENCY),
            (
                "routes.txt",
                "route_id,agency_id,route_short_name,route_type\nc,a,9,3\nn,a,1,3\nh,a,H,3\nblank,a,,3\n",
            ),
            (
                "trips.txt",
                "route_id,service_id,trip_id,sub_agency_id\nc,s,tc,1\nn,s,tn,\nh,s,th,\nblank,s,tb,\n",
            ),
            (
                "route_sub_agencies.txt",
                "route_id,sub_agency_id,route_licence_number\nc,1,100777\n",
            ),
            ("stops.txt", STOPS),
            (
                "stop_times.txt",
                "trip_id,arrival_time,departure_time,stop_id,stop_sequence\n",
            ),
            ("calendar.txt", CALENDAR),
        ],
    );
    let (prepared, diagnostics) = prepare_feed(&base.t, &base_profile, &profile, feed, &synonyms);
    let source = &prepared.sources[0];
    let routes = &source.routes;
    // R049: the claimed line's routes, detour included, sorted by ID.
    assert_eq!(routes[0].primary_tier, RouteTier::CompanionAssertion);
    assert_eq!(routes[0].primary, [claimed, detour]);
    // R050: the same number in a compatible mode (trolleybus for bus), not the tram.
    assert_eq!(routes[1].primary_tier, RouteTier::StructuralTripEvidence);
    assert_eq!(routes[1].primary, [same_number]);
    // R051: H takes H4 as a relaxed candidate only.
    assert!(routes[2].primary.is_empty());
    assert_eq!(routes[2].structural, [variant]);
    // B21: no number, no candidates.
    assert!(routes[3].primary.is_empty() && routes[3].structural.is_empty());
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == "overlay_route_label_missing")
    );
}

#[test]
fn ovl_r054_non_contiguous_calls_are_read_per_trip_in_sequence_order() {
    let base = Base::new();
    let base_profile = BaseProfile::road().unwrap();
    let synonyms = synonym_lines();
    let profile = generic_profile("");
    let feed = feed(
        &profile,
        &[
            ("agency.txt", AGENCY),
            (
                "routes.txt",
                "route_id,agency_id,route_short_name,route_type\nr,a,1,3\n",
            ),
            ("trips.txt", "route_id,service_id,trip_id\nr,s,t1\nr,s,t2\n"),
            ("stops.txt", STOPS),
            (
                "stop_times.txt",
                "trip_id,arrival_time,departure_time,stop_id,stop_sequence\n\
t1,10:05:00,10:05:00,b,7\nt2,11:00:00,11:00:00,a,1\nt1,10:00:00,10:00:00,a,3\nt2,11:05:00,11:05:00,b,2\n",
            ),
            ("calendar.txt", CALENDAR),
        ],
    );
    let (prepared, _) = prepare_feed(&base.t, &base_profile, &profile, feed, &synonyms);
    let source = &prepared.sources[0];
    let stops = |trip: usize| -> Vec<(u32, Option<i32>)> {
        source
            .trip_calls(&source.trips[trip])
            .iter()
            .map(|c| (c.stop, c.sequence))
            .collect()
    };
    assert_eq!(stops(0), [(0, Some(3)), (1, Some(7))]);
    assert_eq!(stops(1), [(0, Some(1)), (1, Some(2))]);
}
