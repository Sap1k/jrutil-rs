//! A generic OSM PBF element stream; which tags matter is the caller's business.
//!
//! The PBF container is decoded here directly (a length-prefixed `BlobHeader`, a `Blob` holding
//! raw or zlib-compressed `PrimitiveBlock`s; <https://wiki.openstreetmap.org/wiki/PBF_Format>).
//! Blobs compressed with anything but zlib are an error; metadata (`Info`, `DenseInfo`) is
//! skipped.
//!
//! Two entry points:
//!
//! - [`tagged_nodes`]: every node with at least one tag, owned, in file order.
//! - [`read_blocks`]: whole decoded blocks in file order, holding the nodes (with or without
//!   tags), ways and relations [`Elements`] asks for. Blobs are decompressed and decoded on
//!   several threads; the visitor still sees the blocks, and the elements in each block, in file
//!   order. Untagged nodes carry no tag storage at all.

mod block;
mod protobuf;
mod stream;

use std::io::{self, Read};

use jr_model::Coord;

pub use block::{
    Block, Element, ElementKind, Member, MemberKind, Members, NodeView, RelationView, Tags, WayView,
};

/// A node with its tags in file order.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: i64,
    pub coord: Coord,
    pub tags: Vec<(Box<str>, Box<str>)>,
}

#[derive(Debug, thiserror::Error)]
pub enum OsmError {
    #[error("OSM PBF: {0}")]
    Io(#[from] io::Error),
    #[error("OSM PBF: truncated file")]
    Truncated,
    #[error("OSM PBF: malformed protobuf in {0}")]
    Malformed(&'static str),
    #[error("OSM PBF: {what} of {size} bytes exceeds the format's limit")]
    TooLarge { what: &'static str, size: u64 },
    #[error("OSM PBF: unsupported blob compression (field {0})")]
    Compression(u32),
    #[error("OSM PBF: string table entry {0} is out of range")]
    StringIndex(u64),
    #[error("OSM PBF: tag text is not UTF-8")]
    Utf8,
    #[error("OSM PBF: relation member type {0} is unknown")]
    MemberType(u64),
}

/// Which elements [`read_blocks`] decodes. Nodes with tags are always decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Elements {
    /// Nodes without tags (their ID and coordinate).
    pub untagged_nodes: bool,
    pub ways: bool,
    pub relations: bool,
}

impl Elements {
    /// Everything.
    pub const ALL: Elements = Elements {
        untagged_nodes: true,
        ways: true,
        relations: true,
    };
    /// Only nodes with tags.
    pub const TAGGED_NODES: Elements = Elements {
        untagged_nodes: false,
        ways: false,
        relations: false,
    };
}

/// Calls `visit` for every decoded block, in file order. A block holds the elements of one
/// `PrimitiveBlock` that `elements` selects, in file order ([`Block::elements`]); there is one
/// call per `OSMData` blob, empty or not. Header blobs are skipped.
pub fn read_blocks<R: Read + Send>(
    pbf: R,
    elements: Elements,
    visit: impl FnMut(&Block),
) -> Result<(), OsmError> {
    stream::read_blocks(pbf, elements, visit)
}

/// Calls `visit` for every node that has at least one tag, in file order.
pub fn tagged_nodes<R: Read + Send>(pbf: R, mut visit: impl FnMut(Node)) -> Result<(), OsmError> {
    read_blocks(pbf, Elements::TAGGED_NODES, |block| {
        for node in block.nodes() {
            visit(Node {
                id: node.id,
                coord: node.coord,
                tags: node
                    .tags
                    .iter()
                    .map(|(key, value)| (key.into(), value.into()))
                    .collect(),
            });
        }
    })
}
