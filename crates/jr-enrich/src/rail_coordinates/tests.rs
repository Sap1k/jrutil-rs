use std::collections::BTreeMap;

use jr_model::Diagnostics;
use jr_model::timetable::{CoordSource, Precision};

use super::Policy;

mod fixture;

use fixture::{Fixture, assert_near, at, id};

/// Metres per call past the last anchor, in degrees of latitude (verdict Q12).
const NORTH_PER_CALL: f64 = 300.0 / 111_320.0;

fn methods(entries: &[(&str, usize)]) -> BTreeMap<String, usize> {
    entries
        .iter()
        .map(|&(method, count)| (method.to_owned(), count))
        .collect()
}

#[test]
fn czptt_r018_r021_sr70_codes_names_and_ambiguous_names() {
    let mut f = Fixture::new();
    // A six-digit code loses its check digit; both rows agree.
    f.sr70_at("533398", "Praha hl.n.", 50.083, 14.435);
    f.sr70_at("533399", " Praha hl.n. ", 50.083, 14.435);
    f.sr70_at("5401", "Too short", 50.0, 15.0);
    f.sr70_at("540013", "-", 50.0, 15.0);
    f.sr70_at("540021", "Kolín", 50.026, 15.214);
    f.sr70_at("540022", "Kolin", 50.026, 15.214);
    let mut diagnostics = Diagnostics::new();
    let evidence = f.evidence(&mut diagnostics);

    let names: Vec<_> = evidence.point_names().into_iter().collect();
    assert_eq!(
        names,
        [(("CZ".into(), "53339".into()), "Praha hl.n.".into())]
    );
    assert_eq!(
        diagnostics.counts_by_code(),
        BTreeMap::from([("czptt_sr70_ambiguous_name", 1)])
    );
    // Names and coordinates are independent: the placeholder and the ambiguous name keep their
    // coordinates.
    assert!(evidence.sr70_coord("CZ", "54001").is_some());
    assert!(evidence.sr70_coord("CZ", "54002").is_some());
    assert!(evidence.sr70_coord("CZ", "5401").is_none());
    // SR70 places points of its own country only.
    assert!(evidence.sr70_coord("DE", "53339").is_none());
}

#[test]
fn czptt_r019_r020_r095_invalid_and_conflicting_sr70_fall_back_to_osm_plc() {
    let mut f = Fixture::new();
    f.path(
        "a",
        &[
            at("CZ", "57076", "Praha", "08:00", true),
            at("CZ", "57016", "Kolín", "08:20", true),
        ],
    );
    f.sr70("57076", "Praha", None, None);
    f.sr70_at("57016", "Kolín", 50.026, 15.214);
    f.sr70_at("57016", "Kolín", 50.027, 15.215);
    f.node(
        1,
        50.083,
        14.435,
        &[("railway", "station"), ("ref:EU:PLC", "CZ57076")],
    );
    f.node(
        2,
        50.026,
        15.214,
        &[("railway", "station"), ("ref:EU:PLC", "CZ57016")],
    );
    let outcome = f.run();

    assert_eq!(
        outcome.diagnostics("czptt_sr70_invalid_code")[0].0,
        "CZ:57076"
    );
    assert_eq!(
        outcome.diagnostics("czptt_sr70_conflicting_code")[0].0,
        "CZ:57016"
    );
    let (praha, rule) = outcome.position(&f.t, "CZ", "57076").unwrap();
    assert_eq!((praha.source, rule), (Some(CoordSource::Osm), "CZPTT-R094"));
    // Praha, placed first, vetoes Kolín's PLC node: 56 km in 20 minutes is too far.
    assert_eq!(
        outcome.diagnostics("czptt_osm_corridor_rejected"),
        [(
            id("CZ", "57016").as_str(),
            "ref_eu_plc: osm:node:2 cannot be reached in time from the placed neighbours"
        )]
    );
    let (kolin, _) = outcome.position(&f.t, "CZ", "57016").unwrap();
    assert_eq!(kolin.source, Some(CoordSource::RouteEnd));
    assert_near(kolin, 14.435, 50.083 + NORTH_PER_CALL);
    assert_eq!(outcome.summary.osm_by_method, methods(&[("ref_eu_plc", 1)]));
    assert_eq!(
        outcome.summary.estimated_by_method,
        methods(&[("route_end_north", 1)])
    );
}

