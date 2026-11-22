//! Tracked reads of the graph: every routing algorithm reaches the graph through a [`Reader`],
//! which notes where in the plane it read.
//!
//! The primitives that take a node (its position, its outgoing and incoming edges, its OSM ID)
//! note the node's position; the snap grid query notes its whole query square. Edge attributes
//! need no note: an edge is only ever obtained from the adjacency of a noted node or from a
//! noted grid query, so the edge lies in the region of the box (its box touches a noted
//! position's snap cell). The read box is what makes a routed result cacheable per region
//! ([`super::Regions`]): the result is a function of the context, the policy, the router code
//! and what lies in the tiles of the box.

use std::cell::Cell;
use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::laea::Laea;

use super::graph::{RoutingGraph, RoutingMode};

/// An axis-aligned box of projected metres; empty while nothing was read.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReadBox {
    pub min_easting: f64,
    pub min_northing: f64,
    pub max_easting: f64,
    pub max_northing: f64,
}

impl ReadBox {
    /// Nothing read.
    pub const EMPTY: ReadBox = ReadBox {
        min_easting: f64::INFINITY,
        min_northing: f64::INFINITY,
        max_easting: f64::NEG_INFINITY,
        max_northing: f64::NEG_INFINITY,
    };

    pub fn is_empty(&self) -> bool {
        self.min_easting > self.max_easting
    }

    fn note(&mut self, point: Laea) {
        self.min_easting = self.min_easting.min(point.easting);
        self.min_northing = self.min_northing.min(point.northing);
        self.max_easting = self.max_easting.max(point.easting);
        self.max_northing = self.max_northing.max(point.northing);
    }
}

/// The graph as an algorithm sees it: every node-taking read is noted in the read box. Not
/// `Sync`: one reader per computation.
#[derive(Debug)]
pub(super) struct Reader<'g> {
    graph: &'g RoutingGraph,
    read: Cell<ReadBox>,
}

impl<'g> Reader<'g> {
    pub fn new(graph: &'g RoutingGraph) -> Self {
        Reader {
            graph,
            read: Cell::new(ReadBox::EMPTY),
        }
    }

    /// Where this reader has read so far.
    pub fn read_box(&self) -> ReadBox {
        self.read.get()
    }

    fn note(&self, point: Laea) {
        let mut read = self.read.get();
        read.note(point);
        self.read.set(read);
    }

    fn note_node(&self, node: u32) -> Laea {
        let point = self.graph.xy(node);
        self.note(point);
        point
    }

    /// A node's position.
    pub fn xy(&self, node: u32) -> Laea {
        self.note_node(node)
    }

    /// A node's OSM ID.
    pub fn node_osm_id(&self, node: u32) -> i64 {
        self.note_node(node);
        self.graph.node_osm_id(node)
    }

    /// The edges leaving a node.
    pub fn outgoing(&self, node: u32) -> Range<u32> {
        self.note_node(node);
        self.graph.outgoing(node)
    }

    /// The edges entering a node, ascending.
    pub fn incoming(&self, node: u32) -> &'g [u32] {
        self.note_node(node);
        self.graph.incoming(node)
    }

    /// The edges listed in the snap cells within `radius` of `point` (possibly repeated).
    pub fn grid_edges(&self, point: Laea, radius: f64, out: &mut Vec<u32>) {
        self.note(Laea {
            easting: point.easting - radius,
            northing: point.northing - radius,
        });
        self.note(Laea {
            easting: point.easting + radius,
            northing: point.northing + radius,
        });
        self.graph.snaps.near(point, radius, out);
    }

    pub fn edge_from(&self, edge: u32) -> u32 {
        self.graph.edge_from(edge)
    }

    pub fn edge_to(&self, edge: u32) -> u32 {
        self.graph.edge_to(edge)
    }

    /// The OSM way of an edge.
    pub fn edge_way(&self, edge: u32) -> i64 {
        self.graph.edge_way(edge)
    }

    pub fn edge_length(&self, edge: u32) -> f64 {
        self.graph.edge_length(edge)
    }

    pub fn edge_cost(&self, edge: u32) -> f64 {
        self.graph.edge_cost(edge)
    }

    pub fn edge_flags(&self, edge: u32) -> u8 {
        self.graph.edge_flags[edge as usize]
    }

    pub fn edge_allows(&self, edge: u32, mode: RoutingMode) -> bool {
        self.graph.edge_allows(edge, mode)
    }

    pub fn edge_is_service(&self, edge: u32) -> bool {
        self.graph.edge_is_service(edge)
    }

    pub fn edge_is_severe(&self, edge: u32) -> bool {
        self.graph.edge_is_severe(edge)
    }

    /// R018 at the node between the two edges (the incoming edge's to node, which the caller
    /// reached through a noted adjacency).
    pub fn transition_allowed(&self, incoming: u32, outgoing: u32) -> bool {
        self.graph.transition_allowed(incoming, outgoing)
    }
}
