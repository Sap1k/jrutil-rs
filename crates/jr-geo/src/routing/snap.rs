//! Snapping a position to edges (POST-R020): a uniform grid over the edges' boxes.

use jr_model::Rule;
use rayon::prelude::*;

use crate::laea::Laea;

use super::graph::{RoutingGraph, RoutingMode};
use super::policy::SnapPolicy;
use super::reader::Reader;

/// A position snaps to every edge of the mode whose segment lies within the snap radius (the
/// distance to the nearest point of the segment; fraction 0 on a zero-length edge). The
/// candidates are ordered by (distance, edge, fraction), the nearest few kept, and of those
/// only the ones within the band of the nearest. A stop stands next to one road, but its
/// recorded position may be closer to a parallel one; the band keeps both without dragging in
/// the whole neighbourhood. A snap past the end of a segment is the end node itself, so a
/// position nearest to a node is equally near every edge meeting there and the edge order
/// decides, as with F#. Any position can be snapped (F# prepared the snaps of the
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

/// The snap cell of a projected coordinate.
pub(super) fn cell(value: f64) -> i32 {
    (value / CELL_M).floor() as i32
}

fn key(x: i32, y: i32) -> u64 {
    (u64::from(x as u32) << 32) | u64::from(y as u32)
}

/// A cell and an edge whose box touches it; ordered as (`key`, edge).
type Entry = [u32; 3];

impl SnapIndex {
    pub fn new(node_xy: &[Laea], edge_from: &[u32], edge_to: &[u32]) -> SnapIndex {
        let cells = |edge: usize| {
            let a = node_xy[edge_from[edge] as usize];
            let b = node_xy[edge_to[edge] as usize];
            let xs = cell(a.easting.min(b.easting))..=cell(a.easting.max(b.easting));
            let ys = cell(a.northing.min(b.northing))..=cell(a.northing.max(b.northing));
            (xs, ys)
        };
        // Counted first, so the entries are allocated once: they are the build's largest
        // transient.
        let total: usize = (0..edge_from.len())
            .into_par_iter()
            .map(|edge| {
                let (xs, ys) = cells(edge);
                xs.count() * ys.count()
            })
            .sum();
        let mut entries: Vec<Entry> = Vec::with_capacity(total);
        for edge in 0..edge_from.len() {
            let (xs, ys) = cells(edge);
            for x in xs {
                for y in ys.clone() {
                    entries.push([x as u32, y as u32, edge as u32]);
                }
            }
        }
        entries.par_sort_unstable();
        let mut index = SnapIndex {
            edges: Vec::with_capacity(total),
            ..SnapIndex::default()
        };
        for [x, y, edge] in entries {
            let cell_key = key(x as i32, y as i32);
            if index.keys.last() != Some(&cell_key) {
                index.keys.push(cell_key);
                index.offsets.push(index.edges.len() as u32);
            }
            index.edges.push(edge);
        }
        index.offsets.push(index.edges.len() as u32);
        index
    }

    /// Every cell with its edges, in key order.
    pub(super) fn cells(&self) -> impl Iterator<Item = ((i32, i32), &[u32])> {
        self.keys.iter().enumerate().map(|(i, &cell_key)| {
            let cell = ((cell_key >> 32) as u32 as i32, cell_key as u32 as i32);
            let edges = &self.edges[self.offsets[i] as usize..self.offsets[i + 1] as usize];
            (cell, edges)
        })
    }

    /// Edges whose box touches the square around the point, possibly with repeats.
    pub(super) fn near(&self, point: Laea, radius: f64, out: &mut Vec<u32>) {
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

impl Reader<'_> {
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
        // A snap clamped to an end is that node exactly, so the edges meeting there are equally
        // near and the edge order decides between them (F#'s f32 coordinates gave the same).
        let snapped = if fraction == 1.0 {
            b
        } else {
            Laea {
                easting: a.easting + fraction * dx,
                northing: a.northing + fraction * dy,
            }
        };
        Snap {
            edge,
            fraction,
            point: snapped,
            distance_m: (point.easting - snapped.easting).hypot(point.northing - snapped.northing),
        }
    }

    /// R020: the snaps of a projected position for a mode, nearest first.
    pub(super) fn snap(&self, mode: RoutingMode, point: Laea, policy: &SnapPolicy) -> Vec<Snap> {
        let mut edges = Vec::new();
        self.grid_edges(point, policy.radius_m, &mut edges);
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

impl RoutingGraph {
    /// R020: the snaps of a projected position for a mode, nearest first.
    pub fn snap(&self, mode: RoutingMode, point: Laea, policy: &SnapPolicy) -> Vec<Snap> {
        Reader::new(self).snap(mode, point, policy)
    }
}
