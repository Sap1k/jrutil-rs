//! OSM nodes as matcher entries (JDF-R009–R013).

use jr_formats::osm::Node;
use jr_model::{Diagnostic, Diagnostics, Severity};

use super::evidence::{CountryLookup, Entry, MunicipalityLookup, SourceClass};
use super::policy::{Policy, TagValues};
use super::{R009, R011};

fn tag<'a>(node: &'a Node, key: &str) -> Option<&'a str> {
    node.tags
        .iter()
        .find(|(k, _)| &**k == key)
        .map(|(_, value)| &**value)
}

fn has_key(node: &Node, key: &str) -> bool {
    node.tags.iter().any(|(k, _)| &**k == key)
}

fn has_tag(node: &Node, tags: &[TagValues]) -> bool {
    node.tags.iter().any(|(key, value)| {
        tags.iter()
            .any(|t| t.key == *key && t.values.iter().any(|v| v == value))
    })
}

/// R009: a node is a road or tram stop.
fn eligible(node: &Node, policy: &Policy) -> bool {
    let osm = &policy.file.osm;
    !osm.excluded_keys.iter().any(|key| has_key(node, key))
        && osm.requires.iter().all(|(key, required)| {
            !has_key(node, key)
                || has_key(node, required)
                || osm
                    .eligible
                    .iter()
                    .any(|t| t.key == *key && has_tag(node, std::slice::from_ref(t)))
        })
        && has_tag(node, &osm.eligible)
}

/// R009–R013, every eligible node in file order. A node without any name is not an entry
/// (it can never be retrieved).
pub(super) fn entries(
    nodes: &[Node],
    towns: &MunicipalityLookup<'_>,
    countries: &CountryLookup,
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> Vec<Entry> {
    let osm = &policy.file.osm;
    let mut entries = Vec::new();
    let mut eligible_count = 0usize;
    let mut completed = 0usize;
    for node in nodes {
        if !eligible(node, policy) {
            continue;
        }
        eligible_count += 1;
        // R010.
        let name = osm.name_tags.iter().find_map(|key| tag(node, key));
        let full_name = tag(node, &osm.full_name_tag);
        // R011: the one municipality containing the node.
        let containing = towns.areas.containing(node.coord);
        let town = match containing.as_slice() {
            [one] => Some(&towns.municipalities[*one]),
            [] => None,
            several => {
                diagnostics.push(
                    Diagnostic::new(
                        "jdf_osm_municipality_ambiguous",
                        Severity::Warning,
                        format!("{}{}", osm.source_prefix, node.id),
                        format!(
                            "in {} municipalities ({}): no okres",
                            several.len(),
                            several
                                .iter()
                                .map(|&m| &*towns.municipalities[m].name)
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    )
                    .rule(R011),
                );
                None
            }
        };
        // R012.
        let (name, okres): (Box<str>, Option<Box<str>>) = match (full_name, name, town) {
            (None, None, _) => continue,
            (Some(full), _, None) => (full.into(), None),
            (Some(full), _, Some(town)) => (full.into(), town.okres.clone()),
            (None, Some(name), None) => (name.into(), None),
            (None, Some(name), Some(town)) => {
                let analyzer = towns.analyzer;
                let covered = analyzer
                    .name_match(&analyzer.tokens(name), &analyzer.tokens(&town.name))
                    .is_exact();
                if covered {
                    (name.into(), town.okres.clone())
                } else {
                    completed += 1;
                    (
                        format!("{}{}{name}", town.name, osm.name_joiner).into(),
                        town.okres.clone(),
                    )
                }
            }
        };
        // R011: the home country with an okres; elsewhere the country containing the node, in
        // the feed's codes (B8 fixed). A home node outside one municipality keeps neither.
        let country = match &okres {
            Some(_) => Some(policy.file.home_country.clone()),
            None => countries
                .country_of(node.coord)
                .filter(|&iso| !policy.file.is_home(iso))
                .map(|iso| policy.file.jdf_country(iso)),
        };
        let id: Box<str> = format!("{}{}", osm.source_prefix, node.id).into();
        entries.push(Entry {
            name,
            okres,
            country,
            coord: node.coord,
            class: SourceClass::Osm,
            source: id.clone(),
            observation: id,
            facts: facts(node, policy),
        });
    }
    diagnostics.push(
        Diagnostic::new(
            "jdf_osm_stops",
            Severity::Info,
            "osm",
            format!(
                "{eligible_count} of {} nodes are stops; {} named entries, {completed} names \
                 completed with their municipality",
                nodes.len(),
                entries.len()
            ),
        )
        .rule(R009),
    );
    entries
}

/// R013: modes, lifecycle and audit tags, in a fixed order.
fn facts(node: &Node, policy: &Policy) -> Vec<(Box<str>, Box<str>)> {
    let evidence = &policy.file.osm.post_evidence;
    let value_is = |key: &str, values: &[Box<str>]| {
        tag(node, key).is_some_and(|value| values.iter().any(|v| v.eq_ignore_ascii_case(value)))
    };
    let affirmative = |key: &str| {
        tag(node, key).is_some_and(|value| {
            let value = value.trim().to_lowercase();
            evidence.affirmative.iter().any(|a| **a == value)
        })
    };
    let negative = |key: &str| {
        tag(node, key).is_some_and(|value| {
            evidence
                .negative
                .iter()
                .any(|n| n.eq_ignore_ascii_case(value.trim()))
        })
    };
    let explicit: Vec<&str> = evidence
        .modes
        .iter()
        .filter(|mode| {
            mode.tags.iter().any(|t| value_is(&t.key, &t.values))
                || mode.access_keys.iter().any(|key| affirmative(key))
        })
        .map(|mode| &*mode.mode)
        .collect();
    let denied: Vec<&str> = evidence
        .modes
        .iter()
        .filter(|mode| mode.access_keys.iter().any(|key| negative(key)))
        .map(|mode| &*mode.mode)
        .collect();
    let lifecycle = evidence
        .lifecycle_keys
        .iter()
        .find(|key| {
            has_key(node, key)
                || tag(node, &evidence.lifecycle_tag).is_some_and(|v| v.eq_ignore_ascii_case(key))
        })
        .unwrap_or(&evidence.active);
    let mut facts: Vec<(Box<str>, Box<str>)> = vec![
        ("source_kind".into(), "osm".into()),
        ("explicit_modes".into(), explicit.join(";").into()),
        ("denied_modes".into(), denied.join(";").into()),
        ("lifecycle".into(), lifecycle.clone()),
    ];
    for key in &evidence.audit_keys {
        if let Some(value) = tag(node, key) {
            facts.push((format!("tag:{key}").into(), value.into()));
        }
    }
    facts
}
