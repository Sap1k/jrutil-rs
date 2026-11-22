//! POST-R062: the pack walked as one ordered join, per stop: the stop's route points and one
//! row per (context, variant, route point).

use std::ops::Range;

use jr_formats::post_evidence::{Availability, Pack};

/// A route point of the stop being evaluated (F# `ReplayRoutePoint`).
#[derive(Clone, Debug)]
pub(crate) struct Point<'a> {
    /// Row in the pack's route points.
    pub row: u32,
    pub id: &'a str,
    pub latitude: f64,
    pub longitude: f64,
    /// Member observation IDs in ordinal order.
    pub observations: Vec<&'a str>,
    pub current: bool,
    pub obsolete: bool,
    pub support: f64,
    pub explicit_modes: Vec<&'a str>,
    pub denied_modes: Vec<&'a str>,
}

/// One evidence row: a context, a variant and a candidate (a route point before consolidation,
/// a hypothesis after), with the variant's and the attachment's facts (F# `ReplayScore`).
#[derive(Clone, Debug)]
pub(crate) struct Row<'a> {
    /// Row in the pack's contexts.
    pub context: u32,
    pub rank: i32,
    pub relative_cost: Option<f64>,
    pub relative_fraction: Option<f64>,
    /// Index of the point, or of the hypothesis after consolidation.
    pub candidate: usize,
    pub excess: Option<f64>,
    pub face: Option<&'a str>,
    pub corridor_distance: Option<f64>,
    pub offset: Option<f64>,
    pub corridor_heading: Option<f64>,
    pub attachment_heading: Option<f64>,
    pub available: bool,
    pub failure: Option<&'a str>,
    pub service_edges: i32,
    pub alternative_count: usize,
    pub cost_gap: Option<f64>,
    pub tied_agree: bool,
}

fn split(text: &str) -> Vec<&str> {
    text.split(';').filter(|token| !token.is_empty()).collect()
}

/// The route points of rows `points` of the pack, with their members.
pub(crate) fn points<'a>(
    pack: &'a Pack,
    points: Range<usize>,
    members: &[Vec<&'a str>],
) -> Vec<Point<'a>> {
    let p = &pack.route_points;
    points
        .map(|row| Point {
            row: row as u32,
            id: &p.route_point_id[row],
            latitude: p.latitude[row],
            longitude: p.longitude[row],
            observations: members[row].clone(),
            current: p.has_current_lifecycle[row],
            obsolete: p.has_obsolete_lifecycle[row],
            support: p.source_support_weight[row],
            explicit_modes: split(&p.explicit_modes[row]),
            denied_modes: split(&p.denied_modes[row]),
        })
        .collect()
}

/// The rows of contexts `contexts` (one stop), in context, rank and route-point order.
/// `first_point` is the stop's first route-point row; `variants` the first variant row of each
/// context plus one past the end.
pub(crate) fn rows<'a>(
    pack: &'a Pack,
    contexts: Range<usize>,
    variant_starts: &[usize],
    attachment_starts: &[usize],
    first_point: usize,
) -> Vec<Row<'a>> {
    let v = &pack.corridor_variants;
    let a = &pack.route_point_evidence;
    let mut rows = Vec::new();
    for context in contexts {
        for variant in variant_starts[context]..variant_starts[context + 1] {
            let variant_failure = v.invariant_failure_reason[variant].as_deref();
            for attachment in attachment_starts[variant]..attachment_starts[variant + 1] {
                let failure = a.invariant_failure_reason[attachment]
                    .as_deref()
                    .or(variant_failure);
                rows.push(Row {
                    context: context as u32,
                    rank: v.variant_rank[variant],
                    relative_cost: v.relative_cost_metres[variant],
                    relative_fraction: v.relative_cost_fraction[variant],
                    candidate: a.route_point[attachment] as usize - first_point,
                    excess: a.routed_excess_metres[attachment],
                    face: a.corridor_face_id[attachment].as_deref(),
                    corridor_distance: a.corridor_distance_metres[attachment],
                    offset: a.signed_lateral_offset_metres[attachment],
                    corridor_heading: a.corridor_heading_degrees[attachment],
                    attachment_heading: a.attachment_heading_degrees[attachment],
                    available: failure.is_none()
                        && a.routing_availability[attachment] == Availability::Available,
                    failure,
                    service_edges: v.service_edge_count[variant],
                    alternative_count: 0,
                    cost_gap: None,
                    tied_agree: true,
                });
            }
        }
    }
    rows
}
