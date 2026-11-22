//! The availability passes (OVL-R074) and capacity gaps (R075), over every source together.

use std::collections::{HashMap, HashSet};

use jr_model::{DateSet, Diagnostic, Diagnostics, Rule, Severity};

use super::{Binding, Matches, Pending, Unmatched};
use crate::authority::trip_subject;
use crate::prepare::Prepared;

/// A base trip on a date is claimed by the source trips bound to it then. Repeatedly, until a
/// pass accepts nothing: each pending (source trip, date) whose equal-best candidates leave
/// exactly one not claimed by another source trip proposes it; a proposal is accepted when it
/// is the only one for that base trip and date. An accepted proposal binds that one date (no
/// margin, method `+target_availability`) and claims the base trip
/// (`trip_target_availability_resolved`).
///
/// Order of resolution: pending dates in source, trip and date order; proposals are decided per
/// base trip and date, claims made in a pass count from the next pass, so the outcome does not
/// depend on the order inside a pass. Accepted bindings are added in order of the first proposal
/// for their base trip and date.
pub const OVL_R074: Rule = Rule {
    id: "OVL-R074",
    summary: "an ambiguous date takes the only equal-best base trip no other source trip claims",
};

/// A pending date whose every candidate another source trip claims is a capacity gap of the
/// base snapshot (`base_snapshot_capacity_gap`); a source trip without bindings whose every
/// pending date is one is a capacity-gap trip. Every candidate still pending is listed for review.
pub const OVL_R075: Rule = Rule {
    id: "OVL-R075",
    summary: "pending dates whose candidates are all claimed are capacity gaps",
};

pub const RULES: &[Rule] = &[OVL_R074, OVL_R075];

type Who = (u32, u32);
type Target = (u32, u32);

pub fn resolve(
    prepared: &Prepared<'_>,
    matches: &mut Matches,
    pending: Vec<Pending>,
    diagnostics: &mut Diagnostics,
) {
    let horizon = prepared.base.timetable.horizon;
    let pending_targets: HashSet<Target> = pending
        .iter()
        .flat_map(|p| p.candidates.iter().map(move |c| (c.base_trip, p.date)))
        .collect();
    let mut claims: HashMap<Target, Vec<Who>> = HashMap::new();
    let claim = |claims: &mut HashMap<Target, Vec<Who>>, target: Target, who: Who| {
        let entry = claims.entry(target).or_default();
        if !entry.contains(&who) {
            entry.push(who);
        }
    };
    for binding in &matches.bindings {
        for date in binding.dates.iter() {
            let target = (binding.candidate.base_trip, date as u32);
            if pending_targets.contains(&target) {
                claim(&mut claims, target, (binding.source, binding.trip));
            }
        }
    }
    let claimed_by_another = |claims: &HashMap<Target, Vec<Who>>, p: &Pending, base_trip: u32| {
        claims
            .get(&(base_trip, p.date))
            .is_some_and(|who| who.iter().any(|&w| w != (p.source, p.trip)))
    };
    let mut unresolved = pending;
    loop {
        let mut order: Vec<Target> = Vec::new();
        let mut proposals: HashMap<Target, Vec<(usize, usize)>> = HashMap::new();
        for (index, p) in unresolved.iter().enumerate() {
            let unclaimed: Vec<usize> = (0..p.candidates.len())
                .filter(|&c| !claimed_by_another(&claims, p, p.candidates[c].base_trip))
                .collect();
            if let [only] = unclaimed.as_slice() {
                let target = (p.candidates[*only].base_trip, p.date);
                let entry = proposals.entry(target).or_default();
                if entry.is_empty() {
                    order.push(target);
                }
                entry.push((index, *only));
            }
        }
        let mut accepted: HashSet<usize> = HashSet::new();
        for target in order {
            let [(index, candidate)] = proposals[&target].as_slice() else {
                continue;
            };
            let p = &unresolved[*index];
            matches.bindings.push(Binding {
                source: p.source,
                trip: p.trip,
                candidate: p.candidates[*candidate].clone(),
                route_tier: p.route_tier,
                dates: DateSet::from_indices([p.date as usize]),
                margin: None,
                by_availability: true,
            });
            claim(&mut claims, target, (p.source, p.trip));
            accepted.insert(*index);
            let source = &prepared.sources[p.source as usize];
            diagnostics.push(
                Diagnostic::new(
                    "trip_target_availability_resolved",
                    Severity::Info,
                    trip_subject(source, &source.trips[p.trip as usize]),
                    format!(
                        "{} selected the only target not already claimed by another source trip",
                        horizon.date_at(p.date as usize).strftime("%Y%m%d")
                    ),
                )
                .rule(OVL_R074),
            );
        }
        if accepted.is_empty() {
            break;
        }
        unresolved = unresolved
            .into_iter()
            .enumerate()
            .filter(|(index, _)| !accepted.contains(index))
            .map(|(_, p)| p)
            .collect();
    }
    for p in &mut unresolved {
        if p.candidates
            .iter()
            .all(|c| claimed_by_another(&claims, p, c.base_trip))
        {
            p.capacity_gap = true;
            let source = &prepared.sources[p.source as usize];
            let message = format!(
                "{} has no unclaimed equal-best national trip",
                horizon.date_at(p.date as usize).strftime("%Y%m%d")
            );
            diagnostics.push(
                Diagnostic::new(
                    "base_snapshot_capacity_gap",
                    Severity::Warning,
                    trip_subject(source, &source.trips[p.trip as usize]),
                    message.clone(),
                )
                .rule(OVL_R075),
            );
            matches.unmatched.push(Unmatched {
                source: p.source,
                trip: p.trip,
                code: "base_snapshot_capacity_gap",
                message,
            });
        }
    }
    let bound: HashSet<Who> = matches
        .bindings
        .iter()
        .map(|b| (b.source, b.trip))
        .collect();
    let mut gap_trips: Vec<Who> = Vec::new();
    for p in &unresolved {
        let who = (p.source, p.trip);
        if !bound.contains(&who)
            && !gap_trips.contains(&who)
            && unresolved
                .iter()
                .filter(|q| (q.source, q.trip) == who)
                .all(|q| q.capacity_gap)
        {
            gap_trips.push(who);
        }
    }
    for (source, trip) in gap_trips {
        matches.flags[source as usize][trip as usize].capacity_gap = true;
    }
    matches.ambiguous = unresolved;
}
