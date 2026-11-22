//! Trip-set authority (OVL-R057–R064).

mod support;

use jr_model::identity::overlay;
use jr_model::timetable::Mode;
use jr_model::{Diagnostics, Timetable};
use jr_overlay::authority::{self, Authority, AuthorityLine, TargetRoute, Withheld};
use jr_overlay::profile::{BaseProfile, SourceProfile};
use support::*;

fn decide(
    base: &Timetable,
    profile: &SourceProfile,
    feed: jr_formats::gtfs::Feed,
) -> (Authority, Diagnostics) {
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
    let result = authority::authority(&prepared, &mut found);
    (result, found)
}

fn national(line: &str) -> AuthorityLine {
    AuthorityLine::National(line.into())
}

const ABC: [(&str, &str, &str); 3] = [
    ("t1", "a", "10:00"),
    ("t1", "b", "10:05"),
    ("t1", "c", "10:10"),
];

#[test]
fn ovl_r057_r061_r064_a_claimed_line_is_owned_and_its_trips_projected() {
    let mut base = Base::new();
    let places = four_places(&mut base);
    let route = base.route("100005", "5", Mode::Bus);
    base.trip(
        route,
        days(0..364),
        &[
            (places[0], "10:00"),
            (places[1], "10:05"),
            (places[2], "10:10"),
        ],
    );
    let profile = claiming_profile("");
    let feed = trip_feed(
        &profile,
        &[("r", "X", 3)],
        &[("r", "t1", "1")],
        &ABC,
        "r,1,100005\n",
    );
    let (result, diagnostics) = decide(&base.t, &profile, feed);
    assert!(diagnostics.is_empty());
    assert_eq!(result.lines.get(&national("100005")), Some(&days(0..364)));
    let projection = &result.projections[0];
    assert_eq!(
        (projection.line.clone(), projection.target),
        (national("100005"), TargetRoute::Base(route))
    );
    assert_eq!(projection.dates, days(0..364));
}

#[test]
fn ovl_r057_r058_a_unique_structural_route_gives_its_line_detours_yield() {
    let mut base = Base::new();
    let places = four_places(&mut base);
    let regular = base.route("100005", "5", Mode::Bus);
    let detour = base.route_with("100005", "5", Mode::Bus, true);
    for route in [regular, detour] {
        base.trip(
            route,
            days(0..364),
            &[
                (places[0], "10:00"),
                (places[1], "10:05"),
                (places[2], "10:10"),
            ],
        );
    }
    let profile = generic_profile("");
    let feed = trip_feed(&profile, &[("r", "5", 3)], &[("r", "t1", "")], &ABC, "");
    let (result, _) = decide(&base.t, &profile, feed);
    assert_eq!(result.projections[0].target, TargetRoute::Base(regular));
    assert_eq!(result.projections[0].line, national("100005"));
    // Two regular routes of the number give no identity: the trip is withheld.
    let other = base.route("100006", "5", Mode::Bus);
    base.trip(
        other,
        days(0..364),
        &[(places[0], "11:00"), (places[1], "11:05")],
    );
    let feed = trip_feed(&profile, &[("r", "5", 3)], &[("r", "t1", "")], &ABC, "");
    let (result, diagnostics) = decide(&base.t, &profile, feed);
    assert!(result.projections.is_empty());
    assert_eq!(result.withheld, [(0, 0, Withheld::NoIdentity)]);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == "trip_set_authority_withheld")
    );
}

#[test]
fn ovl_r059_another_mode_and_source_native_trips_run_on_derived_routes() {
    let mut base = Base::new();
    let places = four_places(&mut base);
    let route = base.route("100005", "5", Mode::Bus);
    base.trip(
        route,
        days(0..364),
        &[
            (places[0], "10:00"),
            (places[1], "10:05"),
            (places[2], "10:10"),
        ],
    );
    let mut profile = claiming_profile("");
    profile.authority.modes.push(Mode::Trolleybus);
    // A trolleybus (route type 11) asserting the bus line keeps its mode on a derived route.
    let feed = trip_feed(
        &profile,
        &[("tb", "X", 11)],
        &[("tb", "t1", "1")],
        &ABC,
        "tb,1,100005\n",
    );
    let (result, _) = decide(&base.t, &profile, feed);
    assert_eq!(result.projections[0].target, TargetRoute::Derived(0));
    assert_eq!(
        result.derived_routes[0].id,
        overlay::route("test-gtfs", "tb", "100005")
    );
    // A tram (source-native in the test profile) of no national line runs on its own route.
    let feed = trip_feed(&profile, &[("tr", "77", 0)], &[("tr", "t1", "")], &ABC, "");
    let (result, _) = decide(&base.t, &profile, feed);
    let projection = &result.projections[0];
    assert_eq!(
        projection.line,
        AuthorityLine::SourceRoute {
            source: 0,
            route: 0
        }
    );
    assert_eq!(
        result.derived_routes[0].id,
        overlay::route("test-gtfs", "tr", "source:test-gtfs:tr")
    );
}

