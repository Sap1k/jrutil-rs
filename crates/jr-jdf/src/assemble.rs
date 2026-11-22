//! Assembling the merged road facts into a `Timetable` (JDF-R405–R473 of
//! `docs/rules/jdf-convert.md`).
//!
//! A parity port of the F# `jdf-to-bundle` conversion with post estimation off: what F#
//! publishes is published, its crashes are diagnostics that drop the affected fact, and the
//! semantic gaps it has stay (verdicts). The order: the line facts of every version (public line,
//! reviewed transport mode, route type and colours) and the output routes they form (`lines`);
//! each trip's published calls (`calls`); the places and boarding points those calls use
//! (`places`); agencies, routes and trips; notes, features, claims and restrictions
//! (`semantics`); the national keys (`keys`).

mod calls;
mod keys;
mod lines;
mod places;
mod policy;
mod post_facts;
mod semantics;

use std::collections::BTreeMap;

use jr_formats::geodata::route_rules::TransportModeRule;
use jr_model::timetable::Violation;
use jr_model::{Diagnostics, Feed, Rule, Severity, Timetable};
use serde::Serialize;

pub use policy::Policy;
pub use post_facts::PostFacts;

use crate::merge::Merged;

/// Every rule the assembly implements.
pub const RULES: &[Rule] = &[
    lines::R405,
    lines::R406,
    lines::R407,
    lines::R408,
    places::R415,
    places::R416,
    places::R417,
    places::R418,
    places::R419,
    places::R420,
    places::R421,
    places::R422,
    places::R423,
    places::R425,
    lines::R426,
    lines::R427,
    lines::R428,
    lines::R429,
    lines::R430,
    calls::R433,
    calls::R434,
    calls::R435,
    calls::R436,
    calls::R437,
    calls::R438,
    calls::R440,
    calls::R441,
    calls::R442,
    calls::R443,
    calls::R444,
    calls::R445,
    calls::R446,
    calls::R448,
    calls::R449,
    calls::R451,
    semantics::R452,
    semantics::R453,
    semantics::R454,
    semantics::R455,
    semantics::R457,
    semantics::R458,
    semantics::R460,
    semantics::R461,
    semantics::R462,
    semantics::R463,
    semantics::R464,
    semantics::R465,
    keys::R466,
    R469,
    calls::R473,
    lines::R478,
];

/// The feed version is the merged export's: its format version and the build's reference date,
/// which dates the merged export.
pub const R469: Rule = Rule {
    id: "JDF-R469",
    summary: "The source version is jdf-<format version>:<reference date>",
};

#[derive(Debug, thiserror::Error)]
pub enum AssembleError {
    /// The timetable built breaks an invariant: a defect of the assembly, not of the data.
    #[error("the assembled timetable breaks {} invariants, first: {}", .0.len(), .0[0])]
    Invariant(Vec<Violation>),
    /// Equally specific transport-mode rules (R406): the rules file needs a review.
    #[error("equally specific transport-mode rules on lines {lines:?}")]
    RuleTie { lines: Vec<u64> },
}

/// What the assembly reads.
#[derive(Debug)]
pub struct AssembleInput<'a> {
    pub merged: Merged,
    /// Reviewed corrections of a line's transport mode (jdf-convert Stage 2).
    pub transport_modes: &'a [TransportModeRule],
    pub jdf: &'a crate::Policy,
}

#[derive(Debug)]
pub struct Assembled {
    pub timetable: Timetable,
    /// One per line version, in the merge's order: what the filtered feed's line selection
    /// reads and the timetable does not carry.
    pub lines: Vec<crate::filtered::LineVersion>,
    /// What the boarding-post estimator reads and the timetable does not carry.
    pub post_facts: PostFacts,
    pub summary: Summary,
}

/// Counts of what was assembled and of the diagnostics the assembly reported, by code.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub stops: usize,
    pub boarding_points: usize,
    pub agencies: usize,
    pub routes: usize,
    pub trips: usize,
    pub calls: usize,
    pub notes: usize,
    pub assignments: usize,
    pub claims: usize,
    pub restrictions: usize,
    pub keys: usize,
    /// Warnings and errors (dropped or unresolved facts) by code.
    pub dropped: BTreeMap<String, usize>,
}

/// Assembles the merged road facts into a timetable.
pub fn assemble(
    input: AssembleInput<'_>,
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> Result<Assembled, AssembleError> {
    let mut local = Diagnostics::new();
    let merged = input.merged;
    let mut timetable = Timetable::new(Feed::Jdf, merged.horizon);
    // R469.
    timetable.source_version = Some(
        format!(
            "jdf-{}:{}",
            policy.feed.source_format_version,
            merged.reference_date.strftime("%Y%m%d")
        )
        .into(),
    );

    let lines = lines::prepare(&merged, input.transport_modes, policy, &mut local)?;
    let plans = calls::plan(&merged, policy, &mut timetable, &mut local);
    let points = places::build(&merged, &plans, policy, &mut timetable, &mut local);
    let routes = lines::build(&merged, &lines, &plans, policy, &mut timetable, &mut local);
    let published = calls::build(&merged, &plans, &routes, &points, policy, &mut timetable);
    semantics::build(
        &merged,
        &lines,
        &routes,
        &published,
        input.jdf,
        policy,
        &mut timetable,
        &mut local,
    );
    keys::build(
        &merged,
        &routes,
        &published,
        policy,
        &mut timetable,
        &mut local,
    );

    let violations = timetable.check();
    if !violations.is_empty() {
        return Err(AssembleError::Invariant(violations));
    }
    let mut dropped = BTreeMap::new();
    for diagnostic in local.iter() {
        if diagnostic.severity != Severity::Info {
            *dropped.entry(diagnostic.code.to_string()).or_default() += 1;
        }
    }
    let summary = Summary {
        stops: timetable.places.len(),
        boarding_points: timetable.boarding_points.len(),
        agencies: timetable.agencies.len(),
        routes: timetable.routes.len(),
        trips: timetable.trips.len(),
        calls: timetable.trips.rows().iter().map(|t| t.calls.len()).sum(),
        notes: timetable.notes.len(),
        assignments: timetable.assignments.len(),
        claims: timetable.connection_claims.len(),
        restrictions: timetable.restrictions.len(),
        keys: timetable.keys.len(),
        dropped,
    };
    diagnostics.append(local);
    let lines = merged
        .versions
        .iter()
        .zip(&routes)
        .map(|(version, &route)| {
            let name = |agency: usize| merged.agencies[agency].name.clone();
            let mut operators = vec![name(version.agency)];
            for alternation in &version.agency_alternations {
                let alternate = name(alternation.agency);
                if !operators.contains(&alternate) {
                    operators.push(alternate);
                }
            }
            crate::filtered::LineVersion {
                route,
                licence: version.route.id.clone(),
                valid_to: version.route.timetable_valid_to,
                operators,
                preferred_systems: version
                    .integrations
                    .iter()
                    .filter(|integration| integration.preferential == Some(true))
                    .map(|integration| integration.transport_system_id)
                    .collect(),
            }
        })
        .collect();
    Ok(Assembled {
        timetable,
        lines,
        post_facts: PostFacts::default(),
        summary,
    })
}
