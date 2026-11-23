//! Stop matching through `fix`, on small batches and tiny evidence. The cases port
//! `JdfFixupsTests.fs` and `ExternalCsvTests.fs` of the F# JrUtil, plus the verdict fixes.

mod fix_support;

use std::collections::BTreeSet;

use fix_support::*;
use jr_jdf::fixed::{FixedStop, Origin};
use jr_model::Coord;

fn stop_at(fixed: &jr_jdf::fixed::FixedBatch, id: u64) -> &FixedStop {
    let index = fixed
        .batch
        .stops
        .iter()
        .position(|s| s.id == id)
        .expect("stop");
    &fixed.stops[index]
}

fn coord_of(fixed: &jr_jdf::fixed::FixedBatch, id: u64) -> Option<Coord> {
    stop_at(fixed, id).location.as_ref().map(|l| l.coord)
}

/// Stop 1 is `Exact stop` with the given okres and country; stop 2 is an unmatched companion.
fn exact_stop_batch(okres: &str, country: &str) -> jr_jdf::batch::Batch {
    batch(
        "b",
        vec![
            located(
                stop(1, "Exact", Some("stop"), None),
                Some(okres),
                Some(country),
            ),
            stop(2, "Nowhere", Some("at all"), None),
        ],
        &[(1, vec![at(1, 0), at(2, 5)])],
    )
}

#[test]
fn jdf_r033_batch_with_one_distinct_called_stop_is_dropped() {
    let evidence = evidence(Vec::new());
    let one_stop = batch(
        "b",
        vec![stop(1, "A", Some("B"), None)],
        &[(1, vec![at(1, 0), at(1, 5)])],
    );
    let (fixed, diagnostics) = fix(one_stop, &evidence);
    assert!(fixed.is_none());
    assert_eq!(codes(&diagnostics), ["jdf_batch_degenerate"]);
}

#[test]
fn jdf_r045_historical_ol_code_matches_canonical_oc_strictly() {
    let evidence = evidence(vec![row(
        "Exact,stop",
        49.65,
        17.25,
        Some("OC"),
        Some("CZ"),
    )]);
    let (fixed, _) = fix(exact_stop_batch("OL", "CZ"), &evidence);
    let fixed = fixed.unwrap();
    assert!(coord_of(&fixed, 1).is_some());
    // The published okres is the source's.
    assert_eq!(stop_at(&fixed, 1).okres.as_deref(), Some("OL"));
}

#[test]
fn jdf_r046_adjacent_okres_candidate_on_the_boundary_is_accepted() {
    // In BB, 360 m east of the AA border.
    let evidence = evidence(vec![row(
        "Exact,stop",
        50.15,
        14.505,
        Some("BB"),
        Some("CZ"),
    )]);
    let (fixed, diagnostics) = fix(exact_stop_batch("AA", "CZ"), &evidence);
    assert!(coord_of(&fixed.unwrap(), 1).is_some());
    assert!(codes(&diagnostics).contains(&"jdf_match_geography"));
}

#[test]
fn jdf_r044_strict_okres_candidate_wins_over_an_adjacent_boundary_candidate() {
    let evidence = evidence(vec![
        row("Exact,stop", 50.15, 14.505, Some("BB"), Some("CZ")),
        row("Exact,stop", 50.15, 14.25, Some("AA"), Some("CZ")),
    ]);
    let (fixed, _) = fix(exact_stop_batch("AA", "CZ"), &evidence);
    assert_eq!(coord_of(&fixed.unwrap(), 1), Some(Coord::new(14.25, 50.15)));
}

#[test]
fn jdf_r046_adjacent_okres_candidate_outside_tolerance_and_wrong_country_are_rejected() {
    let deep = evidence(vec![row("Exact,stop", 50.15, 14.9, Some("BB"), Some("CZ"))]);
    let (fixed, _) = fix(exact_stop_batch("AA", "CZ"), &deep);
    assert!(coord_of(&fixed.unwrap(), 1).is_none_or(|c| c != Coord::new(14.9, 50.15)));
    let german = evidence(vec![row("Exact,stop", 50.15, 14.25, Some("AA"), Some("D"))]);
    let (fixed, _) = fix(exact_stop_batch("AA", "CZ"), &german);
    assert_ne!(
        stop_at(fixed.as_ref().unwrap(), 1)
            .location
            .as_ref()
            .map(|l| l.origin),
        Some(Origin::Catalogue)
    );
}

