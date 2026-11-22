//! The estimator as a pass (POST-R030–R057, R130–R140): contexts, their routed evidence, the
//! decisions, and what is published of them. (Skeleton: the estimator's package fills this in.)

use jr_formats::geodata::registry;
use jr_formats::post_evidence::{Pack, PackIdentity};
use jr_geo::routing::{RoutingGraph, RoutingPolicy};
use jr_jdf::assemble::PostFacts;
use jr_model::{Diagnostics, EditBuilder, Edits, ReportTable, Timetable};

use crate::evaluate::EvaluationPolicy;

/// The pass name recorded in the timetable's history.
pub const PASS: &str = "post_estimator";

#[derive(Debug)]
pub struct EstimateInput<'a> {
    /// The road timetable as assembled.
    pub timetable: &'a Timetable,
    pub facts: &'a PostFacts,
    pub graph: &'a RoutingGraph,
    pub routing: &'a RoutingPolicy,
    pub policy: &'a EvaluationPolicy,
    /// The registry's pinned posts (`posts.csv`).
    pub registry: &'a [registry::Post],
    /// What the evidence pack says it was captured from.
    pub identity: PackIdentity,
}

#[derive(Debug)]
pub struct Estimated {
    pub edits: Edits,
    /// The evidence the decisions were made from, for the command line to write on request.
    pub pack: Option<Pack>,
    /// Review reports, each a table the command line writes as `<name>.csv`.
    pub reports: Vec<ReportTable>,
    /// The counts published in diagnostics.json under `estimated_posts`.
    pub summary: Summary,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Summary {}

#[derive(Debug, thiserror::Error)]
pub enum EstimateError {}

pub fn estimate(
    input: EstimateInput<'_>,
    _diagnostics: &mut Diagnostics,
) -> Result<Estimated, EstimateError> {
    Ok(Estimated {
        edits: EditBuilder::new(input.timetable, PASS).finish(),
        pack: None,
        reports: Vec::new(),
        summary: Summary::default(),
    })
}
