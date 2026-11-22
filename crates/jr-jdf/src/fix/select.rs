//! Exact selection (JDF-R043–R049): from a stop's exact candidates to one candidate group and
//! its position.

use std::sync::Arc;

use jr_geo::plane;
use jr_geo::stop_index::Candidate;
use jr_model::Coord;

use super::evidence::{Evidence, SourceClass};
use super::policy::Policy;

/// The okres and country a stop is matched with.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Locality<'a> {
    pub okres: Option<&'a str>,
    pub country: Option<&'a str>,
}

/// R043–R046 evaluations of one selection (R074 reports them per batch).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Counters {
    pub strict: u32,
    pub border: u32,
    pub country_rejects: u32,
    pub non_adjacent: u32,
    pub outside_border: u32,
}

impl Counters {
    pub(crate) fn add(&mut self, other: Counters) {
        self.strict += other.strict;
        self.border += other.border;
        self.country_rejects += other.country_rejects;
        self.non_adjacent += other.non_adjacent;
        self.outside_border += other.outside_border;
    }
}

/// A selected candidate group (R048) and what the stop takes from it (R049).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Match {
    /// The group's entries in corpus order.
    pub entries: Vec<u32>,
    /// The centroid of the group.
    pub coord: Coord,
    /// The first entry's (R049).
    pub first: u32,
    pub class: SourceClass,
    /// The distinct sources, sorted, joined by `+` (R049).
    pub source: Box<str>,
}

/// A memoised selection: the match (if any), whether accepted exact candidates formed no usable
/// group (R048), the counters.
#[derive(Clone, Debug)]
pub(crate) struct Selection {
    pub matched: Option<Arc<Match>>,
    pub ambiguous: bool,
    pub counters: Counters,
}

/// R043–R046: the exact candidates the stop's country and okres accept, in corpus order.
pub(crate) fn exact_matches(
    evidence: &Evidence,
    policy: &Policy,
    stop: Locality<'_>,
    exact: impl Iterator<Item = u32>,
    counters: &mut Counters,
) -> Vec<u32> {
    let file = &policy.file;
    // R043.
    let by_country: Vec<u32> = exact
        .filter(|&entry| {
            let candidate = &evidence.entries[entry as usize];
            let accepted = match stop.country {
                Some(country) => candidate
                    .country
                    .as_deref()
                    .is_some_and(|c| file.same_country(c, country)),
                None => true,
            };
            if !accepted {
                counters.country_rejects += 1;
            }
            accepted
        })
        .collect();
    // R044.
    let expected = match (stop.country, stop.okres) {
        (Some(country), _) if !file.is_home(country) => return by_country,
        (_, None) => return by_country,
        (_, Some(okres)) => file.okres_alias(okres),
    };
    let strict: Vec<u32> = by_country
        .iter()
        .copied()
        .filter(|&entry| {
            evidence.entries[entry as usize]
                .okres
                .as_deref()
                .is_some_and(|okres| file.okres_alias(okres) == expected)
        })
        .collect();
    if !strict.is_empty() {
        counters.strict += strict.len() as u32;
        return strict;
    }
    // R046.
    by_country
        .into_iter()
        .filter(|&entry| {
            let candidate = &evidence.entries[entry as usize];
            let Some(actual) = candidate.okres.as_deref() else {
                counters.non_adjacent += 1;
                return false;
            };
            if !evidence.okresy_adjacent(expected, actual, policy) {
                counters.non_adjacent += 1;
                return false;
            }
            let within = evidence.okres_area(expected, policy).is_some_and(|area| {
                evidence
                    .okres_areas
                    .boundary_distance_m(area, candidate.coord)
                    <= file.matching.okres_border_tolerance_m
            });
            if within {
                counters.border += 1;
            } else {
                counters.outside_border += 1;
            }
            within
        })
        .collect()
}

/// R048: the group when it is non-empty, shares one okres and country, and is tight enough.
fn valid_group(evidence: &Evidence, policy: &Policy, group: Vec<u32>) -> Option<Vec<u32>> {
    let first = &evidence.entries[*group.first()? as usize];
    if group.iter().any(|&entry| {
        let entry = &evidence.entries[entry as usize];
        entry.okres != first.okres || entry.country != first.country
    }) {
        return None;
    }
    let points: Vec<Coord> = group
        .iter()
        .map(|&entry| evidence.entries[entry as usize].coord)
        .collect();
    (plane::radius_m(&points) < policy.file.matching.max_group_radius_m).then_some(group)
}

/// R047, R048: the checked group, else the OSM group.
pub(crate) fn select_group(
    evidence: &Evidence,
    policy: &Policy,
    accepted: &[u32],
) -> Option<Vec<u32>> {
    let (checked, osm): (Vec<u32>, Vec<u32>) = accepted
        .iter()
        .partition(|&&entry| evidence.entries[entry as usize].class.is_checked());
    valid_group(evidence, policy, checked).or_else(|| valid_group(evidence, policy, osm))
}

/// R049: position, first entry, sources.
pub(crate) fn make_match(evidence: &Evidence, group: Vec<u32>) -> Match {
    let points: Vec<Coord> = group
        .iter()
        .map(|&entry| evidence.entries[entry as usize].coord)
        .collect();
    let mut sources: Vec<&str> = group
        .iter()
        .map(|&entry| &*evidence.entries[entry as usize].source)
        .collect();
    sources.sort_unstable();
    sources.dedup();
    let classes = group
        .iter()
        .map(|&entry| evidence.entries[entry as usize].class);
    let class = classes.min().unwrap_or(SourceClass::Osm);
    Match {
        coord: plane::centroid(&points).unwrap_or(Coord::new(0.0, 0.0)),
        first: group[0],
        class,
        source: sources.join("+").into(),
        entries: group,
    }
}

/// R043–R049 over candidates (only the exact ones count).
pub(crate) fn top_match(
    evidence: &Evidence,
    policy: &Policy,
    stop: Locality<'_>,
    candidates: &[Candidate],
    counters: &mut Counters,
) -> (Option<Match>, bool) {
    let exact = candidates
        .iter()
        .filter(|c| c.quality.is_exact())
        .map(|c| c.entry);
    let accepted = exact_matches(evidence, policy, stop, exact, counters);
    let matched =
        select_group(evidence, policy, &accepted).map(|group| make_match(evidence, group));
    let ambiguous = matched.is_none() && !accepted.is_empty();
    (matched, ambiguous)
}

/// The primary selection of a name string with an okres and country, memoised (verdict Q18).
pub(crate) fn selection(
    evidence: &Evidence,
    policy: &Policy,
    name: &str,
    stop: Locality<'_>,
) -> Selection {
    let key = (
        Box::from(name),
        stop.okres.map(Box::from),
        stop.country.map(Box::from),
    );
    if let Some(found) = evidence.selections.read().expect("memo").get(&key) {
        return found.clone();
    }
    let candidates = evidence.candidates(name);
    let mut counters = Counters::default();
    let (matched, ambiguous) = top_match(evidence, policy, stop, &candidates, &mut counters);
    let computed = Selection {
        matched: matched.map(Arc::new),
        ambiguous,
        counters,
    };
    evidence
        .selections
        .write()
        .expect("memo")
        .entry(key)
        .or_insert(computed)
        .clone()
}
