//! Trip matching (OVL-R065–R075, R133, R134).

mod support;

use jr_model::timetable::Mode;
use jr_model::{Diagnostics, PlaceId, RouteId, Timetable};
use jr_overlay::authority;
use jr_overlay::matching::{self, Matches};
use jr_overlay::profile::{BaseProfile, RouteTier, SourceProfile, TripTier};
use support::*;

fn run_matching(
    base: &Timetable,
    profile: &SourceProfile,
    feed: jr_formats::gtfs::Feed,
) -> (Matches, Vec<String>) {
    let base_profile = BaseProfile::road().unwrap();
    let synonyms = synonym_lines();
    let mut diagnostics = Diagnostics::new();
    let prepared = run(
        base,
        &base_profile,
        profile,
        feed,
        Vec::new(),
        &[],
        &synonyms,
        &mut diagnostics,
    );
    let mut found = Diagnostics::new();
    let owned = authority::authority(&prepared, &mut Diagnostics::new());
    let matches = matching::match_trips(&prepared, &owned, &mut found);
    let codes = found.iter().map(|d| d.code.to_string()).collect();
    (matches, codes)
}

/// The base places and a bus route `5` of line `line`.
fn base_with(lines: &[&str]) -> (Base, Vec<PlaceId>, Vec<RouteId>) {
    let mut base = Base::new();
    let places = four_places(&mut base);
    let routes = lines
        .iter()
        .map(|line| base.route(line, "5", Mode::Bus))
        .collect();
    (base, places, routes)
}

fn abc(base: &mut Base, route: RouteId, places: &[PlaceId], times: [&str; 3]) -> jr_model::TripId {
    base.trip(
        route,
        days(0..364),
        &[
            (places[0], times[0]),
            (places[1], times[1]),
            (places[2], times[2]),
        ],
    )
}

fn source_abc<'t>(trip: &'t str, times: [&'t str; 3]) -> Vec<(&'t str, &'t str, &'t str)> {
    vec![
        (trip, "a", times[0]),
        (trip, "b", times[1]),
        (trip, "c", times[2]),
    ]
}

fn bound_trips(base: &Timetable, matches: &Matches) -> Vec<(u32, String, TripTier)> {
    let _ = base;
    matches
        .bindings
        .iter()
        .map(|b| {
            (
                b.trip,
                format!("{}", b.candidate.base_trip),
                b.candidate.tier,
            )
        })
        .collect()
}

#[test]
fn ovl_r066_r071_the_strongest_tier_wins_over_a_nearer_time() {
    let (mut base, places, routes) = base_with(&["100005"]);
    // The exact twin 10 minutes later beats nothing: tiers first. A full signature exists.
    abc(&mut base, routes[0], &places, ["10:00", "10:05", "10:10"]);
    abc(&mut base, routes[0], &places, ["10:00", "10:06", "10:20"]);
    let profile = generic_profile("");
    let feed = trip_feed(
        &profile,
        &[("r", "5", 3)],
        &[("r", "s1", "")],
        &source_abc("s1", ["10:00", "10:05", "10:10"]),
        "",
    );
    let (matches, _) = run_matching(&base.t, &profile, feed);
    assert_eq!(matches.bindings.len(), 1);
    let binding = &matches.bindings[0];
    assert_eq!(binding.candidate.tier, TripTier::FullSignature);
    assert_eq!(binding.route_tier, RouteTier::StructuralTripEvidence);
    assert_eq!(binding.dates, days(0..364));
    assert_eq!(binding.margin, None);
    // Shifted by a minute, the earlier trip has equal endpoints only; both share pattern_first.
    let feed = trip_feed(
        &profile,
        &[("r", "5", 3)],
        &[("r", "s1", "")],
        &source_abc("s1", ["10:00", "10:07", "10:20"]),
        "",
    );
    let (matches, _) = run_matching(&base.t, &profile, feed);
    assert_eq!(
        matches.bindings[0].candidate.tier,
        TripTier::PatternEndpoints
    );
    assert_eq!(matches.bindings[0].candidate.base_trip, 1);
}

