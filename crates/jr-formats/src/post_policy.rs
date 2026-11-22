//! The post estimator's policy document (`docs/rules/posts.md` 2.4, 2.5; POST-R004).
//!
//! Two forms: a policy v2 file (`schema_version: 2`, heuristic scorer) or a v3 document
//! `{"schema_version": 3, "policy": {…v2…}, "scorer": {"kind": "heuristic" | "learned",
//! "model": {…}}}` with the learned two-stage model `post-scorer-two-stage-v1`. This module
//! says what the document says: every key of sections 2.4 and 2.5 as a typed field, unknown
//! keys an error. Policy v2 values are `Option`s: the F# deserializer turns a missing key into
//! 0 or `false` and then validates; that default and the validation belong to the evaluator
//! (`jr_posts::evaluate`). The model's structure (formats, vector lengths, finite parameters,
//! positive scales, distinct features, stage 2 = stage 1 + the collective features, thresholds)
//! is part of the model format and checked here, as F# `JdfPostScorer.parseModel` does.
//!
//! Deliberate differences from F#: unknown keys are rejected (System.Text.Json ignored them);
//! a JSON `null` for a policy value reads as missing (F# rejected it).

use serde::Deserialize;
use serde_json::Value;

use crate::post_evidence::ids::sha256_hex;

/// `schema_version` of a policy v2 file and of the embedded policy.
pub const POLICY_SCHEMA_VERSION: i64 = 2;
/// `schema_version` of a document with a scorer.
pub const DOCUMENT_SCHEMA_VERSION: i64 = 3;
/// `scorer.model.format`.
pub const MODEL_FORMAT: &str = "post-scorer-two-stage-v1";
/// `stage1.format`, `stage2.format`.
pub const STAGE_FORMAT: &str = "post-scorer-conditional-logit-v1";
/// The features stage 2 appends to stage 1's, in order (POST-R107).
pub const COLLECTIVE_FEATURES: [&str; 6] = [
    "reverse_mass",
    "has_reverse",
    "same_movement_mass",
    "has_same_movement",
    "same_next_mass",
    "same_previous_mass",
];

/// A parsed policy document.
#[derive(Clone, Debug, PartialEq)]
pub struct PolicyDocument {
    /// `sha` of the document text (a UTF-8 byte-order mark is not part of the text), the
    /// manifest's `policy_document_sha256`.
    pub sha256: String,
    /// 2 for a bare policy, 3 for a document with a scorer.
    pub schema_version: i64,
    pub policy: PolicyV2,
    pub scorer: Scorer,
}

/// Which scorer the document names. A v2 file names the heuristic.
#[derive(Clone, Debug, PartialEq)]
pub enum Scorer {
    Heuristic,
    Learned(Box<Model>),
}

/// The policy v2 object (section 2.4). Every value is as stored; `None` means the key is
/// absent.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PolicyV2 {
    pub schema_version: Option<i64>,
    pub policy_id: Option<String>,
    pub consolidation: Consolidation,
    pub geometry_weights: GeometryWeights,
    pub hard_gates: HardGates,
    pub spatial_isolation: SpatialIsolation,
    pub resolution: Resolution,
    pub consensus: Consensus,
    pub established_post: EstablishedPost,
    pub alternative_corridors: AlternativeCorridors,
    pub modality: Modality,
    pub authored_resolution: AuthoredResolution,
}

/// `consolidation.*` (POST-R070).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Consolidation {
    pub maximum_diameter_metres: Option<f64>,
    pub maximum_chainage_difference_metres: Option<f64>,
    pub maximum_heading_difference_degrees: Option<f64>,
    pub opposite_side_tolerance_metres: Option<f64>,
    pub distinguished_context_minimum_count: Option<i32>,
    pub distinguished_context_minimum_score: Option<f64>,
    pub distinguished_context_minimum_margin: Option<f64>,
}

/// `geometry_weights.*` (POST-R082).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct GeometryWeights {
    pub alignment: Option<f64>,
    pub side: Option<f64>,
    pub proximity: Option<f64>,
    pub routed_excess: Option<f64>,
}

/// `hard_gates.*` (POST-R081, R082).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct HardGates {
    pub centreline_neutral_tolerance_metres: Option<f64>,
    pub maximum_corridor_distance_metres: Option<f64>,
    pub maximum_routed_excess_metres: Option<f64>,
}

