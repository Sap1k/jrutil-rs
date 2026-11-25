//! The one-to-one assignment of source trips to base trips, date by date, over every source
//! together (OVL-R134), with the availability passes (R074) and capacity gaps (R075).

use std::cmp::Reverse;
use std::collections::HashMap;

use jr_model::{DateSet, Diagnostic, Diagnostics, Rule, Severity};
use rayon::prelude::*;

use super::availability::{self, Who};
use super::decide::{self, TripOptions};
use super::{Binding, Matches, Pending, Unmatched};
use crate::authority::trip_subject;
use crate::prepare::Prepared;

/// On each date, a base trip is bound by at most one source trip of each source. The options of
/// every source trip running that day (R071) are taken best first: by trip tier, then rank
/// (R070), then options of primary candidate routes before structural ones, then the newer
/// revision (R015), then source trip ID, then base trip. A source trip reaching its first
/// option still free takes it; when its free options of that tier and rank are of several
/// meanings it is pending (R074), and equivalent ones it takes too if they are still free once
/// every source trip has had its turn (R071, Q15). A source trip whose better options were
/// taken thus takes its next-best one; one whose every option was taken is a capacity gap that
/// day (R075). The outcome depends on neither the input order nor the thread count. Claims of
/// different sources on one base trip and date are two descriptions of one journey: R082
/// merges or quarantines them.
///
/// Several source trips of one source nearest to one base trip used to bind it together and be
/// quarantined (`overlay_fact_conflict`, B1, verdict Q1), losing the date for all of them.
pub const OVL_R134: Rule = Rule {
    id: "OVL-R134",
    summary: "per date and source, each base trip goes to one source trip, best rank first",
};

pub const RULES: &[Rule] = &[OVL_R134];

/// A job: index into the options.
type Job = u32;

/// An accepted date: (job, date, group, option indexes, margin).
type Accepted = (Job, usize, u32, Vec<usize>, Option<i64>);

/// What a date's assignment decided.
#[derive(Default)]
struct DateOutcome {
    /// (job, group, option indexes, margin).
    accepted: Vec<(Job, u32, Vec<usize>, Option<i64>)>,
    /// R074, in resolution order.
    available: Vec<(Pending, usize)>,
    /// What stays pending, its capacity gap marked (R075).
    pending: Vec<Pending>,
    /// Jobs whose every option was taken.
    lost: Vec<Job>,
    /// Jobs whose date was ambiguous: (job, equal-best count, classes).
    ambiguous: Vec<(Job, usize, usize)>,
}

/// The per-job part of the order (R134): newer revision, source, source trip ID.
struct JobKey<'p> {
    revision: Reverse<Option<jr_model::Date>>,
    source: u32,
    trip_id: &'p str,
}