#[test]
fn ovl_r069_r070_time_features_rank_and_give_the_margin() {
    let (mut base, places, routes) = base_with(&["100005"]);
    abc(&mut base, routes[0], &places, ["10:00", "10:05", "10:10"]);
    abc(&mut base, routes[0], &places, ["10:30", "10:35", "10:40"]);
    let profile = generic_profile("");
    let feed = trip_feed(
        &profile,
        &[("r", "5", 3)],
        &[("r", "s1", "")],
        &source_abc("s1", ["10:03", "10:09", "10:13"]),
        "",
    );
    let (matches, _) = run_matching(&base.t, &profile, feed);
    let binding = &matches.bindings[0];
    assert_eq!(binding.candidate.tier, TripTier::PatternNearest);
    assert_eq!(binding.candidate.base_trip, 0);
    let rank = binding.candidate.rank;
    assert_eq!(
        (
            rank.first_departure_delta,
            rank.duration_delta,
            rank.aggregate_delta
        ),
        (180, 0, 1200)
    );
    assert_eq!(
        (
            rank.maximum_delta,
            rank.squared_delta,
            rank.negative_aligned
        ),
        (240, 4 * 180 * 180 + 2 * 240 * 240, -3)
    );
    // The runner-up departs 27 minutes later: the margin is the first-departure difference.
    assert_eq!(binding.margin, Some(27 * 60 - 180));
}

#[test]
fn ovl_r067_a_skipped_stop_is_one_pattern_edit() {
    let (mut base, places, routes) = base_with(&["100005"]);
    base.trip(
        routes[0],
        days(0..364),
        &[
            (places[0], "10:00"),
            (places[1], "10:05"),
            (places[2], "10:10"),
            (places[3], "10:15"),
        ],
    );
    // Short test patterns: 3 of 4 calls agree.
    let mut profile = generic_profile("");
    profile.trips.pattern_edit.minimum_agreement = 0.6;
    let calls = [
        ("s1", "a", "10:00"),
        ("s1", "b", "10:05"),
        ("s1", "d", "10:15"),
    ];
    let feed = trip_feed(&profile, &[("r", "5", 3)], &[("r", "s1", "")], &calls, "");
    let (matches, _) = run_matching(&base.t, &profile, feed);
    let candidate = &matches.bindings[0].candidate;
    assert_eq!(
        (candidate.tier, candidate.rank.edits),
        (TripTier::PatternEditNearest, 1)
    );
    assert_eq!(&*candidate.alignment, &[Some(0), Some(1), None, Some(2)]);
    // Different endpoints never align.
    let calls = [
        ("s1", "b", "10:05"),
        ("s1", "c", "10:10"),
        ("s1", "d", "10:15"),
    ];
    let feed = trip_feed(&profile, &[("r", "5", 3)], &[("r", "s1", "")], &calls, "");
    let (matches, codes) = run_matching(&base.t, &profile, feed);
    assert!(matches.bindings.is_empty());
    assert!(codes.contains(&"trip_signature_unresolved".to_owned()));
}

#[test]
fn ovl_r065_r068_with_keep_source_calls_a_superset_binds_as_pattern_nearest() {
    let (mut base, places, routes) = base_with(&["100005"]);
    base.trip(
        routes[0],
        days(0..364),
        &[(places[0], "10:00"), (places[2], "10:10")],
    );
    let mut profile = generic_profile("");
    profile.trips.pattern_edit.minimum_agreement = 0.6;
    let mut keep = profile.clone();
    keep.trips.keep_source_calls = true;
    let calls = source_abc("s1", ["10:00", "10:05", "10:10"]);
    let feed = trip_feed(&keep, &[("r", "5", 3)], &[("r", "s1", "")], &calls, "");
    let (matches, _) = run_matching(&base.t, &keep, feed);
    let candidate = &matches.bindings[0].candidate;
    assert_eq!(
        (candidate.tier, candidate.rank.edits),
        (TripTier::PatternNearest, 1)
    );
    assert_eq!(&*candidate.alignment, &[Some(0), Some(2)]);
    // Without it, the same trip is a pattern edit.
    let feed = trip_feed(&profile, &[("r", "5", 3)], &[("r", "s1", "")], &calls, "");
    let (matches, _) = run_matching(&base.t, &profile, feed);
    assert_eq!(
        matches.bindings[0].candidate.tier,
        TripTier::PatternEditNearest
    );
}

