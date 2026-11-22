//! A fixed batch: what stop matching hands the merge.
//!
//! Stop matching ([`crate::fix`]) works on one batch at a time, because a batch is the trip
//! context its rules need. Its result is the batch plus, for each of its stops, the name the
//! stop is published under, the okres and country it lies in, and where it is. The merge
//! ([`crate::merge`]) reads nothing else about a stop. There are no fixed batches on disk and
//! no extension files: this is the whole interface.

use jr_model::Coord;

use crate::batch::Batch;

#[derive(Debug, Clone)]
pub struct FixedBatch {
    pub batch: Batch,
    /// One per `batch.stops` entry, in the same order.
    pub stops: Vec<FixedStop>,
    /// Where each stop's boarding posts might be, for the post estimator.
    pub post_candidates: Vec<PostCandidate>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FixedStop {
    /// The published name triple (JDF-R036–R038, R056–R060): it can differ from the source's.
    pub town: Box<str>,
    pub district: Option<Box<str>>,
    pub nearby_place: Option<Box<str>>,
    /// The source's okres and country, or the ones inferred for it (JDF-R053, R055, R061–R063).
    pub okres: Option<Box<str>>,
    pub country: Option<Box<str>>,
    pub location: Option<Location>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    pub coord: Coord,
    pub origin: Origin,
    /// What the position was taken from, for diagnostics and review: catalogue rows, OSM
    /// nodes, or the trip an estimate was interpolated along.
    pub source: Box<str>,
}

/// How a stop's position is known. The first three are precise; the estimates are not, and the
/// merge prefers a precise position over an estimated one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Origin {
    /// A reviewed correction (the gap-fill catalogue).
    Gapfill,
    /// A regional stop catalogue.
    Catalogue,
    /// OpenStreetMap.
    Osm,
    /// Interpolated along a trip by scheduled time.
    RouteTime,
    /// Placed past the last located stop of a trip.
    RouteEnd,
}

impl Origin {
    pub const fn is_precise(self) -> bool {
        matches!(self, Self::Gapfill | Self::Catalogue | Self::Osm)
    }
}

/// One observed position that may be a boarding post of a stop (JDF-R066).
#[derive(Debug, Clone, PartialEq)]
pub struct PostCandidate {
    /// Position of the stop in `batch.stops`.
    pub stop: u32,
    pub coord: Coord,
    /// The catalogue row or OSM node it is, as an identifier stable between builds.
    pub observation: Box<str>,
    /// The tags and attributes the estimator reads (modes, lifecycle, local reference), as
    /// name and value pairs in a fixed order.
    pub facts: Vec<(Box<str>, Box<str>)>,
}
