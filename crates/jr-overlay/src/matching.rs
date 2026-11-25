//! Trip matching (OVL-R065–R076, R133, R134): each active source trip, date by date, to the base
//! trips it is. A source trip's candidates within the time tolerance (R133) are found in
//! parallel; then, date by date over every source together, each base trip goes to one source
//! trip of each source, best rank first, and the others take their next-best candidate (R134,
//! B1 fixed, verdict Q1). Equivalent ties expand (Q15); equal-best candidates of different
//! meaning are left to the availability passes (R074).

mod assignment;
mod availability;
pub mod candidates;
mod coverage;
mod decide;
pub(crate) mod report;
pub mod score;

use jr_model::{DateSet, Diagnostics, Rule};
use rayon::prelude::*;

pub use candidates::{Candidate, Token};
pub use coverage::{Coverage, ModeCoverage};
pub use decide::Unmatched;
pub use score::{MISSING_DELTA, MISSING_SUM, Rank};

use crate::authority::Authority;
use crate::prepare::Prepared;
use crate::profile::RouteTier;
use crate::report::ReportTable;

/// The rules of matching.
pub const RULES: &[&[Rule]] = &[
    candidates::RULES,
    score::RULES,
    decide::RULES,
    assignment::RULES,
    availability::RULES,
    coverage::RULES,
];

/// A source trip bound to a base trip on some dates (R072).
#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    pub source: u32,
    /// Index into the source's `trips`.
    pub trip: u32,
    /// The base trip, its tier, alignment and rank.
    pub candidate: Candidate,
    /// The route tier of the candidate set it came from (R071).
    pub route_tier: RouteTier,
    /// The dates it holds.
    pub dates: DateSet,
    /// R070: the margin to the first strictly worse candidate when it was first accepted.
    pub margin: Option<i64>,
    /// R074: decided by the availability pass (one date).
    pub by_availability: bool,
}

impl Binding {
    /// The method reports carry: `<route tier>+<trip tier>`.
    pub fn method(&self) -> String {
        format!(
            "{}+{}",
            self.route_tier.as_str(),
            self.candidate.tier.as_str()
        )
    }
}

/// Equal-best candidates of different meaning on one date (R071), left to R074.
#[derive(Clone, Debug, PartialEq)]
pub struct Pending {
    pub source: u32,
    pub trip: u32,
    /// Day offset in the horizon.
    pub date: u32,
    pub route_tier: RouteTier,
    pub candidates: Vec<Candidate>,
    /// R075: every candidate was claimed by another source trip.
    pub capacity_gap: bool,
}

/// What a source trip's matching found besides bindings (R073, R075, R122 populations).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TripFlags {
    /// Some candidate runs on one of its dates.
    pub shared_date_candidate: bool,
    /// Some date had candidates of the chosen route tier.
    pub pattern_compatible: bool,
    /// R073 `base_snapshot_gap`.
    pub snapshot_gap: bool,
    /// R075: every pending date a capacity gap, no binding.
    pub capacity_gap: bool,
}

/// The outcome of matching.
#[derive(Debug, Default)]
pub struct Matches {
    /// R072, R074: in source and trip order, then the availability bindings by date and
    /// resolution order.
    pub bindings: Vec<Binding>,
    /// R075: what stayed ambiguous after R074, in pending order.
    pub ambiguous: Vec<Pending>,
    /// R073, R075: the unmatched reasons, in the order they were found.
    pub unmatched: Vec<Unmatched>,
    /// Per source, per trip.
    pub flags: Vec<Vec<TripFlags>>,
    /// R076.
    pub coverage: Vec<Coverage>,
}

impl Matches {
    /// The matching reports (the unmatched reasons are completed by projection).
    pub fn reports(&self, prepared: &Prepared<'_>) -> Vec<ReportTable> {
        vec![
            report::trip_candidate_scores(prepared, self),
            report::ambiguous_trip_candidates(prepared, self),
            report::pattern_edits(prepared, self),
        ]
    }
}

/// R065–R076, R133, R134.
pub fn match_trips(
    prepared: &Prepared<'_>,
    authority: &Authority,
    diagnostics: &mut Diagnostics,
) -> Matches {
    let jobs: Vec<(u32, u32)> = prepared
        .sources
        .iter()
        .enumerate()
        .flat_map(|(source, prepared)| {
            (0..prepared.trips.len() as u32).map(move |trip| (source as u32, trip))
        })
        .collect();
    let options: Vec<decide::TripOptions> = jobs
        .par_iter()
        .map(|&(source, trip)| decide::options(prepared, source, trip))
        .collect();
    let mut matches = Matches {
        flags: prepared
            .sources
            .iter()
            .map(|s| vec![TripFlags::default(); s.trips.len()])
            .collect(),
        ..Matches::default()
    };
    assignment::assign(prepared, options, &mut matches, diagnostics);
    matches.coverage = coverage::coverage(prepared, authority, &matches);
    matches
}