#[test]
fn czptt_r022_r025_osm_candidates_tags_plc_country_and_order() {
    let mut f = Fixture::new();
    f.node(
        10,
        50.0,
        14.0,
        &[
            ("railway", "halt"),
            ("name", " Foo "),
            ("alt_name", "Foo"),
            ("name:de", "Fú"),
            ("ref:EU:PLC", " cz 57 076 "),
            ("addr:country", "DE"),
        ],
    );
    f.node(
        9,
        48.0,
        16.0,
        &[
            ("public_transport", "stop_position"),
            ("addr:country", "AUT"),
            ("ref:EU:PLC", "AT1234"),
        ],
    );
    f.node(11, 50.0, 14.0, &[("railway", "crossing"), ("name", "Foo")]);
    f.node(12, 50.0, 14.0, &[("railway", "Station"), ("name", "Foo")]);
    f.node(
        13,
        50.0,
        18.0,
        &[
            ("railway", "station"),
            ("addr:country", "  "),
            ("is_in:country_code", "pl "),
        ],
    );
    let mut diagnostics = Diagnostics::new();
    let evidence = f.evidence(&mut diagnostics);

    let summary: Vec<_> = evidence
        .candidates
        .iter()
        .map(|candidate| {
            (
                candidate.id,
                candidate.plc.as_deref(),
                candidate.country.as_deref(),
                candidate
                    .names
                    .iter()
                    .map(|name| name.full())
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (9, None, Some("AT"), vec![]),
            (10, Some("CZ57076"), Some("CZ"), vec!["FOO", "FU"]),
            (13, None, Some("PL"), vec![]),
        ]
    );
    assert!(diagnostics.is_empty());
}

#[test]
fn czptt_r089_r094_plc_and_exact_name_matches() {
    let mut f = Fixture::new();
    f.path(
        "a",
        &[
            at("AT", "11111", "Node station", "08:00", true),
            at("AT", "22222", "Way station", "08:10", true),
        ],
    );
    f.node(
        1,
        48.20,
        16.30,
        &[
            ("railway", "station"),
            ("name", "Node station"),
            ("ref:EU:PLC", "AT11111"),
        ],
    );
    f.node(
        2,
        48.30,
        16.40,
        &[("railway", "station"), ("name", "Way station")],
    );
    let outcome = f.run();

    assert_eq!(
        outcome.summary.osm_by_method,
        methods(&[("normalized_exact_name", 1), ("ref_eu_plc", 1)])
    );
    assert!(outcome.diagnostics.is_empty(), "{:?}", outcome.diagnostics);
    f.apply(outcome);
    let node = f.position_of("AT", "11111").unwrap();
    assert_eq!(
        (node.precision, node.source),
        (Precision::Exact, Some(CoordSource::Osm))
    );
    assert_near(node, 16.30, 48.20);
    assert_near(f.position_of("AT", "22222").unwrap(), 16.40, 48.30);
}

/// The F# test "Unknown-country OSM names allow railway suffix and fuzzy matching", with the
/// timetable name carrying the parenthesis this time: the core applies to both sides (B6).
#[test]
fn czptt_r091_r092_r094_railway_core_and_fuzzy_matches_independent_of_threads() {
    let mut f = Fixture::new();
    f.path(
        "a",
        &[
            at("CZ", "57076", "Praha", "08:00", true),
            at("AT", "44444", "Unterretzbach z", "08:10", true),
            at("PL", "55555", "Głuchołas.F-ka Mebli", "08:20", true),
            at("DE", "66666", "Bärenstein (Erzgeb)", "08:25", true),
            at("CZ", "57016", "Kolín", "08:30", true),
        ],
    );
    f.sr70_at("57076", "Praha", 50.083, 14.435);
    f.sr70_at("57016", "Kolín", 50.026, 15.214);
    f.node(
        1,
        50.07,
        14.70,
        &[("railway", "station"), ("name", "Unterretzbach")],
    );
    f.node(
        2,
        50.05,
        14.95,
        &[
            ("railway", "halt"),
            ("name", "Głuchołazy Fabryka Mebli"),
            ("addr:country", "DE"),
        ],
    );
    f.node(
        3,
        50.04,
        15.10,
        &[("railway", "halt"), ("name", "Bärenstein")],
    );
    let run_on = |threads: usize| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| f.run())
    };
    let outcome = run_on(1);
    let parallel = run_on(4);
    assert_eq!(outcome.edits, parallel.edits);
    assert_eq!(outcome.diagnostics, parallel.diagnostics);

    let rule_and_place = |country, code| {
        let (position, rule) = outcome.position(&f.t, country, code).unwrap();
        (rule, position.coord.lon)
    };
    assert_eq!(rule_and_place("AT", "44444"), ("CZPTT-R094", 14.70));
    assert_eq!(rule_and_place("PL", "55555"), ("CZPTT-R094", 14.95));
    assert_eq!(rule_and_place("DE", "66666"), ("CZPTT-R094", 15.10));
    assert_eq!(
        outcome.summary.osm_by_method,
        methods(&[("normalized_fuzzy_name", 1), ("normalized_railway_name", 2)])
    );
    assert!(outcome.summary.estimated_by_method.is_empty());
}

