//! The SR70 index and the OSM candidates (CZPTT-R018–R025).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use jr_formats::{osm, sr70};
use jr_geo::names::Prepared;
use jr_model::geo::distance_m;
use jr_model::{Coord, Diagnostic, Diagnostics, Severity};

use super::{CZPTT_R019, CZPTT_R020, CZPTT_R021, OsmPolicy, Policy};

/// What the pass places points by: the SR70 catalogue and the OSM railway candidates.
#[derive(Debug)]
pub struct Evidence {
    /// `(country, code)`: the SR70 country only (CZPTT-R021, R097).
    pub(super) sr70: BTreeMap<(Box<str>, Box<str>), Sr70Point>,
    /// By node ID (CZPTT-R025).
    pub(super) candidates: Vec<Candidate>,
    /// Candidate indices, ascending, by PLC, normalised name and railway core.
    pub(super) by_plc: HashMap<Box<str>, Vec<usize>>,
    pub(super) by_name: HashMap<Box<str>, Vec<usize>>,
    pub(super) by_core: HashMap<Box<str>, Vec<usize>>,
}

#[derive(Debug)]
pub(super) struct Sr70Point {
    pub coord: Option<Coord>,
    pub name: Option<Box<str>>,
}

#[derive(Debug)]
pub(super) struct Candidate {
    pub id: i64,
    pub coord: Coord,
    pub country: Option<Box<str>>,
    pub plc: Option<Box<str>>,
    pub names: Vec<Prepared>,
}

/// What the rows of one SR70 code say.
#[derive(Default)]
struct Sr70Code {
    /// Distinct coordinates as `(latitude, longitude)` bit patterns.
    coords: BTreeSet<(u64, u64)>,
    invalid: bool,
    names: BTreeSet<Box<str>>,
}

impl Evidence {
    /// CZPTT-R018–R025. Reports conflicting and invalid SR70 codes, ambiguous SR70 names and
    /// nothing else; which tags make a node a candidate is policy.
    pub fn build(
        sr70: &[sr70::Row],
        nodes: impl IntoIterator<Item = osm::Node>,
        policy: &Policy,
        diagnostics: &mut Diagnostics,
    ) -> Evidence {
        let mut evidence = Evidence {
            sr70: sr70_points(sr70, policy, diagnostics),
            candidates: Vec::new(),
            by_plc: HashMap::new(),
            by_name: HashMap::new(),
            by_core: HashMap::new(),
        };
        let mut candidates: Vec<Candidate> = nodes
            .into_iter()
            .filter_map(|node| candidate(&node, policy))
            .collect();
        // Stable: of two nodes with one ID (never in a real extract), the first is kept.
        candidates.sort_by_key(|candidate| candidate.id);
        candidates.dedup_by_key(|candidate| candidate.id);
        for (index, candidate) in candidates.iter().enumerate() {
            if let Some(plc) = &candidate.plc {
                push_unique(&mut evidence.by_plc, plc, index);
            }
            for name in &candidate.names {
                if !name.full().is_empty() {
                    push_unique(&mut evidence.by_name, name.full(), index);
                }
                if !name.core().is_empty() {
                    push_unique(&mut evidence.by_core, name.core(), index);
                }
            }
        }
        evidence.candidates = candidates;
        evidence
    }

    /// The SR70 name of a Czech point (CZPTT-R021), for the compiler, which names places with
    /// it.
    pub fn point_names(&self) -> BTreeMap<(Box<str>, Box<str>), Box<str>> {
        self.sr70
            .iter()
            .filter_map(|(key, point)| point.name.clone().map(|name| (key.clone(), name)))
            .collect()
    }

    pub(super) fn sr70_coord(&self, country: &str, code: &str) -> Option<Coord> {
        self.sr70
            .get(&(Box::from(country), Box::from(code)))
            .and_then(|point| point.coord)
    }

    /// Candidates whose PLC is the point's country and code (CZPTT-R094).
    pub(super) fn plc_candidates(&self, country: &str, code: &str) -> &[usize] {
        let plc = format!("{country}{code}").to_uppercase();
        self.by_plc.get(plc.as_str()).map_or(&[], Vec::as_slice)
    }

    /// Candidates with the point's PLC farther from its SR70 coordinate than the policy allows
    /// (CZPTT-R097), with their distance.
    pub(super) fn plc_disagreements(
        &self,
        country: &str,
        code: &str,
        sr70: Coord,
        policy: &Policy,
    ) -> Vec<(usize, f64)> {
        self.plc_candidates(country, code)
            .iter()
            .map(|&candidate| {
                (
                    candidate,
                    distance_m(sr70, self.candidates[candidate].coord),
                )
            })
            .filter(|&(_, distance)| distance > policy.authority.sr70_osm_disagreement_m)
            .collect()
    }

    /// The candidate's OSM object, for diagnostics.
    pub(super) fn object_id(&self, candidate: usize) -> String {
        format!("osm:node:{}", self.candidates[candidate].id)
    }
}

fn push_unique(index: &mut HashMap<Box<str>, Vec<usize>>, key: &str, candidate: usize) {
    let entries = index.entry(Box::from(key)).or_default();
    if entries.last() != Some(&candidate) {
        entries.push(candidate);
    }
}