#[test]
fn jdf_r046_non_adjacent_okres_candidate_is_rejected() {
    let evidence = evidence(vec![row(
        "Exact,stop",
        50.15,
        16.25,
        Some("CC"),
        Some("CZ"),
    )]);
    let (fixed, _) = fix(exact_stop_batch("AA", "CZ"), &evidence);
    let location = stop_at(fixed.as_ref().unwrap(), 1).location.clone();
    assert!(location.is_none_or(|l| !l.origin.is_precise()));
}

#[test]
fn jdf_r044_foreign_exact_match_does_not_require_a_czech_okres() {
    let evidence = evidence(vec![row("Exact,stop", 48.6, 17.0, None, Some("SK"))]);
    let (fixed, _) = fix(exact_stop_batch("PU", "SK"), &evidence);
    assert!(
        stop_at(&fixed.unwrap(), 1)
            .location
            .as_ref()
            .unwrap()
            .origin
            .is_precise()
    );
}

#[test]
fn jdf_r043_countries_compare_through_the_jdf_iso_table() {
    // Verdict Q8: the catalogue's `DE` is the timetable's `D` (B7 fixed for matching).
    let evidence = evidence(vec![row("Breitenau,ZOLL", 50.7, 13.8, None, Some("DE"))]);
    let fixed = fix(
        batch(
            "b",
            vec![
                located(stop(1, "Breitenau", Some("ZOLL"), None), None, Some("D")),
                stop(2, "Nowhere", Some("at all"), None),
            ],
            &[(1, vec![at(1, 0), at(2, 5)])],
        ),
        &evidence,
    )
    .0
    .unwrap();
    assert!(
        stop_at(&fixed, 1)
            .location
            .as_ref()
            .unwrap()
            .origin
            .is_precise()
    );
}

#[test]
fn jdf_r047_checked_exact_match_wins_over_an_osm_exact_match() {
    let (evidence, _) = evidence_with(
        vec![(
            "manual.csv",
            vec![row("Exact,stop", 50.15, 14.25, Some("AA"), Some("CZ"))],
        )],
        vec![bus_stop(7, "Exact,stop", 50.15, 14.32)],
        Vec::new(),
    );
    let fixed = fix(exact_stop_batch("AA", "CZ"), &evidence).0.unwrap();
    let location = stop_at(&fixed, 1).location.clone().unwrap();
    assert_eq!(location.coord, Coord::new(14.25, 50.15));
    assert_eq!(location.origin, Origin::Catalogue);
    assert_eq!(&*location.source, "external:manual");
}

#[test]
fn jdf_r007_gapfill_catalogue_positions_are_reviewed_corrections() {
    let (evidence, _) = evidence_with(
        vec![
            (
                "gapfill.csv",
                vec![row("Exact,stop", 50.15, 14.25, Some("AA"), Some("CZ"))],
            ),
            (
                "manual.csv",
                vec![row("Exact,stop", 50.1501, 14.2501, Some("AA"), Some("CZ"))],
            ),
        ],
        Vec::new(),
        Vec::new(),
    );
    let fixed = fix(exact_stop_batch("AA", "CZ"), &evidence).0.unwrap();
    let location = stop_at(&fixed, 1).location.clone().unwrap();
    assert_eq!(location.origin, Origin::Gapfill);
    assert_eq!(&*location.source, "external:gapfill+external:manual");
    // R066: both rows are post candidates, sorted by observation.
    let observations: Vec<&str> = fixed
        .post_candidates
        .iter()
        .map(|c| &*c.observation)
        .collect();
    // JDF-R007: F#'s identity, a hash of the relative path and the five columns.
    let id = |path: &str, lat: &str, lon: &str| {
        let row = format!("{path}|Exact,stop|{lat}|{lon}|AA|CZ");
        format!(
            "external:{path}:sha256:{}",
            jr_formats::post_evidence::ids::sha256_hex(row.as_bytes())
        )
    };
    let mut expected = [
        id("gapfill.csv", "50.15", "14.25"),
        id("manual.csv", "50.1501", "14.2501"),
    ];
    expected.sort();
    assert_eq!(observations, expected);
}

