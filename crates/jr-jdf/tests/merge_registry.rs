//! Registry numbers, provisional numbers and the registry candidates (R207–R210, R223, R224,
//! R229, R230, R232–R234), ported from the F# `JdfMergerTests` registry cases.

mod support;

use jr_formats::geodata::registry::Status;
use jr_jdf::merge::{self, ProvisionalReason, Registry, provisional_number};
use jr_model::Diagnostics;
use support::merge::*;

fn numbers(merged: &jr_jdf::merge::Merged) -> Vec<(String, u64)> {
    let mut pairs: Vec<(String, u64)> = merged
        .stops
        .iter()
        .map(|s| (s.name.display_name(), s.number))
        .collect();
    pairs.sort();
    pairs
}

const BASE: u64 = 1_000_000_000;

#[test]
fn r210_r229_r230_registered_stops_keep_their_numbers_whatever_the_batch_order() {
    // "Registered stops keep their numbers whatever the batch order".
    let registry = registry(&[
        registry_stop(7, "Most", Some("nádraží"), None, None),
        registry_stop(3, "Litvínov", Some("Citadela"), None, None),
    ]);
    let first = || {
        stops_batch(
            "a",
            vec![
                stop("Litvínov", Some("Citadela"), None),
                stop("Žatec", None, Some("náměstí")),
            ],
        )
    };
    let second = || {
        stops_batch(
            "b",
            vec![
                stop("Most", Some("nádraží"), None),
                stop("Louny", None, Some("nádraží")),
            ],
        )
    };
    let third = || stops_batch("c", vec![stop("Most", Some("nádraží"), None)]);
    let (forward, _) = merge_in(2026, vec![first(), second(), third()], &registry);
    let (backward, _) = merge_in(2026, vec![third(), second(), first()], &registry);
    assert_eq!(numbers(&forward), numbers(&backward));
    let ids: std::collections::BTreeMap<String, u64> = numbers(&forward).into_iter().collect();
    assert_eq!(ids["Most,nádraží"], 7);
    assert_eq!(ids["Litvínov,Citadela"], 3);
    assert!(ids["Žatec,náměstí"] >= BASE);
    assert!(ids["Louny,nádraží"] >= BASE);
}

#[test]
fn r230_r232_unregistered_stops_are_offered_with_deterministic_provisional_numbers() {
    // "Unregistered stops are listed with deterministic provisional numbers".
    let registry = registry(&[registry_stop(1, "Most", Some("nádraží"), None, None)]);
    let batch = || {
        stops_batch(
            "a",
            vec![at(stop("Bílina", Some("lázně"), None), 50.5480, 13.7752)],
        )
    };
    let (merged, _) = merge_in(2026, vec![batch()], &registry);
    let (again, _) = merge_in(2026, vec![batch()], &registry);
    let provisional = merged.stops[0].number;
    assert_eq!(provisional, again.stops[0].number);
    assert_eq!(merged.registry_candidates.len(), 1);
    let row = &merged.registry_candidates[0];
    assert_eq!(row.provisional_number, Some(provisional));
    assert_eq!(row.spelling.display_name(), "Bílina,lázně");
    assert_eq!(
        row.location.map(|c| (c.lat, c.lon)),
        Some((50.548, 13.7752))
    );
    assert_eq!(&*row.reason, "new");
    assert!(row.suggestions.is_empty());
}

#[test]
fn r220_r223_r224_same_named_registered_stops_are_split_by_reference() {
    // "Same-named registered stops are split by reference coordinates".
    let registry = registry(&[
        registry_stop(
            1,
            "Babice",
            None,
            Some("hřbitov"),
            Some((49.12776, 17.474625)),
        ),
        registry_stop(
            2,
            "Babice",
            None,
            Some("hřbitov"),
            Some((49.14000, 17.474625)),
        ),
    ]);
    let near = |lat| {
        stops_batch(
            "a",
            vec![at(stop("Babice", None, Some("hřbitov")), lat, 17.4746)],
        )
    };
    assert_eq!(
        merge_in(2026, vec![near(49.1399)], &registry).0.stops[0].number,
        2
    );
    assert_eq!(
        merge_in(2026, vec![near(49.1278)], &registry).0.stops[0].number,
        1
    );
    let elsewhere = merge_in(2026, vec![near(49.2)], &registry).0;
    assert!(elsewhere.stops[0].number >= BASE);
    let unlocated = stops_batch("a", vec![stop("Babice", None, Some("hřbitov"))]);
    let (quarantined, _) = merge_in(2026, vec![unlocated], &registry);
    assert!(quarantined.stops[0].number >= BASE);
    assert_eq!(&*quarantined.registry_candidates[0].reason, "ambiguous");
}

#[test]
fn r224_repeated_ambiguous_spellings_share_one_quarantined_stop() {
    // B1 fixed: F# made a second provisional stop for the second batch.
    let registry = registry(&[
        registry_stop(1, "Babice", None, Some("hřbitov"), None),
        registry_stop(2, "Babice", None, Some("hřbitov"), None),
    ]);
    let unlocated = |name: &str| stops_batch(name, vec![stop("Babice", None, Some("hřbitov"))]);
    let (merged, diagnostics) = merge_in(2026, vec![unlocated("a"), unlocated("b")], &registry);
    assert_eq!(merged.stops.len(), 1);
    assert_eq!(
        merged.stops[0].provisional,
        Some(ProvisionalReason::Ambiguous)
    );
    assert_eq!(count(&diagnostics, "jdf_stop_ambiguous"), 2);
    assert_eq!(count(&diagnostics, "jdf_stop_quarantined"), 1);
    assert_eq!(merged.registry_candidates.len(), 1);
}

