//! Small hand-built evidence packs for the evaluator's tests.
#![allow(dead_code)]

use jr_formats::post_evidence::ids::{context_id, movement_family_id, same_stop_block_id};
use jr_formats::post_evidence::{
    Availability, BlockRole, Contexts, CorridorVariants, Observations, Pack, PackIdentity,
    RoutePointEvidence, RoutePoints, RouterEvidence,
};
use jr_formats::post_policy;
use jr_posts::evaluate::{EvaluationPolicy, Settings};

/// An observation of a route point.
#[derive(Clone, Debug)]
pub struct Obs {
    pub id: String,
    pub raw_tags: String,
}

pub fn osm(node: u64) -> Obs {
    Obs {
        id: format!("osm:node:{node}"),
        raw_tags: "highway=bus_stop".into(),
    }
}

pub fn catalogue(file: &str, n: u64) -> Obs {
    Obs {
        id: format!("external:{file}:sha256:{n:064}"),
        raw_tags: String::new(),
    }
}

/// A route point; `id` must sort as the points of a stop are listed.
#[derive(Clone, Debug)]
pub struct Point {
    pub id: String,
    pub latitude: f64,
    pub longitude: f64,
    pub observations: Vec<Obs>,
    pub explicit_modes: String,
    pub denied_modes: String,
    pub obsolete: bool,
}

/// A point `north` metres north and `east` metres east of 50 N, 14 E.
pub fn point(id: &str, north: f64, east: f64, observations: Vec<Obs>) -> Point {
    Point {
        id: format!("route-point:{id}"),
        latitude: 50.0 + north / 110_540.0,
        longitude: 14.0 + east / (111_320.0 * (50.0f64 * std::f64::consts::PI / 180.0).cos()),
        observations,
        explicit_modes: String::new(),
        denied_modes: String::new(),
        obsolete: false,
    }
}

/// An attachment of a route point to a variant.
#[derive(Clone, Debug)]
pub struct Att {
    pub excess: f64,
    pub face: String,
    pub distance: f64,
    pub offset: f64,
    pub corridor_heading: f64,
    pub attachment_heading: f64,
    pub failure: Option<String>,
}

/// A right-side (3 m), aligned attachment `distance` metres from the corridor without detour: geometry
/// `1 − 0.004 × distance`.
pub fn good(face: &str, distance: f64) -> Att {
    Att {
        excess: 0.0,
        face: face.into(),
        distance,
        offset: -3.0,
        corridor_heading: 90.0,
        attachment_heading: 90.0,
        failure: None,
    }
}

/// An attachment that could not be routed.
pub fn unreachable() -> Att {
    Att {
        failure: Some("routed-excess-or-disconnected".into()),
        ..good("", 0.0)
    }
}

/// A corridor variant with one attachment per route point of the stop, in point order.
#[derive(Clone, Debug)]
pub struct Variant {
    pub relative_cost: f64,
    pub relative_fraction: f64,
    pub ingress: String,
    pub egress: String,
    pub service_edges: i32,
    pub attachments: Vec<Att>,
    pub unavailable: Option<String>,
}

pub fn variant(ingress: &str, attachments: Vec<Att>) -> Variant {
    Variant {
        relative_cost: 0.0,
        relative_fraction: 0.0,
        ingress: ingress.into(),
        egress: "egress".into(),
        service_edges: 0,
        attachments,
        unavailable: None,
    }
}

/// A context.
#[derive(Clone, Debug)]
pub struct Context {
    pub stop: u64,
    pub mode: String,
    pub line: String,
    pub direction: i32,
    pub pattern: String,
    pub position: i32,
    pub role: BlockRole,
    pub previous: Option<u64>,
    pub next: Option<u64>,
    pub authored: Option<String>,
    pub variants: Vec<Variant>,
}

pub fn context(stop: u64, line: &str, variants: Vec<Variant>) -> Context {
    Context {
        stop,
        mode: "A".into(),
        line: line.into(),
        direction: 0,
        pattern: format!("pattern-{line}"),
        position: 1,
        role: BlockRole::Through,
        previous: Some(stop + 1000),
        next: Some(stop + 2000),
        authored: None,
        variants,
    }
}