/// A catalogue row without an okres and another catalogue's row with one describe one stop: the
/// stop is located. Two rows that state different okresy stay ambiguous.
#[test]
fn jdf_r048_a_row_without_okres_agrees_with_one_that_states_it() {
    let (both, _) = evidence_with(
        vec![
            (
                "catalogue.csv",
                vec![row("Exact,stop", 48.8963, 18.0421, None, Some("SK"))],
            ),
            (
                "other.csv",
                vec![row("Exact,stop", 48.8963, 18.0414, Some("TN"), Some("SK"))],
            ),
        ],
        Vec::new(),
        Vec::new(),
    );
    let fixed = fix(exact_stop_batch("TN", "SK"), &both).0.unwrap();
    let location = stop_at(&fixed, 1).location.clone().unwrap();
    assert!(location.origin.is_precise());
    assert_eq!(&*location.source, "external:catalogue+external:other");

    let evidence = evidence(vec![
        row("Exact,stop", 48.8963, 18.0421, Some("TN"), Some("SK")),
        row("Exact,stop", 48.8963, 18.0414, Some("PU"), Some("SK")),
    ]);
    let (fixed, diagnostics) = fix(exact_stop_batch("TN", "SK"), &evidence);
    assert!(
        !stop_at(&fixed.unwrap(), 1)
            .location
            .as_ref()
            .is_some_and(|l| l.origin.is_precise())
    );
    assert!(codes(&diagnostics).contains(&"jdf_match_ambiguous"));
}

/// Catalogue rows of one name 2 km apart place nothing, and a bare municipality name loses its
/// match: in both cases the reviewed row places the stop. Where the rows do agree they are
/// pooled as before (the test of R007).
#[test]
fn jdf_r082_an_unmatched_stop_takes_its_reviewed_row() {
    let (apart, _) = evidence_with(
        vec![
            (
                "gapfill.csv",
                vec![row("Exact,stop", 50.15, 14.25, Some("AA"), Some("CZ"))],
            ),
            (
                "manual.csv",
                vec![
                    row("Exact,stop", 50.1501, 14.2501, Some("AA"), Some("CZ")),
                    row("Exact,stop", 50.17, 14.25, Some("AA"), Some("CZ")),
                ],
            ),
        ],
        Vec::new(),
        Vec::new(),
    );
    let (fixed, diagnostics) = fix(exact_stop_batch("AA", "CZ"), &apart);
    let location = stop_at(&fixed.unwrap(), 1).location.clone().unwrap();
    assert_eq!(location.origin, Origin::Gapfill);
    assert_eq!(&*location.source, "external:gapfill");
    assert!(codes(&diagnostics).contains(&"jdf_match_reviewed"));

    let (village, _) = evidence_with(
        vec![(
            "gapfill.csv",
            vec![row("Adamov", 50.15, 14.75, Some("BB"), Some("CZ"))],
        )],
        Vec::new(),
        Vec::new(),
    );
    let stops = vec![
        stop(1, "Adamov", None, None),
        stop(2, "Nowhere", Some("at all"), None),
    ];
    let (fixed, _) = fix(
        batch("b", stops, &[(1, vec![at(1, 0), at(2, 5)])]),
        &village,
    );
    assert!(
        stop_at(&fixed.unwrap(), 1)
            .location
            .as_ref()
            .is_some_and(|l| l.origin.is_precise())
    );
}

#[test]
fn jdf_r048_candidates_far_apart_are_ambiguous() {
    let evidence = evidence(vec![
        row("Exact,stop", 50.15, 14.25, Some("AA"), Some("CZ")),
        row("Exact,stop", 50.25, 14.25, Some("AA"), Some("CZ")),
    ]);
    let (fixed, diagnostics) = fix(exact_stop_batch("AA", "CZ"), &evidence);
    assert!(
        !stop_at(&fixed.unwrap(), 1)
            .location
            .as_ref()
            .is_some_and(|l| l.origin.is_precise())
    );
    assert!(codes(&diagnostics).contains(&"jdf_match_ambiguous"));
}

