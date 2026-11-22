//! The post estimator's policy document (POST-R004, R181).

use jr_formats::post_policy::{self, PolicyDocumentError, Scorer};
use serde_json::{Value, json};

fn stage(features: &[&str]) -> Value {
    let n = features.len();
    json!({"format": "post-scorer-conditional-logit-v1", "features": features,
           "mean": vec![0.0; n], "scale": vec![1.0; n], "weights": vec![0.5; n],
           "null_utility": -2.0, "threshold": 0.5})
}

fn collective(first: &[&'static str]) -> Vec<&'static str> {
    let mut features = first.to_vec();
    features.extend(post_policy::COLLECTIVE_FEATURES);
    features
}

fn document(model: Value) -> Value {
    json!({"schema_version": 3,
           "policy": {"schema_version": 2, "policy_id": "p", "consolidation": {"maximum_diameter_metres": 2}},
           "scorer": {"kind": "learned", "model": model}})
}

fn model(stage2: &[&str], thresholds: Value) -> Value {
    json!({"format": "post-scorer-two-stage-v1", "stage1": stage(&["side"]),
           "stage2": stage(stage2), "thresholds": thresholds})
}

fn parse(value: &Value) -> Result<post_policy::PolicyDocument, PolicyDocumentError> {
    post_policy::parse(value.to_string().as_bytes())
}

#[test]
fn post_r004_a_v3_document_carries_its_policy_and_learned_model() {
    let value = document(model(
        &collective(&["side"]),
        json!({"simple": 0.6, "complex": 0.58, "area": 0.67}),
    ));
    let parsed = parse(&value).unwrap();
    assert_eq!(parsed.schema_version, 3);
    assert_eq!(parsed.policy.policy_id.as_deref(), Some("p"));
    assert_eq!(
        parsed.policy.consolidation.maximum_diameter_metres,
        Some(2.0)
    );
    assert_eq!(
        parsed.policy.hard_gates.maximum_routed_excess_metres, None,
        "missing stays missing"
    );
    let Scorer::Learned(model) = parsed.scorer else {
        panic!("learned")
    };
    assert_eq!(model.thresholds.area, Some(0.67));
    assert_eq!(model.stage1.threshold, Some(0.5));
    assert_eq!(
        parsed.sha256,
        jr_formats::post_evidence::ids::sha256_hex(value.to_string().as_bytes())
    );
}

#[test]
fn post_r004_a_v2_file_names_the_heuristic_and_the_area_threshold_is_optional() {
    let parsed = parse(&json!({"schema_version": 2, "policy_id": "p"})).unwrap();
    assert_eq!(parsed.scorer, Scorer::Heuristic);
    let value = document(model(
        &collective(&["side"]),
        json!({"simple": 0.6, "complex": 0.58, "area": null}),
    ));
    let Scorer::Learned(model) = parse(&value).unwrap().scorer else {
        panic!()
    };
    assert_eq!(model.thresholds.area, None);
}

#[test]
fn post_r004_a_byte_order_mark_is_not_part_of_the_document_hash() {
    let text = json!({"schema_version": 2, "policy_id": "p"}).to_string();
    let mut bytes = b"\xEF\xBB\xBF".to_vec();
    bytes.extend(text.as_bytes());
    assert_eq!(
        post_policy::parse(&bytes).unwrap().sha256,
        jr_formats::post_evidence::ids::sha256_hex(text.as_bytes())
    );
}

#[test]
fn post_r004_model_parsing_rejects_a_stage_2_without_the_collective_features() {
    let value = document(model(&["side"], json!({"simple": 0.8, "complex": 0.9})));
    assert!(matches!(parse(&value), Err(PolicyDocumentError::Model(_))));
}

#[test]
fn post_r004_other_schema_versions_and_scorer_kinds_are_rejected() {
    assert!(matches!(
        parse(&json!({"schema_version": 4})),
        Err(PolicyDocumentError::SchemaVersion(4))
    ));
    let mut value = document(json!({}));
    value["scorer"] = json!({"kind": "neural"});
    assert!(matches!(
        parse(&value),
        Err(PolicyDocumentError::ScorerKind)
    ));
    value["scorer"] = json!({"kind": "learned"});
    assert!(matches!(parse(&value), Err(PolicyDocumentError::NoModel)));
}

#[test]
fn post_r004_non_positive_scales_are_rejected() {
    let mut value = model(
        &collective(&["side"]),
        json!({"simple": 0.6, "complex": 0.6}),
    );
    value["stage1"]["scale"] = json!([0.0]);
    assert!(matches!(
        parse(&document(value)),
        Err(PolicyDocumentError::Model(_))
    ));
}

#[test]
fn post_r181_unknown_policy_keys_are_rejected() {
    let typo = json!({"schema_version": 2, "policy_id": "p", "consolidation": {"maximum_diametre_metres": 2}});
    assert!(matches!(parse(&typo), Err(PolicyDocumentError::Json(_))));
    let mut value = document(model(
        &collective(&["side"]),
        json!({"simple": 0.6, "complex": 0.6}),
    ));
    value["scorer"]["model"]["thresholds"]["areas"] = json!(0.5);
    assert!(parse(&value).is_err());
}
