//! Every rule the engine declares is a rule of `docs/rules/overlay.md`.

#[test]
fn every_rule_is_a_heading_of_the_overlay_inventory() {
    let inventory = include_str!("../../../docs/rules/overlay.md");
    let mut seen = Vec::new();
    for rule in jr_overlay::rules() {
        assert!(
            inventory.contains(&format!("#### {} ", rule.id)),
            "{} has no heading in docs/rules/overlay.md",
            rule.id
        );
        assert!(!seen.contains(&rule.id), "{} declared twice", rule.id);
        seen.push(rule.id);
    }
    assert!(seen.len() > 30);
}