/// Stops 1–5 on one line, each matched by its own catalogue row at `x` metres east of 14.0E.
fn travel_time_case(rows: &[(u64, f64, &str)], minutes: &[u16]) -> Vec<bool> {
    let east = |metres: f64| 14.0 + metres / 71_700.0;
    let mut catalogue = Vec::new();
    let mut nodes = Vec::new();
    for &(id, metres, source) in rows {
        let name = format!("Stop,{id}");
        if source == "osm" {
            nodes.push(bus_stop(id as i64, &name, 50.0, east(metres)));
        } else {
            catalogue.push(row(&name, 50.0, east(metres), None, None));
        }
    }
    let (evidence, _) = evidence_with(vec![("test.csv", catalogue)], nodes, Vec::new());
    let stops = rows
        .iter()
        .map(|&(id, _, _)| stop(id, "Stop", Some(&id.to_string()), None))
        .collect();
    let calls = rows
        .iter()
        .zip(minutes)
        .map(|(&(id, _, _), &m)| at(id, m))
        .collect();
    let fixed = fix(batch("b", stops, &[(1, calls)]), &evidence).0.unwrap();
    rows.iter()
        .map(|&(id, _, _)| {
            stop_at(&fixed, id)
                .location
                .as_ref()
                .is_some_and(|l| l.origin.is_precise())
        })
        .collect()
}

#[test]
fn jdf_r070_longer_consistent_run_outvotes_a_short_run_of_wrong_matches() {
    let result = travel_time_case(
        &[
            (1, 0.0, "cat"),
            (2, 3_000.0, "cat"),
            (3, 6_000.0, "cat"),
            (4, 200_000.0, "cat"),
            (5, 203_000.0, "cat"),
        ],
        &[0, 3, 6, 9, 12],
    );
    assert_eq!(result, [true, true, true, false, false]);
}

#[test]
fn jdf_r070_checked_match_survives_a_tied_conflict_with_an_osm_match() {
    let result = travel_time_case(&[(1, 0.0, "cat"), (2, 200_000.0, "osm")], &[0, 5]);
    assert_eq!(result, [true, false]);
}

#[test]
fn jdf_r070_indistinguishable_conflicting_matches_are_both_rejected() {
    let result = travel_time_case(&[(1, 0.0, "cat"), (2, 200_000.0, "cat")], &[0, 5]);
    assert_eq!(result, [false, false]);
}

#[test]
fn jdf_r069_impossible_middle_match_is_rejected_and_estimated_by_scheduled_time() {
    let east = |metres: f64| 14.0 + metres / 71_700.0;
    let evidence = evidence(vec![
        row("Stop,first", 50.0, east(0.0), None, None),
        row(
            "Stop,middle",
            50.0,
            east(1_000_000.0).min(179.0),
            None,
            None,
        ),
        row("Stop,not passing", 50.0, east(8_000.0), None, None),
        row("Stop,last", 50.0, east(10_000.0), None, None),
    ]);
    let stops = vec![
        stop(1, "Stop", Some("first"), None),
        stop(300, "Stop", Some("middle"), None),
        stop(400, "Stop", Some("not passing"), None),
        stop(2, "Stop", Some("last"), None),
    ];
    let calls = vec![at(1, 0), at(300, 5), passes(400), at(2, 10)];
    let (fixed, diagnostics) = fix(batch("b", stops, &[(1, calls)]), &evidence);
    let fixed = fixed.unwrap();
    assert!(codes(&diagnostics).contains(&"jdf_match_implausible"));
    let middle = stop_at(&fixed, 300).location.clone().unwrap();
    assert_eq!(middle.origin, Origin::RouteTime);
    assert_eq!(&*middle.source, "estimated:route-time");
    let (first, last) = (coord_of(&fixed, 1).unwrap(), coord_of(&fixed, 2).unwrap());
    assert!((middle.coord.lon - (first.lon + last.lon) / 2.0).abs() < 0.001);
    // The untimed call keeps its match: it is no anchor and no conflict.
    assert!(
        stop_at(&fixed, 400)
            .location
            .as_ref()
            .unwrap()
            .origin
            .is_precise()
    );
}

#[test]
fn jdf_r077_unmatched_route_origin_is_offset_north_of_its_first_known_stop() {
    let evidence = evidence(vec![row("Stop,known", 50.0, 14.0, None, None)]);
    let stops = vec![
        stop(1, "Stop", Some("origin"), None),
        stop(2, "Stop", Some("known"), None),
    ];
    let fixed = fix(
        batch("b", stops, &[(1, vec![at(1, 0), at(2, 5)])]),
        &evidence,
    )
    .0
    .unwrap();
    let origin = stop_at(&fixed, 1).location.clone().unwrap();
    assert_eq!(origin.origin, Origin::RouteEnd);
    assert_eq!(&*origin.source, "estimated:route-end-north");
    assert!(origin.coord.lat > 50.0);
    assert!((origin.coord.lat - (50.0 + 300.0 / 111_320.0)).abs() < 1e-9);
}

