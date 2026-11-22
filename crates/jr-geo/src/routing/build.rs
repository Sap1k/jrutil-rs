//! Building the graph from OSM facts (POST-R011–R018).

use jr_model::geo::distance_m;
use jr_model::{Coord, Rule};
use rayon::prelude::*;

use crate::laea;

use super::graph::{GraphCounts, ROAD, Restriction, RoutingGraph, SERVICE, SEVERE, TRAM};
use super::input::{
    OsmMemberType, OsmNode, OsmRelation, OsmTags, OsmWay, normalized, tag, tag_in, tag_is,
};
use super::policy::GraphPolicy;
use super::snap::SnapIndex;

/// A way is a road when its `highway` is one of the routable classes, it is not closed
/// (`highway=construction`, or any `construction`, `abandoned`, `disused`, `proposed` key) and
/// it is not generally blocked (`access`, `vehicle` or `motor_vehicle` = no/private) unless
/// `bus` or `psv` permits it. Link roads, `track` and `pedestrian` are not routable (B5, kept
/// until the switch, verdict Q3). Conditional tags are not read.
pub const POST_R011: Rule = Rule {
    id: "POST-R011",
    summary: "a way is a road by highway class, unless closed or blocked without bus permission",
};

/// A road with `bus` or `psv` = no/private stays routable (its tagging may be wrong) but every
/// edge of it costs the restricted-access factor times its length.
pub const POST_R012: Rule = Rule {
    id: "POST-R012",
    summary: "a road closed to buses stays routable at a penalised cost",
};

/// `highway=service` marks the service class: counted in a variant, and it splits threads
/// (R022) so a bus bay is a thread of its own next to its through road.
pub const POST_R013: Rule = Rule {
    id: "POST-R013",
    summary: "service roads are a class of their own",
};

/// `railway=tram`, not closed, is tram track. A way may be road and tram at once (street
/// track); its edges then carry both flags.
pub const POST_R014: Rule = Rule {
    id: "POST-R014",
    summary: "a railway=tram way that is not closed is tram track",
};

/// `oneway` yes/true/1 gives forward edges only, -1/reverse backward only, otherwise a
/// roundabout is forward only; road and tram alike. Bus contraflow (`oneway:bus`) is not read
/// (B7, kept until the switch).
pub const POST_R015: Rule = Rule {
    id: "POST-R015",
    summary: "one-way ways give edges in one direction",
};

/// A node with any `barrier` tag blocks every edge from or to it, for road and tram, unless the
/// node itself has an affirmative `bus` or `psv`. Gates and lift gates block too (B6, kept until
/// the switch).
pub const POST_R016: Rule = Rule {
    id: "POST-R016",
    summary: "barrier nodes block edges unless the node permits buses",
};

/// A relation with a `restriction` tag and members `from` (way), `via` (node in the extract),
/// `to` (way) restricts turns at the via node: arriving from the from way, an `only_*`
/// restriction allows only the to way and any other forbids it. Via ways, `restriction:bus`,
/// `except` are not read (B7). The OSM data says which turns a road vehicle may not make.
pub const POST_R018: Rule = Rule {
    id: "POST-R018",
    summary: "turn restrictions with a via node are honoured",
};

#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("routing extract: node {0} appears twice")]
    DuplicateNode(i64),
    #[error("routing extract: node {0} comes after a way; the extract must be type-ordered")]
    NodeAfterWay(i64),
    #[error("routing extract: more than 2^32 nodes or edges")]
    TooLarge,
}

#[derive(Clone, Copy, Debug)]
struct RawEdge {
    from: u32,
    to: u32,
    way: i64,
    length: f64,
    flags: u8,
}

/// Builds a [`RoutingGraph`] from a type-ordered stream: nodes, then ways, then relations
/// (relations may also come between ways).
#[derive(Debug)]
pub struct GraphBuilder<'p> {
    policy: &'p GraphPolicy,
    ids: Vec<i64>,
    coords: Vec<Coord>,
    blocked: Vec<bool>,
    ascending: bool,
    frozen: bool,
    edges: Vec<RawEdge>,
    /// (from way, to way, via raw node, only).
    restrictions: Vec<(i64, i64, u32, bool)>,
    counts: GraphCounts,
}

