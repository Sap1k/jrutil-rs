//! Stop reconciliation across batches (R213, R216–R227, R231, R235–R237, R284), ported from the
//! F# `JdfMergerTests` (named in each test).

mod support;

use jr_jdf::merge::ProvisionalReason;
use support::merge::*;

#[test]
fn r236_post_observations_follow_their_stop() {
    // "Post observations remap to canonical stops deterministically".
    let first = batch("a")
        .stop(at(stop("Candidate stop", None, None), 50.0, 14.0))
        .observation(0, "catalogue:a", 50.0)
        .route("a", jiff::civil::date(2025, 12, 14), None, false)
        .build();
    let second = batch("b")
        .stop(at(stop("Candidate stop", None, None), 50.0, 14.0))
        .observation(0, "osm:b", 50.0)
        .observation(0, "osm:b", 50.1)
        .route("b", jiff::civil::date(2025, 12, 14), None, false)
        .build();
    let (merged, diagnostics) = merge_stops(vec![second, first]);
    assert_eq!(merged.stops.len(), 1);
    let observations: Vec<&str> = merged.stops[0]
        .post_candidates
        .iter()
        .map(|c| &*c.observation)
        .collect();
    assert_eq!(observations, ["catalogue:a", "osm:b"]);
    assert_eq!(count(&diagnostics, "jdf_post_observation_conflict"), 1);
}

#[test]
fn r213_r218_r226_r227_r237_an_expanded_cis_name_replaces_the_abbreviated_alias() {
    // "Expanded CIS stop name replaces abbreviated alias".
    let abbreviated = batch("a")
        .stop_with(
            at(
                stop("Ústí n.L.", Some("hl.nádr."), None),
                50.6595592,
                14.0436811,
            ),
            &["@"],
        )
        .post(1, 1, "1")
        .route("a", jiff::civil::date(2025, 12, 14), None, false)
        .build();
    let expanded = batch("b")
        .stop_with(
            at(
                stop("Ústí nad Labem", Some("Hlavní nádraží"), None),
                50.6594901,
                14.0445786,
            ),
            &["W"],
        )
        .post(1, 2, "2")
        .post(1, 1, "other")
        .route("b", jiff::civil::date(2025, 12, 14), None, false)
        .build();
    let (merged, diagnostics) = merge_stops(vec![abbreviated, expanded]);
    assert_eq!(display_names(&merged), ["Ústí nad Labem,Hlavní nádraží"]);
    assert_eq!(
        merged.code_sets.get(merged.stops[0].codes),
        [Box::from("@"), Box::from("W")]
    );
    let posts: Vec<u64> = merged.stops[0].posts.iter().map(|p| p.post_id).collect();
    assert_eq!(posts, [1, 2]);
    assert_eq!(count(&diagnostics, "jdf_post_attribute_conflict"), 1);
    assert_eq!(merged.statistics.exact, 1);
}

#[test]
fn r226_a_clean_alias_wins_over_a_redundant_three_component_one() {
    // "Clean alias wins over a redundant three-component alias".
    let (merged, _) = merge_stops(vec![
        stops_batch(
            "a",
            vec![at(
                stop("Chomutov", Some("Chomutov"), Some("žel.st.")),
                50.4562,
                13.3993,
            )],
        ),
        stops_batch(
            "b",
            vec![at(
                stop("Chomutov", Some("žel.st."), None),
                50.4562,
                13.3993,
            )],
        ),
    ]);
    assert_eq!(display_names(&merged), ["Chomutov,žel.st."]);
}

#[test]
fn r219_r226_a_hierarchical_cis_name_replaces_the_shortened_mhd_form() {
    // "Hierarchical CIS name replaces shortened MHD form".
    let (merged, _) = merge_stops(vec![
        stops_batch(
            "a",
            vec![at(
                stop("Dolní Jiřetín", Some("rozc."), None),
                50.5550505,
                13.5900450,
            )],
        ),
        stops_batch(
            "b",
            vec![at(
                stop("Horní Jiřetín", Some("Dolní Jiřetín"), Some("rozcestí")),
                50.5550388,
                13.5899196,
            )],
        ),
    ]);
    assert_eq!(
        display_names(&merged),
        ["Horní Jiřetín,Dolní Jiřetín,rozcestí"]
    );
    assert_eq!(merged.statistics.suffix, 1);
}

