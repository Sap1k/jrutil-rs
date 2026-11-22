//! Regions of the graph and their digests: when a routed result may be reused on another
//! graph.
//!
//! A tile is a block of [`TILE_CELLS`] × [`TILE_CELLS`] snap cells (2,048 m). A tile's content
//! is the edges listed in its snap cells (every edge whose box touches the tile), ascending by
//! edge number; its digest hashes, per edge in that order, the from and to node's OSM IDs, the
//! way's OSM ID, the flags, the bits of both endpoints' projected coordinates, and the turn
//! restrictions that apply on arriving by the edge (via its to node, from its way), each as
//! (only, the to way's OSM ID or a marker for a way not on the graph).
//!
//! Why equal digests make a cached result exact: a routing result is a function of the
//! context, the policy, the router code and whatever the reader read ([`super::ReadBox`]);
//! everything read lies in the tiles of the read box (a node's edges touch its tile; the snap
//! query notes its whole square; edges are only reached through those); equal digests mean
//! equal content in equal relative edge order (edge numbers are by from node's OSM ID, then
//! source order, and both are hashed: two edges of one from node lie in that node's tile), so
//! every tie broken by edge number falls the same way. Edge numbers themselves differ between
//! graphs; a cached result names its snap edges by [`StableEdge`].

use std::collections::HashMap;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::graph::{NO_WAY, RoutingGraph};
use super::reader::ReadBox;
use super::snap::cell;

/// Snap cells per tile side (format constant of the routing cache).
pub const TILE_CELLS: i32 = 16;

/// A rectangle of tiles, inclusive; empty when `min_x > max_x`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TileRange {
    pub min_x: i32,
    pub min_y: i32,
    pub max_x: i32,
    pub max_y: i32,
}

impl TileRange {
    pub const EMPTY: TileRange = TileRange {
        min_x: 0,
        min_y: 0,
        max_x: -1,
        max_y: -1,
    };

    pub fn is_empty(&self) -> bool {
        self.min_x > self.max_x || self.min_y > self.max_y
    }
}

/// The digest of the tiles of a range (SHA-256, the first 128 bits).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RegionDigest(pub [u8; 16]);

/// Every tile's digest.
#[derive(Debug)]
pub struct Regions {
    tiles: HashMap<(i32, i32), [u8; 16]>,
}

fn tile(value: f64) -> i32 {
    cell(value).div_euclid(TILE_CELLS)
}

fn truncated(hasher: Sha256) -> [u8; 16] {
    let full = hasher.finalize();
    let mut out = [0u8; 16];
    out.copy_from_slice(&full[..16]);
    out
}

impl Regions {
    /// The tiles of a read box.
    pub fn tiles(&self, read: ReadBox) -> TileRange {
        if read.is_empty() {
            return TileRange::EMPTY;
        }
        TileRange {
            min_x: tile(read.min_easting),
            min_y: tile(read.min_northing),
            max_x: tile(read.max_easting),
            max_y: tile(read.max_northing),
        }
    }

    /// The digest of the tiles present in a range, row by row.
    pub fn digest(&self, range: TileRange) -> RegionDigest {
        let mut hasher = Sha256::new();
        if !range.is_empty() {
            for y in range.min_y..=range.max_y {
                for x in range.min_x..=range.max_x {
                    if let Some(digest) = self.tiles.get(&(x, y)) {
                        hasher.update(x.to_le_bytes());
                        hasher.update(y.to_le_bytes());
                        hasher.update(digest);
                    }
                }
            }
        }
        RegionDigest(truncated(hasher))
    }

    /// The number of tiles with edges.
    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }
}

impl RoutingGraph {
    /// Every tile's digest, computed in parallel.
    pub fn regions(&self) -> Regions {
        let mut cells: Vec<((i32, i32), usize)> = self
            .snaps
            .cells()
            .enumerate()
            .map(|(index, ((x, y), _))| {
                ((x.div_euclid(TILE_CELLS), y.div_euclid(TILE_CELLS)), index)
            })
            .collect();
        cells.sort_unstable();
        let cell_edges: Vec<&[u32]> = self.snaps.cells().map(|(_, edges)| edges).collect();
        let groups: Vec<&[((i32, i32), usize)]> = cells.chunk_by(|a, b| a.0 == b.0).collect();
        let tiles = groups
            .par_iter()
            .map(|group| {
                let mut edges: Vec<u32> = group
                    .iter()
                    .flat_map(|&(_, index)| cell_edges[index].iter().copied())
                    .collect();
                edges.sort_unstable();
                edges.dedup();
                let mut hasher = Sha256::new();
                for edge in edges {
                    self.hash_edge(edge, &mut hasher);
                }
                (group[0].0, truncated(hasher))
            })
            .collect();
        Regions { tiles }
    }

    /// One edge's record in its tiles' digests.
    fn hash_edge(&self, edge: u32, hasher: &mut Sha256) {
        let (from, to) = (self.edge_from(edge), self.edge_to(edge));
        hasher.update(self.node_osm_id(from).to_le_bytes());
        hasher.update(self.node_osm_id(to).to_le_bytes());
        hasher.update(self.edge_way(edge).to_le_bytes());
        hasher.update([self.edge_flags[edge as usize]]);
        for node in [from, to] {
            let point = self.xy(node);
            hasher.update(point.easting.to_bits().to_le_bytes());
            hasher.update(point.northing.to_bits().to_le_bytes());
        }
        // The restrictions that apply on arriving by this edge.
        let way = self.edge_way[edge as usize];
        let first = self
            .restrictions
            .partition_point(|r| (r.via, r.from_way) < (to, way));
        let applying: Vec<_> = self.restrictions[first..]
            .iter()
            .take_while(|r| r.via == to && r.from_way == way)
            .collect();
        hasher.update((applying.len() as u32).to_le_bytes());
        for restriction in applying {
            hasher.update([u8::from(restriction.only)]);
            match restriction.to_way {
                NO_WAY => hasher.update([0u8]),
                to_way => {
                    hasher.update([1u8]);
                    hasher.update(self.way_ids[to_way as usize].to_le_bytes());
                }
            }
        }
    }

    /// An edge named independently of the graph's numbering.
    pub fn stable_edge(&self, edge: u32) -> StableEdge {
        let (from, to, way) = (
            self.edge_from(edge),
            self.edge_to(edge),
            self.edge_way[edge as usize],
        );
        let occurrence = self
            .outgoing(from)
            .take_while(|&other| other < edge)
            .filter(|&other| self.edge_to(other) == to && self.edge_way[other as usize] == way)
            .count();
        StableEdge {
            from: self.node_osm_id(from),
            to: self.node_osm_id(to),
            way: self.edge_way(edge),
            occurrence: occurrence as u32,
        }
    }

    /// The edge a stable name names in this graph, if any.
    pub fn resolve_edge(&self, stable: &StableEdge) -> Option<u32> {
        let from = self.node_of(stable.from)?;
        let to = self.node_of(stable.to)?;
        self.outgoing(from)
            .filter(|&edge| self.edge_to(edge) == to && self.edge_way(edge) == stable.way)
            .nth(stable.occurrence as usize)
    }
}

/// An edge by its from node, to node and way (OSM IDs) and its occurrence among the from node's
/// edges with that to node and way.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StableEdge {
    pub from: i64,
    pub to: i64,
    pub way: i64,
    pub occurrence: u32,
}
