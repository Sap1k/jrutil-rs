//! Shortest paths over directed edges (POST-R021) and the bounded explorations of R052.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::hash::{BuildHasherDefault, Hasher};

use jr_model::Rule;

use super::graph::{RoutingGraph, RoutingMode};
use super::policy::SearchPolicy;
use super::snap::Snap;

/// The shortest path from one snap to another is an A* search whose states are directed edges:
/// a state's cost is the penalised cost (R012) to the end of its edge, the start costs the rest
/// of its edge after the snap, and the target is reached when its edge is taken from the queue,
/// less the part of the edge after the target snap (a target behind the start on the same edge
/// is not reached, as in F#). Turns obey R018. The heuristic is the distance to the circle
/// around the target edge's ends, which never overestimates. The search stops after a ceiling of
/// expansions and does not expand states beyond a cost ceiling; equal priorities are taken in
/// edge order, so the path is the same on every run. A bus route between neighbouring stops
/// is never long; the ceilings bound the work an unreachable target can cause.
pub const POST_R021: Rule = Rule {
    id: "POST-R021",
    summary: "edge-based A* between two snaps, with expansion and cost ceilings",
};

/// Why no path was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RouteFailure {
    /// The expansion ceiling was reached.
    SearchLimit,
    /// Everything reachable within the cost ceiling was searched.
    Disconnected,
}

/// A routed path between two snaps.
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    /// Penalised cost in metres from the origin snap to the target snap.
    pub cost: f64,
    pub origin: Snap,
    pub target: Snap,
    /// The directed edges from the origin's edge to the target's.
    pub edges: Vec<u32>,
}

/// A hasher for dense integer keys; maps are only looked up, never iterated into output.
#[derive(Default, Clone, Copy, Debug)]
pub(super) struct IdHasher(u64);

impl Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = (self.0.rotate_left(5) ^ u64::from(byte)).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }

    fn write_u32(&mut self, value: u32) {
        self.0 = u64::from(value).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
}

pub(super) type EdgeMap<V> = HashMap<u32, V, BuildHasherDefault<IdHasher>>;

/// A queue entry: the smallest key first, then the smallest edge.
#[derive(Clone, Copy, Debug)]
struct Entry {
    key: f64,
    edge: u32,
    cost: f64,
}

impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Entry {}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Entry {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .key
            .total_cmp(&self.key)
            .then(other.edge.cmp(&self.edge))
            .then(other.cost.total_cmp(&self.cost))
    }
}

/// Costs of the edges an exploration reached, and the edge each was reached from (forward) or
/// continues to (reverse); `u32::MAX` for the seed.
#[derive(Debug)]
pub(super) struct Exploration {
    pub reached: EdgeMap<(f64, u32)>,
    pub limit_hit: bool,
}

impl Exploration {
    pub fn cost(&self, edge: u32) -> Option<f64> {
        self.reached.get(&edge).map(|&(cost, _)| cost)
    }