#[test]
fn ovl_r071_equivalent_ties_expand_distinct_ones_stay_ambiguous() {
    // Two identical trips of one line: equivalent, both bound.
    let (mut base, places, routes) = base_with(&["100005"]);
    abc(&mut base, routes[0], &places, ["10:00", "10:05", "10:10"]);
    abc(&mut base, routes[0], &places, ["10:00", "10:05", "10:10"]);
    let profile = generic_profile("");
    let calls = source_abc("s1", ["10:00", "10:05", "10:10"]);
    let feed = trip_feed(&profile, &[("r", "5", 3)], &[("r", "s1", "")], &calls, "");
    let (matches, codes) = run_matching(&base.t, &profile, feed);
    assert_eq!(bound_trips(&base.t, &matches).len(), 2);
    assert!(codes.contains(&"trip_equivalent_tie_expanded".to_owned()));
    // The same trips on two lines: two meanings, ambiguous on every date.
    let (mut base, places, routes) = base_with(&["100005", "100006"]);
    abc(&mut base, routes[0], &places, ["10:00", "10:05", "10:10"]);
    abc(&mut base, routes[1], &places, ["10:00", "10:05", "10:10"]);
    let feed = trip_feed(&profile, &[("r", "5", 3)], &[("r", "s1", "")], &calls, "");
    let (matches, codes) = run_matching(&base.t, &profile, feed);
    assert!(matches.bindings.is_empty());
    assert_eq!(matches.ambiguous.len(), 364);
    assert!(codes.contains(&"trip_same_date_ambiguous".to_owned()));
    assert!(codes.contains(&"trip_validity_unresolved".to_owned()));
}

/// Base trips X (line 100005) and Y (line 100006), identical; source trip `free` runs exactly
/// their times on route `5` (both are its equal-best candidates, of two meanings), and each
/// claim is a source trip asserting a line, a minute off at the middle call (a weaker tier).
fn availability_case(claims: &[(&str, &str)]) -> (Matches, Vec<String>) {
    let (mut base, places, routes) = base_with(&["100005", "100006"]);
    abc(&mut base, routes[0], &places, ["10:00", "10:05", "10:10"]);
    abc(&mut base, routes[1], &places, ["10:00", "10:05", "10:10"]);
    let profile = claiming_profile("");
    let route_ids = ["q1", "q2"];
    let mut routes = vec![("r", "5", 3)];
    let mut trips = vec![("r", "free", "")];
    let mut calls = source_abc("free", ["10:00", "10:05", "10:10"]);
    let mut companion = String::new();
    for (index, (trip, line)) in claims.iter().enumerate() {
        let route = route_ids[index];
        routes.push((route, "Q", 3));
        trips.push((route, trip, "1"));
        calls.extend(source_abc(trip, ["10:00", "10:06", "10:10"]));
        companion.push_str(&format!("{route},1,{line}\n"));
    }
    let feed = trip_feed(&profile, &routes, &trips, &calls, &companion);
    run_matching(&base.t, &profile, feed)
}

#[test]
fn ovl_r074_an_ambiguous_date_takes_the_base_trip_nobody_else_claims() {
    let (matches, codes) = availability_case(&[("claimed", "100005")]);
    let free: Vec<_> = matches.bindings.iter().filter(|b| b.trip == 0).collect();
    assert_eq!(free.len(), 364);
    assert!(
        free.iter()
            .all(|b| b.by_availability && b.candidate.base_trip == 1 && b.dates.count() == 1)
    );
    assert!(codes.contains(&"trip_target_availability_resolved".to_owned()));
    assert!(matches.ambiguous.is_empty());
}

#[test]
fn ovl_r075_an_ambiguous_date_whose_candidates_are_all_claimed_is_a_capacity_gap() {
    let (matches, codes) = availability_case(&[("x", "100005"), ("y", "100006")]);
    assert!(matches.bindings.iter().all(|b| b.trip != 0));
    assert_eq!(matches.ambiguous.len(), 364);
    assert!(matches.ambiguous.iter().all(|p| p.capacity_gap));
    assert!(matches.flags[0][0].capacity_gap);
    assert!(codes.contains(&"base_snapshot_capacity_gap".to_owned()));
}