#[test]
fn r220_compatible_names_beyond_the_match_distance_stay_separate() {
    // "Compatible names beyond 75 metres remain separate".
    let (merged, _) = merge_stops(vec![
        stops_batch(
            "a",
            vec![at(
                stop("Ústí n.L.", Some("hl.nádr."), None),
                50.6595,
                14.0436,
            )],
        ),
        stops_batch(
            "b",
            vec![at(
                stop("Ústí nad Labem", Some("Hlavní nádraží"), None),
                50.6595,
                14.0460,
            )],
        ),
    ]);
    assert_eq!(merged.stops.len(), 2);
}

#[test]
fn r225_a_later_precise_position_updates_an_existing_alias() {
    // "A later precise location updates an existing alias".
    let name = || stop("Same name", Some("station"), None);
    let (merged, _) = merge_stops(vec![
        stops_batch("a", vec![name()]),
        stops_batch("b", vec![at(name(), 50.0, 14.0)]),
        stops_batch("c", vec![at(name(), 50.0, 14.002)]),
    ]);
    assert_eq!(merged.stops.len(), 2);
}

#[test]
fn r221_a_minor_spelling_difference_nearby_is_a_fuzzy_match() {
    // "Balanced fuzzy match accepts a minor spelling difference nearby".
    let (merged, diagnostics) = merge_stops(vec![
        stops_batch(
            "a",
            vec![at(
                stop("Testov", Some("stanice u stare nemocnice"), None),
                50.0,
                14.0,
            )],
        ),
        stops_batch(
            "b",
            vec![at(
                stop("Testov", Some("stanice u staré nemocnice"), None),
                50.0001,
                14.0001,
            )],
        ),
    ]);
    assert_eq!(merged.stops.len(), 1);
    assert_eq!(merged.statistics.fuzzy, 1);
    assert_eq!(count(&diagnostics, "jdf_stop_merged"), 1);
}

#[test]
fn r216_r221_nearby_unrelated_names_and_other_okresy_stay_separate() {
    // "Nearby unrelated names and conflicting regions remain separate".
    let mut other_okres = at(stop("Testov", Some("škola"), None), 50.0001, 14.0001);
    other_okres.okres = Some("MO".into());
    let (merged, _) = merge_stops(vec![
        stops_batch(
            "a",
            vec![at(stop("Testov", Some("škola"), None), 50.0, 14.0)],
        ),
        stops_batch(
            "b",
            vec![at(
                stop("Testov", Some("nemocnice"), None),
                50.0001,
                14.0001,
            )],
        ),
        stops_batch("c", vec![other_okres]),
    ]);
    assert_eq!(merged.stops.len(), 3);
}

#[test]
fn r219_without_positions_a_unique_suffix_locality_merges() {
    // "Missing coordinates merge only a unique suffix locality".
    let (merged, _) = merge_stops(vec![
        stops_batch(
            "a",
            vec![stop(
                "Horní Jiřetín",
                Some("Dolní Jiřetín"),
                Some("rozcestí"),
            )],
        ),
        stops_batch("b", vec![stop("Dolní Jiřetín", Some("rozc."), None)]),
    ]);
    assert_eq!(merged.stops.len(), 1);
}

#[test]
fn r222_r224_ambiguous_suffix_candidates_stay_separate() {
    // "Ambiguous suffix candidates remain separate".
    let (merged, diagnostics) = merge_stops(vec![
        stops_batch(
            "a",
            vec![
                stop("Horní Jiřetín", Some("Dolní Jiřetín"), Some("rozcestí")),
                stop("Most", Some("Dolní Jiřetín"), Some("rozcestí")),
            ],
        ),
        stops_batch("b", vec![stop("Dolní Jiřetín", Some("rozc."), None)]),
    ]);
    assert_eq!(merged.stops.len(), 3);
    assert_eq!(merged.statistics.ambiguous, 1);
    assert_eq!(count(&diagnostics, "jdf_stop_ambiguous"), 1);
    assert_eq!(count(&diagnostics, "jdf_stop_quarantined"), 1);
    let quarantined: Vec<_> = merged
        .stops
        .iter()
        .filter(|s| s.provisional == Some(ProvisionalReason::Ambiguous))
        .collect();
    assert_eq!(quarantined.len(), 1);
}

#[test]
fn r224_each_ambiguous_identity_is_one_quarantined_stop() {
    // Verdict Q2 (fixes B1): the repeat joins the quarantined stop instead of making another.
    let (merged, diagnostics) = merge_stops(vec![
        stops_batch(
            "a",
            vec![
                stop("Horní Jiřetín", Some("Dolní Jiřetín"), Some("rozcestí")),
                stop("Most", Some("Dolní Jiřetín"), Some("rozcestí")),
            ],
        ),
        stops_batch("b", vec![stop("Dolní Jiřetín", Some("rozc."), None)]),
        stops_batch("c", vec![stop("Dolní Jiřetín", Some("rozcestí"), None)]),
    ]);
    assert_eq!(merged.stops.len(), 3);
    assert_eq!(count(&diagnostics, "jdf_stop_quarantined"), 1);
}

