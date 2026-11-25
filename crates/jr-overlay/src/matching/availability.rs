//! The availability passes (OVL-R074) and capacity gaps (R075), on one date, after the
//! assignment (R134) has claimed base trips.

use std::collections::HashMap;

use jr_model::Rule;

use super::Pending;

/// A base trip on a date is claimed, for each source, by the source trip of that source the
/// assignment (R134) gave it. Repeatedly, until a pass accepts nothing: each pending (source
/// trip, date) whose equal-best candidates leave exactly one not claimed by another source trip
/// of its source proposes it; a proposal is accepted when it is the only one of its source for
/// that base trip and date. An accepted proposal binds that one date (no margin, method
/// `+target_availability`) and claims the base trip (`trip_target_availability_resolved`).
///
/// Order of resolution: pending dates in source and trip order; proposals are decided per base
/// trip, claims made in a pass count from the next pass, so the outcome does not depend on the
/// order inside a pass. Accepted bindings are added by date, then in order of the first
/// proposal for their base trip.
pub const OVL_R074: Rule = Rule {
    id: "OVL-R074",
    summary: "an ambiguous date takes the only equal-best base trip no other source trip claims",
};

/// A date on which every candidate of a source trip is claimed by another source trip (of its
/// source) is a
/// capacity gap of the base snapshot (`base_snapshot_capacity_gap`): a pending date after
/// R074, or a date whose every option the assignment gave away (R134). A source trip without
/// bindings whose every such date is a gap is a capacity-gap trip. Every candidate still pending
/// is listed for review.
pub const OVL_R075: Rule = Rule {
    id: "OVL-R075",
    summary: "dates whose candidates are all claimed by other source trips are capacity gaps",
};

pub const RULES: &[Rule] = &[OVL_R074, OVL_R075];

/// A source trip: (source, trip).
pub(super) type Who = (u32, u32);

/// R074 and R075 on one date. `claims` maps (source, base trip) to the source trip of that
/// source holding it that day. Returns the accepted proposals in resolution order (pending,
/// candidate index); what stays in `pending` is marked with its capacity gap.
pub(super) fn resolve(
    pending: &mut Vec<Pending>,
    claims: &mut HashMap<(u32, u32), Who>,
) -> Vec<(Pending, usize)> {
    let claimed_by_another = |claims: &HashMap<(u32, u32), Who>, p: &Pending, base_trip: u32| {
        claims
            .get(&(p.source, base_trip))
            .is_some_and(|&who| who != (p.source, p.trip))
    };
    let mut accepted_all = Vec::new();
    loop {
        let mut order: Vec<(u32, u32)> = Vec::new();
        let mut proposals: HashMap<(u32, u32), Vec<(usize, usize)>> = HashMap::new();
        for (index, p) in pending.iter().enumerate() {
            let unclaimed: Vec<usize> = (0..p.candidates.len())
                .filter(|&c| !claimed_by_another(claims, p, p.candidates[c].base_trip))
                .collect();
            if let [only] = unclaimed.as_slice() {
                let target = (p.source, p.candidates[*only].base_trip);
                let entry = proposals.entry(target).or_default();
                if entry.is_empty() {
                    order.push(target);
                }
                entry.push((index, *only));
            }
        }
        let mut accepted: Vec<(usize, usize)> = Vec::new();
        for target in order {
            if let [(index, candidate)] = proposals[&target].as_slice() {
                accepted.push((*index, *candidate));
                let p = &pending[*index];
                claims.insert(target, (p.source, p.trip));
            }
        }
        if accepted.is_empty() {
            break;
        }
        let mut taken: Vec<Option<usize>> = vec![None; pending.len()];
        for &(index, candidate) in &accepted {
            taken[index] = Some(candidate);
        }
        let mut kept = Vec::with_capacity(pending.len());
        let mut moved: Vec<Option<(Pending, usize)>> = vec![None; pending.len()];
        for (index, p) in std::mem::take(pending).into_iter().enumerate() {
            match taken[index] {
                Some(candidate) => moved[index] = Some((p, candidate)),
                None => kept.push(p),
            }
        }
        // In resolution order.
        for (index, _) in accepted {
            accepted_all.push(moved[index].take().expect("an accepted proposal"));
        }
        *pending = kept;
    }
    for p in pending.iter_mut() {
        p.capacity_gap = p
            .candidates
            .iter()
            .all(|c| claimed_by_another(claims, p, c.base_trip));
    }
    accepted_all
}