#[test]
fn ovl_r133_a_base_trip_departing_beyond_the_tolerance_is_not_a_candidate() {
    let (mut base, places, routes) = base_with(&["100005"]);
    abc(&mut base, routes[0], &places, ["10:00", "10:05", "10:10"]);
    let mut profile = generic_profile("");
    profile.trips.maximum_first_departure_delta_seconds = 420;
    // Eight minutes off: beyond seven.
    let calls = source_abc("s1", ["10:08", "10:13", "10:18"]);
    let feed = trip_feed(&profile, &[("r", "5", 3)], &[("r", "s1", "")], &calls, "");
    let (matches, codes) = run_matching(&base.t, &profile, feed);
    assert!(matches.bindings.is_empty());
    assert_eq!(
        matches.unmatched[0].code,
        "trip_departure_deviation_exceeded"
    );
    assert!(codes.contains(&"trip_departure_deviation_exceeded".to_owned()));
    // Seven minutes off: a candidate.
    let calls = source_abc("s1", ["10:07", "10:12", "10:17"]);
    let feed = trip_feed(&profile, &[("r", "5", 3)], &[("r", "s1", "")], &calls, "");
    let (matches, _) = run_matching(&base.t, &profile, feed);
    assert_eq!(matches.bindings.len(), 1);
    assert_eq!(matches.bindings[0].dates, days(0..364));
}

/// Bindings as (source trip ID, base trip index, number of dates), sorted.
fn assigned(matches: &Matches, ids: &[&str]) -> Vec<(String, u32, usize)> {
    let mut found: Vec<_> = matches
        .bindings
        .iter()
        .map(|b| {
            (
                ids[b.trip as usize].to_owned(),
                b.candidate.base_trip,
                b.dates.count(),
            )
        })
        .collect();
    found.sort();
    found
}

#[test]
fn ovl_r134_a_base_trip_goes_to_the_nearest_source_trip_and_the_other_moves_on() {
    // Base trips at 10:00 and 10:10; both source trips are nearest to 10:00.
    let (mut base, places, routes) = base_with(&["100005"]);
    abc(&mut base, routes[0], &places, ["10:00", "10:05", "10:10"]);
    abc(&mut base, routes[0], &places, ["10:10", "10:15", "10:20"]);
    let profile = generic_profile("");
    let near = source_abc("near", ["10:01", "10:06", "10:11"]);
    let next = source_abc("next", ["10:03", "10:08", "10:13"]);
    let expected = vec![("near".to_owned(), 0, 364), ("next".to_owned(), 1, 364)];
    // In either input order, on one thread or several: the same assignment.
    for order in [["near", "next"], ["next", "near"]] {
        let calls: Vec<_> = if order[0] == "near" {
            near.iter().chain(&next).copied().collect()
        } else {
            next.iter().chain(&near).copied().collect()
        };
        let trips: Vec<_> = order.iter().map(|&trip| ("r", trip, "")).collect();
        for threads in [1, 4] {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap();
            let feed = trip_feed(&profile, &[("r", "5", 3)], &trips, &calls, "");
            let (matches, codes) = pool.install(|| run_matching(&base.t, &profile, feed));
            assert_eq!(assigned(&matches, &order), expected);
            assert!(!codes.contains(&"base_snapshot_capacity_gap".to_owned()));
        }
    }
}

#[test]
fn ovl_r134_with_nothing_left_the_tied_source_trip_is_a_capacity_gap() {
    let (mut base, places, routes) = base_with(&["100005"]);
    abc(&mut base, routes[0], &places, ["10:00", "10:05", "10:10"]);
    // Equal ranks: the source trip ID decides.
    let profile = generic_profile("");
    let calls: Vec<_> = source_abc("b", ["10:00", "10:06", "10:10"])
        .into_iter()
        .chain(source_abc("a", ["10:00", "10:06", "10:10"]))
        .collect();
    let feed = trip_feed(
        &profile,
        &[("r", "5", 3)],
        &[("r", "b", ""), ("r", "a", "")],
        &calls,
        "",
    );
    let (matches, codes) = run_matching(&base.t, &profile, feed);
    assert_eq!(
        assigned(&matches, &["b", "a"]),
        vec![("a".to_owned(), 0, 364)]
    );
    assert!(matches.flags[0][0].capacity_gap);
    assert!(codes.contains(&"base_snapshot_capacity_gap".to_owned()));
    // Equal ranks: the newer revision decides before the ID.
    let profile = generic_profile(
        "[hooks.revision]\ncolumn = \"trip_id\"\npattern = '_(?<revision>\\d{6})(?:_|$)'\ndate_format = \"%y%m%d\"\n",
    );
    let calls: Vec<_> = source_abc("a_260101", ["10:00", "10:06", "10:10"])
        .into_iter()
        .chain(source_abc("b_260201", ["10:00", "10:06", "10:10"]))
        .collect();
    let feed = trip_feed(
        &profile,
        &[("r", "5", 3)],
        &[("r", "a_260101", ""), ("r", "b_260201", "")],
        &calls,
        "",
    );
    let (matches, _) = run_matching(&base.t, &profile, feed);
    assert_eq!(
        assigned(&matches, &["a_260101", "b_260201"]),
        vec![("b_260201".to_owned(), 0, 364)]
    );
}

