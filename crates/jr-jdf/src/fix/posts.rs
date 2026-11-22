//! Post candidates (JDF-R066, R067, R071): the observed positions near a matched stop.

use std::collections::BTreeSet;

use jr_geo::stop_index::Candidate;
use jr_model::geo::distance_m;

use super::evidence::Evidence;
use super::names::WorkStop;
use super::policy::Policy;
use super::select::{Counters, exact_matches, make_match, select_group};
use crate::fixed::PostCandidate;

/// R066, R067: the candidates of one stop, distinct by observation and sorted by it.
pub(crate) fn candidates(
    position: u32,
    stop: &WorkStop,
    candidates: &[Candidate],
    evidence: &Evidence,
    policy: &Policy,
) -> Vec<PostCandidate> {
    let exact = || {
        candidates
            .iter()
            .filter(|c| c.quality.is_exact())
            .map(|c| c.entry)
    };
    let mut ignored = Counters::default();
    let accepted = exact_matches(evidence, policy, stop.locality(), exact(), &mut ignored);
    let Some(group) = select_group(evidence, policy, &accepted) else {
        return Vec::new();
    };
    let countries: BTreeSet<&str> = group
        .iter()
        .filter_map(|&entry| evidence.entries[entry as usize].country.as_deref())
        .collect();
    let centre = make_match(evidence, group).coord;
    let mut seen = BTreeSet::new();
    let mut out: Vec<PostCandidate> = accepted
        .into_iter()
        .filter_map(|entry| {
            let entry = &evidence.entries[entry as usize];
            let near = distance_m(entry.coord, centre) <= policy.file.posts.candidate_radius_m;
            let country_ok = countries.is_empty()
                || entry
                    .country
                    .as_deref()
                    .is_none_or(|c| countries.contains(c));
            (near && country_ok && seen.insert(entry.observation.clone())).then(|| PostCandidate {
                stop: position,
                coord: entry.coord,
                observation: entry.observation.clone(),
                facts: entry.facts.clone(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.observation.cmp(&b.observation));
    out
}
