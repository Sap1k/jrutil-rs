//! Projection (OVL-R077–R123): what the prepared sources, trip-set authority and the bindings
//! mean for the timetable, as one batch of edits, with the review reports and a summary.
//!
//! The batch is built against the base timetable in this order: source agencies, derived
//! routes and their line keys, source-native places, source posts, place names and positions,
//! then per base trip in handle order its parts (splits, services, calls, routes, headsigns),
//! the added trips in ID order, route presentation, transfers, keys and call keys. Per base trip
//! and per source trip the work is computed in parallel; the batch is assembled sequentially.

mod additions;
mod batch;
mod capability;
mod claims;
mod headsign;
mod keys;
mod report;
mod selection;
mod slices;
mod stops;
mod summary;
mod transfers;

use jr_model::{DateSet, Diagnostics, Edits, Rule};

pub use summary::Summary;

use crate::authority::Authority;
use crate::matching::Matches;
use crate::prepare::Prepared;
use crate::report::ReportTable;

/// The rules of projection, module by module.
pub const RULES: &[&[Rule]] = &[
    capability::RULES,
    stops::RULES,
    selection::RULES,
    claims::RULES,
    slices::RULES,
    headsign::RULES,
    additions::RULES,
    batch::RULES,
    transfers::RULES,
    keys::RULES,
    report::RULES,
];

/// What projection returns.
#[derive(Debug)]
pub struct Projected {
    pub edits: Edits,
    pub reports: Vec<ReportTable>,
    pub summary: Summary,
    /// Per source, per trip: the days it is accepted (R120, R121).
    pub accepted: Vec<Vec<DateSet>>,
}

/// R077–R123.
pub fn project(
    prepared: &Prepared<'_>,
    authority: &Authority,
    matches: &Matches,
    diagnostics: &mut Diagnostics,
) -> Projected {
    let stops = stops::accepted(prepared, authority, matches);
    let selections = selection::selections(prepared, matches, &stops);
    let resolved = claims::resolve(prepared, authority, matches, &selections, diagnostics);
    let mut headsigns = headsign::Headsigns::new(prepared, authority);
    let plans = slices::plan(prepared, authority, &resolved, &mut headsigns, diagnostics);
    let added = additions::plan(prepared, authority, &resolved, &mut headsigns, diagnostics);
    let built = batch::build(
        prepared,
        authority,
        matches,
        &stops,
        &selections,
        &plans,
        &added,
        diagnostics,
    );
    let context = batch::Context {
        prepared,
        selections: &selections,
        authority,
        matches,
        stops: &stops,
        outputs: &built.outputs,
        of_base: &built.of_base,
        of_addition: &built.of_addition,
        additions: &added,
        handles: &built.post_handles,
        used_posts: &built.used_posts,
        derived: &built.derived_handles,
    };
    let (reports, accepted) = report::reports(&context, &built, &headsigns);
    let mut summary = built.summary.clone();
    summary.count(diagnostics);
    Projected {
        edits: built.edits,
        reports,
        summary,
        accepted,
    }
}

/// R120: the diagnostics that stand once every decision is final.
pub fn effective(
    prepared: &Prepared<'_>,
    matches: &Matches,
    authority: &Authority,
    accepted: &[Vec<DateSet>],
    diagnostics: Diagnostics,
) -> Diagnostics {
    let mut kept = Diagnostics::new();
    kept.extend(report::effective(
        prepared,
        matches,
        authority,
        accepted,
        diagnostics.iter().cloned().collect(),
    ));
    kept
}