/// The pack of `stops` (stop number and its points) and `contexts`.
pub fn pack(stops: &[(u64, Vec<Point>)], contexts: Vec<Context>) -> Pack {
    pack_with(stops, contexts, RouterEvidence::Rust)
}

pub fn pack_with(
    stops: &[(u64, Vec<Point>)],
    mut contexts: Vec<Context>,
    router: RouterEvidence,
) -> Pack {
    let mut stops = stops.to_vec();
    stops.sort_by_key(|(stop, _)| *stop);
    let mut route_points = RoutePoints::default();
    let mut observations: Vec<(u64, String, u32, String)> = Vec::new();
    let mut first_point = std::collections::BTreeMap::new();
    for (stop, points) in &stops {
        let mut points = points.clone();
        points.sort_by(|a, b| a.id.cmp(&b.id));
        first_point.insert(*stop, (route_points.len(), points.len()));
        for point in points {
            let row = route_points.len() as u32;
            let mut ids: Vec<&str> = point.observations.iter().map(|o| o.id.as_str()).collect();
            ids.sort_unstable();
            route_points.stop.push(*stop);
            route_points.route_point_id.push(point.id.clone().into());
            route_points
                .representative_observation_id
                .push(ids[0].into());
            route_points.latitude.push(point.latitude);
            route_points.longitude.push(point.longitude);
            route_points.has_current_lifecycle.push(!point.obsolete);
            route_points.has_obsolete_lifecycle.push(point.obsolete);
            route_points
                .source_support_weight
                .push(point.observations.len() as f64);
            route_points
                .explicit_modes
                .push(point.explicit_modes.clone().into());
            route_points
                .denied_modes
                .push(point.denied_modes.clone().into());
            for observation in &point.observations {
                observations.push((
                    *stop,
                    observation.id.clone(),
                    row,
                    observation.raw_tags.clone(),
                ));
            }
        }
    }
    observations.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    let mut table = Observations::default();
    for (_, id, row, tags) in observations {
        let point = row as usize;
        let obsolete = route_points.has_obsolete_lifecycle[point];
        table.route_point.push(row);
        table.observation_id.push(id.into());
        table.source_kind.push("osm".into());
        table.source_object_id.push(None);
        table.observed_at.push(None);
        table.latitude.push(route_points.latitude[point]);
        table.longitude.push(route_points.longitude[point]);
        table.support_weight.push(1.0);
        table.raw_tags.push(tags.into());
        table
            .explicit_modes
            .push(route_points.explicit_modes[point].to_lowercase().into());
        table
            .denied_modes
            .push(route_points.denied_modes[point].to_lowercase().into());
        table
            .lifecycle
            .push(if obsolete { "abandoned" } else { "active" }.into());
    }

    contexts.sort_by(|a, b| {
        let key = |c: &Context| {
            (
                c.stop,
                c.mode.clone(),
                c.line.clone(),
                c.direction,
                c.pattern.clone(),
                c.position,
                c.role.as_str(),
                c.authored.clone().unwrap_or_default(),
            )
        };
        key(a).cmp(&key(b))
    });
    let mut table_contexts = Contexts::default();
    let mut variants = CorridorVariants::default();
    let mut attachments = RoutePointEvidence::default();
    for (row, c) in contexts.iter().enumerate() {
        let id = context_id(
            c.stop,
            &c.mode,
            &c.line,
            0,
            c.direction,
            &c.pattern,
            c.position,
            c.role.as_str(),
            c.authored.as_deref(),
        );
        let family = movement_family_id(
            c.stop,
            &c.mode,
            c.variants[0]
                .unavailable
                .is_none()
                .then_some(c.variants[0].ingress.as_str()),
            c.variants[0]
                .unavailable
                .is_none()
                .then_some(c.variants[0].egress.as_str()),
            c.role.as_str(),
        );
        table_contexts.context_id.push(id.into());
        table_contexts.stop.push(c.stop);
        table_contexts.mode.push(c.mode.clone().into());
        table_contexts.line_id.push(c.line.clone().into());
        table_contexts.route_distinction.push(0);
        table_contexts.direction.push(c.direction);
        table_contexts.pattern_hash.push(c.pattern.clone().into());
        table_contexts.pattern_position.push(c.position);
        table_contexts.role.push(c.role);
        table_contexts.movement_family_id.push(family.into());
        table_contexts.previous_stop.push(c.previous);
        table_contexts.next_stop.push(c.next);
        table_contexts
            .authored_post_key
            .push(c.authored.clone().map(Into::into));
        table_contexts.same_stop_block_id.push(match c.role {
            BlockRole::Through => None,
            _ => {
                Some(same_stop_block_id(&c.line, 0, &c.pattern, c.position, c.position + 1).into())
            }
        });
        let (first, count) = first_point[&c.stop];
        let mut cost = 100.0;
        for (rank, v) in c.variants.iter().enumerate() {
            let variant_row = variants.len() as u32;
            let available = v.unavailable.is_none();
            variants.context.push(row as u32);
            variants.variant_rank.push(rank as i32);
            let set = |value: f64| available.then_some(value);
            variants
                .corridor_id
                .push(available.then(|| format!("corridor-{rank}").into()));
            cost += v.relative_cost;
            variants.absolute_cost_metres.push(set(cost));
            variants.relative_cost_metres.push(set(v.relative_cost));
            variants
                .relative_cost_fraction
                .push(set(v.relative_fraction));
            variants.path_length_metres.push(set(cost));
            variants
                .directed_edge_count
                .push(if available { 5 } else { 0 });
            variants.repeated_directed_edge_count.push(0);
            variants.service_edge_count.push(v.service_edges);
            variants.restricted_access_edge_count.push(0);
            variants.access_penalty_metres.push(set(0.0));
            variants
                .ingress_thread_id
                .push(available.then(|| v.ingress.clone().into()));
            variants
                .egress_thread_id
                .push(available.then(|| v.egress.clone().into()));
            variants.routing_availability.push(if available {
                Availability::Available
            } else {
                Availability::Unavailable
            });
            variants
                .invariant_failure_reason
                .push(v.unavailable.clone().map(Into::into));
            assert_eq!(v.attachments.len(), count, "one attachment per point");
            for (offset, att) in v.attachments.iter().enumerate() {
                let ok = available && att.failure.is_none();
                let value = |x: f64| ok.then_some(x);
                attachments.variant.push(variant_row);
                attachments.route_point.push((first + offset) as u32);
                attachments.routed_excess_metres.push(value(att.excess));
                attachments.snap_edge_id.push(ok.then_some(1));
                attachments.snap_way_id.push(ok.then_some(1));
                attachments.snap_fraction.push(value(0.5));
                attachments.snap_projected_longitude.push(value(14.0));
                attachments.snap_projected_latitude.push(value(50.0));
                attachments.snap_distance_metres.push(value(att.distance));
                attachments
                    .corridor_face_id
                    .push(ok.then(|| att.face.clone().into()));
                attachments
                    .corridor_distance_metres
                    .push(value(att.distance));
                attachments
                    .signed_lateral_offset_metres
                    .push(value(att.offset));
                attachments
                    .corridor_heading_degrees
                    .push(value(att.corridor_heading));
                attachments
                    .attachment_heading_degrees
                    .push(value(att.attachment_heading));
                attachments.heading_difference_degrees.push(value(0.0));
                attachments
                    .proximity_distance_metres
                    .push(value(att.distance));
                attachments.routing_availability.push(if ok {
                    Availability::Available
                } else {
                    Availability::Unavailable
                });
                attachments.invariant_failure_reason.push(if ok {
                    None
                } else {
                    Some(
                        att.failure
                            .clone()
                            .or(v.unavailable.clone())
                            .unwrap_or_default()
                            .into(),
                    )
                });
            }
        }
    }
    Pack {
        identity: PackIdentity {
            capture_tool_version: "test-tool".into(),
            merged_jdf_sha256: "0".repeat(64),
            routing_pbf_sha256: "1".repeat(64),
            source_id: "test-source".into(),
            router,
        },
        observations: table,
        route_points,
        contexts: table_contexts,
        corridor_variants: variants,
        route_point_evidence: attachments,
    }
}