#[test]
fn ovl_r073_unmatched_trips_get_the_first_reason_that_applies() {
    let mut base = Base::new();
    let places: Vec<_> = FOUR[..3]
        .iter()
        .enumerate()
        .map(|(index, (_, name, lat))| base.place(index as u64 + 1, name, Some((*lat, 14.0))))
        .collect();
    let route = base.route("100005", "5", Mode::Bus);
    // Runs only on day 0.
    base.trip(
        route,
        days([0]),
        &[
            (places[0], "10:00"),
            (places[1], "10:05"),
            (places[2], "10:10"),
        ],
    );
    let profile = generic_profile("");
    let reason = |route: &str, calls: &[(&str, &str, &str)], route_type: u32| {
        let feed = trip_feed(
            &profile,
            &[("r", route, route_type)],
            &[("r", "s1", "")],
            calls,
            "",
        );
        let (matches, _) = run_matching(&base.t, &profile, feed);
        matches.unmatched.first().map(|u| u.code)
    };
    let abc = source_abc("s1", ["11:00", "11:05", "11:10"]);
    // Delta has no base place.
    assert_eq!(
        reason("5", &[("s1", "a", "10:00"), ("s1", "d", "10:05")], 3),
        Some("trip_stop_unresolved")
    );
    assert_eq!(
        reason("9", &abc, 3),
        Some("trip_route_candidate_unresolved")
    );
    assert_eq!(
        reason(
            "5",
            &[
                ("s1", "c", "10:00"),
                ("s1", "b", "10:05"),
                ("s1", "a", "10:10")
            ],
            3
        ),
        Some("trip_signature_unresolved")
    );
    // The source runs every day; the base trip only on day 0, so it is matched there.
    assert_eq!(reason("5", &abc, 3), None);
}

#[test]
fn ovl_r073_a_candidate_running_on_none_of_the_dates_is_a_snapshot_gap() {
    let (mut base, places, routes) = base_with(&["100005"]);
    base.trip(
        routes[0],
        days([]),
        &[
            (places[0], "10:00"),
            (places[1], "10:05"),
            (places[2], "10:10"),
        ],
    );
    // An unpublished trip is not a candidate: publish one on a day the source does not run.
    base.trip(
        routes[0],
        days([363]),
        &[
            (places[0], "10:00"),
            (places[1], "10:05"),
            (places[2], "10:10"),
        ],
    );
    let profile = generic_profile("");
    let calls = source_abc("s1", ["10:00", "10:05", "10:10"]);
    let mut feed = trip_feed(&profile, &[("r", "5", 3)], &[("r", "s1", "")], &calls, "");
    // The source runs on the first week only.
    feed.calendars[0].end_date = Some(day(6));
    let (matches, codes) = run_matching(&base.t, &profile, feed);
    assert!(matches.bindings.is_empty());
    assert_eq!(matches.unmatched[0].code, "base_snapshot_gap");
    assert!(matches.flags[0][0].snapshot_gap);
    assert!(codes.contains(&"base_snapshot_gap".to_owned()));
}

#[test]
fn ovl_r073_an_active_trip_without_calls_has_no_call_pattern() {
    let (base, _, _) = base_with(&["100005"]);
    let profile = generic_profile("");
    let feed = trip_feed(&profile, &[("r", "5", 3)], &[("r", "s1", "")], &[], "");
    let (matches, _) = run_matching(&base.t, &profile, feed);
    assert_eq!(matches.unmatched[0].code, "trip_call_pattern_unavailable");
}