#[test]
fn ovl_r060_r063_an_unplaced_call_blocks_its_line_on_its_dates() {
    let mut base = Base::new();
    // Only Alfa, Beta and Gama are base places: the source's Delta stays unplaced.
    let places: Vec<_> = FOUR[..3]
        .iter()
        .enumerate()
        .map(|(index, (_, name, lat))| base.place(index as u64 + 1, name, Some((*lat, 14.0))))
        .collect();
    let route = base.route("100005", "5", Mode::Bus);
    base.trip(
        route,
        days(0..364),
        &[
            (places[0], "10:00"),
            (places[1], "10:05"),
            (places[2], "10:10"),
        ],
    );
    let profile = claiming_profile("");
    let mut calls = ABC.to_vec();
    calls.extend([("t2", "a", "11:00"), ("t2", "d", "11:05")]);
    let feed = trip_feed(
        &profile,
        &[("r", "X", 3)],
        &[("r", "t1", "1"), ("r", "t2", "1")],
        &calls,
        "r,1,100005
",
    );
    let (result, diagnostics) = decide(&base.t, &profile, feed);
    assert_eq!(result.withheld, [(0, 1, Withheld::UnplacedCall)]);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == "trip_set_authority_withheld"
                && d.message == Withheld::UnplacedCall.message())
    );
    assert_eq!(result.blocked.get(&national("100005")), Some(&days(0..364)));
    assert_eq!(result.lines.get(&national("100005")), Some(&days([])));
    assert!(result.projections.is_empty());
}

/// Two published bus lines of integrated system 30001, neither run by the source.
fn silence_base() -> Timetable {
    let mut base = Base::new();
    let places = four_places(&mut base);
    for line in ["100001", "100002"] {
        let route = base.route(line, line, Mode::Bus);
        base.t.assignments.push(jr_model::timetable::Assignment {
            scope: jr_model::timetable::Scope::Route(route),
            kind: jr_model::timetable::AssignmentKind::Feature(
                jr_model::timetable::Feature::IntegratedTransport,
            ),
            source_code: Some("30001".into()),
            note: None,
            source_object: None,
        });
        base.trip(
            route,
            days(0..364),
            &[(places[0], "10:00"), (places[1], "10:05")],
        );
    }
    base.t
}

#[test]
fn ovl_r062_lines_of_the_integrated_system_the_source_does_not_run_are_silenced() {
    let base = silence_base();
    let run_with = |maximum: u32| {
        let profile = generic_profile(&format!(
            "[[authority.integrated_systems]]
system = \"30001\"
maximum_silenced_lines = {maximum}
"
        ));
        let feed = trip_feed(&profile, &[("r", "X", 3)], &[("r", "t1", "")], &ABC, "");
        let (result, diagnostics) = decide(&base, &profile, feed);
        let codes: Vec<String> = diagnostics.iter().map(|d| d.code.to_string()).collect();
        (result, codes)
    };
    let (result, codes) = run_with(1);
    assert!(codes.contains(&"trip_set_authority_silence_withheld".to_owned()));
    assert!(result.lines.is_empty());
    let (result, codes) = run_with(2);
    assert_eq!(
        codes
            .iter()
            .filter(|c| *c == "trip_set_authority_silent")
            .count(),
        2
    );
    assert_eq!(result.lines.get(&national("100001")), Some(&days(0..364)));
    assert_eq!(result.lines.get(&national("100002")), Some(&days(0..364)));
}
