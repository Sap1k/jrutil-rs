//! The international filter on merged versions (R279, R280) and the merge's rule ledger and
//! policy.

mod support;

use std::collections::BTreeSet;

use jiff::civil::date;
use jr_formats::jdf::{Km, RouteType};
use jr_jdf::fixed::FixedBatch;
use jr_jdf::international::{Reason, RouteDecision};
use jr_jdf::merge::{self, Registry, apply_international};
use jr_model::Diagnostics;
use support::merge::*;

fn cross_border(name: &str, licence: &str, foreign_km: u64) -> FixedBatch {
    let mut foreign = stop("Grenzort", None, None);
    foreign.okres = None;
    foreign.country = Some("D".into());
    let mut batch = batch(name)
        .stop(stop(&format!("Domov {name}"), None, None))
        .stop(foreign)
        .route(licence, date(2025, 12, 14), None, false)
        .route_type(RouteType::International)
        .trip(licence, 1, &[])
        .build();
    batch.batch.trip_stops[1].km = Some(Km {
        units: foreign_km,
        scale: 0,
    });
    batch
}

#[test]
fn r279_r280_rejected_versions_go_with_their_stops_and_kept_ones_are_marked() {
    let (mut merged, mut diagnostics) = merge_in(
        2026,
        vec![
            cross_border("a", "100001", 30),
            cross_border("b", "100002", 500),
        ],
        &Registry::empty(),
    );
    assert_eq!(merged.stops.len(), 3);
    let records = apply_international(&mut merged, &jr_jdf::Policy::builtin(), &mut diagnostics);
    assert_eq!(records.len(), 2);
    assert!(
        matches!(&records[0].decision, RouteDecision::KeepTrips(c) if c.reason == Reason::RegionalAdjacent)
    );
    assert!(
        matches!(&records[1].decision, RouteDecision::Drop(c) if c.reason == Reason::TripSpanExceedsLimit)
    );
    assert_eq!(merged.versions.len(), 1);
    assert!(merged.versions[0].regional_cross_border);
    // Domov b went with its version; Grenzort is still used by the kept one.
    assert_eq!(display_names(&merged), ["Domov a", "Grenzort"]);
    assert_eq!(count(&diagnostics, "jdf_international_route_dropped"), 1);
}

#[test]
fn the_builtin_merge_policy_loads() {
    let policy = merge::Policy::builtin();
    assert_eq!(policy.stops.match_distance_m, 75.0);
    assert_eq!(policy.stops.provisional_base, 1_000_000_000);
    assert_eq!(&*policy.candidates.reason_alias, "alias");
    // Registry::new reads the base from it.
    let mut diagnostics = Diagnostics::new();
    Registry::new(&[], &policy, &mut diagnostics);
    assert!(diagnostics.is_empty());
}

#[test]
fn every_merge_rule_has_a_heading_in_the_ledger() {
    const MERGE: &str = include_str!("../../../docs/rules/jdf-merge.md");
    let headings: BTreeSet<&str> = MERGE
        .lines()
        .filter_map(|line| line.strip_prefix("#### "))
        .filter_map(|heading| heading.split_whitespace().next())
        .collect();
    let mut seen = BTreeSet::new();
    for rule in merge::RULES {
        assert!(headings.contains(rule.id), "{} is undocumented", rule.id);
        assert!(seen.insert(rule.id), "{} is declared twice", rule.id);
    }
}
