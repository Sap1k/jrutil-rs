//! Deciding from an evidence pack which physical post each context uses (`docs/rules/posts.md`,
//! sections 3.6–3.10; POST-R062–R123). The pack is the boundary: on a pack F# JrUtil captured,
//! [`evaluate`] decides exactly as the F# evaluator (`JdfPostInferenceEvaluator.
//! evaluateWithScorer`) did.
//!
//! Per stop: the route points consolidate into hypotheses (R070–R073); the heuristic decides one
//! post per movement family (R080–R088); with a learned scorer the two-stage model decides the
//! unlabelled contexts (R100–R109). Every context then gets a [`ContextDecision`] (R120) and
//! authored posts may take an estimated position (R122). Stops are evaluated in parallel and put
//! back in stop order (R180).
//!
//! Configuration: the per-build thresholds and the model are the policy document
//! (`jr_formats::post_policy`, read into [`EvaluationPolicy`]); the constants F# compiled in are
//! `policy/posts.toml` ([`Constants`]).

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;

use jr_formats::post_evidence::{self, BlockRole, Pack, PackError};
use rayon::prelude::*;

mod consolidate;
mod geometry;
mod heuristic;
mod learned;
mod policy;
mod replay;
mod rules;
pub mod scorer;

pub use policy::{
    CaptureConstants, Constants, EvaluationPolicy, GeometryConstants, HeuristicConstants,
    LearnedConstants, MetricConstants, ModeConstants, PolicyError, RegistryConstants, ScorerChoice,
    Settings,
};
pub use rules::*;

/// A consolidated hypothesis of a stop (R072).
#[derive(Clone, Debug, PartialEq)]
pub struct Hypothesis {
    /// `sha("{stop}|{observation ids}")`, 64 lower-case hex digits.
    pub id: String,
    /// Distinct member observation IDs, ordinal order: how hypotheses are compared across
    /// captures (section 5).
    pub observation_ids: Vec<Box<str>>,
    /// Member route points (rows of the pack's route points), in route-point-id order.
    pub route_points: Vec<u32>,
    /// The medoid route point (row of the pack's route points).
    pub representative: u32,
    /// The medoid's position.
    pub latitude: f64,
    pub longitude: f64,
}

/// The heuristic's choice for a movement family (R088).
#[derive(Clone, Debug, PartialEq)]
pub struct HeuristicChoice {
    /// Index into the stop's hypotheses.
    pub hypothesis: usize,
    pub score: f64,
    pub margin: f64,
    /// The smallest corridor face among the hypothesis' rows.
    pub face: Option<Box<str>>,
}

/// The heuristic decision of one movement family at a stop (R088).
#[derive(Clone, Debug, PartialEq)]
pub struct FamilyDecision {
    pub movement_family_id: Box<str>,
    /// Mode, line and role of the family's first context (the F# record's fields).
    pub mode: Box<str>,
    pub line_id: Box<str>,
    pub role: BlockRole,
    /// `Some` when physical, `None` for a centroid decision.
    pub physical: Option<HeuristicChoice>,
}

/// The learned scorer's probabilities for a context (R108).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LearnedScores {
    pub post_probability: f64,
    pub area_probability: f64,
}

/// What was decided for a context.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DecisionKind {
    /// Unlabelled; a physical post: learned `Physical` or heuristic family `Physical`.
    Physical,
    /// Unlabelled; the learned area fallback, published as a post like `Physical` (verdict Q6).
    Area,
    /// Unlabelled; the learned scorer abstained. No location.
    Abstained,
    /// Unlabelled; the learned scorer had no candidate for the context. No location.
    Unseen,
    /// Unlabelled; the heuristic family decision is not physical. No location.
    Centroid,
    /// Authored: never a location (its post may be positioned, see [`AuthoredPosition`]).
    Authored,
}

impl DecisionKind {
    /// Whether the context boards at an estimated post.
    pub fn is_located(self) -> bool {
        matches!(self, DecisionKind::Physical | DecisionKind::Area)
    }
}

