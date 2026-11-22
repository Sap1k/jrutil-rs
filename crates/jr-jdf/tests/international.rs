//! The international route filter (R269–R278, R281), ported from the F#
//! `InternationalRouteFilterTests` (named in each test) plus the verdict cases.

mod support;

use jr_formats::jdf::{RouteType, TimeCell};
use jr_jdf::Policy;
use jr_jdf::international::{
    Classified, Disposition, Reason, RouteDecision, RouteView, StopCountry, TripView, classify,
    is_emitted,
};
use jr_jdf::policy::FilterPolicy;
use jr_model::Diagnostics;
use support::*;

fn decide(route: &RouteView<'_>) -> (RouteDecision, Diagnostics) {
    let mut diagnostics = Diagnostics::new();
    let decision = classify(route, &Policy::builtin(), &mut diagnostics);
    (decision, diagnostics)
}

fn kept(route: &RouteView<'_>) -> Classified {
    match decide(route).0 {
        RouteDecision::KeepTrips(classified) => classified,
        other => panic!("not kept after classification: {other:?}"),
    }
}

fn dropped(route: &RouteView<'_>) -> Classified {
    match decide(route).0 {
        RouteDecision::Drop(classified) => classified,
        other => panic!("not dropped: {other:?}"),
    }
}

#[test]
fn r269_keep_all_keeps_everything_without_a_decision() {
    // "Keep-all preserves backward-compatible conversion".
    let mut policy = Policy::builtin();
    policy.international.policy = FilterPolicy::KeepAll;
    let city = RouteType::City;
    let mut diagnostics = Diagnostics::new();
    let decision = classify(
        &synthetic("UA", 1000, 500, false, &city, false),
        &policy,
        &mut diagnostics,
    );
    assert_eq!(decision, RouteDecision::Keep);
    assert!(diagnostics.is_empty());
}

#[test]
fn r271_r274_a_short_neighbouring_route_is_kept_even_when_labelled_city() {
    // "Short neighboring route is retained even when mislabeled city" (DE is an alias of D).
    let city = RouteType::City;
    let decision = kept(&synthetic("DE", 114, 44, false, &city, false));
    assert_eq!(decision.reason, Reason::RegionalAdjacent);
    assert_eq!(decision.countries, [Box::from("CZ"), Box::from("D")]);
    assert_eq!(
        (decision.max_span_km, decision.max_depth_km),
        (Some(114.0), Some(44.0))
    );
    assert!(decision.is_regional_cross_border());
}

#[test]
fn r276_an_integrated_route_has_the_relaxed_envelope() {
    // "Integrated route uses relaxed envelope".
    let international = RouteType::International;
    let retained = kept(&synthetic("A", 180, 70, true, &international, false));
    assert_eq!(retained.reason, Reason::RegionalAdjacent);
    let rejected = dropped(&synthetic("A", 180, 70, false, &international, false));
    assert_eq!(rejected.reason, Reason::TripSpanExceedsLimit);
}

#[test]
fn r274_r275_long_and_non_neighbouring_routes_are_rejected() {
    // "Long neighboring and non-neighboring routes are rejected".
    let international = RouteType::International;
    let vienna = dropped(&synthetic("AT", 133, 84, false, &international, false));
    assert_eq!(vienna.reason, Reason::TripSpanExceedsLimit);
    assert_eq!(
        vienna.trips[0].disposition,
        Disposition::Rejected {
            reason: Reason::TripSpanExceedsLimit,
            span_km: Some(133.0),
            depth_km: Some(84.0),
        }
    );
    let regional = RouteType::Regional;
    let ukraine = dropped(&synthetic("UA", 80, 20, false, &regional, false));
    assert_eq!(ukraine.reason, Reason::NonAdjacentCountry);
}