impl<'p> GraphBuilder<'p> {
    pub fn new(policy: &'p GraphPolicy) -> Self {
        GraphBuilder {
            policy,
            ids: Vec::new(),
            coords: Vec::new(),
            blocked: Vec::new(),
            ascending: true,
            frozen: false,
            edges: Vec::new(),
            restrictions: Vec::new(),
            counts: GraphCounts::default(),
        }
    }

    pub fn node(&mut self, node: &OsmNode<'_>) -> Result<(), GraphError> {
        if self.frozen {
            return Err(GraphError::NodeAfterWay(node.id));
        }
        if self.ids.last().is_some_and(|&last| last >= node.id) {
            self.ascending = false;
        }
        self.ids.push(node.id);
        self.coords.push(node.coord);
        self.blocked.push(self.barrier_blocked(node.tags));
        Ok(())
    }

    /// R016.
    fn barrier_blocked(&self, tags: OsmTags<'_>) -> bool {
        let policy = self.policy;
        tag(tags, &policy.barrier_key).is_some() && !self.transit_permission(tags)
    }

    fn transit_permission(&self, tags: OsmTags<'_>) -> bool {
        let policy = self.policy;
        policy
            .transit_access_keys
            .iter()
            .any(|key| tag_in(tags, key, &policy.affirmative_access))
    }

    /// The nodes are complete: sort them if the source did not, refuse duplicates.
    fn freeze(&mut self) -> Result<(), GraphError> {
        if self.frozen {
            return Ok(());
        }
        self.frozen = true;
        if u32::try_from(self.ids.len()).is_err() {
            return Err(GraphError::TooLarge);
        }
        if !self.ascending {
            let mut order: Vec<u32> = (0..self.ids.len() as u32).collect();
            order.par_sort_unstable_by_key(|&i| (self.ids[i as usize], i));
            self.ids = order.iter().map(|&i| self.ids[i as usize]).collect();
            self.coords = order.iter().map(|&i| self.coords[i as usize]).collect();
            self.blocked = order.iter().map(|&i| self.blocked[i as usize]).collect();
        }
        if let Some(pair) = self.ids.windows(2).find(|pair| pair[0] == pair[1]) {
            return Err(GraphError::DuplicateNode(pair[0]));
        }
        Ok(())
    }

    fn raw_node(&self, osm_id: i64) -> Option<u32> {
        self.ids.binary_search(&osm_id).ok().map(|i| i as u32)
    }

    /// R011–R014: the way's flags; 0 when it is neither road nor tram.
    fn way_flags(&self, tags: OsmTags<'_>) -> u8 {
        let policy = self.policy;
        let any_closed_key = policy
            .closed_keys
            .iter()
            .any(|key| tag(tags, key).is_some());
        let mut flags = 0;
        let highway = tag(tags, &policy.highway_key).map(normalized);
        if let Some(highway) = highway {
            let routable = policy.road_highways.iter().any(|class| **class == *highway);
            let closed = *highway == *policy.closed_highway || any_closed_key;
            let blocked = policy
                .general_access_keys
                .iter()
                .any(|key| tag_in(tags, key, &policy.negative_access));
            if routable && !closed && !(blocked && !self.transit_permission(tags)) {
                flags |= ROAD;
                let severe = policy
                    .transit_access_keys
                    .iter()
                    .any(|key| tag_in(tags, key, &policy.negative_access));
                if severe {
                    flags |= SEVERE;
                }
                if *highway == *policy.service_highway {
                    flags |= SERVICE;
                }
            }
        }
        if tag_is(tags, &policy.tram.key, &policy.tram.value) && !any_closed_key {
            flags |= TRAM;
        }
        flags
    }