/// The production policy (`conservative-routed-v4`) as a v2 document text.
pub const POLICY_V2: &str = r#"{
  "schema_version": 2,
  "policy_id": "conservative-routed-v4|tuned-safe-v3|kostany-diagnostic-best-safe",
  "consolidation": {"maximum_diameter_metres": 2.0, "maximum_chainage_difference_metres": 20.0,
    "maximum_heading_difference_degrees": 30.0, "opposite_side_tolerance_metres": 2.0,
    "distinguished_context_minimum_count": 2, "distinguished_context_minimum_score": 0.55,
    "distinguished_context_minimum_margin": 0.15},
  "geometry_weights": {"alignment": 0.40, "side": 0.25, "proximity": 0.20, "routed_excess": 0.15},
  "hard_gates": {"centreline_neutral_tolerance_metres": 2.0, "maximum_corridor_distance_metres": 100.0,
    "maximum_routed_excess_metres": 500.0},
  "spatial_isolation": {"minimum_separation_metres": 12.0, "maximum_adjustment": 0.05},
  "resolution": {"minimum_plausible_score": 0.90, "minimum_physical_score": 0.98,
    "minimum_physical_margin": 0.02, "material_routed_advantage": 0.05},
  "consensus": {"minimum_contexts": 1, "minimum_winning_share": 0.67,
    "minimum_per_context_score": 0.90, "minimum_per_context_lead": 0.02},
  "established_post": {"minimum_supporting_contexts": 3, "minimum_runner_up_ratio": 2.0,
    "maximum_geometry_disadvantage": 0.15, "maximum_popularity_adjustment": 0.10,
    "per_context_adjustment": 0.02},
  "alternative_corridors": {"absolute_tie_metres": 0.0, "relative_tie_fraction": 0.02, "maximum_variants": 3},
  "modality": {"source_support_per_weight": 0.01, "maximum_source_support_adjustment": 0.05,
    "explicit_support_adjustment": 0.05, "estimated_support_adjustment": 0.03,
    "conflict_adjustment": -0.05, "maximum_combined_supporting_adjustment": 0.10},
  "authored_resolution": {"minimum_physical_score": 0.75, "minimum_physical_margin": 0.20,
    "require_unanimous_contexts": true}
}"#;