/// The decision for one context (R120).
#[derive(Clone, Debug, PartialEq)]
pub struct ContextDecision {
    /// Row of the context in the pack.
    pub context: u32,
    /// Index into [`Decisions::stops`].
    pub stop: usize,
    pub kind: DecisionKind,
    /// The chosen hypothesis (index into the stop's hypotheses) for `Physical` and `Area`.
    pub hypothesis: Option<usize>,
    /// The context's movement family (index into the stop's families).
    pub family: usize,
    /// Set when the learned scorer decided the context.
    pub learned: Option<LearnedScores>,
    /// The F# assignment's `score`: the post probability under the learned scorer, else the
    /// family's heuristic score when physical.
    pub score: Option<f64>,
    /// The family's heuristic margin when physical; none under the learned scorer, whose
    /// probabilities are in `learned` (B10 fixed).
    pub margin: Option<f64>,
}

/// The decisions at one stop.
#[derive(Clone, Debug, PartialEq)]
pub struct StopDecisions {
    pub stop: u64,
    /// The stop's hypotheses in id order.
    pub hypotheses: Vec<Hypothesis>,
    /// The heuristic family decisions in (mode, line, family) order.
    pub families: Vec<FamilyDecision>,
    /// The stop's contexts in [`Decisions::contexts`].
    pub contexts: Range<usize>,
}

/// An authored post that takes an estimated position (R122).
#[derive(Clone, Debug, PartialEq)]
pub struct AuthoredPosition {
    pub stop: u64,
    /// `id:<n>` or `num:<text>` (R043).
    pub authored_post_key: Box<str>,
    /// Index into [`Decisions::stops`] and into that stop's hypotheses.
    pub stop_index: usize,
    pub hypothesis: usize,
    pub latitude: f64,
    pub longitude: f64,
}

/// Counts per rule (R123).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Counts {
    /// Contexts decided (R120).
    pub contexts: usize,
    /// Stops with route points in the pack, one-point stops included (R123).
    pub candidate_stops: usize,
    /// Stops with contexts.
    pub evaluated_stops: usize,
    /// Hypotheses at evaluated stops (R072) and merges made (R071).
    pub hypotheses: usize,
    pub merges: usize,
    /// Heuristic family decisions, physical and centroid (R088).
    pub families_physical: usize,
    pub families_centroid: usize,
    /// Context decisions by kind (R100, R108, R120).
    pub physical: usize,
    pub area: usize,
    pub abstained: usize,
    pub unseen: usize,
    pub centroid: usize,
    pub authored: usize,
    /// Unlabelled contexts without a location (the F# `weak_or_unresolved_contexts`).
    pub unresolved: usize,
    /// Distinct (stop, hypothesis) published by a located context (R121; the F#
    /// `single_internal_posts`).
    pub estimated_posts: usize,
    /// Authored posts positioned (R122).
    pub authored_positions: usize,
    /// Stop-level sources found in the pack (R101; learned scorer only).
    pub stop_level_sources: usize,
}

impl Counts {
    /// The counts keyed by the rule that produces them.
    pub fn per_rule(&self) -> Vec<(&'static str, &'static str, usize)> {
        vec![
            ("POST-R071", "merges", self.merges),
            ("POST-R072", "hypotheses", self.hypotheses),
            ("POST-R088", "families_physical", self.families_physical),
            ("POST-R088", "families_centroid", self.families_centroid),
            ("POST-R101", "stop_level_sources", self.stop_level_sources),
            ("POST-R108", "physical", self.physical),
            ("POST-R108", "area", self.area),
            ("POST-R108", "abstained", self.abstained),
            ("POST-R100", "unseen", self.unseen),
            ("POST-R120", "centroid", self.centroid),
            ("POST-R120", "authored", self.authored),
            ("POST-R120", "contexts", self.contexts),
            ("POST-R121", "estimated_posts", self.estimated_posts),
            ("POST-R122", "authored_positions", self.authored_positions),
            ("POST-R123", "candidate_stops", self.candidate_stops),
            ("POST-R123", "evaluated_stops", self.evaluated_stops),
            ("POST-R123", "unresolved", self.unresolved),
        ]
    }
}