    /// R015: (forward, backward).
    fn directions(&self, tags: OsmTags<'_>) -> (bool, bool) {
        let policy = self.policy;
        if tag_in(tags, &policy.oneway_key, &policy.oneway_forward) {
            (true, false)
        } else if tag_in(tags, &policy.oneway_key, &policy.oneway_backward) {
            (false, true)
        } else if tag_is(tags, &policy.roundabout.key, &policy.roundabout.value) {
            (true, false)
        } else {
            (true, true)
        }
    }

    /// R017.
    pub fn way(&mut self, way: &OsmWay<'_>) -> Result<(), GraphError> {
        self.freeze()?;
        let flags = self.way_flags(way.tags);
        if flags == 0 {
            return Ok(());
        }
        let (forward, backward) = self.directions(way.tags);
        let before = self.edges.len();
        let mut left = way.nodes.first().and_then(|&id| self.raw_node(id));
        for &next in way.nodes.iter().skip(1) {
            let right = self.raw_node(next);
            if let (Some(l), Some(r)) = (left, right)
                && l != r
                && !self.blocked[l as usize]
                && !self.blocked[r as usize]
            {
                let length = distance_m(self.coords[l as usize], self.coords[r as usize]);
                let edge = |from, to| RawEdge {
                    from,
                    to,
                    way: way.id,
                    length,
                    flags,
                };
                if forward {
                    self.edges.push(edge(l, r));
                }
                if backward {
                    self.edges.push(edge(r, l));
                }
            }
            left = right;
        }
        if self.edges.len() > before {
            if flags & ROAD != 0 {
                self.counts.road_ways += 1;
            }
            if flags & TRAM != 0 {
                self.counts.tram_ways += 1;
            }
        }
        Ok(())
    }