#[test]
fn czptt_r093_r100_protected_qualifier_blocks_fuzzy_and_the_place_is_estimated_by_time() {
    let mut f = Fixture::new();
    f.path(
        "a",
        &[
            at("CZ", "57076", "Schmilka-Hirschmühle", "08:00", true),
            at("DE", "10535", "Bad Schandau Ost", "08:10", true),
            at("CZ", "57016", "Krippen Hp", "08:20", true),
        ],
    );
    f.sr70_at("57076", "Schmilka-Hirschmühle", 50.900, 14.180);
    f.sr70_at("57016", "Krippen Hp", 50.920, 14.220);
    f.node(
        1,
        50.9189,
        14.1389,
        &[
            ("railway", "station"),
            ("name", "Bad Schandau"),
            ("addr:country", "DE"),
        ],
    );
    let outcome = f.run();

    let (position, rule) = outcome.position(&f.t, "DE", "10535").unwrap();
    assert_eq!(rule, "CZPTT-R100");
    assert_eq!(
        (position.precision, position.source),
        (Precision::Estimated, Some(CoordSource::RouteTime))
    );
    assert_near(position, 14.200, 50.910);
    assert!(outcome.summary.osm_by_method.is_empty());
    assert_eq!(outcome.diagnostics("czptt_estimated_position").len(), 1);
}

#[test]
fn czptt_r095_r096_corridor_veto_and_ranked_ambiguity_with_country_hint() {
    let mut f = Fixture::new();
    f.path(
        "a",
        &[
            at("CZ", "57076", "Praha", "08:00", true),
            at("AT", "44444", "Far Name", "08:10", true),
            at("CZ", "57016", "Kolín", "08:20", true),
            at("DE", "55555", "Ambiguous", "08:30", true),
            at("SK", "66666", "Wrong Country", "08:40", true),
        ],
    );
    f.sr70_at("57076", "Praha", 50.083, 14.435);
    f.sr70_at("57016", "Kolín", 50.026, 15.214);
    f.node(
        1,
        0.0,
        0.0,
        &[
            ("railway", "station"),
            ("name", "Far Name"),
            ("addr:country", "AT"),
        ],
    );
    // Node 3 is nearer Kolín, but tagged with another country than the point's.
    f.node(
        2,
        50.04,
        15.22,
        &[
            ("railway", "station"),
            ("name", "Ambiguous"),
            ("addr:country", "DEU"),
        ],
    );
    f.node(
        3,
        50.03,
        15.22,
        &[
            ("railway", "station"),
            ("name", "Ambiguous"),
            ("addr:country", "PL"),
        ],
    );
    f.node(
        4,
        48.1,
        17.1,
        &[
            ("railway", "station"),
            ("name", "Wrong Country"),
            ("addr:country", "PL"),
        ],
    );
    let outcome = f.run();

    let (ambiguous, rule) = outcome.position(&f.t, "DE", "55555").unwrap();
    assert_eq!(rule, "CZPTT-R096");
    assert_near(ambiguous, 15.22, 50.04);
    assert_eq!(
        outcome.diagnostics("czptt_osm_ambiguous_ranked"),
        [(
            id("DE", "55555").as_str(),
            "normalized_exact_name: 2 plausible candidates (osm:node:2,osm:node:3), ranked first: osm:node:2"
        )]
    );
    let rejected: Vec<_> = outcome
        .diagnostics("czptt_osm_corridor_rejected")
        .into_iter()
        .map(|(subject, _)| subject.to_owned())
        .collect();
    assert_eq!(rejected, [id("AT", "44444"), id("SK", "66666")]);
    for code in ["44444", "66666"] {
        let country = if code == "44444" { "AT" } else { "SK" };
        let (position, _) = outcome.position(&f.t, country, code).unwrap();
        assert_eq!(position.precision, Precision::Estimated);
    }
    assert_eq!(outcome.summary.unresolved_passenger_places, 0);
}

