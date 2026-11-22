//! The column schema of each relation and its manifest fingerprint (section 2.3).

use arrow_schema::{DataType, Field, Schema};

use super::RouterEvidence;
use super::ids::sha256_hex;

/// A column's stored type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ColumnType {
    String,
    Double,
    Int32,
    Int64,
    Bool,
}

impl ColumnType {
    fn arrow(self) -> DataType {
        match self {
            ColumnType::String => DataType::Utf8,
            ColumnType::Double => DataType::Float64,
            ColumnType::Int32 => DataType::Int32,
            ColumnType::Int64 => DataType::Int64,
            ColumnType::Bool => DataType::Boolean,
        }
    }

    /// The .NET type name the F# writer's fingerprint embeds.
    fn clr(self) -> &'static str {
        match self {
            ColumnType::String => "System.String",
            ColumnType::Double => "System.Double",
            ColumnType::Int32 => "System.Int32",
            ColumnType::Int64 => "System.Int64",
            ColumnType::Bool => "System.Boolean",
        }
    }

    /// The type name a Rust pack's fingerprint embeds (verdict Q1: format-compatible, own
    /// fingerprint).
    fn own(self) -> &'static str {
        match self {
            ColumnType::String => "utf8",
            ColumnType::Double => "float64",
            ColumnType::Int32 => "int32",
            ColumnType::Int64 => "int64",
            ColumnType::Bool => "bool",
        }
    }
}

use ColumnType::{Bool, Double, Int32, Int64, String as Text};

pub(crate) type Column = (&'static str, ColumnType, bool);

pub(crate) const OBSERVATIONS: &[Column] = &[
    ("gtfs_stop_place_id", Text, false),
    ("route_point_id", Text, false),
    ("observation_id", Text, false),
    ("source_kind", Text, false),
    ("source_object_id", Text, true),
    ("observed_at", Text, true),
    ("latitude", Double, false),
    ("longitude", Double, false),
    ("support_weight", Double, false),
    ("raw_tags", Text, false),
    ("explicit_modes", Text, false),
    ("denied_modes", Text, false),
    ("lifecycle", Text, false),
];

pub(crate) const ROUTE_POINTS: &[Column] = &[
    ("gtfs_stop_place_id", Text, false),
    ("route_point_id", Text, false),
    ("representative_observation_id", Text, false),
    ("observation_ids", Text, false),
    ("latitude", Double, false),
    ("longitude", Double, false),
    ("has_current_lifecycle", Bool, false),
    ("has_obsolete_lifecycle", Bool, false),
    ("source_support_weight", Double, false),
    ("explicit_modes", Text, false),
    ("denied_modes", Text, false),
];

pub(crate) const CONTEXTS: &[Column] = &[
    ("context_id", Text, false),
    ("gtfs_stop_place_id", Text, false),
    ("mode", Text, false),
    ("line_id", Text, false),
    ("route_distinction", Int32, false),
    ("direction", Int32, false),
    ("pattern_hash", Text, false),
    ("pattern_position", Int32, false),
    ("same_stop_block_role", Text, false),
    ("movement_family_id", Text, false),
    ("context_previous_stop_id", Text, true),
    ("context_next_stop_id", Text, true),
    ("assignment_kind", Text, false),
    ("authored_post_key", Text, true),
    ("same_stop_block_id", Text, true),
];

pub(crate) const CORRIDOR_VARIANTS: &[Column] = &[
    ("context_id", Text, false),
    ("gtfs_stop_place_id", Text, false),
    ("variant_rank", Int32, false),
    ("corridor_id", Text, true),
    ("absolute_cost_metres", Double, true),
    ("relative_cost_metres", Double, true),
    ("relative_cost_fraction", Double, true),
    ("path_length_metres", Double, true),
    ("directed_edge_count", Int32, false),
    ("repeated_directed_edge_count", Int32, false),
    ("service_edge_count", Int32, false),
    ("restricted_access_edge_count", Int32, false),
    ("access_penalty_metres", Double, true),
    ("ingress_thread_id", Text, true),
    ("egress_thread_id", Text, true),
    ("routing_availability", Text, false),
    ("invariant_failure_reason", Text, true),
];

pub(crate) const ROUTE_POINT_EVIDENCE: &[Column] = &[
    ("context_id", Text, false),
    ("gtfs_stop_place_id", Text, false),
    ("variant_rank", Int32, false),
    ("corridor_id", Text, true),
    ("route_point_id", Text, false),
    ("routed_excess_metres", Double, true),
    ("snap_edge_id", Int32, true),
    ("snap_way_id", Int64, true),
    ("snap_fraction", Double, true),
    ("snap_projected_longitude", Double, true),
    ("snap_projected_latitude", Double, true),
    ("snap_distance_metres", Double, true),
    ("corridor_face_id", Text, true),
    ("corridor_distance_metres", Double, true),
    ("signed_lateral_offset_metres", Double, true),
    ("corridor_heading_degrees", Double, true),
    ("attachment_heading_degrees", Double, true),
    ("heading_difference_degrees", Double, true),
    ("proximity_distance_metres", Double, true),
    ("routing_availability", Text, false),
    ("invariant_failure_reason", Text, true),
];

/// The arrow schema a relation is written with and must be read with (names, types and
/// nullability in order).
pub(crate) fn arrow_schema(columns: &[Column]) -> Schema {
    Schema::new(
        columns
            .iter()
            .map(|&(name, ty, nullable)| Field::new(name, ty.arrow(), nullable))
            .collect::<Vec<_>>(),
    )
}

/// Whether a decoded schema is the relation's: same names, types and nullability in order.
pub(crate) fn matches(schema: &Schema, columns: &[Column]) -> bool {
    schema.fields().len() == columns.len()
        && schema
            .fields()
            .iter()
            .zip(columns)
            .all(|(field, &(name, ty, nullable))| {
                field.name() == name
                    && *field.data_type() == ty.arrow()
                    && field.is_nullable() == nullable
            })
}

/// The manifest's `schema_fingerprint` of a relation: `sha(join("|", "<name>:<type>:<nullable>"))`.
/// An F# pack embeds the .NET type names and `True`/`False` (section 2.3); a Rust pack its own
/// names and `true`/`false` (verdict Q1), so neither is ever mistaken for the other's.
pub(crate) fn fingerprint(columns: &[Column], router: RouterEvidence) -> std::string::String {
    let parts: Vec<std::string::String> = columns
        .iter()
        .map(|&(name, ty, nullable)| match router {
            RouterEvidence::FSharp => {
                let nullable = if nullable { "True" } else { "False" };
                format!("{name}:{}:{nullable}", ty.clr())
            }
            RouterEvidence::Rust => format!("{name}:{}:{nullable}", ty.own()),
        })
        .collect();
    sha256_hex(parts.join("|").as_bytes())
}
