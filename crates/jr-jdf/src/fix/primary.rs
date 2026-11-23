//! Matching one set of stop names (F# `matchStops`): exact selection, the town-only vetoes
//! (JDF-R050, R051), secondary matching by kilometres (R052) and geography from town names
//! (R053).

use std::sync::Arc;

use jr_geo::stop_index::Candidate;
use jr_model::geo::distance_m;

use super::evidence::Evidence;
use super::names::{WorkStop, first_part};
use super::policy::Policy;
use super::select::{Counters, Match, reviewed_match, selection, top_match};
use super::trips::Matrix;
use crate::batch::Batch;

/// Something a rule did to a stop, diagnosed once the stop set is chosen.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Event {
    /// R048: exact candidates, but no usable group.
    Ambiguous,
    /// R050.
    ForeignTownVeto,
    /// R051.
    MunicipalityName,
    /// R052.
    Secondary,
    SecondaryDisagreement,
    /// R082.
    Reviewed,
    /// R053: okres and country inferred from the town name.
    FromTownName,
}

/// One stop after matching.
#[derive(Clone, Debug)]
pub(crate) struct Matched {
    pub stop: WorkStop,
    pub matched: Option<Arc<Match>>,
    /// Every retrieved candidate of the stop's name (any score).
    pub candidates: Arc<[Candidate]>,
    pub events: Vec<Event>,
}

/// R039–R053 for one stop set, aligned with it; the counters of the primary selections.
pub(crate) fn match_stops(
    stops: Vec<WorkStop>,
    matrices: [&Matrix; 2],
    batch: &Batch,
    evidence: &Evidence,
    policy: &Policy,
) -> (Vec<Matched>, Counters) {
    let mut counters = Counters::default();
    let mut matched: Vec<Matched> = stops
        .into_iter()
        .map(|stop| {
            let name = stop.name_string();
            let selection = selection(evidence, policy, &name, stop.locality());
            counters.add(selection.counters);
            let mut events = Vec::new();
            if selection.ambiguous {
                events.push(Event::Ambiguous);
            }
            let mut chosen = selection.matched;
            if stop.town_only()
                && let Some(found) = &chosen
            {
                let entry_name = &evidence.entries[found.first as usize].name;
                if evidence
                    .first_foreign_town(first_part(entry_name))
                    .is_some()
                {
                    events.push(Event::ForeignTownVeto);
                    chosen = None;
                } else if evidence.is_municipality(&stop.town) {
                    events.push(Event::MunicipalityName);
                    chosen = None;
                }
            }
            Matched {
                candidates: evidence.candidates(&name),
                stop,
                matched: chosen,
                events,
            }
        })
        .collect();
    for matrix in matrices {
        secondary(&mut matched, matrix, batch, evidence, policy);
    }
    // R082: what the catalogues and the kilometres left unplaced takes its reviewed row.
    for state in &mut matched {
        if state.matched.is_none()
            && let Some(found) =
                reviewed_match(evidence, policy, state.stop.locality(), &state.candidates)
        {
            state.matched = Some(Arc::new(found));
            state.events.push(Event::Reviewed);
        }
    }
    for state in &mut matched {
        if state.matched.is_none() && from_town_name(&mut state.stop, evidence, policy) {
            state.events.push(Event::FromTownName);
        }
    }
    (matched, counters)
}

/// R052: one pass over a matrix.
fn secondary(
    states: &mut [Matched],
    matrix: &Matrix,
    batch: &Batch,
    evidence: &Evidence,
    policy: &Policy,
) {
    let tolerance = policy.file.matching.secondary_km_tolerance;
    let mut last: Vec<Option<(jr_model::Coord, f64)>> = vec![None; matrix.columns];
    // Per stop, the first row result with a match, else the first row result.
    let mut results: Vec<Option<(Option<Arc<Match>>, bool)>> = vec![None; states.len()];
    for row in &matrix.rows {
        let state = &states[row.stop];
        let current = &state.matched;
        let mut suggestions: Vec<Option<Arc<Match>>> = Vec::new();
        for (column, call) in row.calls.iter().enumerate() {
            let km = call.and_then(|call| batch.trip_stops[call].km);
            let suggestion = match (current, km, last[column]) {
                (Some(found), Some(km), _) => {
                    last[column] = Some((found.coord, km.value()));
                    current.clone()
                }
                (None, Some(km), Some((position, last_km))) => {
                    let reach = (km.value() - last_km).abs() + tolerance;
                    let near: Vec<Candidate> = state
                        .candidates
                        .iter()
                        .copied()
                        .filter(|c| {
                            let coord = evidence.entries[c.entry as usize].coord;
                            distance_m(coord, position) / 1000.0 < reach
                        })
                        .collect();
                    let mut ignored = Counters::default();
                    top_match(evidence, policy, state.stop.locality(), &near, &mut ignored)
                        .0
                        .map(Arc::new)
                }
                _ => current.clone(),
            };
            if suggestion != *current {
                suggestions.push(suggestion);
            }
        }
        let (result, disagreed) = match suggestions.first() {
            Some(Some(pick)) if suggestions.iter().all(|s| s.as_ref() == Some(pick)) => {
                (Some(pick.clone()), false)
            }
            Some(Some(_)) => (current.clone(), true),
            _ => (current.clone(), false),
        };
        let slot = &mut results[row.stop];
        match slot {
            None => *slot = Some((result, disagreed)),
            Some((existing, _)) if existing.is_none() && result.is_some() => {
                *slot = Some((result, disagreed));
            }
            _ => {}
        }
    }
    for (state, result) in states.iter_mut().zip(results) {
        let Some((result, disagreed)) = result else {
            continue;
        };
        if result != state.matched {
            state.matched = result;
            state.events.push(Event::Secondary);
        } else if disagreed {
            state.events.push(Event::SecondaryDisagreement);
        }
    }
}

/// R053: okres and country of an unmatched stop from its town name.
fn from_town_name(stop: &mut WorkStop, evidence: &Evidence, policy: &Policy) -> bool {
    let home = &*policy.file.home_country;
    let czech = || {
        let mut regions: Vec<(&str, &str)> = Vec::new();
        for entry in evidence.municipalities_named(&stop.town) {
            let municipality = &evidence.municipalities[entry as usize];
            if let Some(okres) = municipality.okres.as_deref() {
                let pair = (&*municipality.name, okres);
                if !regions.contains(&pair) {
                    regions.push(pair);
                }
            }
        }
        regions
            .into_iter()
            .map(|(_, okres)| Box::<str>::from(okres))
            .collect::<Vec<_>>()
    };
    match (stop.country.as_deref(), stop.okres.as_deref()) {
        (Some(country), None) if country == home => {
            if let [okres] = czech().as_slice() {
                stop.okres = Some(okres.clone());
                return true;
            }
            false
        }
        (None, None) => {
            let foreign = evidence.first_foreign_town(&stop.town);
            match (czech().as_slice(), foreign) {
                ([okres], None) => {
                    stop.okres = Some(okres.clone());
                    stop.country = Some(home.into());
                    true
                }
                ([], Some(iso)) => {
                    stop.country = Some(policy.file.jdf_country(iso));
                    true
                }
                _ => false,
            }
        }
        _ => false,
    }
}