/// `spatial_isolation.*` (POST-R085).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SpatialIsolation {
    pub minimum_separation_metres: Option<f64>,
    pub maximum_adjustment: Option<f64>,
}

/// `resolution.*` (POST-R087).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Resolution {
    /// Validated, never read by a decision (POST-R089).
    pub minimum_plausible_score: Option<f64>,
    pub minimum_physical_score: Option<f64>,
    pub minimum_physical_margin: Option<f64>,
    pub material_routed_advantage: Option<f64>,
}

/// `consensus.*` (POST-R086, R087).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Consensus {
    pub minimum_contexts: Option<i32>,
    pub minimum_winning_share: Option<f64>,
    pub minimum_per_context_score: Option<f64>,
    pub minimum_per_context_lead: Option<f64>,
}

/// `established_post.*` (POST-R085, R086).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct EstablishedPost {
    pub minimum_supporting_contexts: Option<i32>,
    pub minimum_runner_up_ratio: Option<f64>,
    pub maximum_geometry_disadvantage: Option<f64>,
    pub maximum_popularity_adjustment: Option<f64>,
    pub per_context_adjustment: Option<f64>,
}

/// `alternative_corridors.*` (POST-R080).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct AlternativeCorridors {
    pub absolute_tie_metres: Option<f64>,
    pub relative_tie_fraction: Option<f64>,
    pub maximum_variants: Option<i32>,
}

/// `modality.*` (POST-R083).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Modality {
    pub source_support_per_weight: Option<f64>,
    pub maximum_source_support_adjustment: Option<f64>,
    pub explicit_support_adjustment: Option<f64>,
    pub estimated_support_adjustment: Option<f64>,
    pub conflict_adjustment: Option<f64>,
    pub maximum_combined_supporting_adjustment: Option<f64>,
}

/// `authored_resolution.*` (POST-R122).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct AuthoredResolution {
    pub minimum_physical_score: Option<f64>,
    pub minimum_physical_margin: Option<f64>,
    pub require_unanimous_contexts: Option<bool>,
}

/// The learned two-stage model (section 2.5).
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub stage1: Stage,
    pub stage2: Stage,
    pub thresholds: Thresholds,
}

/// One conditional-logit stage.
#[derive(Clone, Debug, PartialEq)]
pub struct Stage {
    pub features: Vec<String>,
    pub mean: Vec<f64>,
    pub scale: Vec<f64>,
    pub weights: Vec<f64>,
    pub null_utility: f64,
    /// Written by the Python trainer; read but ignored by F# and by the evaluator.
    pub threshold: Option<f64>,
}