#[test]
fn jdf_r015_both_polygons_of_an_okres_count() {
    // `LT`'s first polygon borders `UL`; F# kept only the last polygon (B2) and rejected this.
    let evidence = evidence(vec![row(
        "Exact,stop",
        50.6,
        13.305,
        Some("UL"),
        Some("CZ"),
    )]);
    let (fixed, _) = fix(exact_stop_batch("LT", "CZ"), &evidence);
    assert!(
        stop_at(&fixed.unwrap(), 1)
            .location
            .as_ref()
            .unwrap()
            .origin
            .is_precise()
    );
}

#[test]
fn jdf_r062_propagation_with_the_ol_code_does_not_abort() {
    // B3: the edge distance of `OL` is looked up as `OC`.
    let evidence = evidence(vec![row(
        "Exact,stop",
        49.65,
        17.25,
        Some("OC"),
        Some("CZ"),
    )]);
    let stops = vec![
        located(stop(1, "Exact", Some("stop"), None), Some("OL"), Some("CZ")),
        stop(2, "Nowhere", Some("at all"), None),
    ];
    let fixed = fix(
        batch("b", stops, &[(1, vec![at_km(1, 0, 0.0), at_km(2, 5, 1.0)])]),
        &evidence,
    )
    .0
    .unwrap();
    // 1 km from a stop 17 km inside its okres: the okres carries over.
    assert_eq!(stop_at(&fixed, 2).okres.as_deref(), Some("OL"));
    assert_eq!(stop_at(&fixed, 2).country.as_deref(), Some("CZ"));
}

#[test]
fn jdf_r061_okres_and_country_propagate_unless_a_border_may_lie_between() {
    let evidence = evidence(vec![row(
        "Exact,stop",
        50.15,
        14.25,
        Some("AA"),
        Some("CZ"),
    )]);
    let make = |km: f64| {
        batch(
            "b",
            vec![
                located(stop(1, "Exact", Some("stop"), None), Some("AA"), Some("CZ")),
                stop(2, "Nowhere", Some("at all"), None),
            ],
            &[(1, vec![at_km(1, 0, 0.0), at_km(2, 5, km)])],
        )
    };
    let (near, diagnostics) = fix(make(1.0), &evidence);
    assert_eq!(stop_at(&near.unwrap(), 2).okres.as_deref(), Some("AA"));
    assert!(codes(&diagnostics).contains(&"jdf_geography_propagated"));
    // 40 km on, the line may have left the okres (R062).
    let (far, diagnostics) = fix(make(40.0), &evidence);
    assert_eq!(stop_at(&far.unwrap(), 2).okres, None);
    assert!(codes(&diagnostics).contains(&"jdf_geography_propagation_reverted"));
}

#[test]
fn jdf_r012_osm_names_are_completed_with_their_municipality() {
    let (evidence, _) = evidence_with(
        Vec::new(),
        vec![
            bus_stop(9, "Náves", 50.15, 14.15),
            bus_stop(10, "Lhota,škola", 50.16, 14.16),
        ],
        Vec::new(),
    );
    let stops = vec![
        stop(1, "Lhota", Some("Náves"), None),
        stop(2, "Lhota", Some("škola"), None),
    ];
    let fixed = fix(
        batch("b", stops, &[(1, vec![at(1, 0), at(2, 5)])]),
        &evidence,
    )
    .0
    .unwrap();
    for id in [1, 2] {
        let stop = stop_at(&fixed, id);
        assert_eq!(stop.location.as_ref().unwrap().origin, Origin::Osm, "{id}");
        // R011 and R055: the municipality's okres, the home country.
        assert_eq!(stop.okres.as_deref(), Some("AA"));
        assert_eq!(stop.country.as_deref(), Some("CZ"));
    }
    assert_eq!(
        &*stop_at(&fixed, 1).location.as_ref().unwrap().source,
        "osm:node:9"
    );
}

