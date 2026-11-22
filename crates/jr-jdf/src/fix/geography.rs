//! Okres and country of the chosen stops (JDF-R055, R061–R065).

use std::sync::Arc;

use jr_model::Coord;

use super::evidence::Evidence;
use super::names::WorkStop;
use super::policy::Policy;
use super::select::Match;
use super::trips::Matrix;
use crate::batch::Batch;

/// Which rule set a stop's okres or country.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cause {
    /// R053 (set during matching).
    TownName,
    /// R055.
    Match,
    /// R061.
    Propagated,
    /// R063.
    TownDistance,
}

#[derive(Clone, Debug)]
pub(crate) struct GeoStop {
    pub stop: WorkStop,
    pub matched: Option<Arc<Match>>,
    pub cause: Option<Cause>,
}

/// R055: a stop without okres takes the match's okres and country.
pub(crate) fn from_match(states: &mut [GeoStop], evidence: &Evidence) {
    for state in states {
        if state.stop.okres.is_some() {
            continue;
        }
        let Some(found) = &state.matched else {
            continue;
        };
        let entry = &evidence.entries[found.first as usize];
        if state.stop.okres != entry.okres || state.stop.country != entry.country {
            state.cause = Some(Cause::Match);
        }
        state.stop.okres = entry.okres.clone();
        state.stop.country = entry.country.clone();
    }
}

fn unfinished(states: &[GeoStop], home: &str) -> bool {
    states.iter().any(|s| {
        s.stop.country.is_none()
            || (s.stop.country.as_deref() == Some(home) && s.stop.okres.is_none())
    })
}

/// R064: the passes over matrix 1, matrix 1 reversed, matrix 2, matrix 2 reversed, each only
/// while some stop lacks a country, or is in the home country without okres. Returns how many
/// propagated okresy R062 reverted.
pub(crate) fn propagate(
    states: &mut [GeoStop],
    matrices: [&Matrix; 2],
    batch: &Batch,
    evidence: &Evidence,
    policy: &Policy,
) -> usize {
    let home = &*policy.file.home_country;
    let mut reverted = 0;
    for matrix in matrices {
        for pass in [matrix.clone(), matrix.reversed()] {
            if unfinished(states, home) {
                reverted += fill_pass(states, &pass, batch, evidence, policy);
            }
        }
    }
    reverted
}

/// Per column, the last matched home-country position: country, okres, km, point, km to the
/// okres's exterior.
type LastPosition = (Box<str>, Box<str>, f64, Coord, f64);

/// R061–R063 and R065 over one matrix.
fn fill_pass(
    states: &mut [GeoStop],
    matrix: &Matrix,
    batch: &Batch,
    evidence: &Evidence,
    policy: &Policy,
) -> usize {
    let home = &*policy.file.home_country;
    let tolerance = policy.file.geography.town_km_tolerance;
    let mut last_okres: Option<Box<str>> = None;
    let mut last_country: Option<Box<str>> = None;
    let mut last: Vec<Option<LastPosition>> = vec![None; matrix.columns];
    let mut results: Vec<Option<(WorkStop, Option<Cause>)>> = vec![None; states.len()];
    let mut reverted = 0;
    for row in &matrix.rows {
        let state = &states[row.stop];
        let source = &state.stop;
        let kms: Vec<Option<f64>> = row
            .calls
            .iter()
            .map(|call| {
                call.and_then(|c| batch.trip_stops[c].km)
                    .map(|km| km.value())
            })
            .collect();
        let mut new = source.clone();
        // R061.
        match (&source.okres, &source.country) {
            (Some(okres), Some(country)) => {
                last_okres = Some(okres.clone());
                last_country = Some(country.clone());
            }
            (None, Some(country)) => {
                if last_country.as_ref() == Some(country) {
                    new.okres = last_okres.clone();
                } else {
                    last_okres = None;
                    last_country = Some(country.clone());
                }
            }
            (Some(okres), None) => {
                last_okres = Some(okres.clone());
                new.country = last_country.clone();
            }
            (None, None) => {
                new.okres = last_okres.clone();
                new.country = last_country.clone();
            }
        }
        // R062.
        if new.okres != source.okres {
            let uncertain = kms
                .iter()
                .zip(&last)
                .all(|(km, position)| match (position, km) {
                    (Some((country, okres, last_km, _, edge_km)), Some(km))
                        if new.country.as_ref() == Some(country)
                            && new.okres.as_ref() == Some(okres) =>
                    {
                        (km - last_km).abs() > *edge_km
                    }
                    _ => true,
                });
            if uncertain {
                new = source.clone();
                reverted += 1;
            }
        }
        let mut cause = None;
        // R063.
        if new.okres.is_none() && new.country.as_deref().is_none_or(|c| c == home) {
            let mut okresy: Vec<&str> = Vec::new();
            for entry in evidence.municipalities_named(&source.town) {
                let municipality = &evidence.municipalities[entry as usize];
                let Some(okres) = municipality.okres.as_deref() else {
                    continue;
                };
                let near = kms
                    .iter()
                    .zip(&last)
                    .any(|(km, position)| match (position, km) {
                        (Some((_, _, last_km, point, _)), Some(km)) => {
                            evidence
                                .municipality_areas
                                .distance_m(entry as usize, *point)
                                / 1000.0
                                < (km - last_km).abs() + tolerance
                        }
                        _ => false,
                    });
                if near && !okresy.contains(&okres) {
                    okresy.push(okres);
                }
            }
            if let [okres] = okresy.as_slice() {
                new.country = Some(home.into());
                new.okres = Some((*okres).into());
                cause = Some(Cause::TownDistance);
            }
        }
        if cause.is_none() && (new.okres != source.okres || new.country != source.country) {
            cause = Some(Cause::Propagated);
        }
        // The column positions come from the stop as it entered the pass.
        if let (Some(found), Some(country), Some(okres)) =
            (&state.matched, &source.country, &source.okres)
            && &**country == home
            && let Some(area) = evidence.okres_area(okres, policy)
        {
            let edge_km = evidence.okres_areas.exterior_distance_m(area, found.coord) / 1000.0;
            for (column, km) in kms.iter().enumerate() {
                if let Some(km) = km {
                    last[column] =
                        Some((country.clone(), okres.clone(), *km, found.coord, edge_km));
                }
            }
        }
        // R065: the row result with the most information wins, the first among equals.
        let rank = |stop: &WorkStop| (stop.okres.is_some(), stop.country.is_some());
        let slot = &mut results[row.stop];
        if slot
            .as_ref()
            .is_none_or(|(existing, _)| rank(&new) > rank(existing))
        {
            *slot = Some((new, cause));
        }
    }
    for (state, result) in states.iter_mut().zip(results) {
        if let Some((stop, cause)) = result {
            if cause.is_some() {
                state.cause = cause;
            }
            state.stop = stop;
        }
    }
    reverted
}