/// Decision thresholds (POST-R108).
#[derive(Clone, Debug, PartialEq)]
pub struct Thresholds {
    pub simple: f64,
    pub complex: f64,
    /// Present only when the stored value is a number.
    pub area: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDocument {
    schema_version: Value,
    policy: Option<Value>,
    scorer: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawScorer {
    kind: Value,
    #[serde(default)]
    model: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawModel {
    format: String,
    stage1: RawStage,
    stage2: RawStage,
    thresholds: RawThresholds,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStage {
    format: String,
    features: Vec<String>,
    mean: Vec<f64>,
    scale: Vec<f64>,
    weights: Vec<f64>,
    null_utility: f64,
    #[serde(default)]
    threshold: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawThresholds {
    simple: f64,
    complex: f64,
    #[serde(default)]
    area: Option<Value>,
}

/// Why a policy document was rejected.
#[derive(Debug, thiserror::Error)]
pub enum PolicyDocumentError {
    #[error("policy document is not UTF-8")]
    Encoding,
    #[error("policy document: {0}")]
    Json(#[from] serde_json::Error),
    #[error("post-inference policy has no numeric schema_version")]
    NoSchemaVersion,
    #[error("unsupported policy schema {0}; expected 2 or 3")]
    SchemaVersion(i64),
    #[error("policy document v3 requires a policy object")]
    NoPolicy,
    #[error("policy document v3 requires a scorer object")]
    NoScorer,
    #[error("scorer kind must be heuristic or learned")]
    ScorerKind,
    #[error("a learned scorer requires a model object")]
    NoModel,
    #[error("scorer model: {0}")]
    Model(&'static str),
}

/// Parses a policy document.
pub fn parse(bytes: &[u8]) -> Result<PolicyDocument, PolicyDocumentError> {
    let text = std::str::from_utf8(bytes).map_err(|_| PolicyDocumentError::Encoding)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let sha256 = sha256_hex(text.as_bytes());
    let root: Value = serde_json::from_str(text)?;
    let version = root
        .get("schema_version")
        .and_then(Value::as_i64)
        .ok_or(PolicyDocumentError::NoSchemaVersion)?;
    match version {
        POLICY_SCHEMA_VERSION => Ok(PolicyDocument {
            sha256,
            schema_version: version,
            policy: serde_json::from_value(root)?,
            scorer: Scorer::Heuristic,
        }),
        DOCUMENT_SCHEMA_VERSION => {
            let raw: RawDocument = serde_json::from_value(root)?;
            let _ = raw.schema_version;
            let policy = raw
                .policy
                .filter(Value::is_object)
                .ok_or(PolicyDocumentError::NoPolicy)?;
            let scorer = raw
                .scorer
                .filter(Value::is_object)
                .ok_or(PolicyDocumentError::NoScorer)?;
            let scorer: RawScorer = serde_json::from_value(scorer)?;
            let scorer = match scorer.kind.as_str() {
                Some("heuristic") => Scorer::Heuristic,
                Some("learned") => {
                    let model = scorer
                        .model
                        .filter(Value::is_object)
                        .ok_or(PolicyDocumentError::NoModel)?;
                    Scorer::Learned(Box::new(parse_model(model)?))
                }
                _ => return Err(PolicyDocumentError::ScorerKind),
            };
            Ok(PolicyDocument {
                sha256,
                schema_version: version,
                policy: serde_json::from_value(policy)?,
                scorer,
            })
        }
        other => Err(PolicyDocumentError::SchemaVersion(other)),
    }
}

/// Parses a `post-scorer-two-stage-v1` model object (F# `JdfPostScorer.parseModel`).
pub fn parse_model(value: Value) -> Result<Model, PolicyDocumentError> {
    let raw: RawModel = serde_json::from_value(value)?;
    if raw.format != MODEL_FORMAT {
        return Err(PolicyDocumentError::Model("unsupported model format"));
    }
    let stage1 = stage(raw.stage1)?;
    let stage2 = stage(raw.stage2)?;
    let expected: Vec<&str> = stage1
        .features
        .iter()
        .map(String::as_str)
        .chain(COLLECTIVE_FEATURES)
        .collect();
    if stage2.features != expected {
        return Err(PolicyDocumentError::Model(
            "stage 2 must use the stage 1 features followed by the collective features",
        ));
    }
    let threshold = |value: f64| {
        if value.is_finite() && value >= 0.0 {
            Ok(value)
        } else {
            Err(PolicyDocumentError::Model("invalid threshold"))
        }
    };
    let area = match raw.thresholds.area {
        Some(Value::Number(number)) => Some(threshold(
            number
                .as_f64()
                .ok_or(PolicyDocumentError::Model("invalid threshold"))?,
        )?),
        _ => None,
    };
    Ok(Model {
        stage1,
        stage2,
        thresholds: Thresholds {
            simple: threshold(raw.thresholds.simple)?,
            complex: threshold(raw.thresholds.complex)?,
            area,
        },
    })
}

fn stage(raw: RawStage) -> Result<Stage, PolicyDocumentError> {
    if raw.format != STAGE_FORMAT {
        return Err(PolicyDocumentError::Model("unsupported stage format"));
    }
    let count = raw.features.len();
    if count == 0
        || raw.mean.len() != count
        || raw.scale.len() != count
        || raw.weights.len() != count
    {
        return Err(PolicyDocumentError::Model(
            "stage vectors must match its feature list",
        ));
    }
    let finite = raw
        .mean
        .iter()
        .chain(&raw.scale)
        .chain(&raw.weights)
        .chain(std::iter::once(&raw.null_utility))
        .all(|value| value.is_finite());
    if !finite || raw.scale.iter().any(|&scale| scale <= 0.0) {
        return Err(PolicyDocumentError::Model(
            "stage parameters must be finite with positive scales",
        ));
    }
    let mut names: Vec<&str> = raw.features.iter().map(String::as_str).collect();
    names.sort_unstable();
    names.dedup();
    if names.len() != count {
        return Err(PolicyDocumentError::Model(
            "stage features must be distinct",
        ));
    }
    Ok(Stage {
        features: raw.features,
        mean: raw.mean,
        scale: raw.scale,
        weights: raw.weights,
        null_utility: raw.null_utility,
        threshold: raw.threshold,
    })
}
