//! The post estimator's evidence pack, evidence-v2 (`docs/rules/posts.md`, sections 2.3 and
//! 3.6, POST-R060–R062; verdicts Q1, Q8, Q9).
//!
//! A pack is five relations and a manifest. In memory it is a [`Pack`] of typed, column-wise
//! tables that another package may build directly (verdict Q8); [`check`] states the
//! invariants such a pack must satisfy, [`write`] turns it into the six files and [`read`]
//! turns the six files back into a checked pack. The crate never opens a file: the caller
//! hands over the bytes and is responsible for the directory holding exactly these six files
//! ([`FILES`]).
//!
//! Format compatibility (verdict Q1): the reader accepts packs captured by F# JrUtil
//! (`router_evidence_version` [`FSHARP_ROUTER_EVIDENCE_VERSION`]) and by jrutil-rs
//! ([`RUST_ROUTER_EVIDENCE_VERSION`]). Both have the same manifest keys, relations, columns and
//! meaning. The router version takes part in the pack ID, and a Rust pack's schema
//! fingerprints name Rust types instead of .NET's, so the F# reader refuses a Rust pack and a
//! Rust pack is never taken for an F# one.

use std::fmt;

mod check;
mod decode;
mod encode;
pub mod ids;
mod manifest;
mod schema;
mod tables;

pub use check::check;
pub use manifest::{CaptureCeilings, FileEntry, Manifest, parse_manifest};
pub use tables::{
    Availability, BlockRole, Contexts, CorridorVariants, Observations, RoutePointEvidence,
    RoutePoints,
};

/// `evidence_format` (format constant).
pub const EVIDENCE_FORMAT: &str = "post-inference-evidence-v2";
/// `schema_version` (format constant).
pub const EVIDENCE_SCHEMA_VERSION: i64 = 2;
/// `router_evidence_version` of packs the F# router captured.
pub const FSHARP_ROUTER_EVIDENCE_VERSION: &str = "packed-directed-v3";
/// `router_evidence_version` of packs the jrutil-rs router captures (verdict Q1: bumped so a
/// Rust pack is never read as an F# one).
pub const RUST_ROUTER_EVIDENCE_VERSION: &str = "jrutil-rs-directed-v1";
/// `variant_enumeration_version` (format constant: up to three cheapest distinct corridors).
pub const VARIANT_ENUMERATION_VERSION: &str = "directed-thread-top3-v1";
/// `capture_ceilings.routed_excess_metres`: attachments beyond it are not captured.
pub const CAPTURE_ROUTED_EXCESS_METRES: f64 = 1000.0;
/// `capture_ceilings.maximum_corridor_variants`.
pub const CAPTURE_MAXIMUM_CORRIDOR_VARIANTS: i64 = 3;
/// `maximum_search_states`.
pub const MAXIMUM_SEARCH_STATES: i64 = 100_000;
/// `maximum_search_distance_metres`.
pub const MAXIMUM_SEARCH_DISTANCE_METRES: f64 = 30_000.0;

/// The manifest's file name.
pub const MANIFEST: &str = "manifest.json";
/// The five relation files in manifest order.
pub const RELATIONS: [&str; 5] = [
    "observations.parquet",
    "route_points.parquet",
    "contexts.parquet",
    "corridor_variants.parquet",
    "route_point_evidence.parquet",
];
/// Every file of a pack directory; a directory with any other file is not a pack (POST-R061).
pub const FILES: [&str; 6] = [
    MANIFEST,
    RELATIONS[0],
    RELATIONS[1],
    RELATIONS[2],
    RELATIONS[3],
    RELATIONS[4],
];

/// The pack is written as the five relations of section 2.3 and a manifest: Snappy, row groups
/// of 65,536 rows, the `obehy.*` footer keys, the manifest listing each file's size, SHA-256,
/// rows and schema fingerprint. Writing is deterministic.
///
/// The pack is the evaluator's boundary: a policy or scorer change is judged on a pack before
/// any build (section 5).
pub const POST_R060: jr_model::Rule = jr_model::Rule {
    id: "POST-R060",
    summary: "pack written as five Snappy Parquet relations and a manifest",
};

/// A pack is read only when its manifest shape, versions, identities, pack ID, ceilings and
/// counts, every file's size, hash, rows, schema, fingerprint and footer, and every relation
/// invariant ([`check`]) hold.
///
/// "The store has already passed structural, hash, ordering, key, FK, sentinel, and coverage
/// checks" (F# evaluator): nothing unvalidated reaches a decision.
pub const POST_R061: jr_model::Rule = jr_model::Rule {
    id: "POST-R061",
    summary: "packs are fully validated before evaluation",
};

/// The five relations must carry the same `obehy.source_id`; F# checked only that each is
/// non-blank.
///
/// A pack has one snapshot source; relations of different snapshots are not one pack.
pub const POST_R182: jr_model::Rule = jr_model::Rule {
    id: "POST-R182",
    summary: "all relations of a pack name the same source_id",
};

