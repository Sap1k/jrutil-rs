//! Snapping a position to edges (POST-R020): a uniform grid over the edges' boxes.

use jr_model::Rule;
use rayon::prelude::*;

use crate::laea::Laea;

use super::graph::{RoutingGraph, RoutingMode};
use super::policy::SnapPolicy;

/// A position snaps to every edge of the mode whose segment lies within the snap radius (the
/// distance to the nearest point of the segment; fraction 0 on a zero-length edge). The
/// candidates are ordered by (distance, edge, fraction), the nearest few kept, and of those
/// only the ones within the band of the nearest. A stop stands next to one road, but its
/// recorded position may be closer to a parallel one; the band keeps both without dragging in
/// the whole neighbourhood. Any position can be snapped (F# prepared the snaps of the
/// coordinates it would route and found none for others; it routed only prepared ones, so
/// the result is the same).
pub const POST_R020: Rule = Rule {
    id: "POST-R020",
    summary: "a position snaps to the nearest edges of its mode within a radius and a band",
};

/// The grid's cell size in projected metres; a performance choice, not a threshold.
const CELL_M: f64 = 128.0;

/// A position on a directed edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snap {
    pub edge: u32,
    /// Along the edge from its from node, in [0, 1].
    pub fraction: f64,
    /// The snapped point, in EPSG:3035.
    pub point: Laea,
    /// From the snapped position to the point, in projected metres.
    pub distance_m: f64,
}

/// Per grid cell, the edges whose box touches it.
#[derive(Debug, Default)]
pub(super) struct SnapIndex {
    /// Sorted cell keys and, per key, its edges in `edges[offsets[i]..offsets[i + 1]]`.
    keys: Vec<u64>,
    offsets: Vec<u32>,
    edges: Vec<u32>,
}

fn cell(value: f64) -> i32 {
    (value / CELL_M).floor() as i32
}

fn key(x: i32, y: i32) -> u64 {
    (u64::from(x as u32) << 32) | u64::from(y as u32)
}

impl SnapIndex {
    pub fn new(node_xy: &[Laea], edge_from: &[u32], edge_to: &[u32]) -> SnapIndex {
        let mut entries: Vec<(u64, u32)> = (0..edge_from.len())
            .into_par_iter()
            .flat_map_iter(|edge| {
                let a = node_xy[edge_from[edge] as usize];
                let b = node_xy[edge_to[edge] as usize];
                let (x0, x1) = (
                    cell(a.easting.min(b.easting)),
                    cell(a.easting.max(b.easting)),
                );
                let (y0, y1) = (
                    cell(a.northing.min(b.northing)),
                    cell(a.northing.max(b.northing)),
                );
                (x0..=x1).flat_map(move |x| (y0..=y1).map(move |y| (key(x, y), edge as u32)))
            })
            .collect();
        entries.par_sort_unstable();
        let mut index = SnapIndex::default();
        for (cell_key, edge) in entries {
            if index.keys.last() != Some(&cell_key) {
                index.keys.push(cell_key);
                index.offsets.push(index.edges.len() as u32);
            }
            index.edges.push(edge);
        }
        index.offsets.push(index.edges.len() as u32);
        index
    }

    /// Edges whose box touches the square around the point, possibly with repeats.
    fn near(&self, point: Laea, radius: f64, out: &mut Vec<u32>) {
        for x in cell(point.easting - radius)..=cell(point.easting + radius) {
            for y in cell(point.northing - radius)..=cell(point.northing + radius) {
                if let Ok(i) = self.keys.binary_search(&key(x, y)) {
                    out.extend_from_slice(
                        &self.edges[self.offsets[i] as usize..self.offsets[i + 1] as usize],
                    );
                }
            }
        }
    }
}

impl RoutingGraph {
    /// The nearest point of an edge to a position.
    pub(super) fn snap_to_edge(&self, point: Laea, edge: u32) -> Snap {
        let a = self.xy(self.edge_from(edge));
        let b = self.xy(self.edge_to(edge));
        let (dx, dy) = (b.easting - a.easting, b.northing - a.northing);
        let length_squared = dx * dx + dy * dy;
        let fraction = if length_squared == 0.0 {
            0.0
        } else {
            (((point.easting - a.easting) * dx + (point.northing - a.northing) * dy)
                / length_squared)
                .clamp(0.0, 1.0)
        };
        let snapped = Laea {
            easting: a.easting + fraction * dx,
            northing: a.northing + fraction * dy,
        };
        Snap {
            edge,
            fraction,
            point: snapped,
            distance_m: (point.easting - snapped.easting).hypot(point.northing - snapped.northing),
        }
    }

    /// R020: the snaps of a projected position for a mode, nearest first.
    pub fn snap(&self, mode: RoutingMode, point: Laea, policy: &SnapPolicy) -> Vec<Snap> {
        let mut edges = Vec::new();
        self.snaps.near(point, policy.radius_m, &mut edges);
        edges.sort_unstable();
        edges.dedup();
        let mut snaps: Vec<Snap> = edges
            .into_iter()
            .filter(|&edge| self.edge_allows(edge, mode))
            .map(|edge| self.snap_to_edge(point, edge))
            .filter(|snap| snap.distance_m <= policy.radius_m)
            .collect();
        snaps.sort_by(|a, b| {
            a.distance_m
                .total_cmp(&b.distance_m)
                .then(a.edge.cmp(&b.edge))
                .then(a.fraction.total_cmp(&b.fraction))
        });
        snaps.truncate(policy.candidates);
        if let Some(nearest) = snaps.first().map(|snap| snap.distance_m) {
            snaps.retain(|snap| snap.distance_m <= nearest + policy.band_m);
        }
        snaps
    }
}