#[test]
fn r275_depth_beyond_the_limit_is_rejected() {
    let regional = RouteType::Regional;
    let deep = dropped(&synthetic("PL", 100, 61, false, &regional, false));
    assert_eq!(deep.reason, Reason::ForeignDepthExceedsLimit);
    // Strictly greater rejects.
    let edge = kept(&synthetic("PL", 120, 60, false, &regional, false));
    assert_eq!(edge.reason, Reason::RegionalAdjacent);
}

#[test]
fn r274_foreign_only_and_missing_kilometre_trips_fail_closed() {
    // "Foreign-only and missing-kilometre trips fail closed".
    let international = RouteType::International;
    let mut foreign_only = synthetic("D", 30, 10, false, &international, false);
    for call in &mut foreign_only.trips[0].calls {
        call.stop = abroad("D");
    }
    foreign_only.route_stops = vec![abroad("D"); 3];
    assert_eq!(dropped(&foreign_only).reason, Reason::ForeignOnlyTrip);

    let mut missing = synthetic("D", 30, 10, false, &international, false);
    missing.trips[0].calls[2].km = None;
    assert_eq!(dropped(&missing).reason, Reason::MissingTimetableKilometres);
}

#[test]
fn r273_a_passed_foreign_stop_does_not_make_a_route_international() {
    // "Passing foreign stop does not make a domestic route international".
    let regional = RouteType::Regional;
    let decision = kept(&synthetic("UA", 1000, 500, false, &regional, true));
    assert_eq!(decision.reason, Reason::Domestic);
    assert!(decision.trips[0].keep);

    assert!(is_emitted(None, Some(TimeCell::At { minutes: 600 })));
    assert!(is_emitted(
        Some(TimeCell::At { minutes: 600 }),
        Some(TimeCell::Empty)
    ));
    assert!(!is_emitted(
        Some(TimeCell::At { minutes: 600 }),
        Some(TimeCell::Passes)
    ));
    assert!(!is_emitted(None, Some(TimeCell::OtherRoute)));
    assert!(!is_emitted(Some(TimeCell::Empty), None));
}

#[test]
fn r278_a_regional_trip_survives_while_its_foreign_only_continuation_is_pruned() {
    // "001398 regional trip survives while its foreign-only continuation is pruned".
    let international = RouteType::International;
    let mut route = synthetic("D", 50, 20, false, &international, false);
    let mut foreign = route.trips[0].clone();
    foreign.trip = 2;
    for call in &mut foreign.calls {
        call.stop = abroad("D");
    }
    route.trips.push(foreign);
    let (decision, diagnostics) = decide(&route);
    let RouteDecision::KeepTrips(decision) = decision else {
        panic!("kept")
    };
    assert_eq!(
        (decision.qualifying_trips, decision.foreign_only_trips),
        (1, 1)
    );
    let kept: Vec<u64> = decision
        .trips
        .iter()
        .filter(|t| t.keep)
        .map(|t| t.trip)
        .collect();
    assert_eq!(kept, [1]);
    assert!(decision.is_regional_cross_border());
    assert_eq!(codes(&diagnostics), ["jdf_international_trip_dropped"]);
    assert_eq!(
        diagnostics.iter().next().map(|d| d.subject.as_str()),
        Some("jdf:route:586001:1/2")
    );
}

#[test]
fn r272_foreign_calls_of_a_trip_that_does_not_run_are_ignored() {
    // "Foreign calls on a trip without retained service dates are ignored".
    let regional = RouteType::Regional;
    let mut route = synthetic("CZ", 20, 10, false, &regional, false);
    route.route_stops.push(abroad("UA"));
    route.trips.push(TripView {
        trip: 3,
        runs: false,
        calls: vec![call(cz(), km(0)), call(abroad("UA"), km(5))],
    });
    let decision = kept(&route);
    assert_eq!(decision.reason, Reason::Domestic);
    assert_eq!(decision.countries, [Box::from("CZ")]);
    assert!(decision.trips.iter().all(|t| t.keep));
    assert_eq!(decision.trips[1].disposition, Disposition::NotRunning);
}