/// Which router captured a pack (`router_evidence_version`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RouterEvidence {
    /// F# JrUtil, `packed-directed-v3`.
    FSharp,
    /// jrutil-rs.
    Rust,
}

impl RouterEvidence {
    pub fn as_str(self) -> &'static str {
        match self {
            RouterEvidence::FSharp => FSHARP_ROUTER_EVIDENCE_VERSION,
            RouterEvidence::Rust => RUST_ROUTER_EVIDENCE_VERSION,
        }
    }

    pub fn parse(text: &str) -> Option<RouterEvidence> {
        match text {
            FSHARP_ROUTER_EVIDENCE_VERSION => Some(RouterEvidence::FSharp),
            RUST_ROUTER_EVIDENCE_VERSION => Some(RouterEvidence::Rust),
            _ => None,
        }
    }
}

/// What a pack was captured from and by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackIdentity {
    /// `capture_tool_version`, with the restriction markers of POST-R005 when restricted.
    pub capture_tool_version: String,
    /// SHA-256 of the merged JDF payload (lower-case hex).
    pub merged_jdf_sha256: String,
    /// SHA-256 of the routing graph's input.
    pub routing_pbf_sha256: String,
    /// The snapshot descriptor's `source_id` (Parquet footer `obehy.source_id`).
    pub source_id: String,
    pub router: RouterEvidence,
}

impl PackIdentity {
    /// `pack_id` (section 2.3): `sha` of the capture tool version, both input hashes (lower
    /// case), the format, schema and router versions and the capture ceilings, joined `|`.
    pub fn pack_id(&self) -> String {
        manifest::pack_id(
            &self.capture_tool_version,
            &self.merged_jdf_sha256,
            &self.routing_pbf_sha256,
            self.router,
        )
    }
}

/// An evidence pack in memory.
#[derive(Clone, Debug, PartialEq)]
pub struct Pack {
    pub identity: PackIdentity,
    pub observations: Observations,
    pub route_points: RoutePoints,
    pub contexts: Contexts,
    pub corridor_variants: CorridorVariants,
    pub route_point_evidence: RoutePointEvidence,
}

/// The six files of a pack, as bytes.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct PackFiles {
    pub manifest: Vec<u8>,
    pub observations: Vec<u8>,
    pub route_points: Vec<u8>,
    pub contexts: Vec<u8>,
    pub corridor_variants: Vec<u8>,
    pub route_point_evidence: Vec<u8>,
}

impl fmt::Debug for PackFiles {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PackFiles")
            .field("manifest", &self.manifest.len())
            .field("observations", &self.observations.len())
            .field("route_points", &self.route_points.len())
            .field("contexts", &self.contexts.len())
            .field("corridor_variants", &self.corridor_variants.len())
            .field("route_point_evidence", &self.route_point_evidence.len())
            .finish()
    }
}

impl PackFiles {
    /// The bytes of a relation by file name.
    pub fn relation(&self, name: &str) -> Option<&[u8]> {
        Some(match name {
            "observations.parquet" => &self.observations,
            "route_points.parquet" => &self.route_points,
            "contexts.parquet" => &self.contexts,
            "corridor_variants.parquet" => &self.corridor_variants,
            "route_point_evidence.parquet" => &self.route_point_evidence,
            _ => return None,
        })
    }
}

/// Reads and fully validates a pack (POST-R061): the manifest's shape, versions, identities,
/// pack ID, ceilings and counts; each relation's size, SHA-256, rows, schema, fingerprint and
/// footer metadata against the manifest; then every invariant of [`check`].
pub fn read(files: &PackFiles) -> Result<Pack, PackError> {
    let manifest = parse_manifest(&files.manifest)?;
    decode::decode(&manifest, files)
}

/// Writes a pack (POST-R060): the five relations (Snappy, row groups of 65,536 rows, the
/// footer metadata of section 2.3) and the manifest, deterministically. The pack is checked
/// first; a pack that [`check`] rejects is not written.
pub fn write(pack: &Pack) -> Result<PackFiles, PackError> {
    check(pack)?;
    encode::encode(pack)
}

