//! Threads, directed threads, faces (POST-R022) and corridor identity (R023).

use std::collections::HashSet;
use std::fmt;
use std::hash::BuildHasherDefault;

use jr_model::Rule;
use serde::{Deserialize, Serialize};

use super::graph::{ROAD, RoutingGraph, SERVICE, TRAM};
use super::reader::Reader;
use super::search::IdHasher;

/// The thread of an edge is the maximal chain of nodes with exactly one onward neighbour that
/// contains it, among edges of its mode mask (road, tram or both) and, unless it is tram only,
/// of its class (service or not). A walk from each end follows the single onward neighbour to
/// a node with none or several (the end is that node's OSM ID) or around a pure cycle (the end
/// is its smallest OSM ID). The identity hashes the mask, the class, both ends in order and
/// whether either walk looped; the thread is reversed when its first end (or, on a cycle, its
/// from node) has the larger OSM ID. A directed thread adds `+` or `-`; a face adds the side of
/// the road (R053). OSM splits roads into ways for reasons that have nothing to do with the
/// road, so identities that survive degree-two splits let route points on one carriageway
/// be compared, and a service bay keeps its own thread beside the through road. The identity
/// is FNV-1a 128 over the payload (F# took SHA-256), 32 hex digits.
pub const POST_R022: Rule = Rule {
    id: "POST-R022",
    summary: "threads are maximal chains of degree-two nodes, identified by their end nodes",
};

/// A corridor's identity is FNV-1a 64 over the little-endian OSM IDs of every path edge's from
/// node, to node and way, in path order: it names a route on the ground, not an edge numbering,
/// so it is the same between runs, graph builds and thread counts.
pub const POST_R023: Rule = Rule {
    id: "POST-R023",
    summary: "a corridor is identified by the OSM node and way IDs of its edges",
};

const FNV64_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV64_PRIME: u64 = 0x0000_0100_0000_01b3;
const FNV128_OFFSET: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
const FNV128_PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;

/// The identity of a thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ThreadId(pub u128);

impl fmt::Display for ThreadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

/// A thread and the direction an edge runs along it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DirectedThread {
    pub thread: ThreadId,
    pub reversed: bool,
}

impl fmt::Display for DirectedThread {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.reversed { '-' } else { '+' };
        write!(f, "{}:{sign}", self.thread)
    }
}

/// The side of a thread a point lies on, in the thread's own direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FaceSide {
    Centre,
    Right,
    Left,
}

/// A thread and a side of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Face {
    pub thread: ThreadId,
    pub side: FaceSide,
}

impl fmt::Display for Face {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let side = match self.side {
            FaceSide::Centre => 'C',
            FaceSide::Right => 'R',
            FaceSide::Left => 'L',
        };
        write!(f, "{}:{side}", self.thread)
    }
}

/// A corridor's identity (R023).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CorridorId(pub u64);

impl fmt::Display for CorridorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

fn fnv128(bytes: &[u8]) -> u128 {
    bytes.iter().fold(FNV128_OFFSET, |hash, &byte| {
        (hash ^ u128::from(byte)).wrapping_mul(FNV128_PRIME)
    })
}

impl RoutingGraph {
    /// R023.
    pub fn corridor_id(&self, edges: &[u32]) -> CorridorId {
        Reader::new(self).corridor_id(edges)
    }

    /// R022: the edge's thread and whether the edge runs against it.
    pub fn directed_thread(&self, edge: u32) -> DirectedThread {
        Reader::new(self).directed_thread(edge)
    }
}

impl Reader<'_> {
    /// R023.
    pub(super) fn corridor_id(&self, edges: &[u32]) -> CorridorId {
        let mut hash = FNV64_OFFSET;
        for &edge in edges {
            for value in [
                self.node_osm_id(self.edge_from(edge)),
                self.node_osm_id(self.edge_to(edge)),
                self.edge_way(edge),
            ] {
                for byte in value.to_le_bytes() {
                    hash = (hash ^ u64::from(byte)).wrapping_mul(FNV64_PRIME);
                }
            }
        }
        CorridorId(hash)
    }

    /// R022: the edge's thread and whether the edge runs against it.
    pub(super) fn directed_thread(&self, edge: u32) -> DirectedThread {
        let flags = self.edge_flags(edge);
        let mask = flags & (ROAD | TRAM);
        let class = flags & SERVICE;
        let (left, right) = (self.edge_from(edge), self.edge_to(edge));
        let (left_end, left_loop) = self.thread_walk(mask, class, right, left);
        let (right_end, right_loop) = self.thread_walk(mask, class, left, right);
        let looped = left_loop || right_loop;
        let payload = format!(
            "{mask}|{class}|{}|{}|{}",
            left_end.min(right_end),
            left_end.max(right_end),
            if looped { "True" } else { "False" }
        );
        let reversed = if looped {
            self.node_osm_id(left) > self.node_osm_id(right)
        } else {
            left_end > right_end
        };
        DirectedThread {
            thread: ThreadId(fnv128(payload.as_bytes())),
            reversed,
        }
    }

    /// The distinct neighbours of `node` other than `previous` along edges of the mask and
    /// class, at most two (only "exactly one" matters).
    fn thread_neighbours(
        &self,
        node: u32,
        previous: u32,
        mask: u8,
        class: u8,
    ) -> ([u32; 2], usize) {
        let mut found = [u32::MAX; 2];
        let mut count = 0;
        let fits = |edge: u32| {
            let flags = self.edge_flags(edge);
            flags & mask != 0 && (mask == TRAM || flags & SERVICE == class)
        };
        let others = self
            .outgoing(node)
            .filter(|&edge| fits(edge))
            .map(|edge| self.edge_to(edge))
            .chain(
                self.incoming(node)
                    .iter()
                    .filter(|&&edge| fits(edge))
                    .map(|&edge| self.edge_from(edge)),
            );
        for other in others {
            if other == previous || found[..count].contains(&other) {
                continue;
            }
            if count == 2 {
                return (found, 3);
            }
            found[count] = other;
            count += 1;
        }
        (found, count)
    }

    /// Walks from `start`, arriving from `previous`: the end's OSM ID and whether it looped.
    fn thread_walk(&self, mask: u8, class: u8, previous: u32, start: u32) -> (i64, bool) {
        let mut visited: HashSet<u32, BuildHasherDefault<IdHasher>> = HashSet::default();
        let (mut prior, mut current) = (previous, start);
        let mut smallest = i64::MAX;
        loop {
            if !visited.insert(current) {
                return (smallest, true);
            }
            smallest = smallest.min(self.node_osm_id(current));
            match self.thread_neighbours(current, prior, mask, class) {
                (next, 1) => {
                    prior = current;
                    current = next[0];
                }
                _ => return (self.node_osm_id(current), false),
            }
        }
    }
}