/// Everything the evaluator decided from a pack.
#[derive(Clone, Debug, PartialEq)]
pub struct Decisions {
    /// Whether the learned scorer decided the unlabelled contexts.
    pub learned: bool,
    /// One decision per context of the pack, in pack order.
    pub contexts: Vec<ContextDecision>,
    /// The stops with contexts, ascending.
    pub stops: Vec<StopDecisions>,
    /// Positioned authored posts by (stop, key).
    pub authored_positions: Vec<AuthoredPosition>,
    pub counts: Counts,
}

impl Decisions {
    /// The hypothesis a context boards at, if it was located.
    pub fn chosen(&self, decision: &ContextDecision) -> Option<&Hypothesis> {
        decision
            .hypothesis
            .map(|index| &self.stops[decision.stop].hypotheses[index])
    }

    /// The heuristic decision of a context's family.
    pub fn family(&self, decision: &ContextDecision) -> &FamilyDecision {
        &self.stops[decision.stop].families[decision.family]
    }
}

/// Why a pack could not be evaluated.
#[derive(Debug, thiserror::Error)]
pub enum EvaluateError {
    #[error(transparent)]
    Pack(#[from] PackError),
}

/// The pack's row structure, computed once.
struct Index<'a> {
    variant_starts: Vec<usize>,
    attachment_starts: Vec<usize>,
    members: Vec<Vec<&'a str>>,
    /// (stop, contexts, route points) per stop with contexts.
    stops: Vec<(u64, Range<usize>, Range<usize>)>,
    candidate_stops: usize,
}

fn index(pack: &Pack) -> Index<'_> {
    let contexts = &pack.contexts;
    let variants = &pack.corridor_variants;
    let attachments = &pack.route_point_evidence;
    let mut variant_starts = vec![0; contexts.len() + 1];
    for &context in &variants.context {
        variant_starts[context as usize + 1] += 1;
    }
    for row in 0..contexts.len() {
        variant_starts[row + 1] += variant_starts[row];
    }
    let mut attachment_starts = vec![0; variants.len() + 1];
    for &variant in &attachments.variant {
        attachment_starts[variant as usize + 1] += 1;
    }
    for row in 0..variants.len() {
        attachment_starts[row + 1] += attachment_starts[row];
    }
    let mut members: Vec<Vec<&str>> = vec![Vec::new(); pack.route_points.len()];
    for row in 0..pack.observations.len() {
        members[pack.observations.route_point[row] as usize]
            .push(&pack.observations.observation_id[row]);
    }
    let mut point_ranges: BTreeMap<u64, Range<usize>> = BTreeMap::new();
    for (row, &stop) in pack.route_points.stop.iter().enumerate() {
        point_ranges
            .entry(stop)
            .and_modify(|range| range.end = row + 1)
            .or_insert(row..row + 1);
    }
    let mut stops: Vec<(u64, Range<usize>, Range<usize>)> = Vec::new();
    for (row, &stop) in contexts.stop.iter().enumerate() {
        match stops.last_mut() {
            Some(last) if last.0 == stop => last.1.end = row + 1,
            _ => stops.push((
                stop,
                row..row + 1,
                point_ranges.get(&stop).cloned().unwrap_or(0..0),
            )),
        }
    }
    Index {
        variant_starts,
        attachment_starts,
        members,
        stops,
        candidate_stops: point_ranges.len(),
    }
}

/// What one stop's evaluation produces.
struct StopResult {
    stop: StopDecisions,
    contexts: Vec<ContextDecision>,
    merges: usize,
}

