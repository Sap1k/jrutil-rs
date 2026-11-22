//! The rule ledger: every rule the crate declares is documented in `docs/rules/czptt.md`.

use std::collections::BTreeSet;

const LEDGER: &str = include_str!("../../../docs/rules/czptt.md");

#[test]
fn every_rule_has_a_heading_in_the_ledger() {
    let headings: BTreeSet<&str> = LEDGER
        .lines()
        .filter_map(|line| line.strip_prefix("#### "))
        .filter_map(|heading| heading.split_whitespace().next())
        .collect();
    let missing: Vec<&str> = jr_czptt::RULES
        .iter()
        .map(|rule| rule.id)
        .filter(|id| !headings.contains(id))
        .collect();
    assert!(missing.is_empty(), "undocumented rules: {missing:?}");
}

#[test]
fn rule_ids_are_unique() {
    let mut seen = BTreeSet::new();
    for rule in jr_czptt::RULES {
        assert!(seen.insert(rule.id), "{} is declared twice", rule.id);
        assert!(!rule.summary.is_empty(), "{} has no summary", rule.id);
    }
}
