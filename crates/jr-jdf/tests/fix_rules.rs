//! Stop matching as a whole: its rule ledger, its policy, and independence of batch order and
//! thread count.

mod fix_support;

use std::collections::BTreeSet;

use fix_support::*;
use jr_model::Diagnostics;
use rayon::prelude::*;

const READ: &str = include_str!("../../../docs/rules/jdf-read.md");

#[test]
fn every_fix_rule_has_a_heading_in_the_ledger() {
    let headings: BTreeSet<&str> = READ
        .lines()
        .filter_map(|line| line.strip_prefix("#### "))
        .filter_map(|heading| heading.split_whitespace().next())
        .collect();
    let mut seen = BTreeSet::new();
    for rule in jr_jdf::fix::RULES {
        assert!(headings.contains(rule.id), "{} has no heading", rule.id);
        assert!(seen.insert(rule.id), "{} is listed twice", rule.id);
    }
}

#[test]
fn the_builtin_fix_policy_loads() {
    let policy = policy();
    assert_eq!(&*policy.file.home_country, "CZ");
    assert_eq!(
        policy.file.matching.okres_aliases.get("OL").map(|c| &**c),
        Some("OC")
    );
    assert_eq!(policy.file.batch.min_called_stops, 2);
}

fn batches() -> Vec<jr_jdf::batch::Batch> {
    let mhd = batch(
        "b-mhd",
        vec![
            stop(1, "Florenc", None, None),
            stop(2, "Muzeum", None, None),
            stop(3, "Anděl", None, None),
        ],
        &[
            (1, vec![at(1, 0), at(2, 3), at(3, 6)]),
            (2, vec![at(3, 0), at(2, 3), at(1, 6)]),
        ],
    );
    let line = batch(
        "b-line",
        vec![
            located(stop(1, "Start", Some("stop"), None), Some("AA"), Some("CZ")),
            located(
                stop(2, "Nová Ves", Some("u lesa"), None),
                Some("AA"),
                Some("CZ"),
            ),
            stop(3, "Nowhere", Some("at all"), None),
            stop(4, "Exact", Some("stop"), None),
        ],
        &[(
            1,
            vec![
                at_km(1, 0, 0.0),
                at_km(2, 3, 1.5),
                at_km(3, 6, 2.5),
                at_km(4, 9, 3.5),
            ],
        )],
    );
    let wrong = batch(
        "b-wrong",
        vec![
            stop(1, "Start", Some("stop"), None),
            stop(2, "Far", Some("away"), None),
        ],
        &[(1, vec![at(1, 0), at(2, 2)])],
    );
    let degenerate = batch(
        "b-empty",
        vec![stop(1, "A", Some("B"), None)],
        &[(1, vec![at(1, 0)])],
    );
    vec![mhd, line, wrong, degenerate]
}

fn evidence() -> jr_jdf::fix::Evidence {
    evidence_with(
        vec![(
            "manual.csv",
            vec![
                row("Praha,,Florenc", 50.09, 14.44, Some("AA"), Some("CZ")),
                row("Praha,,Muzeum", 50.08, 14.43, Some("AA"), Some("CZ")),
                row("Praha,,Anděl", 50.07, 14.40, Some("AA"), Some("CZ")),
                row("Start,stop", 50.15, 14.20, Some("AA"), Some("CZ")),
                row("Nová Ves,u lesa", 50.15, 14.22, Some("AA"), Some("CZ")),
                row("Nová Ves,u lesa", 50.25, 14.45, Some("AA"), Some("CZ")),
                row("Exact,stop", 50.15, 14.25, Some("AA"), Some("CZ")),
                row("Far,away", 48.0, 18.0, None, None),
            ],
        )],
        vec![bus_stop(9, "Exact,stop", 50.1501, 14.2501)],
        vec![foreign("Most", "be")],
    )
    .0
}

fn run(batches: Vec<jr_jdf::batch::Batch>, threads: usize) -> Vec<(String, String)> {
    let evidence = evidence();
    let policy = policy();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap();
    let mut results: Vec<(String, String)> = pool.install(|| {
        batches
            .into_par_iter()
            .map(|batch| {
                let name = batch.name.to_string();
                let mut diagnostics = Diagnostics::new();
                let fixed = jr_jdf::fix::fix(batch, &evidence, &policy, &mut diagnostics);
                (name, format!("{fixed:?}\n{diagnostics:?}"))
            })
            .collect()
    });
    results.sort();
    results
}

#[test]
fn fixing_does_not_depend_on_batch_order_or_thread_count() {
    let forward = run(batches(), 1);
    let mut reversed = batches();
    reversed.reverse();
    let parallel = run(reversed, 4);
    assert_eq!(forward, parallel);
}