    /// R018.
    pub fn relation(&mut self, relation: &OsmRelation<'_>) -> Result<(), GraphError> {
        self.freeze()?;
        let policy = self.policy;
        let Some(value) = tag(relation.tags, &policy.restriction_key) else {
            return Ok(());
        };
        let member = |role: &str| relation.members.iter().find(|m| m.role == role);
        let resolved = match (
            member(&policy.restriction_from_role),
            member(&policy.restriction_via_role),
            member(&policy.restriction_to_role),
        ) {
            (Some(from), Some(via), Some(to))
                if from.kind == OsmMemberType::Way
                    && via.kind == OsmMemberType::Node
                    && to.kind == OsmMemberType::Way =>
            {
                self.raw_node(via.id).map(|via| (from.id, to.id, via))
            }
            _ => None,
        };
        match resolved {
            Some((from_way, to_way, via)) => {
                let only = normalized(value).starts_with(&*policy.restriction_only_prefix);
                self.restrictions.push((from_way, to_way, via, only));
            }
            None => self.counts.restrictions_ignored += 1,
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<RoutingGraph, GraphError> {
        self.freeze()?;
        let restricted_access_factor = self.policy.restricted_access_factor;
        if u32::try_from(self.edges.len()).is_err() {
            return Err(GraphError::TooLarge);
        }
        // Vertices: the nodes edges touch, in OSM ID order.
        let mut compact = vec![u32::MAX; self.ids.len()];
        for edge in &self.edges {
            compact[edge.from as usize] = 0;
            compact[edge.to as usize] = 0;
        }
        let mut raw_of = Vec::new();
        for (raw, slot) in compact.iter_mut().enumerate() {
            if *slot == 0 {
                *slot = raw_of.len() as u32;
                raw_of.push(raw as u32);
            }
        }
        let node_ids: Vec<i64> = raw_of.iter().map(|&raw| self.ids[raw as usize]).collect();
        let node_xy: Vec<laea::Laea> = raw_of
            .par_iter()
            .map(|&raw| laea::from_wgs84(self.coords[raw as usize]))
            .collect();
        let node_count = node_ids.len();
        drop(std::mem::take(&mut self.coords));

        // Outgoing CSR by a stable counting scatter.
        let mut out_offsets = vec![0u32; node_count + 1];
        let mut in_offsets = vec![0u32; node_count + 1];
        for edge in &self.edges {
            out_offsets[compact[edge.from as usize] as usize + 1] += 1;
            in_offsets[compact[edge.to as usize] as usize + 1] += 1;
        }
        for n in 0..node_count {
            out_offsets[n + 1] += out_offsets[n];
            in_offsets[n + 1] += in_offsets[n];
        }
        let edge_count = self.edges.len();
        let mut cursor = out_offsets.clone();
        let mut order = vec![0u32; edge_count];
        for (index, edge) in self.edges.iter().enumerate() {
            let from = compact[edge.from as usize] as usize;
            order[cursor[from] as usize] = index as u32;
            cursor[from] += 1;
        }
        let pick = |f: &dyn Fn(&RawEdge) -> u32| -> Vec<u32> {
            order.iter().map(|&i| f(&self.edges[i as usize])).collect()
        };
        let edge_from = pick(&|e| compact[e.from as usize]);
        let edge_to = pick(&|e| compact[e.to as usize]);
        let edge_length: Vec<f64> = order
            .iter()
            .map(|&i| self.edges[i as usize].length)
            .collect();
        let edge_way: Vec<i64> = order.iter().map(|&i| self.edges[i as usize].way).collect();
        let edge_flags: Vec<u8> = order
            .iter()
            .map(|&i| self.edges[i as usize].flags)
            .collect();
        drop(order);
        drop(std::mem::take(&mut self.edges));
        let edge_cost: Vec<f64> = edge_length
            .iter()
            .zip(&edge_flags)
            .map(|(&length, &flags)| {
                if flags & SEVERE != 0 {
                    length * restricted_access_factor
                } else {
                    length
                }
            })
            .collect();

        // Incoming edges, ascending per node.
        let mut cursor = in_offsets.clone();
        let mut in_edges = vec![0u32; edge_count];
        for (edge, &to) in edge_to.iter().enumerate() {
            in_edges[cursor[to as usize] as usize] = edge as u32;
            cursor[to as usize] += 1;
        }

        // Restrictions at vertices; one whose via node is on no edge cannot apply.
        let mut restrictions = Vec::with_capacity(self.restrictions.len());
        for &(from_way, to_way, via, only) in &self.restrictions {
            match compact[via as usize] {
                u32::MAX => self.counts.restrictions_off_graph += 1,
                via => restrictions.push(Restriction {
                    via,
                    from_way,
                    only,
                    to_way,
                }),
            }
        }
        restrictions.sort_unstable();
        let mut restricted_via = vec![0u64; node_count.div_ceil(64)];
        for r in &restrictions {
            restricted_via[r.via as usize / 64] |= 1 << (r.via % 64);
        }

        // The A* heuristic measures in the projection; scaled by the smallest ratio of ground
        // length to projected length of any edge it stays a lower bound of every path's cost.
        let heuristic_scale = (0..edge_count)
            .into_par_iter()
            .filter_map(|e| {
                let (a, b) = (node_xy[edge_from[e] as usize], node_xy[edge_to[e] as usize]);
                let projected = (b.easting - a.easting).hypot(b.northing - a.northing);
                (projected >= 1.0).then(|| edge_length[e] / projected)
            })
            .reduce(|| 1.0, f64::min)
            .min(1.0);

        let snaps = SnapIndex::new(&node_xy, &edge_from, &edge_to);
        let mut counts = self.counts;
        counts.nodes = node_count;
        counts.edges = edge_count;
        counts.road_edges = edge_flags.iter().filter(|&&f| f & ROAD != 0).count();
        counts.tram_edges = edge_flags.iter().filter(|&&f| f & TRAM != 0).count();
        counts.severe_edges = edge_flags.iter().filter(|&&f| f & SEVERE != 0).count();
        counts.service_edges = edge_flags.iter().filter(|&&f| f & SERVICE != 0).count();
        counts.restrictions = restrictions.len();
        Ok(RoutingGraph {
            node_ids,
            node_xy,
            out_offsets,
            edge_from,
            edge_to,
            edge_length,
            edge_cost,
            edge_way,
            edge_flags,
            in_offsets,
            in_edges,
            restrictions,
            restricted_via,
            heuristic_scale,
            snaps,
            counts,
        })
    }
}
