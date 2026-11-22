//! Routed evidence of one context (POST-R050–R053): corridor variants from the previous
//! anchor to the next, and where every route point of the stop attaches to each.

use std::collections::BTreeMap;

use jr_model::{Coord, Rule};
use serde::{Deserialize, Serialize};

use crate::laea::{self, Laea};

use super::geometry::angle_difference;
use super::graph::{RoutingGraph, RoutingMode};
use super::policy::RoutingPolicy;
use super::reader::{ReadBox, Reader};
use super::search::{Direction, Path};
use super::snap::Snap;
use super::thread::{CorridorId, DirectedThread, Face};

/// The previous and the next anchor coordinates each give their first few snaps; of those,
/// distinct by (edge, fraction), the nearest per directed thread is kept, and the nearest few
/// of those in (distance, edge, fraction) order. One snap per carriageway direction keeps a
/// dense station from multiplying the searches while every way out of it stays.
pub const POST_R050: Rule = Rule {
    id: "POST-R050",
    summary: "anchor snaps: the nearest per directed thread, a few per side",
};

/// Every (start, finish) pair of anchor snaps is routed (R021); the paths found, distinct by
/// corridor (the first in pair order kept), are ordered by (cost, corridor) and the cheapest
/// few kept. No snap on a side gives one unavailable variant `snap-unavailable`, no path one
/// `disconnected`; then every route point gets an unavailable attachment
/// `baseline-unavailable`. Costs relative to the cheapest variant, the access penalty and the
/// edge counts are facts for the scorer; no publication threshold applies here.
pub const POST_R051: Rule = Rule {
    id: "POST-R051",
    summary: "up to three cheapest distinct corridors between the anchor snaps",
};

/// Per variant, the costs from its origin snap forwards and to its target snap backwards are
/// explored up to the variant's cost plus the evidence horizon. Each snap of a route point that
/// both reached gives a detour through it (the parts before and after, each at least zero; a
/// snap behind the origin or past the target is dropped); the cheapest by (cost, distance,
/// edge, fraction) is the attachment, its excess over the variant's cost the routed excess.
/// Reasons when there is none: `search-limit` (an exploration hit the expansion ceiling),
/// `routed-excess-or-disconnected`, `evidence-horizon` (excess beyond the horizon). Capture
/// reaches further than any publication policy, so replay can tighten it without routing.
pub const POST_R052: Rule = Rule {
    id: "POST-R052",
    summary: "a route point attaches to a variant by its cheapest detour within a horizon",
};

/// `routing_availability`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Availability {
    Available,
    Unavailable,
}

impl Availability {
    pub fn as_str(self) -> &'static str {
        match self {
            Availability::Available => "available",
            Availability::Unavailable => "unavailable",
        }
    }
}

/// `invariant_failure_reason` of a variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum VariantFailure {
    SnapUnavailable,
    Disconnected,
}

impl VariantFailure {
    pub fn as_str(self) -> &'static str {
        match self {
            VariantFailure::SnapUnavailable => "snap-unavailable",
            VariantFailure::Disconnected => "disconnected",
        }
    }
}

/// `invariant_failure_reason` of an attachment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AttachmentFailure {
    BaselineUnavailable,
    SearchLimit,
    RoutedExcessOrDisconnected,
    EvidenceHorizon,
    CorridorUnavailable,
}

impl AttachmentFailure {
    pub fn as_str(self) -> &'static str {
        match self {
            AttachmentFailure::BaselineUnavailable => "baseline-unavailable",
            AttachmentFailure::SearchLimit => "search-limit",
            AttachmentFailure::RoutedExcessOrDisconnected => "routed-excess-or-disconnected",
            AttachmentFailure::EvidenceHorizon => "evidence-horizon",
            AttachmentFailure::CorridorUnavailable => "corridor-unavailable",
        }
    }
}

/// One row of `corridor_variants` without the context's identifiers (section 2.3). Costs and
/// lengths in metres; thread and corridor identities print as the pack writes them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CorridorVariant {
    pub variant_rank: u32,
    pub corridor_id: Option<CorridorId>,
    /// Penalised cost of the path.
    pub absolute_cost_metres: Option<f64>,
    /// `max(0, cost − cost₀)`.
    pub relative_cost_metres: Option<f64>,
    /// `relative / cost₀`, 0 when `cost₀ ≤ 0`.
    pub relative_cost_fraction: Option<f64>,
    /// Sum of the edge lengths.
    pub path_length_metres: Option<f64>,
    pub directed_edge_count: u32,
    /// Edges minus distinct edges.
    pub repeated_directed_edge_count: u32,
    pub service_edge_count: u32,
    pub restricted_access_edge_count: u32,
    /// `max(0, cost − length)`.
    pub access_penalty_metres: Option<f64>,
    /// Directed thread of the first and last path edge.
    pub ingress_thread_id: Option<DirectedThread>,
    pub egress_thread_id: Option<DirectedThread>,
    pub routing_availability: Availability,
    pub invariant_failure_reason: Option<VariantFailure>,
}

