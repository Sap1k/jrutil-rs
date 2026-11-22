//! The full result of a judged comparison as JSON, object keys in sorted order.

use serde_json::{Map, Value, json};

use super::Comparison;
use super::ledger::Verdict;
use super::tally::{Bucket, Sample};

/// Counts, statuses and samples of `comparison` judged by `verdict`, as pretty JSON.
pub fn report_json(comparison: &Comparison, verdict: &Verdict) -> String {
    let relations: Vec<Value> = comparison
        .relations
        .iter()
        .map(|relation| {
            json!({
                "name": relation.name,
                "mode": relation.mode.as_str(),
                "rows_a": relation.rows[0],
                "rows_b": relation.rows[1],
            })
        })
        .collect();
    let categories: Vec<Value> = verdict
        .defaults
        .iter()
        .map(|default| {
            let groups: Vec<Value> = comparison.categories[&default.category]
                .groups
                .iter()
                .map(|(ledger, bucket)| {
                    json!({ "ledger": ledger, "count": bucket.count, "samples": samples(bucket) })
                })
                .collect();
            json!({
                "category": default.category,
                "count": default.count,
                "unexplained": default.unexplained,
                "status": default.status.as_str(),
                "groups": groups,
            })
        })
        .collect();
    let ledger: Vec<Value> = verdict
        .categories
        .iter()
        .map(|category| {
            json!({
                "name": category.name,
                "matches": category.matches,
                "reason": category.reason,
                "count": category.count,
                "min": category.min,
                "max": category.max,
                "status": category.status.as_str(),
            })
        })
        .collect();
    let unreferenced: Vec<Value> = comparison
        .unreferenced
        .iter()
        .map(|item| json!({ "target": item.target, "a": item.counts[0], "b": item.counts[1] }))
        .collect();
    let trip_pairing = comparison.trip_pairing.map(|pairing| {
        json!({
            "by_id": pairing.by_id,
            "by_content": pairing.by_content,
            "near": pairing.near,
            "by_dates": pairing.by_dates,
            "unpaired_a": pairing.unpaired[0],
            "unpaired_b": pairing.unpaired[1],
        })
    });
    let report = json!({
        "trip_identity": if trip_pairing.is_some() { "journey" } else { "trip_id" },
        "trip_pairing": trip_pairing,
        "passed": verdict.passed(),
        "differences": comparison.differences(),
        "relations": relations,
        "categories": categories,
        "ledger": ledger,
        "unreferenced": unreferenced,
    });
    let mut text = serde_json::to_string_pretty(&sorted(report)).unwrap_or_default();
    text.push('\n');
    text
}

fn samples(bucket: &Bucket) -> Vec<Value> {
    bucket
        .samples
        .iter()
        .map(|Sample { key, column, a, b }| json!({ "key": key, "column": column, "a": a, "b": b }))
        .collect()
}

/// The value with every object's keys sorted (serde_json keeps insertion order here).
fn sorted(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|x, y| x.0.cmp(&y.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, sorted(value)))
                    .collect::<Map<_, _>>(),
            )
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}