    /// The chain of links from `edge` (exclusive) to the seed.
    pub fn chain(&self, edge: u32) -> impl Iterator<Item = u32> + '_ {
        std::iter::successors(self.reached.get(&edge).map(|&(_, link)| link), |&edge| {
            self.reached.get(&edge).map(|&(_, link)| link)
        })
        .take_while(|&link| link != u32::MAX)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Direction {
    /// Costs to the end of each edge, from the seed snap onwards.
    Forward,
    /// Costs from the start of each edge to the seed snap.
    Reverse,
}

impl RoutingGraph {
    /// R021: the cheapest path from `start` to `finish`.
    pub fn route(
        &self,
        mode: RoutingMode,
        start: &Snap,
        finish: &Snap,
        policy: &SearchPolicy,
    ) -> Result<Path, RouteFailure> {
        let target = finish.edge;
        let (a, b) = (
            self.xy(self.edge_from(target)),
            self.xy(self.edge_to(target)),
        );
        let centre = (
            (a.easting + b.easting) / 2.0,
            (a.northing + b.northing) / 2.0,
        );
        let radius = (a.easting - centre.0).hypot(a.northing - centre.1);
        let heuristic = |node: u32| {
            let p = self.xy(node);
            ((p.easting - centre.0).hypot(p.northing - centre.1) - radius).max(0.0)
        };
        let mut labels: EdgeMap<(f64, u32)> = EdgeMap::default();
        let mut queue = BinaryHeap::new();
        let initial = (1.0 - start.fraction) * self.edge_cost(start.edge);
        labels.insert(start.edge, (initial, u32::MAX));
        queue.push(Entry {
            key: initial + heuristic(self.edge_to(start.edge)),
            edge: start.edge,
            cost: initial,
        });
        let mut explored = 0;
        while explored < policy.max_expansions {
            let Some(Entry { edge, cost, .. }) = queue.pop() else {
                return Err(RouteFailure::Disconnected);
            };
            if cost > labels[&edge].0 || cost > policy.max_cost_m {
                continue;
            }
            explored += 1;
            if edge == target {
                let reached = cost - (1.0 - finish.fraction) * self.edge_cost(edge);
                if reached < -policy.tolerance_m {
                    // The target lies behind the start on the start's edge; no later visit
                    // can lower this edge's cost.
                    return Err(RouteFailure::Disconnected);
                }
                let mut edges = vec![edge];
                let mut current = edge;
                while current != start.edge {
                    match labels[&current].1 {
                        u32::MAX => break,
                        previous => {
                            edges.push(previous);
                            current = previous;
                        }
                    }
                }
                edges.reverse();
                return Ok(Path {
                    cost: reached.max(0.0),
                    origin: *start,
                    target: *finish,
                    edges,
                });
            }
            let node = self.edge_to(edge);
            for next in self.outgoing(node) {
                if !self.edge_allows(next, mode) || !self.transition_allowed(edge, next) {
                    continue;
                }
                let candidate = cost + self.edge_cost(next);
                if labels.get(&next).is_some_and(|&(old, _)| old <= candidate) {
                    continue;
                }
                labels.insert(next, (candidate, edge));
                queue.push(Entry {
                    key: candidate + heuristic(self.edge_to(next)),
                    edge: next,
                    cost: candidate,
                });
            }
        }
        Err(RouteFailure::SearchLimit)
    }

    /// R052: every edge reachable from (forward) or reaching (reverse) a snap within
    /// `max_cost`, by Dijkstra over directed edges.
    pub(super) fn explore(
        &self,
        mode: RoutingMode,
        seed: &Snap,
        max_cost: f64,
        direction: Direction,
        policy: &SearchPolicy,
    ) -> Exploration {
        let mut reached: EdgeMap<(f64, u32)> = EdgeMap::default();
        let mut queue = BinaryHeap::new();
        let share = match direction {
            Direction::Forward => 1.0 - seed.fraction,
            Direction::Reverse => seed.fraction,
        };
        let initial = share * self.edge_cost(seed.edge);
        reached.insert(seed.edge, (initial, u32::MAX));
        queue.push(Entry {
            key: initial,
            edge: seed.edge,
            cost: initial,
        });
        let mut explored = 0;
        let relax = |reached: &mut EdgeMap<(f64, u32)>,
                     queue: &mut BinaryHeap<Entry>,
                     next: u32,
                     candidate: f64,
                     link: u32| {
            if reached.get(&next).is_some_and(|&(old, _)| old <= candidate) {
                return;
            }
            reached.insert(next, (candidate, link));
            queue.push(Entry {
                key: candidate,
                edge: next,
                cost: candidate,
            });
        };
        while explored < policy.max_expansions {
            let Some(Entry { edge, cost, .. }) = queue.pop() else {
                break;
            };
            if cost > max_cost + policy.tolerance_m {
                break;
            }
            if cost > reached[&edge].0 || cost > max_cost {
                continue;
            }
            explored += 1;
            match direction {
                Direction::Forward => {
                    for next in self.outgoing(self.edge_to(edge)) {
                        if self.edge_allows(next, mode) && self.transition_allowed(edge, next) {
                            let candidate = cost + self.edge_cost(next);
                            relax(&mut reached, &mut queue, next, candidate, edge);
                        }
                    }
                }
                Direction::Reverse => {
                    for &previous in self.incoming(self.edge_from(edge)) {
                        if self.edge_allows(previous, mode)
                            && self.transition_allowed(previous, edge)
                        {
                            let candidate = cost + self.edge_cost(previous);
                            relax(&mut reached, &mut queue, previous, candidate, edge);
                        }
                    }
                }
            }
        }
        Exploration {
            reached,
            limit_hit: explored >= policy.max_expansions,
        }
    }
}