/// One row of `route_point_evidence` without the context's identifiers (section 2.3).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Attachment {
    pub variant_rank: u32,
    /// The variant's.
    pub corridor_id: Option<CorridorId>,
    /// `max(0, detour cost − variant cost)`, metres.
    pub routed_excess_metres: Option<f64>,
    /// Graph edge number: diagnostic only, it depends on the extract.
    pub snap_edge_id: Option<u32>,
    pub snap_way_id: Option<i64>,
    pub snap_fraction: Option<f64>,
    /// The snap point in WGS84.
    pub snap_projected_longitude: Option<f64>,
    pub snap_projected_latitude: Option<f64>,
    /// From the route point to its snap edge, metres.
    pub snap_distance_metres: Option<f64>,
    pub corridor_face_id: Option<Face>,
    /// Equal to `snap_distance_metres`.
    pub corridor_distance_metres: Option<f64>,
    /// Negative to the right of travel, metres.
    pub signed_lateral_offset_metres: Option<f64>,
    /// Azimuth of travel at the attachment, degrees.
    pub corridor_heading_degrees: Option<f64>,
    /// Azimuth of the snap edge, degrees.
    pub attachment_heading_degrees: Option<f64>,
    /// The smaller angle between the two, degrees.
    pub heading_difference_degrees: Option<f64>,
    /// Equal to `corridor_distance_metres`.
    pub proximity_distance_metres: Option<f64>,
    pub routing_availability: Availability,
    pub invariant_failure_reason: Option<AttachmentFailure>,
}

/// The routed evidence of one context: the variants by rank, and per variant the attachment of
/// every route point in the order given.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoutedEvidence {
    pub variants: Vec<CorridorVariant>,
    pub attachments: Vec<Vec<Attachment>>,
}

/// What one context routes: from the previous anchor coordinates past the route points of the
/// stop to the next anchor coordinates.
#[derive(Clone, Copy, Debug)]
pub struct ContextGeometry<'a> {
    pub mode: RoutingMode,
    pub previous: &'a [Coord],
    pub route_points: &'a [Coord],
    pub next: &'a [Coord],
}

impl Attachment {
    fn unavailable(rank: u32, corridor_id: Option<CorridorId>, reason: AttachmentFailure) -> Self {
        Attachment {
            variant_rank: rank,
            corridor_id,
            routed_excess_metres: None,
            snap_edge_id: None,
            snap_way_id: None,
            snap_fraction: None,
            snap_projected_longitude: None,
            snap_projected_latitude: None,
            snap_distance_metres: None,
            corridor_face_id: None,
            corridor_distance_metres: None,
            signed_lateral_offset_metres: None,
            corridor_heading_degrees: None,
            attachment_heading_degrees: None,
            heading_difference_degrees: None,
            proximity_distance_metres: None,
            routing_availability: Availability::Unavailable,
            invariant_failure_reason: Some(reason),
        }
    }
}

impl CorridorVariant {
    fn unavailable(reason: VariantFailure) -> Self {
        CorridorVariant {
            variant_rank: 0,
            corridor_id: None,
            absolute_cost_metres: None,
            relative_cost_metres: None,
            relative_cost_fraction: None,
            path_length_metres: None,
            directed_edge_count: 0,
            repeated_directed_edge_count: 0,
            service_edge_count: 0,
            restricted_access_edge_count: 0,
            access_penalty_metres: None,
            ingress_thread_id: None,
            egress_thread_id: None,
            routing_availability: Availability::Unavailable,
            invariant_failure_reason: Some(reason),
        }
    }
}