/// Why a pack was rejected.
#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error("manifest.json is not JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error(
        "unsupported post-inference evidence format {found:?}; evidence v1 is intentionally \
         rejected, recapture v2"
    )]
    Format { found: Option<String> },
    #[error("evidence manifest shape is obsolete or incomplete ({0}); recapture evidence v2")]
    Shape(&'static str),
    #[error("evidence manifest field {field} has the wrong type")]
    FieldType { field: &'static str },
    #[error("unsupported evidence schema version {0}")]
    SchemaVersion(i64),
    #[error("evidence router contract {router:?}/{variants:?} is not supported; recapture")]
    RouterContract { router: String, variants: String },
    #[error("evidence capture tool version is missing")]
    CaptureToolVersion,
    #[error("evidence identity {field} is not a SHA-256: {value:?}")]
    Sha256 { field: &'static str, value: String },
    #[error("evidence pack ID {stored} does not match its inputs and ceilings ({expected})")]
    PackId { stored: String, expected: String },
    #[error("evidence routing ceilings do not match the supported router contract")]
    Ceilings,
    #[error("evidence manifest relation counts must be non-negative")]
    NegativeCount,
    #[error("evidence manifest lists {0} more than once")]
    DuplicateFile(String),
    #[error("evidence manifest is missing {0}")]
    MissingFile(&'static str),
    #[error("evidence manifest must list exactly the five v2 relations")]
    ExtraFile,
    #[error("evidence file {file}: size {actual} differs from the manifest's {declared}")]
    FileSize {
        file: &'static str,
        declared: i64,
        actual: usize,
    },
    #[error("evidence file {file}: hash mismatch")]
    FileHash { file: &'static str },
    #[error("evidence file {file}: manifest metadata is invalid")]
    FileMetadata { file: &'static str },
    #[error("evidence file {file}: manifest row count is inconsistent")]
    RowCount { file: &'static str },
    #[error("evidence file {file}: {source}")]
    Parquet {
        file: &'static str,
        source: parquet::errors::ParquetError,
    },
    #[error("evidence file {file}: {source}")]
    Arrow {
        file: &'static str,
        source: arrow_schema::ArrowError,
    },
    #[error("evidence file {file}: schema type/nullability mismatch")]
    Schema { file: &'static str },
    #[error("evidence file {file}: footer metadata {key} mismatch")]
    FooterMetadata {
        file: &'static str,
        key: &'static str,
    },
    #[error("evidence file {file}: schema fingerprint mismatch")]
    Fingerprint { file: &'static str },
    #[error("evidence file {file}: physical row count mismatch")]
    PhysicalRows { file: &'static str },
    #[error("evidence {relation} row {row}: {violation}")]
    Invariant {
        relation: &'static str,
        row: usize,
        violation: Violation,
    },
    #[error("evidence semantic counts do not match the manifest")]
    Counts,
}

impl PackError {
    pub(crate) fn invariant(relation: &'static str, row: usize, violation: Violation) -> Self {
        PackError::Invariant {
            relation,
            row,
            violation,
        }
    }
}

/// A broken invariant of the relations (the F# store's semantic checks).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Violation {
    #[error("column vectors have different lengths")]
    ColumnLengths,
    #[error("identity field {0} is blank or not a SHA-256")]
    Identity(&'static str),
    #[error("invalid stop identity {0:?}")]
    StopIdentity(String),
    #[error("text {0:?} is not one of the allowed values")]
    Enumeration(String),
    #[error("observations are not uniquely stop-major ordered")]
    ObservationOrder,
    #[error("observation contains invalid numeric facts")]
    ObservationNumeric,
    #[error("observation references an unknown route point")]
    ObservationRoutePoint,
    #[error("route points are not uniquely stop-major ordered")]
    RoutePointOrder,
    #[error("route-point observation membership is inconsistent")]
    RoutePointMembership,
    #[error("route point contains invalid numeric facts")]
    RoutePointNumeric,
    #[error("contexts are not uniquely stop-major ordered")]
    ContextOrder,
    #[error("context's stop has no route points")]
    ContextStop,
    #[error("context assignment or block facts are invalid")]
    ContextFacts,
    #[error("context neighbour facts conflict with its block role")]
    ContextNeighbours,
    #[error("context identity is inconsistent")]
    ContextIdentity,
    #[error("same-stop block identity is inconsistent")]
    BlockIdentity,
    #[error("movement-family identity is inconsistent")]
    MovementFamily,
    #[error("context is missing corridor variants")]
    MissingVariants,
    #[error("corridor ranks are not contiguous from zero")]
    VariantRank,
    #[error("corridor stop, rank, or counts are invalid")]
    VariantCounts,
    #[error("unavailable corridor sentinel is invalid")]
    UnavailableVariant,
    #[error("available corridor facts are invalid")]
    AvailableVariant,
    #[error("corridor costs are not monotonically ordered")]
    VariantCostOrder,
    #[error("baseline-relative rank-0 costs must be zero")]
    BaselineCost,
    #[error("corridor references an unknown context")]
    VariantContext,
    #[error("attachments are not uniquely context-major ordered")]
    AttachmentOrder,
    #[error("attachment foreign keys are inconsistent")]
    AttachmentForeignKey,
    #[error("unavailable corridor has an available attachment")]
    AttachmentOfUnavailable,
    #[error("unavailable attachment sentinel is invalid")]
    UnavailableAttachment,
    #[error("available attachment facts are invalid")]
    AvailableAttachment,
    #[error("attachment numeric ranges are invalid")]
    AttachmentRange,
    #[error("attachment coverage is incomplete")]
    AttachmentCoverage,
    #[error("attachment references an unknown corridor")]
    AttachmentVariant,
}