#[test]
fn jdf_r051_town_only_stop_named_like_a_municipality_has_no_primary_match() {
    let evidence = evidence(vec![row("Adamov", 50.15, 14.75, Some("BB"), Some("CZ"))]);
    let stops = vec![
        stop(1, "Adamov", None, None),
        stop(2, "Nowhere", Some("at all"), None),
    ];
    let (fixed, diagnostics) = fix(
        batch("b", stops, &[(1, vec![at(1, 0), at(2, 5)])]),
        &evidence,
    );
    assert!(
        !stop_at(&fixed.unwrap(), 1)
            .location
            .as_ref()
            .is_some_and(|l| l.origin.is_precise())
    );
    assert!(codes(&diagnostics).contains(&"jdf_match_municipality_name"));
}

#[test]
fn jdf_r050_town_only_stop_matched_to_a_european_town_name_is_not_matched() {
    let (evidence, _) = evidence_with(
        vec![(
            "manual.csv",
            vec![row("Ostrov", 50.15, 14.25, Some("AA"), Some("CZ"))],
        )],
        Vec::new(),
        vec![foreign("Ostrov", "sk")],
    );
    let stops = vec![
        stop(1, "Ostrov", None, None),
        stop(2, "Nowhere", Some("at all"), None),
    ];
    let (fixed, diagnostics) = fix(
        batch("b", stops, &[(1, vec![at(1, 0), at(2, 5)])]),
        &evidence,
    );
    assert!(
        !stop_at(&fixed.unwrap(), 1)
            .location
            .as_ref()
            .is_some_and(|l| l.origin.is_precise())
    );
    assert!(codes(&diagnostics).contains(&"jdf_match_foreign_town"));
}

#[test]
fn jdf_r052_secondary_match_picks_the_candidate_within_kilometre_reach() {
    let evidence = evidence(vec![
        row("Start,stop", 50.15, 14.20, Some("AA"), Some("CZ")),
        row("Nová Ves,u lesa", 50.15, 14.22, Some("AA"), Some("CZ")),
        row("Nová Ves,u lesa", 50.25, 14.45, Some("AA"), Some("CZ")),
    ]);
    let stops = vec![
        located(stop(1, "Start", Some("stop"), None), Some("AA"), Some("CZ")),
        located(
            stop(2, "Nová Ves", Some("u lesa"), None),
            Some("AA"),
            Some("CZ"),
        ),
    ];
    let (fixed, diagnostics) = fix(
        batch("b", stops, &[(1, vec![at_km(1, 0, 0.0), at_km(2, 3, 1.5)])]),
        &evidence,
    );
    assert!(codes(&diagnostics).contains(&"jdf_match_ambiguous"));
    assert!(codes(&diagnostics).contains(&"jdf_match_secondary"));
    assert_eq!(coord_of(&fixed.unwrap(), 2), Some(Coord::new(14.22, 50.15)));
}

#[test]
fn jdf_r053_okres_and_country_of_an_unmatched_stop_from_its_town_name() {
    let (evidence, _) = evidence_with(Vec::new(), Vec::new(), vec![foreign("Dresden", "de")]);
    let stops = vec![
        located(stop(1, "Lhota", Some("u kostela"), None), None, Some("CZ")),
        stop(2, "Dresden", Some("Hbf"), None),
    ];
    let (fixed, diagnostics) = fix(
        batch("b", stops, &[(1, vec![at(1, 0), at(2, 5)])]),
        &evidence,
    );
    let fixed = fixed.unwrap();
    assert_eq!(stop_at(&fixed, 1).okres.as_deref(), Some("AA"));
    // The gazetteer's ISO `DE` is written as the JDF `D` (verdict Q8).
    assert_eq!(stop_at(&fixed, 2).country.as_deref(), Some("D"));
    assert!(codes(&diagnostics).contains(&"jdf_geography_from_town"));
}

#[test]
fn jdf_r036_okres_code_in_brackets_after_the_town_name() {
    let evidence = evidence(vec![row(
        "Exact,stop",
        50.15,
        14.25,
        Some("AA"),
        Some("CZ"),
    )]);
    let stops = vec![
        stop(1, "Exact [AA]", Some("stop"), None),
        stop(2, "Nowhere", Some("x"), None),
    ];
    let fixed = fix(
        batch("b", stops, &[(1, vec![at(1, 0), at(2, 5)])]),
        &evidence,
    )
    .0
    .unwrap();
    let first = stop_at(&fixed, 1);
    assert_eq!(
        (&*first.town, first.okres.as_deref()),
        ("Exact", Some("AA"))
    );
    assert!(first.location.as_ref().unwrap().origin.is_precise());
}

