//! The rule ledger and the built-in policy.

use std::collections::BTreeSet;

use jr_jdf::Policy;
use jr_jdf::policy::{FilterPolicy, NoteKind};

const READ: &str = include_str!("../../../docs/rules/jdf-read.md");
const MERGE: &str = include_str!("../../../docs/rules/jdf-merge.md");
const CONVERT: &str = include_str!("../../../docs/rules/jdf-convert.md");

#[test]
fn every_rule_has_a_heading_in_the_ledger() {
    let headings: BTreeSet<&str> = READ
        .lines()
        .chain(MERGE.lines())
        .chain(CONVERT.lines())
        .filter_map(|line| line.strip_prefix("#### "))
        .filter_map(|heading| heading.split_whitespace().next())
        .collect();
    let missing: Vec<&str> = jr_jdf::rules()
        .map(|rule| rule.id)
        .filter(|id| !headings.contains(id))
        .collect();
    assert!(missing.is_empty(), "undocumented rules: {missing:?}");
}

#[test]
fn rule_ids_are_unique() {
    let mut seen = BTreeSet::new();
    for rule in jr_jdf::rules() {
        assert!(seen.insert(rule.id), "{} is declared twice", rule.id);
        assert!(!rule.summary.is_empty(), "{} has no summary", rule.id);
    }
}

#[test]
fn the_builtin_policy_loads() {
    let policy = Policy::builtin();
    assert_eq!(policy.international.policy, FilterPolicy::RegionalAdjacent);
    assert_eq!(policy.calendar.note_types.len(), 8);
    assert_eq!(
        policy.calendar.note_types.get("3"),
        Some(&NoteKind::RunsOnly)
    );
    for holiday in &policy.holidays.fixed {
        assert!(
            jiff::civil::Date::new(2024, holiday.month, holiday.day).is_ok(),
            "{holiday:?}"
        );
    }
}