fn snap_order(a: &Snap, b: &Snap) -> std::cmp::Ordering {
    a.distance_m
        .total_cmp(&b.distance_m)
        .then(a.edge.cmp(&b.edge))
        .then(a.fraction.total_cmp(&b.fraction))
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl RoutingGraph {
    /// R050: the anchor snaps of one side.
    pub fn anchor_snaps(
        &self,
        mode: RoutingMode,
        coords: &[Coord],
        policy: &RoutingPolicy,
    ) -> Vec<Snap> {
        Reader::new(self).anchor_snaps(mode, coords, policy)
    }

    /// R050–R053: the routed evidence of one context. Pure; call it from any number of
    /// threads.
    pub fn routed_evidence(
        &self,
        context: &ContextGeometry<'_>,
        policy: &RoutingPolicy,
    ) -> RoutedEvidence {
        Reader::new(self).routed_evidence(context, policy)
    }

    /// [`Self::routed_evidence`] and the box of everything it read: the result is a function
    /// of the context, the policy, the router code and the graph within that box's tiles
    /// ([`super::Regions`]).
    pub fn routed_evidence_read(
        &self,
        context: &ContextGeometry<'_>,
        policy: &RoutingPolicy,
    ) -> (RoutedEvidence, ReadBox) {
        let reader = Reader::new(self);
        let evidence = reader.routed_evidence(context, policy);
        (evidence, reader.read_box())
    }
}

impl Reader<'_> {
    /// R050: the anchor snaps of one side.
    pub(super) fn anchor_snaps(
        &self,
        mode: RoutingMode,
        coords: &[Coord],
        policy: &RoutingPolicy,
    ) -> Vec<Snap> {
        let mut seen = Vec::new();
        let mut per_thread: BTreeMap<DirectedThread, Snap> = BTreeMap::new();
        for &coord in coords {
            let snaps = self.snap(mode, laea::from_wgs84(coord), &policy.snap);
            for snap in snaps.into_iter().take(policy.evidence.snaps_per_anchor) {
                let key = (snap.edge, snap.fraction.to_bits());
                if seen.contains(&key) {
                    continue;
                }
                seen.push(key);
                per_thread
                    .entry(self.directed_thread(snap.edge))
                    .and_modify(|kept| {
                        if snap_order(&snap, kept).is_lt() {
                            *kept = snap;
                        }
                    })
                    .or_insert(snap);
            }
        }
        let mut snaps: Vec<Snap> = per_thread.into_values().collect();
        snaps.sort_by(snap_order);
        snaps.truncate(policy.evidence.max_anchor_snaps);
        snaps
    }

    /// R050–R053: the routed evidence of one context.
    pub(super) fn routed_evidence(
        &self,
        context: &ContextGeometry<'_>,
        policy: &RoutingPolicy,
    ) -> RoutedEvidence {
        let mode = context.mode;
        let starts = self.anchor_snaps(mode, context.previous, policy);
        let finishes = self.anchor_snaps(mode, context.next, policy);
        let mut paths: Vec<(CorridorId, Path)> = Vec::new();
        for start in &starts {
            for finish in &finishes {
                if let Ok(path) = self.route(mode, start, finish, &policy.search) {
                    let id = self.corridor_id(&path.edges);
                    if paths.iter().all(|(known, _)| *known != id) {
                        paths.push((id, path));
                    }
                }
            }
        }
        paths.sort_by(|(a_id, a), (b_id, b)| a.cost.total_cmp(&b.cost).then(a_id.cmp(b_id)));
        paths.truncate(policy.evidence.max_variants);
        if paths.is_empty() {
            let reason = if starts.is_empty() || finishes.is_empty() {
                VariantFailure::SnapUnavailable
            } else {
                VariantFailure::Disconnected
            };
            let failed = context
                .route_points
                .iter()
                .map(|_| Attachment::unavailable(0, None, AttachmentFailure::BaselineUnavailable))
                .collect();
            return RoutedEvidence {
                variants: vec![CorridorVariant::unavailable(reason)],
                attachments: vec![failed],
            };
        }
        let baseline = paths[0].1.cost;
        let points: Vec<(Laea, Vec<Snap>)> = context
            .route_points
            .iter()
            .map(|&coord| {
                let position = laea::from_wgs84(coord);
                (position, self.snap(mode, position, &policy.snap))
            })
            .collect();
        let mut variants = Vec::with_capacity(paths.len());
        let mut attachments = Vec::with_capacity(paths.len());
        for (rank, (corridor, path)) in paths.iter().enumerate() {
            let rank = count(rank);
            variants.push(self.variant(rank, *corridor, path, baseline));
            attachments.push(self.attachments(mode, rank, *corridor, path, &points, policy));
        }
        RoutedEvidence {
            variants,
            attachments,
        }
    }

    /// R051: the facts of one found variant.
    fn variant(
        &self,
        rank: u32,
        corridor: CorridorId,
        path: &Path,
        baseline: f64,
    ) -> CorridorVariant {
        let length: f64 = path.edges.iter().map(|&edge| self.edge_length(edge)).sum();
        let mut distinct = path.edges.clone();
        distinct.sort_unstable();
        distinct.dedup();
        let relative = (path.cost - baseline).max(0.0);
        let edges = || path.edges.iter().copied();
        CorridorVariant {
            variant_rank: rank,
            corridor_id: Some(corridor),
            absolute_cost_metres: Some(path.cost),
            relative_cost_metres: Some(relative),
            relative_cost_fraction: Some(if baseline <= 0.0 {
                0.0
            } else {
                relative / baseline
            }),
            path_length_metres: Some(length),
            directed_edge_count: count(path.edges.len()),
            repeated_directed_edge_count: count(path.edges.len() - distinct.len()),
            service_edge_count: count(edges().filter(|&e| self.edge_is_service(e)).count()),
            restricted_access_edge_count: count(
                edges().filter(|&e| self.edge_is_severe(e)).count(),
            ),
            access_penalty_metres: Some((path.cost - length).max(0.0)),
            ingress_thread_id: path.edges.first().map(|&e| self.directed_thread(e)),
            egress_thread_id: path.edges.last().map(|&e| self.directed_thread(e)),
            routing_availability: Availability::Available,
            invariant_failure_reason: None,
        }
    }

    /// R052, R053: the attachment of every route point to one variant.
    fn attachments(
        &self,
        mode: RoutingMode,
        rank: u32,
        corridor: CorridorId,
        path: &Path,
        points: &[(Laea, Vec<Snap>)],
        policy: &RoutingPolicy,
    ) -> Vec<Attachment> {
        let horizon = policy.evidence.horizon_m;
        let tolerance = policy.search.tolerance_m;
        let max_cost = path.cost + horizon;
        let forward = self.explore(
            mode,
            &path.origin,
            max_cost,
            Direction::Forward,
            &policy.search,
        );
        let reverse = self.explore(
            mode,
            &path.target,
            max_cost,
            Direction::Reverse,
            &policy.search,
        );
        let failed = |reason| Attachment::unavailable(rank, Some(corridor), reason);
        points
            .iter()
            .map(|(position, snaps)| {
                if forward.limit_hit || reverse.limit_hit {
                    return failed(AttachmentFailure::SearchLimit);
                }
                let best = snaps
                    .iter()
                    .filter_map(|snap| {
                        let cost = self.edge_cost(snap.edge);
                        let before = forward.cost(snap.edge)? - (1.0 - snap.fraction) * cost;
                        let after = reverse.cost(snap.edge)? - snap.fraction * cost;
                        (before >= -tolerance && after >= -tolerance)
                            .then(|| (before.max(0.0) + after.max(0.0), snap))
                    })
                    .min_by(|(a_cost, a), (b_cost, b)| {
                        a_cost.total_cmp(b_cost).then(snap_order(a, b))
                    });
                let Some((via, snap)) = best else {
                    return failed(AttachmentFailure::RoutedExcessOrDisconnected);
                };
                let excess = (via - path.cost).max(0.0);
                if excess > horizon {
                    return failed(AttachmentFailure::EvidenceHorizon);
                }
                let geometry = self
                    .attachment_geometry(&path.edges, *position, snap, &policy.evidence)
                    .or_else(|| {
                        let mut through: Vec<u32> = forward.chain(snap.edge).collect();
                        through.reverse();
                        through.push(snap.edge);
                        through.extend(reverse.chain(snap.edge));
                        self.attachment_geometry(&through, *position, snap, &policy.evidence)
                    });
                let Some(geometry) = geometry else {
                    return failed(AttachmentFailure::CorridorUnavailable);
                };
                let snapped = laea::to_wgs84(snap.point);
                Attachment {
                    variant_rank: rank,
                    corridor_id: Some(corridor),
                    routed_excess_metres: Some(excess),
                    snap_edge_id: Some(snap.edge),
                    snap_way_id: Some(self.edge_way(snap.edge)),
                    snap_fraction: Some(snap.fraction),
                    snap_projected_longitude: Some(snapped.lon),
                    snap_projected_latitude: Some(snapped.lat),
                    snap_distance_metres: Some(snap.distance_m),
                    corridor_face_id: Some(geometry.face),
                    corridor_distance_metres: Some(snap.distance_m),
                    signed_lateral_offset_metres: Some(geometry.signed_lateral_offset),
                    corridor_heading_degrees: Some(geometry.corridor_heading),
                    attachment_heading_degrees: Some(geometry.attachment_heading),
                    heading_difference_degrees: Some(angle_difference(
                        geometry.corridor_heading,
                        geometry.attachment_heading,
                    )),
                    proximity_distance_metres: Some(snap.distance_m),
                    routing_availability: Availability::Available,
                    invariant_failure_reason: None,
                }
            })
            .collect()
    }
}
