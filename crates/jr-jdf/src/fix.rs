//! Stop matching, one batch at a time: names, okres and country, positions (JDF-R033–R079).
//!
//! [`Evidence`] is built once per build from the catalogues, the OSM stops, the okres and
//! municipality polygons, the European town names and the synonym table (R006–R016). [`fix`]
//! then takes one batch through the rules of `docs/rules/jdf-read.md` stage 8–11: names
//! (R036–R038), exact matching (R039–R049, the name rules in `jr_geo`), the town-only vetoes and
//! secondary matching (R050–R053), MHD naming (R056–R060), okres and country (R055,
//! R061–R065), post candidates (R066, R067, R071), plausibility (R068–R070), the checks
//! (R072–R074) and positions (R075–R078). Matching is per batch (verdict Q18); the
//! context-free part (candidates and the primary selection of a name, okres and country) is
//! memoised in the evidence, so the answer does not depend on which batch asks first.
//!
//! Distances are `jr_model::geo::distance_m`, centroids means in a local tangent plane
//! (verdict Q5).

mod checks;
mod evidence;
mod geography;
mod locations;
mod mhd;
mod names;
mod osm;
mod plausibility;
mod policy;
mod posts;
mod primary;
mod rules;
mod select;
mod trips;

use std::collections::{BTreeMap, BTreeSet};

use jr_model::{Coord, Diagnostic, Diagnostics, Rule, Severity};

pub use evidence::{Evidence, EvidenceInputs};
pub use policy::{Policy, PolicyFile, RoadNames};
pub use rules::*;

use crate::batch::Batch;
use crate::fixed::{FixedBatch, FixedStop, Location, Origin};
use evidence::SourceClass;
use geography::{Cause, GeoStop};
use names::WorkStop;
use primary::Event;

/// Every rule this module implements.
pub const RULES: &[Rule] = &[
    R006, R007, R008, R009, R010, R011, R012, R013, R015, R016, R033, R034, R035, R036, R037, R038,
    R043, R044, R045, R046, R047, R048, R049, R050, R051, R052, R053, R055, R056, R057, R058, R059,
    R060, R061, R062, R063, R064, R065, R066, R067, R068, R069, R070, R071, R072, R073, R074, R075,
    R076, R077, R078, R082,
];

/// Reports into one batch's diagnostics.
struct Report<'a> {
    batch: &'a str,
    diagnostics: &'a mut Diagnostics,
}

impl Report<'_> {
    fn batch(&mut self, code: &'static str, severity: Severity, message: String, rule: Rule) {
        self.diagnostics
            .push(Diagnostic::new(code, severity, self.batch, message).rule(rule));
    }

    fn stop(
        &mut self,
        code: &'static str,
        severity: Severity,
        stop: u64,
        message: String,
        rule: Rule,
    ) {
        self.diagnostics.push(
            Diagnostic::new(
                code,
                severity,
                format!("{}/stop {stop}", self.batch),
                message,
            )
            .rule(rule),
        );
    }
}