#[test]
fn czptt_r095_one_anomalous_occurrence_does_not_veto_a_match() {
    let mut f = Fixture::new();
    f.path(
        "a",
        &[
            at("CZ", "57076", "Anchor A", "08:00", true),
            at("AT", "44444", "Foreign", "08:10", true),
            at("CZ", "57016", "Anchor B", "08:20:00", true),
            // 6 km in 30 seconds.
            at("AT", "44444", "Foreign", "08:20:30", true),
        ],
    );
    f.sr70_at("57076", "Anchor A", 50.08, 14.44);
    f.sr70_at("57016", "Anchor B", 50.08, 14.60);
    f.node(
        1,
        50.08,
        14.52,
        &[("railway", "station"), ("name", "Foreign")],
    );
    let outcome = f.run();

    assert_eq!(
        outcome.summary.osm_by_method,
        methods(&[("normalized_exact_name", 1)])
    );
    assert!(outcome.summary.estimated_by_method.is_empty());
    assert!(
        outcome
            .diagnostics("czptt_osm_corridor_rejected")
            .is_empty()
    );
}

#[test]
fn czptt_r097_sr70_is_authoritative_over_disagreeing_osm_and_name_matches() {
    let mut f = Fixture::new();
    f.path(
        "a",
        &[
            at("CZ", "57076", "Praha", "08:00", true),
            // Passed without stopping; an OSM station of the same name lies elsewhere.
            at("CZ", "57423", "Vchynice", "08:10", false),
            at("CZ", "57016", "Kolín", "08:20", true),
        ],
    );
    f.sr70_at("57076", "Praha", 50.083, 14.435);
    f.sr70_at("57423", "Vchynice", 50.500, 14.038);
    f.sr70_at("57016", "Kolín", 50.026, 15.214);
    f.node(
        1,
        50.083,
        14.435,
        &[("railway", "station"), ("ref:EU:PLC", "CZ57076")],
    );
    f.node(
        2,
        50.100,
        14.500,
        &[("railway", "station"), ("ref:EU:PLC", "CZ57016")],
    );
    f.node(
        3,
        49.816,
        13.564,
        &[("railway", "station"), ("name", "Vchynice")],
    );
    let outcome = f.run();

    assert_eq!(outcome.summary.sr70, 3);
    assert!(outcome.summary.osm_by_method.is_empty());
    let (vchynice, rule) = outcome.position(&f.t, "CZ", "57423").unwrap();
    assert_eq!(
        (vchynice.source, rule),
        (Some(CoordSource::Sr70), "CZPTT-R097")
    );
    assert_near(vchynice, 14.038, 50.500);
    let (kolin, _) = outcome.position(&f.t, "CZ", "57016").unwrap();
    assert_near(kolin, 15.214, 50.026);
    let disagreements = outcome.diagnostics("czptt_osm_sr70_disagreement");
    assert_eq!(disagreements.len(), 1);
    assert_eq!(disagreements[0].0, id("CZ", "57016"));
    assert!(disagreements[0].1.contains("osm:node:2"));
}

