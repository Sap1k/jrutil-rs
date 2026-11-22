//! The directed road and tram graph in compressed sparse rows (POST-R010, R017–R019).

use jr_model::Rule;

use crate::laea::{self, Laea};

use super::snap::SnapIndex;

/// Every routable OSM node that an edge touches is a vertex, numbered in ascending OSM ID; every
/// consecutive node pair of an eligible way gives one or two directed edges, numbered by from
/// node and, within it, in source order (the order F#'s counting scatter gives, so ties broken by
/// edge number fall the same way). Positions are WGS84 projected to EPSG:3035
/// ([`crate::laea`]) in `f64` (verdict Q2): snapping, headings and sides are plane geometry in
/// that projection. An edge's length is the plane distance of its projected nodes, as in F#:
/// costs, path lengths, excesses and the heading window are projected metres, which in Czechia
/// run about 0.2 % longer than the ground. The learned scorer was trained on evidence measured
/// this way, so the unit is part of its input contract (format-compatible, verdict Q1); this
/// is not a great-circle distance and `jr_model::geo::distance_m` is deliberately not used.
/// In memory, no temporary files.
pub const POST_R010: Rule = Rule {
    id: "POST-R010",
    summary: "graph vertices are the OSM nodes of eligible ways, projected to EPSG:3035",
};

/// A consecutive node pair of an eligible way gives a forward and/or a backward edge (R015)
/// unless a node is missing from the extract (the chain breaks there), the two references are
/// the same node, or either node is a barrier (R016). Each edge carries its way's ID and flags.
pub const POST_R017: Rule = Rule {
    id: "POST-R017",
    summary: "consecutive way nodes give directed edges carrying the way's ID and flags",
};

/// Buses and trolleybuses both use road edges (the OSM filter carries no trolley-wire network);
/// trams use tram edges. A context of mode `E` routes as a tram, `T` as a trolleybus, anything
/// else as a bus.
pub const POST_R019: Rule = Rule {
    id: "POST-R019",
    summary: "bus and trolleybus route on road edges, tram on tram edges",
};

/// The road flag (R011).
pub(super) const ROAD: u8 = 1;
/// The tram flag (R014).
pub(super) const TRAM: u8 = 2;
/// The severe access flag (R012).
pub(super) const SEVERE: u8 = 4;
/// The service class flag (R013).
pub(super) const SERVICE: u8 = 8;

/// A routing mode (R019).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RoutingMode {
    Bus,
    Trolleybus,
    Tram,
}

impl RoutingMode {
    /// The mode a context of the given JDF mode code routes as.
    pub fn for_context_mode(code: &str) -> RoutingMode {
        match code {
            "E" => RoutingMode::Tram,
            "T" => RoutingMode::Trolleybus,
            _ => RoutingMode::Bus,
        }
    }

    pub(super) fn flag(self) -> u8 {
        match self {
            RoutingMode::Bus | RoutingMode::Trolleybus => ROAD,
            RoutingMode::Tram => TRAM,
        }
    }
}

/// A turn restriction resolved to a graph node (R018). The ways are indexes into the graph's
/// ways, or [`NO_WAY`] for a way that gave no edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Restriction {
    pub via: u32,
    pub from_way: u32,
    pub only: bool,
    pub to_way: u32,
}

/// A restriction's way that is not on the graph: no edge is of it.
pub(super) const NO_WAY: u32 = u32::MAX;

/// Counts of what the graph holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GraphCounts {
    pub nodes: usize,
    pub edges: usize,
    /// Edges usable by buses and trolleybuses (road flag).
    pub road_edges: usize,
    /// Edges usable by trams.
    pub tram_edges: usize,
    pub severe_edges: usize,
    pub service_edges: usize,
    /// Ways that gave at least one edge, by flag.
    pub road_ways: usize,
    pub tram_ways: usize,
    pub restrictions: usize,
    /// Turn restrictions whose via node is in the extract but on no edge (they cannot apply).
    pub restrictions_off_graph: usize,
    /// Relations with a restriction tag whose members are not from way, via node, to way, or
    /// whose via node is missing from the extract (R018 ignores them).
    pub restrictions_ignored: usize,
}

