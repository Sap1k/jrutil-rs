//! What the evaluator is configured by: the compiled-in constants (`policy/posts.toml`) and the
//! policy document's values with F#'s defaults and validation (POST-R004).

use jr_formats::post_evidence::{CAPTURE_MAXIMUM_CORRIDOR_VARIANTS, CAPTURE_ROUTED_EXCESS_METRES};
use jr_formats::post_policy::{PolicyDocument, PolicyV2, Scorer};
use serde::Deserialize;

use super::scorer::Model;

const BUILTIN: &str = include_str!("../../../../policy/posts.toml");

/// `policy/posts.toml`: constants F# JrUtil compiled in. Every field belongs to the rule named
/// in the file.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Constants {
    pub metric: MetricConstants,
    pub modes: ModeConstants,
    pub geometry: GeometryConstants,
    pub heuristic: HeuristicConstants,
    pub learned: LearnedConstants,
    pub capture: CaptureConstants,
    pub registry: RegistryConstants,
}

impl Constants {
    /// The constants shipped with the evaluator.
    pub fn builtin() -> Constants {
        Constants::parse(BUILTIN).expect("policy/posts.toml")
    }

    pub fn parse(text: &str) -> Result<Constants, toml::de::Error> {
        toml::from_str(text)
    }
}

/// The local metric (section 3 conventions; B18).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricConstants {
    pub metres_per_degree_latitude: f64,
    pub metres_per_degree_longitude: f64,
    pub isolation_metres_per_degree_latitude: f64,
}

/// POST-R081.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModeConstants {
    pub inheritance_radius_m: f64,
}

/// POST-R082.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeometryConstants {
    pub alignment_range_deg: f64,
    pub proximity_range_m: f64,
}

/// POST-R085, R087.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeuristicConstants {
    pub popularity_context_cap: usize,
    pub geometry_margin_tolerance: f64,
}

/// POST-R101–R108 (the model contract).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearnedConstants {
    pub stop_level_minimum_stops: usize,
    pub stop_level_maximum_share: f64,
    pub maximum_post_diameter_m: f64,
    pub minimum_numbered_bays: usize,
    pub empty_stop_latitude: f64,
    pub maximum_area_diameter_m: f64,
    pub excess_cap_m: f64,
    pub complex_stop_posts: usize,
}

/// POST-R030–R033, R040.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureConstants {
    pub max_distance_from_stop_m: f64,
    pub current_lifecycles: Vec<Box<str>>,
    pub obsolete_lifecycles: Vec<Box<str>>,
    pub minimum_route_points: usize,
    pub modes: Vec<Box<str>>,
}

/// POST-R132.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryConstants {
    pub inferred_post_match_m: f64,
    pub metres_per_degree: f64,
}

/// The policy document's v2 values as the evaluator uses them: a missing key is 0 or `false`
/// (System.Text.Json's behaviour in F#), then validated as F# `validatePolicyForEvidence`
/// does against the pack's capture ceilings.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub policy_id: String,
    pub maximum_diameter_metres: f64,
    pub maximum_chainage_difference_metres: f64,
    pub maximum_heading_difference_degrees: f64,
    pub opposite_side_tolerance_metres: f64,
    pub distinguished_context_minimum_count: i32,
    pub distinguished_context_minimum_score: f64,
    pub distinguished_context_minimum_margin: f64,
    pub weight_alignment: f64,
    pub weight_side: f64,
    pub weight_proximity: f64,
    pub weight_routed_excess: f64,
    pub centreline_neutral_tolerance_metres: f64,
    pub maximum_corridor_distance_metres: f64,
    pub maximum_routed_excess_metres: f64,
    pub minimum_separation_metres: f64,
    pub maximum_isolation_adjustment: f64,
    pub minimum_physical_score: f64,
    pub minimum_physical_margin: f64,
    pub material_routed_advantage: f64,
    pub minimum_contexts: i32,
    pub minimum_winning_share: f64,
    pub minimum_per_context_score: f64,
    pub minimum_per_context_lead: f64,
    pub minimum_supporting_contexts: i32,
    pub minimum_runner_up_ratio: f64,
    pub maximum_geometry_disadvantage: f64,
    pub maximum_popularity_adjustment: f64,
    pub per_context_adjustment: f64,
    pub absolute_tie_metres: f64,
    pub relative_tie_fraction: f64,
    pub maximum_variants: i32,
    pub source_support_per_weight: f64,
    pub maximum_source_support_adjustment: f64,
    pub explicit_support_adjustment: f64,
    pub estimated_support_adjustment: f64,
    pub conflict_adjustment: f64,
    pub maximum_combined_supporting_adjustment: f64,
    pub authored_minimum_physical_score: f64,
    pub authored_minimum_physical_margin: f64,
    pub require_unanimous_contexts: bool,
}

