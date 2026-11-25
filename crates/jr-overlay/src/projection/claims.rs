//! Claims on a base trip and date (OVL-R082–R085).

use std::collections::{BTreeMap, HashMap};

use jr_model::{DateSet, Diagnostic, Diagnostics, Rule, Severity};
use rayon::prelude::*;

use super::capability;
use super::selection::{self, Selection};
use crate::authority::{Authority, trip_subject};
use crate::matching::Matches;
use crate::prepare::Prepared;
use crate::profile::Capability;

/// Per base trip and date, the distinct selections of the bindings holding it: all compatible
/// merge (`cross_source_fact_coalesced` when from several sources); else, from one source with
/// exactly one claim of the newest revision (R015), that one (`overlay_newer_source_selected`);
/// else none wins (`overlay_fact_conflict`, `cross_source_fact_conflict`) and the claiming
/// source trips count as represented that day. A conflict keeps the national trip on that date
/// even on a line-date under authority (B23 fixed, verdict Q3).
///
/// R134 leaves one claim per source on a base trip and day, so the claims compared here are
/// those of several sources.
pub const OVL_R082: Rule = Rule {
    id: "OVL-R082",
    summary: "per base trip and date: merge compatible claims, else newest revision, else keep",
};

/// A binding of a projected source trip that does not state every source call, or may not
/// state schedules and boarding points, loses the dates its trip's projection runs on, so the
/// source trip is added as its own trip there.
///
/// "Partial evidence cannot represent a complete authoritative source instance."
pub const OVL_R083: Rule = Rule {
    id: "OVL-R083",
    summary: "an incomplete binding of a projected trip yields to the source trip's addition",
};

/// Across base trips in ID order, a projected source trip selected on one date by a base trip
/// is not selected by later ones on that date (`equivalent_baseline_instance_removed`).
///
/// "One source instance must not produce multiple baseline variants."
pub const OVL_R084: Rule = Rule {
    id: "OVL-R084",
    summary: "one projected source instance selects one base trip per date",
};

/// A source trip is represented on a date where its bound base trip has a resolved selection
/// compatible with the binding's.
pub const OVL_R085: Rule = Rule {
    id: "OVL-R085",
    summary: "a source trip is represented where its base trip's selection agrees with it",
};

pub const RULES: &[Rule] = &[OVL_R082, OVL_R083, OVL_R084, OVL_R085];

/// The claims of one base trip.
#[derive(Clone, Debug, Default)]
pub struct TripClaims {
    /// R082–R084: the selection resolved per date.
    pub resolved: BTreeMap<u32, Selection>,
    /// R082: dates with quarantined conflicts.
    pub conflicts: DateSet,
    /// R082: the distinct claims' source trips of each conflict (date, source, trip).
    pub claimants: Vec<(u32, u32, u32)>,
}

/// The outcome of R082–R085.
#[derive(Debug)]
pub struct Resolved {
    /// Per base trip that some binding targets.
    pub trips: HashMap<u32, TripClaims>,
    /// Per source, per trip: represented dates (R082, R085).
    pub represented: Vec<Vec<DateSet>>,
}

fn date_text(prepared: &Prepared<'_>, date: u32) -> String {
    prepared
        .base
        .timetable
        .horizon
        .date_at(date as usize)
        .strftime("%Y%m%d")
        .to_string()
}

fn subject_of(prepared: &Prepared<'_>, selection: &Selection) -> String {
    let source = &prepared.sources[selection.source as usize];
    trip_subject(source, &source.trips[selection.trip as usize])
}

