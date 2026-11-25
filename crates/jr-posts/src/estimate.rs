//! The estimator as a pass (POST-R030–R057, R130–R140): contexts and their routed evidence
//! ([`crate::capture`]), the decisions ([`crate::evaluate`]), and what is published of them
//! ([`crate::plan`]).

use jr_formats::geodata::registry;
use jr_formats::post_evidence::{Pack, PackError, PackIdentity};
use jr_geo::routing::{ContextCache, RoutingGraph, RoutingPolicy};
use jr_jdf::assemble::PostFacts;
use jr_model::{Diagnostics, EditBuilder, Edits, ReportTable, Timetable};

use crate::capture::{self, CaptureInput};
use crate::evaluate::{self, EvaluateError, EvaluationPolicy, ScorerChoice};
use crate::plan;

/// The pass name recorded in the timetable's history.
pub const PASS: &str = "post_estimator";

#[derive(Debug)]
pub struct EstimateInput<'a> {
    /// The road timetable as assembled.
    pub timetable: &'a Timetable,
    pub facts: &'a PostFacts,
    /// Only the capture routes; it frees the graph when it has.
    pub graph: RoutingGraph,
    pub routing: &'a RoutingPolicy,
    pub policy: &'a EvaluationPolicy,
    /// The registry's pinned posts (`posts.csv`).
    pub registry: &'a [registry::Post],
    /// What the evidence pack says it was captured from.
    pub identity: PackIdentity,
    /// Routed evidence kept between builds (the capture's); the caller reads its counts.
    pub cache: Option<&'a mut ContextCache>,
    /// Report the scorer's training rows and each context's calls (POST-R141).
    pub training: bool,
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

/// The counts of `estimated_posts` (POST-R139). The first six carry F#'s
/// `compiler.estimated_posts` names and meaning; the rest are this pass's own.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Summary {
    pub scorer: &'static str,
    pub policy_id: String,
    /// Stops with route points in the pack, one-point stops included (R123).
    pub candidate_bearing_stops: usize,
    /// Authored posts given a position (R122, R137).
    pub authored_posts_positioned: usize,
    /// Estimated posts published (R130); F# counted them twice, under both names.
    pub single_internal_posts: usize,
    pub physical_internal_posts: usize,
    /// Unlabelled contexts without a post (abstained, unseen, centroid); F# counted them twice.
    pub centroid_pattern_fallbacks: usize,
    pub weak_or_unresolved_contexts: usize,
    /// Of the estimated posts, those chosen only by learned `Area` decisions (verdict Q6).
    pub area_posts: usize,
    /// Estimated posts whose ordinal the registry pinned, and new ones (R132, R133).
    pub registry_reused_ordinals: usize,
    pub registry_new_ordinals: usize,
    /// Calls now boarding at an estimated post (R135).
    pub calls_estimated: usize,
    pub capture: capture::CaptureCounts,
    pub evaluation: evaluate::Counts,
}