fn assign_date(
    prepared: &Prepared<'_>,
    options: &[TripOptions],
    keys: &[JobKey<'_>],
    active: &[(Job, u32)],
    date: usize,
) -> DateOutcome {
    let base = &prepared.base;
    let group = |job: Job, group: u32| &options[job as usize].groups[group as usize];
    let mut edges: Vec<(usize, usize)> = Vec::new();
    for (slot, &(job, g)) in active.iter().enumerate() {
        for option in 0..group(job, g).ranked.len() {
            edges.push((slot, option));
        }
    }
    let compare = |&(left_slot, left): &(usize, usize), &(right_slot, right): &(usize, usize)| {
        let (lj, lg) = active[left_slot];
        let (rj, rg) = active[right_slot];
        let (lo, ro) = (group(lj, lg), group(rj, rg));
        let (lc, rc) = (&lo.ranked[left], &ro.ranked[right]);
        let (lk, rk) = (&keys[lj as usize], &keys[rj as usize]);
        (lc.tier, lc.rank, !lo.primary)
            .cmp(&(rc.tier, rc.rank, !ro.primary))
            .then_with(|| {
                (lk.revision, lk.source, lk.trip_id).cmp(&(rk.revision, rk.source, rk.trip_id))
            })
            .then_with(|| lc.base_trip.cmp(&rc.base_trip))
    };
    edges.sort_by(compare);
    let mut outcome = DateOutcome::default();
    let mut claims: HashMap<(u32, u32), Who> = HashMap::new();
    let mut done = vec![false; active.len()];
    let mut pending: Vec<Pending> = Vec::new();
    // (index into the accepted, equivalent options not yet taken).
    let mut spares: Vec<(usize, Vec<usize>)> = Vec::new();
    for (slot, option) in edges {
        let (job, g) = active[slot];
        let options_of = group(job, g);
        let candidate = &options_of.ranked[option];
        let source = options[job as usize].source;
        if done[slot] || claims.contains_key(&(source, candidate.base_trip)) {
            continue;
        }
        done[slot] = true;
        let level = (candidate.tier, candidate.rank);
        let at_level: Vec<usize> = (0..options_of.ranked.len())
            .filter(|&i| (options_of.ranked[i].tier, options_of.ranked[i].rank) == level)
            .collect();
        let free: Vec<usize> = at_level
            .iter()
            .copied()
            .filter(|&i| !claims.contains_key(&(source, options_of.ranked[i].base_trip)))
            .collect();
        let mut classes: Vec<u32> = Vec::new();
        for &i in &free {
            let trip = options_of.ranked[i].base_trip;
            if !classes
                .iter()
                .any(|&class| decide::equivalent(base, class, trip))
            {
                classes.push(trip);
            }
        }
        let who = (options[job as usize].source, options[job as usize].trip);
        if classes.len() > 1 {
            outcome.ambiguous.push((job, at_level.len(), classes.len()));
            pending.push(Pending {
                source: who.0,
                trip: who.1,
                date: date as u32,
                route_tier: options_of.route_tier,
                candidates: at_level
                    .iter()
                    .map(|&i| options_of.ranked[i].clone())
                    .collect(),
                capacity_gap: false,
            });
            continue;
        }
        // One of the equivalent options now; the others once every source trip has had its
        // turn, so that a source trip of their own takes them first.
        claims.insert((source, candidate.base_trip), who);
        let spare: Vec<usize> = free.into_iter().filter(|&i| i != option).collect();
        if !spare.is_empty() {
            spares.push((outcome.accepted.len(), spare));
        }
        // R070: the margin to the first strictly worse candidate of the tier.
        let margin = options_of
            .ranked
            .iter()
            .find(|c| c.tier == level.0 && c.rank > level.1)
            .map(|c| super::score::margin(&level.1, &c.rank));
        outcome.accepted.push((job, g, vec![option], margin));
    }
    for (slot, &(job, _)) in active.iter().enumerate() {
        if !done[slot] {
            outcome.lost.push(job);
        }
    }
    outcome.available = availability::resolve(&mut pending, &mut claims);
    outcome.pending = pending;
    // R071, verdict Q15: equivalent base trips nobody else took are the source trip's too.
    for (at, spare) in spares {
        let (job, g, taken, _) = &mut outcome.accepted[at];
        let options_of = group(*job, *g);
        let who = (options[*job as usize].source, options[*job as usize].trip);
        for i in spare {
            let base_trip = options_of.ranked[i].base_trip;
            if let std::collections::hash_map::Entry::Vacant(slot) =
                claims.entry((who.0, base_trip))
            {
                slot.insert(who);
                taken.push(i);
            }
        }
        taken.sort_unstable();
    }
    outcome
}

/// R134 with R071–R075 over every source trip's options; fills the bindings, the pending and
/// unmatched dates and the flags of `matches`.
pub fn assign(
    prepared: &Prepared<'_>,
    options: Vec<TripOptions>,
    matches: &mut Matches,
    diagnostics: &mut Diagnostics,
) {
    let horizon = prepared.base.timetable.horizon;
    let keys: Vec<JobKey<'_>> = options
        .iter()
        .map(|o| {
            let source = &prepared.sources[o.source as usize];
            let trip = &source.trips[o.trip as usize];
            JobKey {
                revision: Reverse(trip.revision),
                source: o.source,
                trip_id: &source.feed.trips[trip.feed_index as usize].trip_id,
            }
        })
        .collect();
    let mut by_date: Vec<Vec<(Job, u32)>> = vec![Vec::new(); horizon.days()];
    for (job, o) in options.iter().enumerate() {
        for (g, group) in o.groups.iter().enumerate() {
            for date in group.dates.iter() {
                by_date[date].push((job as Job, g as u32));
            }
        }
    }
    let outcomes: Vec<DateOutcome> = by_date
        .par_iter()
        .enumerate()
        .map(|(date, active)| assign_date(prepared, &options, &keys, active, date))
        .collect();

    // Per job, its per-date events in date order: (job, date, kind, diagnostic, unmatched).
    let mut events: Vec<(Job, usize, u8, Diagnostic, Option<Unmatched>)> = Vec::new();
    let mut accepted: Vec<Accepted> = Vec::new();
    let mut available: Vec<(usize, Pending, usize)> = Vec::new();
    let mut ambiguous: Vec<Pending> = Vec::new();
    let mut gaps: Vec<(Job, usize)> = Vec::new();
    let job_of: HashMap<Who, Job> = options
        .iter()
        .enumerate()
        .map(|(job, o)| ((o.source, o.trip), job as Job))
        .collect();
    let subject = |job: Job| {
        let o = &options[job as usize];
        let source = &prepared.sources[o.source as usize];
        trip_subject(source, &source.trips[o.trip as usize])
    };
    let day = |date: usize| horizon.date_at(date).strftime("%Y%m%d").to_string();
    for (date, outcome) in outcomes.into_iter().enumerate() {
        for (job, g, free, margin) in outcome.accepted {
            if free.len() > 1 {
                events.push((
                    job,
                    date,
                    0,
                    Diagnostic::new(
                        "trip_equivalent_tie_expanded",
                        Severity::Info,
                        subject(job),
                        format!(
                            "{} overlays {} semantically equivalent target trips",
                            day(date),
                            free.len()
                        ),
                    )
                    .rule(decide::OVL_R071),
                    None,
                ));
            }
            accepted.push((job, date, g, free, margin));
        }
        for (job, count, classes) in outcome.ambiguous {
            let message = format!(
                "{} has {count} equal-best candidates across {classes} semantic classes",
                day(date)
            );
            let o = &options[job as usize];
            events.push((
                job,
                date,
                1,
                Diagnostic::new(
                    "trip_same_date_ambiguous",
                    Severity::Warning,
                    subject(job),
                    message.clone(),
                )
                .rule(decide::OVL_R071),
                Some(Unmatched {
                    source: o.source,
                    trip: o.trip,
                    code: "trip_same_date_ambiguous",
                    message,
                    date: Some(date as u32),
                }),
            ));
        }
        for job in outcome.lost {
            let message = format!(
                "{} has no national trip within the tolerance that a better-ranked source trip did not take",
                day(date)
            );
            let o = &options[job as usize];
            events.push((
                job,
                date,
                2,
                Diagnostic::new(
                    "base_snapshot_capacity_gap",
                    Severity::Warning,
                    subject(job),
                    message.clone(),
                )
                .rule(OVL_R134),
                Some(Unmatched {
                    source: o.source,
                    trip: o.trip,
                    code: "base_snapshot_capacity_gap",
                    message,
                    date: Some(date as u32),
                }),
            ));
            gaps.push((job, date));
        }
        for (p, candidate) in outcome.available {
            available.push((date, p, candidate));
        }
        ambiguous.extend(outcome.pending);
    }

    // R072: bindings in source and trip order, each collecting its dates.
    accepted.sort_by_key(|&(job, date, ..)| (job, date));
    type Key = (
        u32,
        crate::profile::RouteTier,
        crate::profile::TripTier,
        u32,
        i64,
        i64,
        i64,
    );
    let mut current: Option<Job> = None;
    let mut keys_of: HashMap<Key, usize> = HashMap::new();
    let mut bound = vec![false; options.len()];
    for (job, date, g, free, margin) in accepted {
        if current != Some(job) {
            current = Some(job);
            keys_of.clear();
        }
        bound[job as usize] = true;
        let o = &options[job as usize];
        let group = &o.groups[g as usize];
        for i in free {
            let candidate = &group.ranked[i];
            let rank = candidate.rank;
            let key = (
                candidate.base_trip,
                group.route_tier,
                candidate.tier,
                rank.edits,
                rank.first_departure_delta,
                rank.aggregate_delta,
                rank.duration_delta,
            );
            match keys_of.get(&key) {
                Some(&at) => matches.bindings[at].dates.insert(date),
                None => {
                    keys_of.insert(key, matches.bindings.len());
                    matches.bindings.push(Binding {
                        source: o.source,
                        trip: o.trip,
                        candidate: candidate.clone(),
                        route_tier: group.route_tier,
                        dates: DateSet::from_indices([date]),
                        margin,
                        by_availability: false,
                    });
                }
            }
        }
    }
    // R074: the availability bindings, by date and resolution order.
    for (date, p, candidate) in available {
        let job = job_of[&(p.source, p.trip)];
        bound[job as usize] = true;
        matches.bindings.push(Binding {
            source: p.source,
            trip: p.trip,
            candidate: p.candidates[candidate].clone(),
            route_tier: p.route_tier,
            dates: DateSet::from_indices([date]),
            margin: None,
            by_availability: true,
        });
        events.push((
            job,
            date,
            3,
            Diagnostic::new(
                "trip_target_availability_resolved",
                Severity::Info,
                subject(job),
                format!(
                    "{} selected the only target not already claimed by another source trip",
                    day(date)
                ),
            )
            .rule(availability::OVL_R074),
            None,
        ));
    }
    // R075 on what stays pending.
    ambiguous.sort_by_key(|p| (job_of[&(p.source, p.trip)], p.date));
    for p in &ambiguous {
        if !p.capacity_gap {
            continue;
        }
        let job = job_of[&(p.source, p.trip)];
        let message = format!(
            "{} has no unclaimed equal-best national trip",
            day(p.date as usize)
        );
        events.push((
            job,
            p.date as usize,
            4,
            Diagnostic::new(
                "base_snapshot_capacity_gap",
                Severity::Warning,
                subject(job),
                message.clone(),
            )
            .rule(availability::OVL_R075),
            Some(Unmatched {
                source: p.source,
                trip: p.trip,
                code: "base_snapshot_capacity_gap",
                message,
                date: Some(p.date),
            }),
        ));
        gaps.push((job, p.date as usize));
    }
    // R073 after each job's dates.
    for (job, o) in options.iter().enumerate() {
        if let Some((diagnostic, unmatched)) = o.unmatched(prepared, bound[job]) {
            events.push((job as Job, usize::MAX, 5, diagnostic, Some(unmatched)));
        }
    }
    events.sort_by_key(|&(job, date, kind, ..)| (job, date, kind));
    for (_, _, _, diagnostic, unmatched) in events {
        diagnostics.push(diagnostic);
        matches.unmatched.extend(unmatched);
    }
    // Flags (R073, R075).
    for (job, o) in options.iter().enumerate() {
        let mut flags = o.flags;
        flags.snapshot_gap = !bound[job] && o.snapshot_gap();
        matches.flags[o.source as usize][o.trip as usize] = flags;
    }
    gaps.sort_unstable();
    gaps.dedup();
    let mut gap_jobs: Vec<Job> = gaps.iter().map(|&(job, _)| job).collect();
    gap_jobs.dedup();
    for job in gap_jobs {
        let o = &options[job as usize];
        let every_pending_a_gap = ambiguous
            .iter()
            .filter(|p| (p.source, p.trip) == (o.source, o.trip))
            .all(|p| p.capacity_gap);
        if !bound[job as usize] && every_pending_a_gap {
            matches.flags[o.source as usize][o.trip as usize].capacity_gap = true;
        }
    }
    matches.ambiguous = ambiguous;
}
