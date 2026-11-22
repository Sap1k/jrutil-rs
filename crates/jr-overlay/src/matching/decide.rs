//! The decision per date of one source trip (OVL-R071), its bindings (R072) and why it stays
//! unmatched (R073).

use std::collections::HashMap;

use jr_model::{Diagnostic, Rule, Severity};

use super::candidates::{self, Candidate};
use super::score;
use super::{Binding, Pending, TripFlags};
use crate::authority::trip_subject;
use crate::prepare::{BaseView, Prepared};
use crate::profile::RouteTier;

/// On every date the source trip runs: the candidates running that day, the primary ones if any,
/// else the structural ones (route tier `structural_trip_evidence`). Of the best tier, those
/// with the best rank (R070) are the best. When they fall into several equivalence classes (the
/// base trip's line, or its route when it has none, with its exact places and times) the date
/// is ambiguous (`trip_same_date_ambiguous`) and pending; otherwise every best candidate takes
/// the date (`trip_equivalent_tie_expanded` when more than one, verdict Q15), with the margin to
/// the first strictly worse candidate.
///
/// The source runs a trip on its own calendar; the base trip it is may differ by date.
pub const OVL_R071: Rule = Rule {
    id: "OVL-R071",
    summary: "per date: the best tier and rank; equivalent ties expand, distinct ones pend",
};

/// Accepted candidates collect their dates: one binding per (base trip, route tier, trip tier,
/// edits, first-departure delta, sum, duration delta), first seen first. Different source trips
/// may bind one base trip on one date (no one-to-one assignment, verdict Q1).
pub const OVL_R072: Rule = Rule {
    id: "OVL-R072",
    summary: "accepted candidates collect dates into bindings; no one-to-one assignment",
};

/// A source trip accepted on no date gets the first reason that applies: an unplaced call
/// (`trip_stop_unresolved`), no base trips on its candidate routes
/// (`trip_route_candidate_unresolved`), no candidate under any tier (`trip_signature_unresolved`),
/// candidates that run on none of its dates (`base_snapshot_gap`), else
/// `trip_validity_unresolved`. An active trip without calls is `trip_call_pattern_unavailable`.
pub const OVL_R073: Rule = Rule {
    id: "OVL-R073",
    summary: "unmatched trips are classified by the first reason that applies",
};

pub const RULES: &[Rule] = &[OVL_R071, OVL_R072, OVL_R073];

/// Why a source trip, or one of its dates, is unmatched (R073, R071, R075).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unmatched {
    pub source: u32,
    pub trip: u32,
    /// The diagnostic code.
    pub code: &'static str,
    /// The diagnostic message.
    pub message: String,
    /// The date it concerns, for a per-date reason.
    pub date: Option<u32>,
}

/// One source trip's result.
#[derive(Debug, Default)]
pub struct Outcome {
    pub bindings: Vec<Binding>,
    pub pending: Vec<Pending>,
    pub unmatched: Vec<Unmatched>,
    pub diagnostics: Vec<Diagnostic>,
    pub flags: TripFlags,
}

/// R072: what makes two accepted candidates one binding.
type BindingKey = (u32, RouteTier, crate::profile::TripTier, u32, i64, i64, i64);

/// A date's decision over the active candidates (indexes into the chosen list).
#[derive(Clone)]
enum Decision {
    None,
    Accepted {
        tier: RouteTier,
        primary: bool,
        best: Vec<usize>,
        margin: Option<i64>,
    },
    Ambiguous {
        tier: RouteTier,
        primary: bool,
        best: Vec<usize>,
        classes: usize,
    },
}

/// R071: whether two base trips are equivalent (line or route, exact places and times).
pub fn equivalent(base: &BaseView<'_>, left: u32, right: u32) -> bool {
    let (a, b) = (&base.trips[left as usize], &base.trips[right as usize]);
    let line = |trip: &crate::prepare::BaseTrip| match &base.route(trip.route).line {
        Some(line) => (Some(line.clone()), None),
        None => (None, Some(trip.route)),
    };
    line(a) == line(b)
        && a.pattern == b.pattern
        && a.arrivals == b.arrivals
        && a.departures == b.departures
}

fn choose(
    base: &BaseView<'_>,
    list: &[Candidate],
    active: &[usize],
) -> Option<(Vec<usize>, Option<i64>, usize)> {
    if active.is_empty() {
        return None;
    }
    let best_tier = active.iter().map(|&i| list[i].tier).min()?;
    let mut ranked: Vec<usize> = active
        .iter()
        .copied()
        .filter(|&i| list[i].tier == best_tier)
        .collect();
    ranked.sort_by(|&a, &b| list[a].rank.cmp(&list[b].rank));
    let first = list[ranked[0]].rank;
    let best: Vec<usize> = ranked
        .iter()
        .copied()
        .take_while(|&i| list[i].rank == first)
        .collect();
    let mut classes: Vec<u32> = Vec::new();
    for &i in &best {
        let trip = list[i].base_trip;
        if !classes.iter().any(|&class| equivalent(base, class, trip)) {
            classes.push(trip);
        }
    }
    let margin = ranked
        .iter()
        .find(|&&i| list[i].rank > first)
        .map(|&i| score::margin(&first, &list[i].rank));
    Some((best, margin, classes.len()))
}