#[test]
fn r270_country_codes_are_trimmed_upper_cased_and_okres_only_stops_are_domestic() {
    let regional = RouteType::Regional;
    let okres_only = StopCountry {
        country: None,
        okres: Some("PB"),
    };
    let unknown = StopCountry::default();
    let route = RouteView {
        subject: "r",
        route_type: Some(&regional),
        integrated: false,
        route_stops: vec![okres_only, abroad(" at "), unknown],
        trips: vec![TripView {
            trip: 1,
            runs: true,
            calls: vec![call(okres_only, km(0)), call(abroad(" at "), km(10))],
        }],
    };
    let decision = kept(&route);
    assert_eq!(decision.countries, [Box::from("A"), Box::from("CZ")]);
    assert_eq!(decision.reason, Reason::RegionalAdjacent);
}

#[test]
fn r277_a_declared_international_route_with_only_domestic_calls_is_dropped() {
    // B13, ported: the verdict keeps both domestic-drop cases, diagnosed.
    let international = RouteType::InternationalOrNational;
    let route = synthetic("CZ", 30, 10, false, &international, false);
    let (decision, diagnostics) = decide(&route);
    let RouteDecision::Drop(decision) = decision else {
        panic!("dropped")
    };
    assert_eq!(
        decision.reason,
        Reason::InternationalMetadataWithoutForeignGeography
    );
    assert_eq!(decision.domestic_trips, 1);
    assert!(decision.trips.iter().all(|t| !t.keep));
    assert_eq!(codes(&diagnostics), ["jdf_international_route_dropped"]);
    let message = &diagnostics.iter().next().expect("one").message;
    assert!(message.starts_with("international_metadata_without_foreign_geography"));
}

#[test]
fn r278_a_trip_through_an_unknown_country_stop_on_a_domestic_route_is_dropped() {
    // B13, ported.
    let regional = RouteType::Regional;
    let mut route = synthetic("CZ", 30, 10, false, &regional, false);
    route.route_stops.push(StopCountry::default());
    route.trips.push(TripView {
        trip: 2,
        runs: true,
        calls: vec![call(cz(), km(0)), call(StopCountry::default(), km(5))],
    });
    let (decision, diagnostics) = decide(&route);
    let RouteDecision::KeepTrips(decision) = decision else {
        panic!("kept")
    };
    assert_eq!(decision.reason, Reason::Domestic);
    assert_eq!(
        decision.trips.iter().map(|t| t.keep).collect::<Vec<_>>(),
        [true, false]
    );
    assert_eq!(codes(&diagnostics), ["jdf_international_trip_dropped"]);
}

#[test]
fn r277_r281_different_rejection_reasons_give_no_qualifying_trip() {
    let regional = RouteType::Regional;
    let mut route = synthetic("UA", 80, 20, false, &regional, false);
    let mut long = synthetic("D", 300, 20, false, &regional, false)
        .trips
        .remove(0);
    long.trip = 2;
    route.trips.push(long);
    route.route_stops.push(abroad("D"));
    let decision = dropped(&route);
    assert_eq!(decision.reason, Reason::NoQualifyingCrossBorderTrip);
    assert_eq!(decision.rejected_trips, 2);
    assert_eq!(
        decision.countries,
        [Box::from("CZ"), Box::from("D"), Box::from("UA")]
    );
    assert_eq!((decision.max_span_km, decision.max_depth_km), (None, None));
}

#[test]
fn r271_a_domestic_route_is_not_classified() {
    let regional = RouteType::Regional;
    let route = RouteView {
        subject: "r",
        route_type: Some(&regional),
        integrated: false,
        route_stops: vec![cz(), cz()],
        trips: vec![TripView {
            trip: 1,
            runs: true,
            calls: vec![call(cz(), km(0)), call(cz(), None)],
        }],
    };
    let (decision, diagnostics) = decide(&route);
    assert_eq!(decision, RouteDecision::Keep);
    assert!(diagnostics.is_empty());
}
