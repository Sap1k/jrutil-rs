//! The road and tram graph and router of the boarding-post estimator (`docs/rules/posts.md`,
//! sections 3.2 and 3.5).
//!
//! [`GraphBuilder`] turns OSM nodes, ways and relations ([`OsmNode`], [`OsmWay`],
//! [`OsmRelation`]) into a [`RoutingGraph`]: directed edges in compressed sparse rows, turn
//! restrictions, and a grid for snapping. [`RoutingGraph::routed_evidence`] computes what one
//! context contributes to the evidence pack: up to three corridor variants from the previous
//! anchor to the next and the attachment of every route point of the stop to each. It is pure,
//! deterministic and shared by any number of threads. Thresholds and tag lists are
//! [`RoutingPolicy`] (`policy/routing.toml`).

mod build;
mod evidence;
mod geometry;
mod graph;
mod input;
mod policy;
mod search;
mod snap;
mod thread;

use jr_model::Rule;

pub use build::{
    GraphBuilder, GraphError, POST_R011, POST_R012, POST_R013, POST_R014, POST_R015, POST_R016,
    POST_R018,
};
pub use evidence::{
    Attachment, AttachmentFailure, Availability, ContextGeometry, CorridorVariant, POST_R050,
    POST_R051, POST_R052, RoutedEvidence, VariantFailure,
};
pub use geometry::POST_R053;
pub use graph::{GraphCounts, POST_R010, POST_R017, POST_R019, RoutingGraph, RoutingMode};
pub use input::{OsmMember, OsmMemberType, OsmNode, OsmRelation, OsmTags, OsmWay};
pub use policy::{EvidencePolicy, GraphPolicy, RoutingPolicy, SearchPolicy, SnapPolicy, TagValue};
pub use search::{POST_R021, Path, RouteFailure};
pub use snap::{POST_R020, Snap};
pub use thread::{CorridorId, DirectedThread, Face, FaceSide, POST_R022, POST_R023, ThreadId};

/// The router's rules, in ID order.
pub const RULES: &[Rule] = &[
    POST_R010, POST_R011, POST_R012, POST_R013, POST_R014, POST_R015, POST_R016, POST_R017,
    POST_R018, POST_R019, POST_R020, POST_R021, POST_R022, POST_R023, POST_R050, POST_R051,
    POST_R052, POST_R053,
];
