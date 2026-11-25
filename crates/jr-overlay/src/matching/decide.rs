//! What one source trip may bind on each of its dates (OVL-R071, R133) and why it stays
//! unmatched (R073). Which candidate a date takes is decided over every source trip together
//! (R134, `assignment.rs`).

use std::collections::BTreeMap;

use jr_model::{DateSet, Diagnostic, Rule, Severity};

use super::TripFlags;
use super::candidates::{self, Candidate};
use crate::authority::trip_subject;
use crate::prepare::{BaseView, Prepared};
use crate::profile::RouteTier;

/// On every date the source trip runs, its options are the candidates running that day: the
/// primary ones if any, else the structural ones (route tier `structural_trip_evidence`), best
/// first by tier, then rank (R070). Of the options the assignment leaves it (R134), those of
/// the best tier and rank are the best. When they fall into several equivalence classes (the
/// base trip's line, or its route when it has none, with its exact places and times) the date
/// is ambiguous (`trip_same_date_ambiguous`) and pending; otherwise the first best option takes
/// the date, with the margin to the first strictly worse candidate of its tier, and so does each
/// other best option no source trip has taken once all have had their turn
/// (`trip_equivalent_tie_expanded` when more than one, verdict Q15): a national duplicate goes
/// first to a source trip of its own.
///
/// The source runs a trip on its own calendar; the base trip it is may differ by date.
pub const OVL_R071: Rule = Rule {
    id: "OVL-R071",
    summary: "per date: the best tier and rank; equivalent ties expand, distinct ones pend",
};

/// Accepted candidates collect their dates: one binding per (base trip, route tier, trip tier,
/// edits, first-departure delta, sum, duration delta), first seen first. A base trip is bound
/// by at most one source trip on a date (R134).
pub const OVL_R072: Rule = Rule {
    id: "OVL-R072",
    summary: "accepted candidates collect dates into bindings",
};

/// A source trip accepted on no date gets the first reason that applies: an unplaced call
/// (`trip_stop_unresolved`), no base trips on its candidate routes
/// (`trip_route_candidate_unresolved`), no candidate under any tier (`trip_signature_unresolved`),
/// candidates that all depart further from it than the tolerance (R133,
/// `trip_departure_deviation_exceeded`), candidates that run on none of its dates
/// (`base_snapshot_gap`), else `trip_validity_unresolved`. An active trip without calls is
/// `trip_call_pattern_unavailable`.
pub const OVL_R073: Rule = Rule {
    id: "OVL-R073",
    summary: "unmatched trips are classified by the first reason that applies",
};

/// A base trip whose first departure deviates from the source trip's by more than the source
/// profile's `trips.maximum_first_departure_delta_seconds` is not a candidate. A candidate
/// without a comparable first departure is kept (nothing to measure).
///
/// The nearest base trip in time is the likely twin only within a tolerance: on the golden
/// slice the deviations of accepted bindings fall from about 700 per minute up to seven
/// minutes to under 80 at eight, and the bindings beyond are the neighbouring run of a dense
/// line (often 10, 15, 20 or 30 minutes off), which another source trip is the twin of.
pub const OVL_R133: Rule = Rule {
    id: "OVL-R133",
    summary: "a base trip departing beyond the profile's tolerance is not a candidate",
};

pub const RULES: &[Rule] = &[OVL_R071, OVL_R072, OVL_R073, OVL_R133];

/// Why a source trip, or one of its dates, is unmatched (R073, R071, R075, R134).
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

/// One source trip's options on a set of its dates (R071, R133).
#[derive(Clone, Debug)]
pub struct DateOptions {
    pub dates: DateSet,
    pub route_tier: RouteTier,
    /// Whether they are of the primary candidate routes.
    pub primary: bool,
    /// The candidates running on these dates, within the tolerance, best first (tier, rank,
    /// then candidate order).
    pub ranked: Vec<Candidate>,
}

/// What R073 needs to know of a trip once the assignment is done.
#[derive(Clone, Copy, Debug, Default)]
struct Evidence {
    unplaced_call: bool,
    route_trips: usize,
    any_candidate: bool,
    any_within_tolerance: bool,
}