#[test]
fn r233_a_new_spelling_of_a_registered_stop_is_offered_as_an_alias() {
    // "A new spelling of a registered stop is offered as an alias".
    let registry = registry(&[registry_stop(
        4,
        "Horní Jiřetín",
        Some("Dolní Jiřetín"),
        Some("rozcestí"),
        None,
    )]);
    let (merged, _) = merge_in(
        2026,
        vec![stops_batch(
            "a",
            vec![stop("Dolní Jiřetín", Some("rozcestí"), None)],
        )],
        &registry,
    );
    assert_eq!(merged.stops[0].number, 4);
    assert_eq!(merged.registry_candidates.len(), 1);
    let row = &merged.registry_candidates[0];
    assert_eq!(
        (&*row.reason, row.suggestions.as_slice()),
        ("alias", &[4u64][..])
    );
}

#[test]
fn r208_merged_registry_numbers_resolve_to_the_surviving_number() {
    // "Merged registry numbers resolve to the surviving number".
    let mut duplicate = registry_stop(2, "Most", Some("hlavní nádraží"), None, None);
    duplicate.merged_into = Some(1);
    let registry = registry(&[
        registry_stop(1, "Most", Some("nádraží"), None, None),
        duplicate,
    ]);
    let (merged, _) = merge_in(
        2026,
        vec![stops_batch(
            "a",
            vec![stop("Most", Some("hlavní nádraží"), None)],
        )],
        &registry,
    );
    assert_eq!(merged.stops[0].number, 1);
    assert!(merged.registry_candidates.is_empty());
}

#[test]
fn r207_r208_invalid_registry_rows_are_dropped_with_a_diagnostic() {
    let mut chained = registry_stop(3, "C", None, None, None);
    chained.merged_into = Some(2);
    let mut merged_row = registry_stop(2, "B", None, None, None);
    merged_row.merged_into = Some(1);
    let mut dangling = registry_stop(4, "D", None, None, None);
    dangling.merged_into = Some(99);
    let rows = [
        registry_stop(1, "A", None, None, None),
        merged_row,
        chained,
        dangling,
        registry_stop(1_000_000_001, "E", None, None, None),
    ];
    let mut diagnostics = Diagnostics::new();
    Registry::new(&rows, &merge::Policy::builtin(), &mut diagnostics);
    assert_eq!(count(&diagnostics, "jdf_registry_row_invalid"), 3);
}

#[test]
fn r209_a_retired_row_still_matches() {
    let mut retired = registry_stop(5, "Most", Some("nádraží"), None, None);
    retired.status = Status::Retired;
    let (merged, _) = merge_in(
        2026,
        vec![stops_batch("a", vec![stop("Most", Some("nádraží"), None)])],
        &registry(&[retired]),
    );
    assert_eq!(merged.stops[0].number, 5);
}

#[test]
fn r230_provisional_numbers_probe_past_collisions() {
    // "Provisional numbers probe past collisions".
    let first = provisional_number("a", BASE, BASE, |_| false).expect("free");
    let second = provisional_number("a", BASE, BASE, |n| n == first).expect("free");
    assert!((BASE..2 * BASE).contains(&first));
    let expected = if first == 2 * BASE - 1 {
        BASE
    } else {
        first + 1
    };
    assert_eq!(second, expected);
}

#[test]
fn r234_provisional_spellings_suggest_similar_registered_stops_of_their_town() {
    let registry = registry(&[
        registry_stop(9, "Bílina", Some("lázne"), None, Some((49.0, 13.0))),
        registry_stop(3, "Bílina", Some("lazně"), None, Some((49.0, 13.0))),
        registry_stop(4, "Bílina", Some("náměstí"), None, None),
    ]);
    let (merged, _) = merge_in(
        2026,
        vec![stops_batch(
            "a",
            vec![at(stop("Bílina", Some("lázně"), None), 50.5, 13.7)],
        )],
        &registry,
    );
    let row = &merged.registry_candidates[0];
    assert_eq!(&*row.reason, "new");
    // Both are as similar; the smaller number first. Náměstí is not similar.
    assert_eq!(row.suggestions, [3, 9]);
}

#[test]
fn r207_r208_r209_a_registry_file_loads_with_merges_and_retired_rows() {
    // StopRegistryTests "Registry files load with aliases merges and posts" and "Invalid
    // registries are rejected" (the parts that are the compiler's: the id range and targets).
    let csv = "id,town,district,nearby_place,okres,country,lat,lon,status,note\n\
               1,Most,nádraží,,MO,CZ,50.5,13.6,active,\n\
               2,Most,\"hlavní nádraží, vlak\",,MO,CZ,,,retired,merged_into:1\n\
               1000000000,Most,,,MO,CZ,,,active,\n";
    let rows = jr_formats::geodata::registry::parse_stops(csv.as_bytes()).expect("parses");
    let mut diagnostics = Diagnostics::new();
    let registry = Registry::new(&rows, &merge::Policy::builtin(), &mut diagnostics);
    assert_eq!(count(&diagnostics, "jdf_registry_row_invalid"), 1);
    let mut spelling = stop("Most", Some("hlavní nádraží, vlak"), None);
    spelling.okres = Some("MO".into());
    let (merged, _) = merge_in(2026, vec![stops_batch("a", vec![spelling])], &registry);
    assert_eq!(merged.stops[0].number, 1);
}