fn evaluate_stop(
    pack: &Pack,
    policy: &EvaluationPolicy,
    facts: &learned::ObservationFacts,
    index: &Index,
    stop_index: usize,
) -> StopResult {
    let (stop, context_range, point_range) = index.stops[stop_index].clone();
    let settings = &policy.settings;
    let constants = &policy.constants;
    let contexts = &pack.contexts;
    let points = replay::points(pack, point_range.clone(), &index.members);
    let rows = replay::rows(
        pack,
        context_range.clone(),
        &index.variant_starts,
        &index.attachment_starts,
        point_range.start,
    );
    let consolidated = consolidate::consolidate(settings, constants, stop, &points, &rows);
    let outcomes = heuristic::decide(settings, constants, pack, &points, &consolidated);
    let learned_decisions = match &policy.scorer {
        ScorerChoice::Learned(model) => Some(learned::decide(
            settings,
            constants,
            model,
            facts,
            pack,
            stop,
            &points,
            &consolidated,
        )),
        ScorerChoice::Heuristic => None,
    };
    let hypotheses: Vec<Hypothesis> = consolidated
        .hypotheses
        .iter()
        .map(|hypothesis| {
            let medoid = &points[hypothesis.medoid];
            Hypothesis {
                id: hypothesis.id.clone(),
                observation_ids: hypothesis
                    .observations
                    .iter()
                    .map(|&id| Box::from(id))
                    .collect(),
                route_points: hypothesis
                    .members
                    .iter()
                    .map(|&member| points[member].row)
                    .collect(),
                representative: medoid.row,
                latitude: medoid.latitude,
                longitude: medoid.longitude,
            }
        })
        .collect();
    let hypothesis_index: HashMap<&str, usize> = hypotheses
        .iter()
        .enumerate()
        .map(|(index, hypothesis)| (hypothesis.id.as_str(), index))
        .collect();
    let families: Vec<FamilyDecision> = outcomes
        .iter()
        .map(|outcome| {
            let first = outcome.first_context as usize;
            FamilyDecision {
                movement_family_id: outcome.family.clone(),
                mode: contexts.mode[first].clone(),
                line_id: contexts.line_id[first].clone(),
                role: contexts.role[first],
                physical: outcome
                    .physical
                    .as_ref()
                    .map(|(hypothesis, score, margin, face)| HeuristicChoice {
                        hypothesis: *hypothesis,
                        score: *score,
                        margin: *margin,
                        face: face.clone(),
                    }),
            }
        })
        .collect();
    let family_index: HashMap<&str, usize> = families
        .iter()
        .enumerate()
        .map(|(index, family)| (&*family.movement_family_id, index))
        .collect();
    let decisions = context_range
        .clone()
        .map(|context| {
            let family = family_index
                .get(&*contexts.movement_family_id[context])
                .copied()
                .unwrap_or(0);
            let heuristic = families.get(family).and_then(|f| f.physical.as_ref());
            let authored = contexts.authored_post_key[context].is_some();
            let learned = match &learned_decisions {
                Some(decisions) if !authored => Some(decisions.get(&(context as u32))),
                _ => None,
            };
            let (score, margin) = match learned {
                // The probabilities are reported as such ([`LearnedScores`]); a learned decision
                // has no margin (B10 fixed).
                Some(Some(decision)) => (Some(decision.post_probability), None),
                _ => (
                    heuristic.map(|choice| choice.score),
                    heuristic.map(|choice| choice.margin),
                ),
            };
            let learned_hypothesis = |decision: &scorer::Decision| {
                decision
                    .hypothesis
                    .as_deref()
                    .and_then(|id| hypothesis_index.get(id).copied())
            };
            let (kind, hypothesis) = if authored {
                (DecisionKind::Authored, None)
            } else if let Some(learned) = learned {
                match learned {
                    Some(decision) => match (decision.resolution, learned_hypothesis(decision)) {
                        (scorer::Resolution::Physical, Some(hypothesis)) => {
                            (DecisionKind::Physical, Some(hypothesis))
                        }
                        (scorer::Resolution::Area, Some(hypothesis)) => {
                            (DecisionKind::Area, Some(hypothesis))
                        }
                        _ => (DecisionKind::Abstained, None),
                    },
                    None => (DecisionKind::Unseen, None),
                }
            } else if let Some(choice) = heuristic {
                (DecisionKind::Physical, Some(choice.hypothesis))
            } else {
                (DecisionKind::Centroid, None)
            };
            ContextDecision {
                context: context as u32,
                stop: stop_index,
                kind,
                hypothesis,
                family,
                learned: learned.flatten().map(|decision| LearnedScores {
                    post_probability: decision.post_probability,
                    area_probability: decision.area_probability,
                }),
                score,
                margin,
            }
        })
        .collect();
    StopResult {
        stop: StopDecisions {
            stop,
            hypotheses,
            families,
            contexts: context_range,
        },
        contexts: decisions,
        merges: consolidated.merges,
    }
}