#[test]
fn r218_r222_an_exact_candidate_wins_over_an_older_suffix_candidate() {
    // "Exact candidate wins over an older suffix candidate".
    let (merged, _) = merge_stops(vec![
        stops_batch("a", vec![at(stop("Dětkovice", None, None), 49.70, 16.95)]),
        stops_batch(
            "b",
            vec![at(stop("Ludmírov", Some("Dětkovice"), None), 49.71, 16.95)],
        ),
        stops_batch("c", vec![stop("Ludmírov", Some("Dětkovice"), None)]),
    ]);
    assert_eq!(merged.stops.len(), 2);
    assert_eq!(merged.statistics.ambiguous, 0);
    assert_eq!(merged.statistics.exact, 1);
}

#[test]
fn r226_the_published_spelling_does_not_depend_on_batch_order() {
    // "Canonical name is independent of input order": the content of batches a and b swapped.
    let abbreviated = || {
        at(
            stop("Ústí n.L.", Some("hl.nádr."), None),
            50.6595592,
            14.0436811,
        )
    };
    let expanded = || {
        at(
            stop("Ústí nad Labem", Some("Hlavní nádraží"), None),
            50.6594901,
            14.0445786,
        )
    };
    let (forward, _) = merge_stops(vec![
        stops_batch("a", vec![abbreviated()]),
        stops_batch("b", vec![expanded()]),
    ]);
    let (reverse, _) = merge_stops(vec![
        stops_batch("a", vec![expanded()]),
        stops_batch("b", vec![abbreviated()]),
    ]);
    assert_eq!(display_names(&forward), display_names(&reverse));
}

#[test]
fn r231_a_precise_position_replaces_an_estimate_and_far_positions_are_diagnosed() {
    let name = || stop("Testov", Some("náves"), None);
    let (merged, diagnostics) = merge_stops(vec![
        stops_batch("a", vec![estimated(name(), 50.0, 14.0)]),
        stops_batch(
            "b",
            vec![at(stop("Testov", Some("náves."), None), 50.0002, 14.0)],
        ),
        // Matches a's alias, which has no precise position, at any distance (R220).
        stops_batch("d", vec![at(name(), 50.5, 14.0)]),
    ]);
    assert_eq!(merged.stops.len(), 1);
    let location = merged.stops[0].location.as_ref().expect("located");
    assert_eq!(location.coord.lat, 50.0002);
    assert_eq!(count(&diagnostics, "jdf_stop_location_conflict"), 1);
}

#[test]
fn r284_a_stop_no_version_uses_is_not_published() {
    let unused = batch("a")
        .stop(stop("Used", None, None))
        .route("a", jiff::civil::date(2025, 12, 14), None, false)
        .stop(stop("Unused", None, None))
        .build();
    let (merged, diagnostics) = merge_stops(vec![unused]);
    assert_eq!(display_names(&merged), ["Used"]);
    assert_eq!(count(&diagnostics, "jdf_unused_dropped"), 1);
}

#[test]
fn r235_merges_are_counted_and_new_stops_diagnosed() {
    let (merged, diagnostics) = merge_stops(vec![
        stops_batch("a", vec![stop("Alfa", None, None)]),
        stops_batch("b", vec![stop("Alfa", None, None)]),
    ]);
    assert_eq!(merged.statistics.exact, 1);
    assert_eq!(count(&diagnostics, "jdf_stop_provisional"), 1);
}

#[test]
fn r217_a_one_component_spelling_finds_a_longer_one_only_by_position() {
    // The index asymmetry of R219 is ported (verdict Q5).
    let long = || stop("Ludmírov", Some("Dětkovice"), None);
    let short = || stop("Dětkovice", None, None);
    let (unlocated, _) = merge_stops(vec![
        stops_batch("a", vec![long()]),
        stops_batch("b", vec![short()]),
    ]);
    assert_eq!(unlocated.stops.len(), 2);
    let (located, _) = merge_stops(vec![
        stops_batch("a", vec![at(long(), 49.7, 16.95)]),
        stops_batch("b", vec![at(short(), 49.7001, 16.95)]),
    ]);
    assert_eq!(located.stops.len(), 1);
}