/// R065–R073 for one source trip.
pub fn match_trip(prepared: &Prepared<'_>, source_index: u32, trip_index: u32) -> Outcome {
    let base = &prepared.base;
    let source = &prepared.sources[source_index as usize];
    let trip = &source.trips[trip_index as usize];
    let mut outcome = Outcome::default();
    if trip.dates.is_empty() {
        return outcome;
    }
    let subject = trip_subject(source, trip);
    let calls = source.trip_calls(trip);
    let unmatched =
        |outcome: &mut Outcome, rule, code: &'static str, message: String, date: Option<u32>| {
            outcome.diagnostics.push(
                Diagnostic::new(code, Severity::Warning, subject.clone(), message.clone())
                    .rule(rule),
            );
            outcome.unmatched.push(Unmatched {
                source: source_index,
                trip: trip_index,
                code,
                message,
                date,
            });
        };
    if calls.is_empty() {
        unmatched(
            &mut outcome,
            OVL_R073,
            "trip_call_pattern_unavailable",
            "The active source trip has no eligible parsed call pattern".to_owned(),
            None,
        );
        return outcome;
    }
    let found = candidates::candidates(base, source, trip);
    let horizon = base.timetable.horizon;
    let mut memo: HashMap<(Vec<usize>, Vec<usize>), Decision> = HashMap::new();
    let mut keys: HashMap<BindingKey, usize> = HashMap::new();
    let mut matched = false;
    for date in trip.dates.iter() {
        let runs = |list: &[Candidate]| -> Vec<usize> {
            (0..list.len())
                .filter(|&i| base.trips[list[i].base_trip as usize].dates.contains(date))
                .collect()
        };
        let primary = runs(&found.primary);
        let structural = runs(&found.structural);
        if !primary.is_empty() || !structural.is_empty() {
            outcome.flags.shared_date_candidate = true;
        }
        let decision = memo
            .entry((primary.clone(), structural.clone()))
            .or_insert_with(|| {
                let (tier, is_primary, list, active) = if !primary.is_empty() {
                    (found.primary_tier, true, &found.primary, &primary)
                } else {
                    (
                        RouteTier::StructuralTripEvidence,
                        false,
                        &found.structural,
                        &structural,
                    )
                };
                match choose(base, list, active) {
                    None => Decision::None,
                    Some((best, _, classes)) if classes > 1 => Decision::Ambiguous {
                        tier,
                        primary: is_primary,
                        best,
                        classes,
                    },
                    Some((best, margin, _)) => Decision::Accepted {
                        tier,
                        primary: is_primary,
                        best,
                        margin,
                    },
                }
            })
            .clone();
        if !primary.is_empty() || !structural.is_empty() {
            outcome.flags.pattern_compatible = true;
        }
        let date_text = horizon.date_at(date).strftime("%Y%m%d").to_string();
        match decision {
            Decision::None => {}
            Decision::Accepted {
                tier,
                primary: is_primary,
                best,
                margin,
            } => {
                matched = true;
                let list = if is_primary {
                    &found.primary
                } else {
                    &found.structural
                };
                if best.len() > 1 {
                    outcome.diagnostics.push(
                        Diagnostic::new(
                            "trip_equivalent_tie_expanded",
                            Severity::Info,
                            subject.clone(),
                            format!(
                                "{date_text} overlays {} semantically equivalent target trips",
                                best.len()
                            ),
                        )
                        .rule(OVL_R071),
                    );
                }
                for index in best {
                    let candidate = &list[index];
                    let rank = candidate.rank;
                    let key = (
                        candidate.base_trip,
                        tier,
                        candidate.tier,
                        rank.edits,
                        rank.first_departure_delta,
                        rank.aggregate_delta,
                        rank.duration_delta,
                    );
                    match keys.get(&key) {
                        Some(&at) => outcome.bindings[at].dates.insert(date),
                        None => {
                            keys.insert(key, outcome.bindings.len());
                            outcome.bindings.push(Binding {
                                source: source_index,
                                trip: trip_index,
                                candidate: candidate.clone(),
                                route_tier: tier,
                                dates: jr_model::DateSet::from_indices([date]),
                                margin,
                                by_availability: false,
                            });
                        }
                    }
                }
            }
            Decision::Ambiguous {
                tier,
                primary: is_primary,
                best,
                classes,
            } => {
                let list = if is_primary {
                    &found.primary
                } else {
                    &found.structural
                };
                let message = format!(
                    "{date_text} has {} equal-best candidates across {classes} semantic classes",
                    best.len()
                );
                unmatched(
                    &mut outcome,
                    OVL_R071,
                    "trip_same_date_ambiguous",
                    message,
                    Some(date as u32),
                );
                outcome.pending.push(Pending {
                    source: source_index,
                    trip: trip_index,
                    date: date as u32,
                    route_tier: tier,
                    candidates: best.into_iter().map(|i| list[i].clone()).collect(),
                    capacity_gap: false,
                });
            }
        }
    }
    if !matched {
        let (code, message) = if calls.iter().any(|call| call.place.is_none()) {
            (
                "trip_stop_unresolved",
                "At least one complete-pattern stop place is unresolved",
            )
        } else if found.route_trips == 0 {
            (
                "trip_route_candidate_unresolved",
                "No target trips exist on the candidate routes inside the GVD",
            )
        } else if found.primary.is_empty() && found.structural.is_empty() {
            (
                "trip_signature_unresolved",
                "No candidate has an exact configured stop/time signature",
            )
        } else if !outcome.flags.shared_date_candidate {
            outcome.flags.snapshot_gap = true;
            (
                "base_snapshot_gap",
                "Compatible target evidence exists, but no target trip operates on a shared source date",
            )
        } else {
            (
                "trip_validity_unresolved",
                "Signature candidates exist, but none is uniquely active on a shared operating date",
            )
        };
        unmatched(&mut outcome, OVL_R073, code, message.to_owned(), None);
    }
    outcome
}