/// A policy value the evaluator refuses.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("post-inference policy {field}: {message}")]
pub struct PolicyError {
    pub field: &'static str,
    pub message: &'static str,
}

fn error(field: &'static str, message: &'static str) -> PolicyError {
    PolicyError { field, message }
}

fn finite(field: &'static str, value: f64) -> Result<f64, PolicyError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(error(field, "must be finite"))
    }
}

fn non_negative(field: &'static str, value: f64) -> Result<f64, PolicyError> {
    if finite(field, value)? < 0.0 {
        return Err(error(field, "must be non-negative"));
    }
    Ok(value)
}

fn positive(field: &'static str, value: f64) -> Result<f64, PolicyError> {
    if finite(field, value)? <= 0.0 {
        return Err(error(field, "must be positive"));
    }
    Ok(value)
}

fn unit(field: &'static str, value: f64) -> Result<f64, PolicyError> {
    if !(0.0..=1.0).contains(&finite(field, value)?) {
        return Err(error(field, "must be in [0,1]"));
    }
    Ok(value)
}

fn count(field: &'static str, value: i32) -> Result<i32, PolicyError> {
    if value < 1 {
        return Err(error(field, "must be at least one"));
    }
    Ok(value)
}

impl Settings {
    /// The document's policy with F#'s defaults, validated for a pack with the supported
    /// capture ceilings (1000 m routed excess, three variants).
    pub fn new(policy: &PolicyV2) -> Result<Settings, PolicyError> {
        let f = |value: Option<f64>| value.unwrap_or(0.0);
        let i = |value: Option<i32>| value.unwrap_or(0);
        let c = &policy.consolidation;
        let w = &policy.geometry_weights;
        let g = &policy.hard_gates;
        let s = &policy.spatial_isolation;
        let r = &policy.resolution;
        let n = &policy.consensus;
        let e = &policy.established_post;
        let a = &policy.alternative_corridors;
        let m = &policy.modality;
        let u = &policy.authored_resolution;
        if policy.schema_version.unwrap_or(0) != 2 {
            return Err(error("schema_version", "unsupported schema; expected 2"));
        }
        let policy_id = policy.policy_id.clone().unwrap_or_default();
        if policy_id.trim().is_empty() {
            return Err(error("policy_id", "is required"));
        }
        let horizon = CAPTURE_ROUTED_EXCESS_METRES;
        let heading = f(c.maximum_heading_difference_degrees);
        if !(0.0..=180.0).contains(&heading) || !heading.is_finite() {
            return Err(error(
                "consolidation.maximum_heading_difference_degrees",
                "must be in [0,180]",
            ));
        }
        let weights = [
            non_negative("geometry_weights[0]", f(w.alignment))?,
            non_negative("geometry_weights[1]", f(w.side))?,
            non_negative("geometry_weights[2]", f(w.proximity))?,
            non_negative("geometry_weights[3]", f(w.routed_excess))?,
        ];
        let settings = Settings {
            policy_id,
            maximum_diameter_metres: positive(
                "consolidation.maximum_diameter_metres",
                f(c.maximum_diameter_metres),
            )?,
            maximum_chainage_difference_metres: non_negative(
                "consolidation.maximum_chainage_difference_metres",
                f(c.maximum_chainage_difference_metres),
            )?,
            maximum_heading_difference_degrees: heading,
            opposite_side_tolerance_metres: non_negative(
                "consolidation.opposite_side_tolerance_metres",
                f(c.opposite_side_tolerance_metres),
            )?,
            distinguished_context_minimum_count: count(
                "consolidation.distinguished_context_minimum_count",
                i(c.distinguished_context_minimum_count),
            )?,
            distinguished_context_minimum_score: unit(
                "consolidation.distinguished_context_minimum_score",
                f(c.distinguished_context_minimum_score),
            )?,
            distinguished_context_minimum_margin: unit(
                "consolidation.distinguished_context_minimum_margin",
                f(c.distinguished_context_minimum_margin),
            )?,
            weight_alignment: weights[0],
            weight_side: weights[1],
            weight_proximity: weights[2],
            weight_routed_excess: weights[3],
            centreline_neutral_tolerance_metres: non_negative(
                "hard_gates.centreline_neutral_tolerance_metres",
                f(g.centreline_neutral_tolerance_metres),
            )?,
            maximum_corridor_distance_metres: positive(
                "hard_gates.maximum_corridor_distance_metres",
                f(g.maximum_corridor_distance_metres),
            )?,
            maximum_routed_excess_metres: positive(
                "hard_gates.maximum_routed_excess_metres",
                f(g.maximum_routed_excess_metres),
            )?,
            minimum_separation_metres: non_negative(
                "spatial_isolation.minimum_separation_metres",
                f(s.minimum_separation_metres),
            )?,
            maximum_isolation_adjustment: unit(
                "spatial_isolation.maximum_adjustment",
                f(s.maximum_adjustment),
            )?,
            minimum_physical_score: unit(
                "resolution.minimum_physical_score",
                f(r.minimum_physical_score),
            )?,
            minimum_physical_margin: unit(
                "resolution.minimum_physical_margin",
                f(r.minimum_physical_margin),
            )?,
            material_routed_advantage: unit(
                "resolution.material_routed_advantage",
                f(r.material_routed_advantage),
            )?,
            minimum_contexts: count("consensus.minimum_contexts", i(n.minimum_contexts))?,
            minimum_winning_share: unit(
                "consensus.minimum_winning_share",
                f(n.minimum_winning_share),
            )?,
            minimum_per_context_score: unit(
                "consensus.minimum_per_context_score",
                f(n.minimum_per_context_score),
            )?,
            minimum_per_context_lead: unit(
                "consensus.minimum_per_context_lead",
                f(n.minimum_per_context_lead),
            )?,
            minimum_supporting_contexts: count(
                "established_post.minimum_supporting_contexts",
                i(e.minimum_supporting_contexts),
            )?,
            minimum_runner_up_ratio: f(e.minimum_runner_up_ratio),
            maximum_geometry_disadvantage: unit(
                "established_post.maximum_geometry_disadvantage",
                f(e.maximum_geometry_disadvantage),
            )?,
            maximum_popularity_adjustment: unit(
                "established_post.maximum_popularity_adjustment",
                f(e.maximum_popularity_adjustment),
            )?,
            per_context_adjustment: non_negative(
                "established_post.per_context_adjustment",
                f(e.per_context_adjustment),
            )?,
            absolute_tie_metres: non_negative(
                "alternative_corridors.absolute_tie_metres",
                f(a.absolute_tie_metres),
            )?,
            relative_tie_fraction: unit(
                "alternative_corridors.relative_tie_fraction",
                f(a.relative_tie_fraction),
            )?,
            maximum_variants: i(a.maximum_variants),
            source_support_per_weight: non_negative(
                "modality.source_support_per_weight",
                f(m.source_support_per_weight),
            )?,
            maximum_source_support_adjustment: unit(
                "modality.maximum_source_support_adjustment",
                f(m.maximum_source_support_adjustment),
            )?,
            explicit_support_adjustment: unit(
                "modality.explicit_support_adjustment",
                f(m.explicit_support_adjustment),
            )?,
            estimated_support_adjustment: unit(
                "modality.estimated_support_adjustment",
                f(m.estimated_support_adjustment),
            )?,
            conflict_adjustment: f(m.conflict_adjustment),
            maximum_combined_supporting_adjustment: unit(
                "modality.maximum_combined_supporting_adjustment",
                f(m.maximum_combined_supporting_adjustment),
            )?,
            authored_minimum_physical_score: unit(
                "authored_resolution.minimum_physical_score",
                f(u.minimum_physical_score),
            )?,
            authored_minimum_physical_margin: unit(
                "authored_resolution.minimum_physical_margin",
                f(u.minimum_physical_margin),
            )?,
            require_unanimous_contexts: u.require_unanimous_contexts.unwrap_or(false),
        };
        let sum: f64 = weights.iter().sum();
        if (sum - 1.0).abs() > 0.000001 {
            return Err(error("geometry_weights", "must sum to 1.0"));
        }
        if settings.maximum_routed_excess_metres > horizon {
            return Err(error(
                "hard_gates.maximum_routed_excess_metres",
                "exceeds the captured routed-excess horizon",
            ));
        }
        let plausible = unit(
            "resolution.minimum_plausible_score",
            f(r.minimum_plausible_score),
        )?;
        if plausible > settings.minimum_physical_score {
            return Err(error(
                "resolution",
                "minimum_plausible_score cannot exceed minimum_physical_score",
            ));
        }
        if settings.minimum_runner_up_ratio < 1.0 || !settings.minimum_runner_up_ratio.is_finite() {
            return Err(error(
                "established_post.minimum_runner_up_ratio",
                "must be finite and at least one",
            ));
        }
        if settings.per_context_adjustment > settings.maximum_popularity_adjustment {
            return Err(error(
                "established_post",
                "per_context_adjustment cannot exceed maximum_popularity_adjustment",
            ));
        }
        if settings.maximum_variants < 1
            || i64::from(settings.maximum_variants) > CAPTURE_MAXIMUM_CORRIDOR_VARIANTS
        {
            return Err(error(
                "alternative_corridors.maximum_variants",
                "must be between 1 and the capture ceiling",
            ));
        }
        let conflict = settings.conflict_adjustment;
        if !(-1.0..=0.0).contains(&conflict) || !conflict.is_finite() {
            return Err(error("modality.conflict_adjustment", "must be in [-1,0]"));
        }
        Ok(settings)
    }
}