#[derive(Debug, thiserror::Error)]
pub enum EstimateError {
    /// The captured pack breaks an invariant: a defect of the capture or the router.
    #[error("the captured evidence pack is invalid: {0}")]
    Pack(#[from] PackError),
    #[error(transparent)]
    Evaluate(#[from] EvaluateError),
}

/// Every rule of the estimator: capture, evaluation (with the pack's), plan.
pub fn rules() -> Vec<jr_model::Rule> {
    capture::RULES
        .iter()
        .chain(evaluate::RULES)
        .chain(plan::RULES)
        .copied()
        .collect()
}

pub fn estimate(
    input: EstimateInput<'_>,
    diagnostics: &mut Diagnostics,
) -> Result<Estimated, EstimateError> {
    let captured = capture::capture(CaptureInput {
        timetable: input.timetable,
        facts: input.facts,
        graph: input.graph,
        routing: input.routing,
        constants: &input.policy.constants,
        identity: input.identity,
        cache: input.cache,
    });
    let mut estimated = estimate_from_pack_with(
        input.timetable,
        input.facts,
        captured.pack,
        input.policy,
        input.registry,
        diagnostics,
        input.training,
    )?;
    estimated.summary.capture = captured.counts;
    Ok(estimated)
}

/// Evaluates a given pack (captured now, or read from disk) and publishes its decisions: the
/// `evaluate` execution mode. The pack's contexts must come from this timetable and facts.
pub fn estimate_from_pack(
    timetable: &Timetable,
    facts: &PostFacts,
    pack: Pack,
    policy: &EvaluationPolicy,
    registry: &[registry::Post],
    diagnostics: &mut Diagnostics,
) -> Result<Estimated, EstimateError> {
    estimate_from_pack_with(timetable, facts, pack, policy, registry, diagnostics, false)
}

/// [`estimate_from_pack`], with the training reports when `training` is set (POST-R141).
pub fn estimate_from_pack_with(
    timetable: &Timetable,
    facts: &PostFacts,
    pack: Pack,
    policy: &EvaluationPolicy,
    registry: &[registry::Post],
    diagnostics: &mut Diagnostics,
    training: bool,
) -> Result<Estimated, EstimateError> {
    let decisions = evaluate::evaluate_with(&pack, policy, training)?;
    let mut builder = EditBuilder::new(timetable, PASS);
    let planned = plan::plan(
        timetable,
        facts,
        &pack,
        &decisions,
        registry,
        &policy.constants,
        &mut builder,
        diagnostics,
        training,
    );
    let counts = &decisions.counts;
    let summary = Summary {
        scorer: match policy.scorer {
            ScorerChoice::Learned(_) => "learned",
            ScorerChoice::Heuristic => "heuristic",
        },
        policy_id: policy.settings.policy_id.clone(),
        candidate_bearing_stops: counts.candidate_stops,
        authored_posts_positioned: planned.counts.authored_positioned,
        single_internal_posts: planned.counts.locations,
        physical_internal_posts: planned.counts.locations,
        centroid_pattern_fallbacks: counts.unresolved,
        weak_or_unresolved_contexts: counts.unresolved,
        area_posts: planned.counts.area_locations,
        registry_reused_ordinals: planned.counts.registry_reused,
        registry_new_ordinals: planned.counts.registry_new,
        calls_estimated: planned.counts.calls_estimated,
        capture: capture::CaptureCounts::default(),
        evaluation: decisions.counts.clone(),
    };
    Ok(Estimated {
        edits: builder.finish(),
        pack: Some(pack),
        reports: [planned.decisions, planned.candidates]
            .into_iter()
            .chain(planned.context_calls)
            .chain(training.then(|| scorer_rows(&decisions.training, policy)))
            .collect(),
        summary,
    })
}

/// POST-R141: the scorer's rows as a table, the features named as the model names them.
fn scorer_rows(rows: &[evaluate::TrainingRow], policy: &EvaluationPolicy) -> ReportTable {
    let feature_names: Vec<&'static str> = match &policy.scorer {
        ScorerChoice::Learned(model) => model.feature_names(),
        ScorerChoice::Heuristic => Vec::new(),
    };
    let mut header = vec![
        "context_id",
        "stop_id",
        "hypothesis_id",
        "post_id",
        "area_id",
        "is_platform",
        "previous_stop",
        "next_stop",
        "stop_post_count",
        "mode",
        "latitude",
        "longitude",
    ];
    header.extend(feature_names);
    let rows = rows
        .iter()
        .map(|row| {
            let mut cells = vec![
                row.context_id.clone(),
                row.stop.to_string(),
                row.hypothesis_id.clone(),
                row.post_id.clone(),
                row.area_id.clone(),
                u8::from(row.is_platform).to_string(),
                row.previous_stop.clone().unwrap_or_default(),
                row.next_stop.clone().unwrap_or_default(),
                row.stop_post_count.to_string(),
                row.mode.clone(),
                row.latitude.to_string(),
                row.longitude.to_string(),
            ];
            cells.extend(row.features.iter().map(f64::to_string));
            cells
        })
        .collect();
    ReportTable {
        name: "post_scorer_rows",
        header,
        rows,
    }
}