/// One batch through R033–R079. `None` for a degenerate batch (R033), with its diagnostic.
pub fn fix(
    batch: Batch,
    evidence: &Evidence,
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> Option<FixedBatch> {
    let batch_name = batch.name.clone();
    let mut report = Report {
        batch: &batch_name,
        diagnostics,
    };
    let file = &policy.file;
    let home = &*file.home_country;

    // R033.
    let called: BTreeSet<u64> = batch
        .trip_stops
        .iter()
        .filter(|call| trips::is_called(call))
        .map(|call| call.stop_id)
        .collect();
    if called.len() < file.batch.min_called_stops {
        report.batch(
            "jdf_batch_degenerate",
            Severity::Warning,
            format!(
                "{} distinct called stops, fewer than {}: batch dropped",
                called.len(),
                file.batch.min_called_stops
            ),
            R033,
        );
        return None;
    }
    // R034.
    if batch.routes.len() != 1 {
        report.batch(
            "jdf_batch_route_count",
            Severity::Warning,
            format!("{} routes, one expected", batch.routes.len()),
            R034,
        );
    }
    // R035.
    let positions = trips::stop_positions(&batch);
    let (forward, backward, unknown) = trips::matrices(&batch, &positions);
    for stop in unknown {
        report.stop(
            "jdf_call_stop_unknown",
            Severity::Warning,
            stop,
            "called at but not in Zastavky: left out of the trip matrices".into(),
            R035,
        );
    }
    let matrices = [&forward, &backward];

    // R036–R038.
    let raw: Vec<WorkStop> = batch.stops.iter().map(WorkStop::from_source).collect();
    let mut branch_counts = [0usize; 3];
    let normalised: Vec<WorkStop> = raw
        .iter()
        .map(|stop| {
            let mut stop = stop.clone();
            if names::okres_from_name(&mut stop, home) {
                report.stop(
                    "jdf_stop_okres_in_name",
                    Severity::Info,
                    stop.id,
                    format!(
                        "okres {} taken from the town name",
                        stop.okres.as_deref().unwrap_or("")
                    ),
                    R036,
                );
            }
            if let Some(branch) = names::normalise_name(&mut stop) {
                branch_counts[usize::from(branch) - 1] += 1;
            }
            stop
        })
        .collect();
    let non_mhd: Vec<WorkStop> = normalised
        .iter()
        .map(|stop| {
            let mut stop = stop.clone();
            if names::split_single_comma(&mut stop) {
                branch_counts[2] += 1;
            }
            stop
        })
        .collect();
    if branch_counts[0] + branch_counts[1] > 0 {
        report.batch(
            "jdf_stop_names_normalised",
            Severity::Info,
            format!(
                "{} names split from the town column, {} nearby places moved to the district",
                branch_counts[0], branch_counts[1]
            ),
            R037,
        );
    }
    if branch_counts[2] > 0 {
        report.batch(
            "jdf_stop_names_comma_split",
            Severity::Info,
            format!("{} town-only names split at their comma", branch_counts[2]),
            R038,
        );
    }

    // R039–R053 on the plain names, then R056–R060.
    let (plain, plain_counters) = primary::match_stops(non_mhd, matrices, &batch, evidence, policy);
    let town_candidates = mhd::town_candidates(&normalised, &batch, evidence, policy);
    let count = |set: &[primary::Matched]| set.iter().filter(|s| s.matched.is_some()).count();
    let (chosen, counters) = match town_candidates.first() {
        None => (plain, plain_counters),
        Some((town, support)) => {
            let towns = mhd::town_list(town, policy);
            let renamed: Vec<WorkStop> = raw
                .iter()
                .map(|stop| {
                    mhd::rename(
                        stop,
                        &towns,
                        evidence,
                        policy,
                        &batch_name,
                        report.diagnostics,
                    )
                })
                .collect();
            let (mhd_set, mhd_counters) =
                primary::match_stops(renamed, matrices, &batch, evidence, policy);
            let (mhd_count, plain_count) = (count(&mhd_set), count(&plain));
            if mhd_count >= plain_count + file.mhd.min_match_gain {
                report.batch(
                    "jdf_mhd_naming",
                    Severity::Info,
                    format!(
                        "stops renamed for MHD naming with {} (support {support}): {mhd_count} \
                         matched stops against {plain_count}",
                        towns.join(", ")
                    ),
                    R060,
                );
                (mhd_set, mhd_counters)
            } else {
                report.batch(
                    "jdf_mhd_naming_not_applied",
                    Severity::Info,
                    format!(
                        "MHD town {town} (support {support}) matches {mhd_count} stops against \
                         {plain_count}: names kept"
                    ),
                    R060,
                );
                (plain, plain_counters)
            }
        }
    };
    if counters != select::Counters::default() {
        report.batch(
            "jdf_match_geography",
            Severity::Info,
            format!(
                "okres filter: {} strict, {} within the border tolerance; rejected: {} by \
                 country, {} not adjacent, {} outside the border tolerance",
                counters.strict,
                counters.border,
                counters.country_rejects,
                counters.non_adjacent,
                counters.outside_border
            ),
            R074,
        );
    }

    // Diagnose what the rules did to the chosen stops; keep the candidates for R066.
    let mut states = Vec::with_capacity(chosen.len());
    let mut candidate_lists = Vec::with_capacity(chosen.len());
    let mut stops_after_matching = Vec::with_capacity(chosen.len());
    for state in chosen {
        let id = state.stop.id;
        let mut cause = None;
        for event in &state.events {
            let (code, rule, message) = match event {
                Event::Ambiguous => (
                    "jdf_match_ambiguous",
                    R048,
                    "exact candidates differ in okres or country or lie too far apart: not \
                     matched",
                ),
                Event::ForeignTownVeto => (
                    "jdf_match_foreign_town",
                    R050,
                    "town-only name matched a European town name: not matched",
                ),
                Event::MunicipalityName => (
                    "jdf_match_municipality_name",
                    R051,
                    "town-only name is a municipality's: no primary match",
                ),
                Event::Secondary => (
                    "jdf_match_secondary",
                    R052,
                    "matched by kilometre distance along the trips",
                ),
                Event::Reviewed => (
                    "jdf_match_reviewed",
                    R082,
                    "placed by its reviewed gap-fill row",
                ),
                Event::SecondaryDisagreement => (
                    "jdf_match_secondary_disagreement",
                    R052,
                    "trips disagree on a secondary match: not matched",
                ),
                Event::FromTownName => {
                    cause = Some(Cause::TownName);
                    continue;
                }
            };
            report.stop(code, Severity::Info, id, message.into(), rule);
        }
        stops_after_matching.push(state.stop.clone());
        candidate_lists.push(state.candidates);
        states.push(GeoStop {
            stop: state.stop,
            matched: state.matched,
            cause,
        });
    }

    // R055, R061–R065.
    geography::from_match(&mut states, evidence);
    let reverted = geography::propagate(&mut states, matrices, &batch, evidence, policy);
    if reverted > 0 {
        report.batch(
            "jdf_geography_propagation_reverted",
            Severity::Info,
            format!("{reverted} propagated okresy reverted: the trip may cross an okres border"),
            R062,
        );
    }
    for state in &states {
        let (code, rule) = match state.cause {
            None => continue,
            Some(Cause::TownName) => ("jdf_geography_from_town", R053),
            Some(Cause::Match) => ("jdf_geography_from_match", R055),
            Some(Cause::Propagated) => ("jdf_geography_propagated", R061),
            Some(Cause::TownDistance) => ("jdf_geography_town_distance", R063),
        };
        report.stop(
            code,
            Severity::Info,
            state.stop.id,
            format!(
                "okres {}, country {} inferred",
                state.stop.okres.as_deref().unwrap_or("-"),
                state.stop.country.as_deref().unwrap_or("-")
            ),
            rule,
        );
    }

    // R066, R067.
    let mut post_candidates: Vec<crate::fixed::PostCandidate> = Vec::new();
    for (position, (stop, candidates)) in stops_after_matching
        .iter()
        .zip(&candidate_lists)
        .enumerate()
    {
        post_candidates.extend(posts::candidates(
            position as u32,
            stop,
            candidates,
            evidence,
            policy,
        ));
    }

    // R068–R070.
    let ordered = trips::ordered_timed_trips(&batch, policy);
    let mut matches: BTreeMap<u64, plausibility::Matched> = BTreeMap::new();
    for state in &states {
        if let Some(found) = &state.matched {
            matches.insert(
                state.stop.id,
                plausibility::Matched {
                    coord: found.coord,
                    checked: found.class.is_checked(),
                },
            );
        }
    }
    let timed: Vec<Vec<(u64, f64)>> = ordered
        .iter()
        .map(|calls| {
            calls
                .iter()
                .filter_map(|&(call, time)| Some((batch.trip_stops[call].stop_id, time?)))
                .collect()
        })
        .collect();
    let rejected = plausibility::rejected(&timed, &matches, policy);
    for &stop in &rejected {
        report.stop(
            "jdf_match_implausible",
            Severity::Warning,
            stop,
            "match contradicted by scheduled travel time: rejected".into(),
            R070,
        );
    }
    for state in &mut states {
        if rejected.contains(&state.stop.id) {
            state.matched = None;
        }
    }
    // R071.
    let surviving: BTreeSet<u64> = states
        .iter()
        .filter(|s| s.matched.is_some())
        .map(|s| s.stop.id)
        .collect();
    post_candidates.retain(|c| surviving.contains(&batch.stops[c.stop as usize].id));

    // R072, R073.
    checks::km_distances(&batch, &states, policy, &mut report);
    checks::missing_geography(&states, &called, policy, &mut report);

    // R075–R078.
    let mut precise: BTreeMap<u64, Location> = BTreeMap::new();
    for state in &states {
        if let Some(found) = &state.matched {
            precise.entry(state.stop.id).or_insert(Location {
                coord: found.coord,
                origin: match found.class {
                    SourceClass::Gapfill => Origin::Gapfill,
                    SourceClass::Catalogue => Origin::Catalogue,
                    SourceClass::Osm => Origin::Osm,
                },
                source: found.source.clone(),
            });
        }
    }
    let anchors: BTreeMap<u64, Coord> = precise.iter().map(|(&id, l)| (id, l.coord)).collect();
    let untimed: Vec<Vec<(u64, Option<f64>)>> = ordered
        .iter()
        .map(|calls| {
            calls
                .iter()
                .map(|&(call, time)| (batch.trip_stops[call].stop_id, time))
                .collect()
        })
        .collect();
    let estimates = locations::estimates(&untimed, &anchors, policy);
    for (&stop, location) in &estimates {
        report.stop(
            "jdf_location_estimated",
            Severity::Info,
            stop,
            format!("position estimated ({})", location.source),
            if location.origin == Origin::RouteTime {
                R076
            } else {
                R077
            },
        );
    }

    let stops: Vec<FixedStop> = states
        .into_iter()
        .map(|state| {
            let id = state.stop.id;
            let location = precise.get(&id).or_else(|| estimates.get(&id)).cloned();
            if location.is_none() {
                let called = called.contains(&id);
                report.stop(
                    "jdf_location_missing",
                    if called {
                        Severity::Warning
                    } else {
                        Severity::Info
                    },
                    id,
                    if called {
                        "no position: not matched and no trip to estimate it along".into()
                    } else {
                        "no position (not called at)".into()
                    },
                    R078,
                );
            }
            FixedStop {
                town: state.stop.town,
                district: state.stop.district,
                nearby_place: state.stop.nearby,
                okres: state.stop.okres,
                country: state.stop.country,
                location,
            }
        })
        .collect();
    Some(FixedBatch {
        batch,
        stops,
        post_candidates,
    })
}