/// The heuristic policy.
pub fn heuristic() -> EvaluationPolicy {
    let document = post_policy::parse(POLICY_V2.as_bytes()).expect("policy");
    EvaluationPolicy::from_document(&document).expect("valid policy")
}

/// The production policy with `edit` applied to its JSON value.
pub fn heuristic_with(
    edit: impl FnOnce(&mut serde_json::Value),
) -> Result<EvaluationPolicy, jr_posts::evaluate::PolicyError> {
    let mut value: serde_json::Value = serde_json::from_str(POLICY_V2).unwrap();
    edit(&mut value);
    let document = post_policy::parse(value.to_string().as_bytes()).expect("document");
    Settings::new(&document.policy)?;
    EvaluationPolicy::from_document(&document)
}

/// A learned document with one stage-1 feature (`side`) of weight 0: every candidate of a
/// context is equally likely; `null` is the null option's utility.
pub fn learned(null: f64, simple: f64, area: Option<f64>) -> EvaluationPolicy {
    let stage = |features: &[&str]| {
        let n = features.len();
        serde_json::json!({
            "format": "post-scorer-conditional-logit-v1",
            "features": features,
            "mean": vec![0.0; n], "scale": vec![1.0; n], "weights": vec![0.0; n],
            "null_utility": (null), "threshold": 0.5
        })
    };
    let mut thresholds = serde_json::json!({"simple": simple, "complex": simple});
    if let Some(area) = area {
        thresholds["area"] = serde_json::json!(area);
    }
    let document = serde_json::json!({
        "schema_version": 3,
        "policy": serde_json::from_str::<serde_json::Value>(POLICY_V2).unwrap(),
        "scorer": {"kind": "learned", "model": {
            "format": "post-scorer-two-stage-v1",
            "stage1": stage(&["side"]),
            "stage2": stage(&["side", "reverse_mass", "has_reverse", "same_movement_mass",
                              "has_same_movement", "same_next_mass", "same_previous_mass"]),
            "thresholds": thresholds
        }}
    });
    let document = post_policy::parse(document.to_string().as_bytes()).expect("document");
    EvaluationPolicy::from_document(&document).expect("valid policy")
}