#[test]
fn czptt_r097_r100_sr70_never_places_a_foreign_point_with_the_same_code() {
    let mut f = Fixture::new();
    f.path(
        "a",
        &[
            at("CZ", "57076", "Praha hl.n.", "08:00", true),
            at("DE", "57076", "Foreign", "08:20", true),
        ],
    );
    f.sr70_at("57076", "Praha hl.n.", 50.083, 14.435);
    let outcome = f.run();

    let (czech, _) = outcome.position(&f.t, "CZ", "57076").unwrap();
    assert_eq!(czech.source, Some(CoordSource::Sr70));
    let (german, rule) = outcome.position(&f.t, "DE", "57076").unwrap();
    assert_eq!(
        (german.source, rule),
        (Some(CoordSource::RouteEnd), "CZPTT-R100")
    );
    assert_near(german, 14.435, 50.083 + NORTH_PER_CALL);
}

/// Y first takes a far PLC node, unconstrained, and vetoes N's correct node; once T is placed,
/// re-evaluation moves Y to its name match and N's node is accepted. Only the final state is
/// reported: Y's PLC node as rejected, nothing for N (B13).
#[test]
fn czptt_r098_re_evaluation_reports_only_the_final_state() {
    let mut f = Fixture::new();
    f.path(
        "a",
        &[
            at("AT", "11111", "Ypsilon", "08:00", true),
            at("AT", "22222", "Tau", "08:10", true),
        ],
    );
    f.path(
        "b",
        &[
            at("CZ", "54001", "Es", "09:00", true),
            at("AT", "22222", "Tau", "09:10", true),
        ],
    );
    f.path(
        "c",
        &[
            at("AT", "11111", "Ypsilon", "10:00", true),
            at("AT", "33333", "Nu", "10:10", true),
        ],
    );
    f.sr70_at("54001", "Es", 50.0, 14.0);
    f.node(
        1,
        50.0,
        16.0,
        &[("railway", "station"), ("ref:EU:PLC", "AT11111")],
    );
    f.node(
        2,
        50.0,
        14.2,
        &[("railway", "station"), ("name", "Ypsilon")],
    );
    f.node(3, 50.0, 14.1, &[("railway", "station"), ("name", "Tau")]);
    f.node(4, 50.0, 14.3, &[("railway", "station"), ("name", "Nu")]);
    let outcome = f.run();

    assert_near(outcome.position(&f.t, "AT", "11111").unwrap().0, 14.2, 50.0);
    assert_near(outcome.position(&f.t, "AT", "22222").unwrap().0, 14.1, 50.0);
    assert_near(outcome.position(&f.t, "AT", "33333").unwrap().0, 14.3, 50.0);
    assert_eq!(
        outcome.diagnostics("czptt_osm_corridor_rejected"),
        [(
            id("AT", "11111").as_str(),
            "ref_eu_plc: osm:node:1 cannot be reached in time from the placed neighbours"
        )]
    );

    // Without re-evaluation the first pass stands: N's node vetoed by Y's far PLC node.
    let mut policy = Policy::builtin();
    policy.resolution.reevaluation_passes = 0;
    let first_pass = f.run_with(&policy);
    let rejected: Vec<_> = first_pass
        .diagnostics("czptt_osm_corridor_rejected")
        .into_iter()
        .map(|(subject, _)| subject.to_owned())
        .collect();
    assert_eq!(rejected, [id("AT", "33333")]);
}