/// The graph. Built once by [`super::GraphBuilder`], then shared read-only by any number of
/// threads.
#[derive(Debug)]
pub struct RoutingGraph {
    pub(super) node_ids: Vec<i64>,
    pub(super) node_xy: Vec<Laea>,
    /// Outgoing edges of node `n` are `out_offsets[n]..out_offsets[n + 1]`.
    pub(super) out_offsets: Vec<u32>,
    pub(super) edge_from: Vec<u32>,
    pub(super) edge_to: Vec<u32>,
    pub(super) edge_length: Vec<f64>,
    /// The edge's way, as an index into `way_ids`.
    pub(super) edge_way: Vec<u32>,
    pub(super) edge_flags: Vec<u8>,
    /// The OSM ways that gave an edge, ascending.
    pub(super) way_ids: Vec<i64>,
    /// What an edge closed to buses costs per metre (R012).
    pub(super) restricted_access_factor: f64,
    /// Incoming edges of node `n` are `in_edges[in_offsets[n]..in_offsets[n + 1]]`, ascending.
    pub(super) in_offsets: Vec<u32>,
    pub(super) in_edges: Vec<u32>,
    /// Sorted by (via, from way, only, to way).
    pub(super) restrictions: Vec<Restriction>,
    /// A bit per node: is it the via node of a restriction?
    pub(super) restricted_via: Vec<u64>,
    pub(super) snaps: SnapIndex,
    pub(super) counts: GraphCounts,
}

impl RoutingGraph {
    pub fn counts(&self) -> GraphCounts {
        self.counts
    }

    pub fn node_count(&self) -> usize {
        self.node_ids.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edge_from.len()
    }

    /// The OSM node of a vertex.
    pub fn node_osm_id(&self, node: u32) -> i64 {
        self.node_ids[node as usize]
    }

    /// A vertex's position in WGS84.
    pub fn node_coord(&self, node: u32) -> jr_model::Coord {
        laea::to_wgs84(self.node_xy[node as usize])
    }

    /// The vertex of an OSM node, if it is on the graph.
    pub fn node_of(&self, osm_id: i64) -> Option<u32> {
        self.node_ids.binary_search(&osm_id).ok().map(|n| n as u32)
    }

    pub fn edge_from(&self, edge: u32) -> u32 {
        self.edge_from[edge as usize]
    }

    pub fn edge_to(&self, edge: u32) -> u32 {
        self.edge_to[edge as usize]
    }

    /// The OSM way an edge belongs to.
    pub fn edge_way(&self, edge: u32) -> i64 {
        self.way_ids[self.edge_way[edge as usize] as usize]
    }

    /// Length in metres on the ground.
    pub fn edge_length(&self, edge: u32) -> f64 {
        self.edge_length[edge as usize]
    }

    /// Penalised cost in metres (R012).
    pub fn edge_cost(&self, edge: u32) -> f64 {
        let length = self.edge_length[edge as usize];
        if self.edge_is_severe(edge) {
            length * self.restricted_access_factor
        } else {
            length
        }
    }

    /// Whether a mode may use the edge.
    pub fn edge_allows(&self, edge: u32, mode: RoutingMode) -> bool {
        self.edge_flags[edge as usize] & mode.flag() != 0
    }

    pub fn edge_is_service(&self, edge: u32) -> bool {
        self.edge_flags[edge as usize] & SERVICE != 0
    }

    pub fn edge_is_severe(&self, edge: u32) -> bool {
        self.edge_flags[edge as usize] & SEVERE != 0
    }

    /// The edges leaving a vertex.
    pub fn outgoing(&self, node: u32) -> std::ops::Range<u32> {
        self.out_offsets[node as usize]..self.out_offsets[node as usize + 1]
    }

    /// The edges entering a vertex, ascending.
    pub fn incoming(&self, node: u32) -> &[u32] {
        let range =
            self.in_offsets[node as usize] as usize..self.in_offsets[node as usize + 1] as usize;
        &self.in_edges[range]
    }

    pub(super) fn xy(&self, node: u32) -> Laea {
        self.node_xy[node as usize]
    }

    /// R018: may a vehicle arriving on `incoming` continue on `outgoing` at their shared node?
    pub fn transition_allowed(&self, incoming: u32, outgoing: u32) -> bool {
        let via = self.edge_to(incoming);
        if self.restricted_via[via as usize / 64] & (1 << (via % 64)) == 0 {
            return true;
        }
        let from_way = self.edge_way[incoming as usize];
        let to_way = self.edge_way[outgoing as usize];
        let first = self
            .restrictions
            .partition_point(|r| (r.via, r.from_way) < (via, from_way));
        self.restrictions[first..]
            .iter()
            .take_while(|r| r.via == via && r.from_way == from_way)
            .all(|r| {
                if r.only {
                    r.to_way == to_way
                } else {
                    r.to_way != to_way
                }
            })
    }
}