/// R122: positions of authored posts, by (stop, key).
fn authored_positions(
    pack: &Pack,
    settings: &Settings,
    stops: &[StopDecisions],
    contexts: &[ContextDecision],
) -> Vec<AuthoredPosition> {
    let keys = &pack.contexts.authored_post_key;
    let mut groups: BTreeMap<(u64, &str), Vec<&ContextDecision>> = BTreeMap::new();
    for decision in contexts {
        if let Some(key) = keys[decision.context as usize].as_deref() {
            groups
                .entry((stops[decision.stop].stop, key))
                .or_default()
                .push(decision);
        }
    }
    let mut positions = Vec::new();
    for ((stop, key), members) in groups {
        let stop_index = members[0].stop;
        let families = &stops[stop_index].families;
        let relevant: Vec<&FamilyDecision> = members
            .iter()
            .map(|decision| &families[decision.family])
            .collect();
        let physical: Vec<&HeuristicChoice> = relevant
            .iter()
            .filter_map(|family| family.physical.as_ref())
            .collect();
        let mut candidates: Vec<usize> = Vec::new();
        for choice in &physical {
            if !candidates.contains(&choice.hypothesis) {
                candidates.push(choice.hypothesis);
            }
        }
        let [candidate] = candidates[..] else {
            continue;
        };
        let eligible = (!settings.require_unanimous_contexts
            || (!relevant.is_empty() && physical.len() == relevant.len()))
            && physical.iter().all(|choice| {
                choice.score >= settings.authored_minimum_physical_score
                    && choice.margin >= settings.authored_minimum_physical_margin
            });
        if eligible {
            let hypothesis = &stops[stop_index].hypotheses[candidate];
            positions.push(AuthoredPosition {
                stop,
                authored_post_key: key.into(),
                stop_index,
                hypothesis: candidate,
                latitude: hypothesis.latitude,
                longitude: hypothesis.longitude,
            });
        }
    }
    positions
}

/// Decides every context of a pack. The pack is checked first ([`post_evidence::check`]);
/// a pack [`post_evidence::read`] returned passes.
pub fn evaluate(pack: &Pack, policy: &EvaluationPolicy) -> Result<Decisions, EvaluateError> {
    post_evidence::check(pack)?;
    let index = index(pack);
    let facts = match policy.scorer {
        ScorerChoice::Learned(_) => learned::observation_facts(pack, &policy.constants),
        ScorerChoice::Heuristic => learned::ObservationFacts::default(),
    };
    let results: Vec<StopResult> = (0..index.stops.len())
        .into_par_iter()
        .map(|stop| evaluate_stop(pack, policy, &facts, &index, stop))
        .collect();
    let mut counts = Counts {
        candidate_stops: index.candidate_stops,
        evaluated_stops: results.len(),
        stop_level_sources: facts.stop_level.len(),
        ..Counts::default()
    };
    let mut stops = Vec::with_capacity(results.len());
    let mut contexts = Vec::with_capacity(pack.contexts.len());
    for result in results {
        counts.merges += result.merges;
        counts.hypotheses += result.stop.hypotheses.len();
        for family in &result.stop.families {
            if family.physical.is_some() {
                counts.families_physical += 1;
            } else {
                counts.families_centroid += 1;
            }
        }
        let mut published: Vec<usize> = Vec::new();
        for decision in &result.contexts {
            match decision.kind {
                DecisionKind::Physical => counts.physical += 1,
                DecisionKind::Area => counts.area += 1,
                DecisionKind::Abstained => counts.abstained += 1,
                DecisionKind::Unseen => counts.unseen += 1,
                DecisionKind::Centroid => counts.centroid += 1,
                DecisionKind::Authored => counts.authored += 1,
            }
            if let Some(hypothesis) = decision.hypothesis
                && !published.contains(&hypothesis)
            {
                published.push(hypothesis);
            }
        }
        counts.estimated_posts += published.len();
        contexts.extend(result.contexts);
        stops.push(result.stop);
    }
    counts.contexts = contexts.len();
    counts.unresolved = counts.abstained + counts.unseen + counts.centroid;
    let authored_positions = authored_positions(pack, &policy.settings, &stops, &contexts);
    counts.authored_positions = authored_positions.len();
    Ok(Decisions {
        learned: matches!(policy.scorer, ScorerChoice::Learned(_)),
        contexts,
        stops,
        authored_positions,
        counts,
    })
}