/// The scorer that decides unlabelled contexts.
#[derive(Clone, Debug, PartialEq)]
pub enum ScorerChoice {
    /// The heuristic (POST-R080–R088): no document, a v2 document, or `kind: heuristic`.
    Heuristic,
    /// The learned two-stage scorer (POST-R100–R109).
    Learned(Box<Model>),
}

/// Everything one evaluation is configured by.
#[derive(Clone, Debug, PartialEq)]
pub struct EvaluationPolicy {
    pub settings: Settings,
    pub scorer: ScorerChoice,
    pub constants: Constants,
}

impl EvaluationPolicy {
    /// A parsed policy document with the built-in constants. Fails when the document's policy
    /// is invalid or its model uses a feature the evaluator does not compute.
    pub fn from_document(document: &PolicyDocument) -> Result<EvaluationPolicy, PolicyError> {
        EvaluationPolicy::with_constants(document, Constants::builtin())
    }

    pub fn with_constants(
        document: &PolicyDocument,
        constants: Constants,
    ) -> Result<EvaluationPolicy, PolicyError> {
        let settings = Settings::new(&document.policy)?;
        let scorer = match &document.scorer {
            Scorer::Heuristic => ScorerChoice::Heuristic,
            Scorer::Learned(model) => ScorerChoice::Learned(Box::new(Model::new(model)?)),
        };
        Ok(EvaluationPolicy {
            settings,
            scorer,
            constants,
        })
    }
}