pub fn resolve(
    prepared: &Prepared<'_>,
    authority: &Authority,
    matches: &Matches,
    selections: &[Selection],
    diagnostics: &mut Diagnostics,
) -> Resolved {
    let mut bindings_by_target: HashMap<u32, Vec<usize>> = HashMap::new();
    for (index, binding) in matches.bindings.iter().enumerate() {
        bindings_by_target
            .entry(binding.candidate.base_trip)
            .or_default()
            .push(index);
    }
    let projected: HashMap<(u32, u32), usize> = authority
        .projections
        .iter()
        .enumerate()
        .map(|(index, p)| ((p.source, p.trip), index))
        .collect();
    let mut targets: Vec<u32> = bindings_by_target.keys().copied().collect();
    targets.sort_unstable();
    // R082, R083 per base trip.
    let outcomes: Vec<(u32, TripClaims, Vec<Diagnostic>)> = targets
        .par_iter()
        .map(|&target| {
            let base_id = prepared.base.timetable.trips[prepared.base.trips[target as usize].trip].id.to_string();
            let bindings = &bindings_by_target[&target];
            let mut dates = DateSet::EMPTY;
            for &b in bindings {
                dates = dates | matches.bindings[b].dates;
            }
            let mut claims = TripClaims::default();
            let mut found = Vec::new();
            for date in dates.iter() {
                let mut distinct: Vec<&Selection> = Vec::new();
                for &b in bindings {
                    if matches.bindings[b].dates.contains(date)
                        && !distinct.iter().any(|s| s.facts == selections[b].facts)
                    {
                        distinct.push(&selections[b]);
                    }
                }
                let compatible = distinct
                    .iter()
                    .all(|a| distinct.iter().all(|b| a.compatible(b)));
                let day = date_text(prepared, date as u32);
                if compatible {
                    let merged = selection::merge(prepared, &distinct);
                    if merged.sources.len() > 1 {
                        found.push(
                            Diagnostic::new(
                                "cross_source_fact_coalesced",
                                Severity::Info,
                                base_id.clone(),
                                format!("Identical claims from {} coalesced on {day}", source_list(prepared, &merged.sources)),
                            )
                            .rule(OVL_R082),
                        );
                    }
                    claims.resolved.insert(date as u32, merged);
                    continue;
                }
                let mut competing: Vec<u32> = distinct.iter().flat_map(|s| s.sources.iter().copied()).collect();
                competing.sort_unstable();
                competing.dedup();
                let revision = |s: &Selection| prepared.sources[s.source as usize].trips[s.trip as usize].revision;
                let newest = distinct.iter().map(|s| revision(s)).max().flatten();
                let newest_claims: Vec<&&Selection> = distinct.iter().filter(|s| revision(s) == newest).collect();
                if competing.len() == 1 && newest_claims.len() == 1 {
                    let chosen = (*newest_claims[0]).clone();
                    let mut superseded: Vec<String> = distinct
                        .iter()
                        .filter(|s| (s.source, s.trip) != (chosen.source, chosen.trip))
                        .map(|s| subject_of(prepared, s))
                        .collect();
                    superseded.dedup();
                    found.push(
                        Diagnostic::new(
                            "overlay_newer_source_selected",
                            Severity::Info,
                            base_id.clone(),
                            format!("{day} selected {} over older claims {}", subject_of(prepared, &chosen), superseded.join(";")),
                        )
                        .rule(OVL_R082),
                    );
                    claims.resolved.insert(date as u32, chosen);
                } else {
                    let code = if competing.len() > 1 { "cross_source_fact_conflict" } else { "overlay_fact_conflict" };
                    found.push(
                        Diagnostic::new(
                            code,
                            Severity::Warning,
                            base_id.clone(),
                            format!(
                                "Conflicting claims from {} quarantined on {day}; national value retained",
                                source_list(prepared, &competing)
                            ),
                        )
                        .rule(OVL_R082),
                    );
                    claims.conflicts.insert(date);
                    for claim in &distinct {
                        claims.claimants.push((date as u32, claim.source, claim.trip));
                    }
                }
            }
            // R083.
            for &b in bindings {
                let binding = &matches.bindings[b];
                let Some(&p) = projected.get(&(binding.source, binding.trip)) else {
                    continue;
                };
                let projection = &authority.projections[p];
                let source = &prepared.sources[binding.source as usize];
                let calls = source.trip_calls(&source.trips[binding.trip as usize]).len();
                let alignment = &binding.candidate.alignment;
                let complete = alignment.iter().all(Option::is_some)
                    && alignment.len() + selections[b].facts.repeats.len() == calls
                    && capability::eligible(prepared, binding, Capability::Schedules)
                    && capability::eligible(prepared, binding, Capability::CallBoardingPoints);
                if !complete {
                    claims.resolved.retain(|&date, selected| {
                        !(projection.dates.contains(date as usize)
                            && (selected.source, selected.trip) == (projection.source, projection.trip))
                    });
                }
            }
            (target, claims, found)
        })
        .collect();
    let mut trips: HashMap<u32, TripClaims> = HashMap::new();
    let mut order: Vec<(String, u32)> = Vec::new();
    for (target, claims, found) in outcomes {
        diagnostics.extend(found);
        order.push((
            prepared.base.timetable.trips[prepared.base.trips[target as usize].trip]
                .id
                .to_string(),
            target,
        ));
        trips.insert(target, claims);
    }
    // R084, base trips in ID order.
    order.sort();
    let mut claimed: HashMap<(u32, u32), DateSet> = HashMap::new();
    for (base_id, target) in &order {
        let claims = trips.get_mut(target).expect("a resolved trip");
        let mut removed = Vec::new();
        for (&date, selected) in &claims.resolved {
            let Some(&p) = projected.get(&(selected.source, selected.trip)) else {
                continue;
            };
            if !authority.projections[p].dates.contains(date as usize) {
                continue;
            }
            let dates = claimed.entry((selected.source, selected.trip)).or_default();
            if dates.contains(date as usize) {
                removed.push((date, selected.source));
            } else {
                dates.insert(date as usize);
            }
        }
        for (date, source) in removed {
            claims.resolved.remove(&date);
            diagnostics.push(
                Diagnostic::new(
                    "equivalent_baseline_instance_removed",
                    Severity::Warning,
                    base_id.clone(),
                    format!(
                        "One {} instance on {} must not produce multiple baseline variants",
                        prepared.sources[source as usize].source_id(),
                        date_text(prepared, date)
                    ),
                )
                .rule(OVL_R084),
            );
        }
    }
    // R085 (and the claimants of R082's conflicts).
    let mut represented: Vec<Vec<DateSet>> = prepared
        .sources
        .iter()
        .map(|s| vec![DateSet::EMPTY; s.trips.len()])
        .collect();
    for (target, claims) in &trips {
        for &b in &bindings_by_target[target] {
            let binding = &matches.bindings[b];
            let slot = &mut represented[binding.source as usize][binding.trip as usize];
            for date in binding.dates.iter() {
                let date32 = date as u32;
                let agrees = claims
                    .resolved
                    .get(&date32)
                    .is_some_and(|resolved| resolved.compatible(&selections[b]));
                if agrees {
                    slot.insert(date);
                }
            }
        }
        for &(date, source, trip) in &claims.claimants {
            represented[source as usize][trip as usize].insert(date as usize);
        }
    }
    Resolved { trips, represented }
}

fn source_list(prepared: &Prepared<'_>, sources: &[u32]) -> String {
    sources
        .iter()
        .map(|&s| prepared.sources[s as usize].source_id())
        .collect::<Vec<_>>()
        .join(";")
}