/// CZPTT-R018–R021.
fn sr70_points(
    rows: &[sr70::Row],
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> BTreeMap<(Box<str>, Box<str>), Sr70Point> {
    let rules = &policy.sr70;
    let mut codes: BTreeMap<Box<str>, Sr70Code> = BTreeMap::new();
    for row in rows {
        if row.code.chars().count() < rules.code_length {
            continue;
        }
        let code: String = row.code.chars().take(rules.code_length).collect();
        let entry = codes.entry(code.into_boxed_str()).or_default();
        if let Some(name) = row
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty() && *name != &*rules.name_placeholder)
        {
            entry.names.insert(name.into());
        }
        match (row.latitude, row.longitude) {
            (Some(latitude), Some(longitude))
                if (-90.0..=90.0).contains(&latitude) && (-180.0..=180.0).contains(&longitude) =>
            {
                // `+ 0.0` folds -0.0 into 0.0: equal coordinates are one coordinate.
                entry
                    .coords
                    .insert(((latitude + 0.0).to_bits(), (longitude + 0.0).to_bits()));
            }
            _ => entry.invalid = true,
        }
    }

    let mut points = BTreeMap::new();
    for (code, facts) in codes {
        let subject = format!("{}:{code}", rules.country);
        if facts.invalid {
            diagnostics.push(
                Diagnostic::new(
                    "czptt_sr70_invalid_code",
                    Severity::Warning,
                    &subject,
                    "a row of this code has no usable coordinate; SR70 does not place it",
                )
                .rule(CZPTT_R019),
            );
        }
        if facts.coords.len() > 1 {
            diagnostics.push(
                Diagnostic::new(
                    "czptt_sr70_conflicting_code",
                    Severity::Warning,
                    &subject,
                    format!(
                        "{} different coordinates; SR70 does not place it",
                        facts.coords.len()
                    ),
                )
                .rule(CZPTT_R020),
            );
        }
        if facts.names.len() > 1 {
            let names: Vec<&str> = facts.names.iter().map(|name| &**name).collect();
            diagnostics.push(
                Diagnostic::new(
                    "czptt_sr70_ambiguous_name",
                    Severity::Warning,
                    &subject,
                    format!(
                        "names disagree ({}); SR70 does not name it",
                        names.join(" | ")
                    ),
                )
                .rule(CZPTT_R021),
            );
        }
        let coord = match (facts.invalid, facts.coords.len()) {
            (false, 1) => facts
                .coords
                .first()
                .map(|&(lat, lon)| Coord::new(f64::from_bits(lon), f64::from_bits(lat))),
            _ => None,
        };
        let name = if facts.names.len() == 1 {
            facts.names.into_iter().next()
        } else {
            None
        };
        points.insert((rules.country.clone(), code), Sr70Point { coord, name });
    }
    points
}

/// The value of a tag, when present and not blank.
fn tag<'n>(node: &'n osm::Node, key: &str) -> Option<&'n str> {
    node.tags
        .iter()
        .find(|(name, _)| &**name == key)
        .map(|(_, value)| &**value)
        .filter(|value| !value.trim().is_empty())
}

/// CZPTT-R022–R025: the node as a candidate, if it is one.
fn candidate(node: &osm::Node, policy: &Policy) -> Option<Candidate> {
    let rules = &policy.osm;
    let eligible = rules.eligible.iter().any(|rule| {
        tag(node, &rule.key).is_some_and(|value| rule.values.iter().any(|v| &**v == value))
    });
    if !eligible {
        return None;
    }
    let plc = tag(node, &rules.plc_tag).and_then(|value| plc(value, rules));
    let country = match &plc {
        Some(plc) => Some(
            plc.chars()
                .take(rules.plc_country_letters)
                .collect::<String>()
                .into(),
        ),
        None => rules
            .country_tags
            .iter()
            .find_map(|key| tag(node, key))
            .map(|value| {
                let value = value.trim().to_uppercase();
                rules
                    .country_alpha3
                    .get(value.as_str())
                    .cloned()
                    .unwrap_or_else(|| value.into())
            }),
    };
    let mut raw_names: Vec<&str> = Vec::new();
    for key in &rules.name_tags {
        if let Some(name) = tag(node, key).map(str::trim)
            && !raw_names.contains(&name)
        {
            raw_names.push(name);
        }
    }
    Some(Candidate {
        id: node.id,
        coord: node.coord,
        country,
        plc,
        names: raw_names
            .into_iter()
            .map(|name| Prepared::new(name, &policy.names))
            .collect(),
    })
}

/// CZPTT-R023.
fn plc(value: &str, rules: &OsmPolicy) -> Option<Box<str>> {
    let compact: String = value
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>()
        .to_uppercase();
    let letters = rules.plc_country_letters;
    let well_formed = compact.len() == letters + rules.plc_code_digits
        && compact.bytes().enumerate().all(|(at, byte)| {
            if at < letters {
                byte.is_ascii_uppercase()
            } else {
                byte.is_ascii_digit()
            }
        });
    well_formed.then(|| compact.into())
}