/// One source trip's options.
#[derive(Debug, Default)]
pub struct TripOptions {
    pub source: u32,
    pub trip: u32,
    pub groups: Vec<DateOptions>,
    pub flags: TripFlags,
    /// `trip_call_pattern_unavailable`: an active trip without calls.
    no_calls: bool,
    evidence: Evidence,
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

/// R065–R071 and R133 for one source trip: its options per date.
pub fn options(prepared: &Prepared<'_>, source_index: u32, trip_index: u32) -> TripOptions {
    let base = &prepared.base;
    let source = &prepared.sources[source_index as usize];
    let trip = &source.trips[trip_index as usize];
    let mut result = TripOptions {
        source: source_index,
        trip: trip_index,
        ..TripOptions::default()
    };
    if trip.dates.is_empty() {
        return result;
    }
    let calls = source.trip_calls(trip);
    if calls.is_empty() {
        result.no_calls = true;
        return result;
    }
    let found = candidates::candidates(base, source, trip);
    let tolerance = i64::from(source.profile.trips.maximum_first_departure_delta_seconds);
    let within = |candidate: &&Candidate| {
        candidate.rank.first_departure_delta == super::MISSING_DELTA
            || candidate.rank.first_departure_delta <= tolerance
    };
    let primary: Vec<&Candidate> = found.primary.iter().filter(within).collect();
    let structural: Vec<&Candidate> = found.structural.iter().filter(within).collect();
    result.evidence = Evidence {
        unplaced_call: calls.iter().any(|call| call.place.is_none()),
        route_trips: found.route_trips,
        any_candidate: !found.primary.is_empty() || !found.structural.is_empty(),
        any_within_tolerance: !primary.is_empty() || !structural.is_empty(),
    };
    // Dates grouped by the candidates running on them, in order of their first date.
    let mut groups: BTreeMap<(Vec<usize>, Vec<usize>), DateSet> = BTreeMap::new();
    let mut order: Vec<(Vec<usize>, Vec<usize>)> = Vec::new();
    for date in trip.dates.iter() {
        let runs = |list: &[&Candidate]| -> Vec<usize> {
            (0..list.len())
                .filter(|&i| base.trips[list[i].base_trip as usize].dates.contains(date))
                .collect()
        };
        let key = (runs(&primary), runs(&structural));
        if key.0.is_empty() && key.1.is_empty() {
            continue;
        }
        result.flags.shared_date_candidate = true;
        result.flags.pattern_compatible = true;
        let dates = groups.entry(key.clone()).or_insert_with(|| {
            order.push(key);
            DateSet::EMPTY
        });
        dates.insert(date);
    }
    for key in order {
        let dates = groups[&key];
        let (route_tier, is_primary, list, active) = if !key.0.is_empty() {
            (found.primary_tier, true, &primary, &key.0)
        } else {
            (
                RouteTier::StructuralTripEvidence,
                false,
                &structural,
                &key.1,
            )
        };
        let mut ranked: Vec<Candidate> = active.iter().map(|&i| list[i].clone()).collect();
        ranked.sort_by_key(|c| (c.tier, c.rank));
        result.groups.push(DateOptions {
            dates,
            route_tier,
            primary: is_primary,
            ranked,
        });
    }
    result
}

impl TripOptions {
    /// R073 for a trip the assignment bound on no date, else `None`.
    pub fn unmatched(
        &self,
        prepared: &Prepared<'_>,
        bound: bool,
    ) -> Option<(Diagnostic, Unmatched)> {
        let source = &prepared.sources[self.source as usize];
        let trip = &source.trips[self.trip as usize];
        if trip.dates.is_empty() || (bound && !self.no_calls) {
            return None;
        }
        let evidence = &self.evidence;
        let (code, message) = if self.no_calls {
            (
                "trip_call_pattern_unavailable",
                "The active source trip has no eligible parsed call pattern",
            )
        } else if evidence.unplaced_call {
            (
                "trip_stop_unresolved",
                "At least one complete-pattern stop place is unresolved",
            )
        } else if evidence.route_trips == 0 {
            (
                "trip_route_candidate_unresolved",
                "No target trips exist on the candidate routes inside the GVD",
            )
        } else if !evidence.any_candidate {
            (
                "trip_signature_unresolved",
                "No candidate has an exact configured stop/time signature",
            )
        } else if !evidence.any_within_tolerance {
            (
                "trip_departure_deviation_exceeded",
                "Every candidate departs further from the source trip than the profile's tolerance",
            )
        } else if !self.flags.shared_date_candidate {
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
        let rule = if code == "trip_departure_deviation_exceeded" {
            OVL_R133
        } else {
            OVL_R073
        };
        let subject = trip_subject(source, trip);
        Some((
            Diagnostic::new(code, Severity::Warning, subject, message.to_owned()).rule(rule),
            Unmatched {
                source: self.source,
                trip: self.trip,
                code,
                message: message.to_owned(),
                date: None,
            },
        ))
    }

    /// R073: `base_snapshot_gap` sets its flag.
    pub fn snapshot_gap(&self) -> bool {
        !self.no_calls
            && !self.evidence.unplaced_call
            && self.evidence.route_trips > 0
            && self.evidence.any_within_tolerance
            && !self.flags.shared_date_candidate
    }
}