#[test]
fn czptt_r100_dense_passenger_corridor_wins_independent_of_run_order() {
    let estimate_with = |dense: &str, sparse: &str| {
        let mut f = Fixture::new();
        // The dense run is split at the missing point: its boundary call counts once.
        f.parts(
            dense,
            &[
                &[
                    at("CZ", "10001", "Dense passenger A", "08:00", true),
                    at("CZ", "10003", "Operational A", "08:05", false),
                    at("DE", "10535", "Missing point", "08:10", true),
                ],
                &[
                    at("DE", "10535", "Missing point", "08:10", true),
                    at("CZ", "10004", "Operational B", "08:15", false),
                    at("CZ", "10002", "Dense passenger B", "08:20", true),
                ],
            ],
        );
        f.path(
            sparse,
            &[
                at("CZ", "10005", "Sparse passenger A", "09:00", true),
                at("CZ", "10003", "Operational A", "09:05", false),
                at("CZ", "10003", "Operational A", "09:10", false),
                at("DE", "10535", "Missing point", "09:20", true),
                at("CZ", "10004", "Operational B", "09:30", false),
                at("CZ", "10004", "Operational B", "09:35", false),
                at("CZ", "10006", "Sparse passenger B", "09:40", true),
            ],
        );
        f.sr70_at("10001", "Dense passenger A", 50.0, 14.0);
        f.sr70_at("10002", "Dense passenger B", 50.0, 14.4);
        f.sr70_at("10003", "Operational A", 49.0, 10.0);
        f.sr70_at("10004", "Operational B", 49.0, 12.0);
        f.sr70_at("10005", "Sparse passenger A", 49.0, 10.0);
        f.sr70_at("10006", "Sparse passenger B", 49.0, 12.0);
        let outcome = f.run();
        outcome.position(&f.t, "DE", "10535").unwrap().0
    };
    let first = estimate_with("z-dense", "a-sparse");
    let second = estimate_with("a-dense", "z-sparse");
    assert_eq!(first.source, Some(CoordSource::RouteTime));
    assert_near(first, 14.2, 50.0);
    assert_eq!(first, second);
}

#[test]
fn czptt_r099_a_timing_only_point_is_never_estimated() {
    let mut f = Fixture::new();
    f.path(
        "a",
        &[
            at("CZ", "10001", "Passenger A", "08:00", true),
            at("DE", "10535", "Unresolved timing point", "08:10", false),
            at("CZ", "10002", "Passenger B", "08:20", true),
        ],
    );
    f.sr70_at("10001", "Passenger A", 50.0, 14.0);
    f.sr70_at("10002", "Passenger B", 50.0, 14.2);
    let outcome = f.run();

    assert!(outcome.position(&f.t, "DE", "10535").is_none());
    assert_eq!(
        outcome.diagnostics("czptt_unresolved_place"),
        [(
            id("DE", "10535").as_str(),
            "timing-only point without SR70 or OSM position; not estimated"
        )]
    );
    assert_eq!(
        (
            outcome.summary.unresolved_other_places,
            outcome.summary.unresolved_passenger_places
        ),
        (1, 0)
    );
}

#[test]
fn czptt_r101_r104_estimates_never_anchor_and_unresolved_passenger_places_are_diagnosed() {
    let mut f = Fixture::new();
    f.path(
        "1",
        &[
            at("CZ", "10001", "Anchor A", "08:00", true),
            at("DE", "10535", "Estimated first", "08:10", true),
            at("CZ", "10002", "Anchor B", "08:20", true),
        ],
    );
    f.path(
        "2",
        &[
            at("DE", "10535", "Estimated first", "09:00", true),
            at("DE", "10536", "Must remain unresolved", "09:10", true),
        ],
    );
    f.sr70_at("10001", "Anchor A", 50.0, 14.0);
    f.sr70_at("10002", "Anchor B", 50.0, 14.2);
    let outcome = f.run();

    let (estimated, _) = outcome.position(&f.t, "DE", "10535").unwrap();
    assert_eq!(estimated.source, Some(CoordSource::RouteTime));
    assert!(outcome.position(&f.t, "DE", "10536").is_none());
    assert_eq!(
        outcome
            .diagnostics("czptt_unresolved_passenger_place")
            .into_iter()
            .map(|(subject, _)| subject)
            .collect::<Vec<_>>(),
        [id("DE", "10536")]
    );
    assert_eq!(outcome.summary.unresolved_passenger_places, 1);
    assert_eq!(outcome.counts().get("czptt_estimated_position"), Some(&1));
}