#[test]
fn jdf_r037_the_published_triple_is_the_normalised_one() {
    let evidence = evidence(Vec::new());
    let stops = vec![
        stop(1, "Praha", None, Some("Muzeum")),
        stop(2, "Brno,,Hlavní nádraží", None, None),
        stop(3, "Hevlín, škola", None, None),
    ];
    let fixed = fix(
        batch("b", stops, &[(1, vec![at(1, 0), at(2, 5), at(3, 9)])]),
        &evidence,
    )
    .0
    .unwrap();
    let triple = |id| {
        let s = stop_at(&fixed, id);
        (
            s.town.to_string(),
            s.district.as_deref().map(String::from),
            s.nearby_place.as_deref().map(String::from),
        )
    };
    assert_eq!(triple(1), ("Praha".into(), Some("Muzeum".into()), None));
    assert_eq!(
        triple(2),
        ("Brno".into(), None, Some("Hlavní nádraží".into()))
    );
    // R038.
    assert_eq!(triple(3), ("Hevlín".into(), Some("škola".into()), None));
}

#[test]
fn jdf_r060_mhd_named_stops_are_renamed_with_their_town() {
    let evidence = evidence(vec![
        row("Praha,,Florenc", 50.09, 14.44, Some("AA"), Some("CZ")),
        row("Praha,,Muzeum", 50.08, 14.43, Some("AA"), Some("CZ")),
        row("Praha,,Anděl", 50.07, 14.40, Some("AA"), Some("CZ")),
    ]);
    let stops = vec![
        stop(1, "Florenc", None, None),
        stop(2, "Muzeum", None, None),
        stop(3, "Anděl", None, None),
    ];
    let (fixed, diagnostics) = fix(
        batch("b", stops, &[(1, vec![at(1, 0), at(2, 3), at(3, 6)])]),
        &evidence,
    );
    let fixed = fixed.unwrap();
    assert!(codes(&diagnostics).contains(&"jdf_mhd_naming"));
    let first = stop_at(&fixed, 1);
    assert_eq!(
        (&*first.town, first.district.as_deref()),
        ("Praha", Some("Florenc"))
    );
    assert!(first.location.as_ref().unwrap().origin.is_precise());
}

#[test]
fn jdf_r006_r008_invalid_rows_are_errors_and_exact_duplicates_collapse() {
    let mut invalid = row("Invalid", 0.0, 181.0, Some("AA"), Some("CZ"));
    invalid.latitude = None;
    let (_, diagnostics) = evidence_with(
        vec![
            (
                "a.csv",
                vec![
                    row("Alpha", 50.1, 14.1, Some("AA"), Some("CZ")),
                    row("Duplicate", 50.0, 14.0, Some("AB"), Some("CZ")),
                ],
            ),
            (
                "nested/b.csv",
                vec![
                    row("Duplicate", 50.0, 14.0, Some("AB"), Some("CZ")),
                    invalid,
                ],
            ),
        ],
        Vec::new(),
        Vec::new(),
    );
    let all = codes(&diagnostics);
    assert!(all.contains(&"jdf_catalogue_row_invalid"));
    assert!(all.contains(&"jdf_catalogue_duplicates"));
    let rows = diagnostics
        .iter()
        .find(|d| d.code == "jdf_catalogue_rows")
        .unwrap();
    assert!(rows.message.starts_with("2 "), "{}", rows.message);
}

#[test]
fn jdf_r073_r078_stops_without_geography_or_position_are_reported() {
    let evidence = evidence(Vec::new());
    let stops = vec![
        stop(1, "Nowhere", Some("one"), None),
        stop(2, "Nowhere", Some("two"), None),
        located(stop(3, "Unused", Some("stop"), None), None, Some("CZ")),
    ];
    let (_, diagnostics) = fix(
        batch("b", stops, &[(1, vec![at(1, 0), at(2, 5)])]),
        &evidence,
    );
    let all: BTreeSet<&str> = codes(&diagnostics).into_iter().collect();
    assert!(all.contains("jdf_stop_without_country"));
    assert!(all.contains("jdf_stop_without_okres"));
    assert!(all.contains("jdf_location_missing"));
}
