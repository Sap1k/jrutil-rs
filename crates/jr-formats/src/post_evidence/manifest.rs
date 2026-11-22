//! `manifest.json` (section 2.3; POST-R061, the F# `loadEvidenceManifest`).

use serde_json::{Map, Value, json};

use super::ids::{is_sha256, sha256_hex};
use super::{
    CAPTURE_MAXIMUM_CORRIDOR_VARIANTS, CAPTURE_ROUTED_EXCESS_METRES, EVIDENCE_FORMAT,
    EVIDENCE_SCHEMA_VERSION, MAXIMUM_SEARCH_DISTANCE_METRES, MAXIMUM_SEARCH_STATES, PackError,
    RouterEvidence, VARIANT_ENUMERATION_VERSION,
};

/// The manifest's keys, exactly (an extra or missing key rejects the pack).
const KEYS: [&str; 18] = [
    "evidence_format",
    "schema_version",
    "pack_id",
    "capture_tool_version",
    "merged_jdf_sha256",
    "routing_pbf_sha256",
    "osm_snapshot",
    "router_evidence_version",
    "variant_enumeration_version",
    "capture_ceilings",
    "maximum_search_states",
    "maximum_search_distance_metres",
    "context_count",
    "route_point_count",
    "observation_count",
    "corridor_variant_count",
    "route_point_evidence_count",
    "files",
];
const CEILING_KEYS: [&str; 2] = ["routed_excess_metres", "maximum_corridor_variants"];
const FILE_KEYS: [&str; 5] = ["path", "sha256", "bytes", "rows", "schema_fingerprint"];

/// `capture_ceilings`.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptureCeilings {
    pub routed_excess_metres: f64,
    pub maximum_corridor_variants: i64,
}

/// One entry of `files`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    pub sha256: String,
    pub bytes: i64,
    pub rows: i64,
    pub schema_fingerprint: String,
}

/// The manifest as stored.
#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    pub evidence_format: String,
    pub schema_version: i64,
    pub pack_id: String,
    pub capture_tool_version: String,
    pub merged_jdf_sha256: String,
    pub routing_pbf_sha256: String,
    /// Always null in F# packs.
    pub osm_snapshot: Option<String>,
    pub router_evidence_version: String,
    pub variant_enumeration_version: String,
    pub capture_ceilings: CaptureCeilings,
    pub maximum_search_states: i64,
    pub maximum_search_distance_metres: f64,
    pub context_count: i64,
    pub route_point_count: i64,
    pub observation_count: i64,
    pub corridor_variant_count: i64,
    pub route_point_evidence_count: i64,
    pub files: Vec<FileEntry>,
}

impl Manifest {
    /// The router that captured the pack (the version was checked on parse).
    pub fn router(&self) -> RouterEvidence {
        RouterEvidence::parse(&self.router_evidence_version).unwrap_or(RouterEvidence::FSharp)
    }

    /// The declared row count of a relation.
    pub(crate) fn declared_rows(&self, file: &str) -> i64 {
        match file {
            "observations.parquet" => self.observation_count,
            "route_points.parquet" => self.route_point_count,
            "contexts.parquet" => self.context_count,
            "corridor_variants.parquet" => self.corridor_variant_count,
            _ => self.route_point_evidence_count,
        }
    }

    /// The manifest as written: snake-case keys in this order, two-space indentation, a final
    /// newline. Numbers that are whole are written without a fraction, as System.Text.Json does.
    pub fn to_bytes(&self) -> Vec<u8> {
        let files: Vec<Value> = self
            .files
            .iter()
            .map(|file| {
                json!({
                    "path": file.path,
                    "sha256": file.sha256,
                    "bytes": file.bytes,
                    "rows": file.rows,
                    "schema_fingerprint": file.schema_fingerprint,
                })
            })
            .collect();
        let value = json!({
            "evidence_format": self.evidence_format,
            "schema_version": self.schema_version,
            "pack_id": self.pack_id,
            "capture_tool_version": self.capture_tool_version,
            "merged_jdf_sha256": self.merged_jdf_sha256,
            "routing_pbf_sha256": self.routing_pbf_sha256,
            "osm_snapshot": self.osm_snapshot,
            "router_evidence_version": self.router_evidence_version,
            "variant_enumeration_version": self.variant_enumeration_version,
            "capture_ceilings": {
                "routed_excess_metres": number(self.capture_ceilings.routed_excess_metres),
                "maximum_corridor_variants": self.capture_ceilings.maximum_corridor_variants,
            },
            "maximum_search_states": self.maximum_search_states,
            "maximum_search_distance_metres": number(self.maximum_search_distance_metres),
            "context_count": self.context_count,
            "route_point_count": self.route_point_count,
            "observation_count": self.observation_count,
            "corridor_variant_count": self.corridor_variant_count,
            "route_point_evidence_count": self.route_point_evidence_count,
            "files": files,
        });
        let mut text = serde_json::to_string_pretty(&value).unwrap_or_default();
        text.push('\n');
        text.into_bytes()
    }
}

/// A whole double as an integer JSON number, any other as a double.
fn number(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() < 9.0e15 {
        json!(value as i64)
    } else {
        json!(value)
    }
}

/// `pack_id` (section 2.3). The ceilings are the supported ones; the F# formula writes them
/// with the invariant culture, i.e. `1000`, `3`, `100000`, `30000`.
pub(crate) fn pack_id(
    capture_tool_version: &str,
    merged_jdf_sha256: &str,
    routing_pbf_sha256: &str,
    router: RouterEvidence,
) -> String {
    let parts = [
        capture_tool_version.to_owned(),
        merged_jdf_sha256.to_ascii_lowercase(),
        routing_pbf_sha256.to_ascii_lowercase(),
        EVIDENCE_FORMAT.to_owned(),
        EVIDENCE_SCHEMA_VERSION.to_string(),
        router.as_str().to_owned(),
        VARIANT_ENUMERATION_VERSION.to_owned(),
        invariant(CAPTURE_ROUTED_EXCESS_METRES),
        CAPTURE_MAXIMUM_CORRIDOR_VARIANTS.to_string(),
        MAXIMUM_SEARCH_STATES.to_string(),
        invariant(MAXIMUM_SEARCH_DISTANCE_METRES),
    ];
    sha256_hex(parts.join("|").as_bytes())
}

/// .NET's invariant `Double.ToString()` for the whole numbers the format uses.
pub(crate) fn invariant(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1.0e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

fn keys_exactly(object: &Map<String, Value>, keys: &[&str]) -> bool {
    object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key))
}

fn text(object: &Map<String, Value>, field: &'static str) -> Result<String, PackError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(PackError::FieldType { field })
}

fn integer(object: &Map<String, Value>, field: &'static str) -> Result<i64, PackError> {
    object
        .get(field)
        .and_then(Value::as_i64)
        .ok_or(PackError::FieldType { field })
}

fn double(object: &Map<String, Value>, field: &'static str) -> Result<f64, PackError> {
    object
        .get(field)
        .and_then(Value::as_f64)
        .ok_or(PackError::FieldType { field })
}

/// Parses and validates `manifest.json` before any relation is looked at: the format (evidence
/// v1 is refused by name), the exact key sets, the schema version, the router contract (the F#
/// or the Rust router version, the variant enumeration), a non-blank capture tool version,
/// well-formed SHA-256 identities, the recomputed pack ID, the supported ceilings and
/// non-negative counts.
pub fn parse_manifest(bytes: &[u8]) -> Result<Manifest, PackError> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let root: Value = serde_json::from_slice(bytes)?;
    let format = root
        .get("evidence_format")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if format.as_deref() != Some(EVIDENCE_FORMAT) {
        return Err(PackError::Format { found: format });
    }
    let object = root.as_object().ok_or(PackError::Shape("not an object"))?;
    if !keys_exactly(object, &KEYS) {
        return Err(PackError::Shape("manifest keys"));
    }
    let ceilings = object
        .get("capture_ceilings")
        .and_then(Value::as_object)
        .filter(|ceilings| keys_exactly(ceilings, &CEILING_KEYS))
        .ok_or(PackError::Shape("capture_ceilings keys"))?;
    let entries = object
        .get("files")
        .and_then(Value::as_array)
        .ok_or(PackError::Shape("files"))?;
    let mut files = Vec::with_capacity(entries.len());
    for entry in entries {
        let entry = entry
            .as_object()
            .filter(|entry| keys_exactly(entry, &FILE_KEYS))
            .ok_or(PackError::Shape("file entry keys"))?;
        files.push(FileEntry {
            path: text(entry, "path")?,
            sha256: text(entry, "sha256")?,
            bytes: integer(entry, "bytes")?,
            rows: integer(entry, "rows")?,
            schema_fingerprint: text(entry, "schema_fingerprint")?,
        });
    }
    let osm_snapshot = match object.get("osm_snapshot") {
        Some(Value::Null) | None => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => {
            return Err(PackError::FieldType {
                field: "osm_snapshot",
            });
        }
    };
    let manifest = Manifest {
        evidence_format: text(object, "evidence_format")?,
        schema_version: integer(object, "schema_version")?,
        pack_id: text(object, "pack_id")?,
        capture_tool_version: text(object, "capture_tool_version")?,
        merged_jdf_sha256: text(object, "merged_jdf_sha256")?,
        routing_pbf_sha256: text(object, "routing_pbf_sha256")?,
        osm_snapshot,
        router_evidence_version: text(object, "router_evidence_version")?,
        variant_enumeration_version: text(object, "variant_enumeration_version")?,
        capture_ceilings: CaptureCeilings {
            routed_excess_metres: double(ceilings, "routed_excess_metres")?,
            maximum_corridor_variants: integer(ceilings, "maximum_corridor_variants")?,
        },
        maximum_search_states: integer(object, "maximum_search_states")?,
        maximum_search_distance_metres: double(object, "maximum_search_distance_metres")?,
        context_count: integer(object, "context_count")?,
        route_point_count: integer(object, "route_point_count")?,
        observation_count: integer(object, "observation_count")?,
        corridor_variant_count: integer(object, "corridor_variant_count")?,
        route_point_evidence_count: integer(object, "route_point_evidence_count")?,
        files,
    };
    validate(&manifest)?;
    Ok(manifest)
}

fn validate(manifest: &Manifest) -> Result<(), PackError> {
    if manifest.schema_version != EVIDENCE_SCHEMA_VERSION {
        return Err(PackError::SchemaVersion(manifest.schema_version));
    }
    let router = RouterEvidence::parse(&manifest.router_evidence_version);
    if router.is_none() || manifest.variant_enumeration_version != VARIANT_ENUMERATION_VERSION {
        return Err(PackError::RouterContract {
            router: manifest.router_evidence_version.clone(),
            variants: manifest.variant_enumeration_version.clone(),
        });
    }
    if manifest.capture_tool_version.trim().is_empty() {
        return Err(PackError::CaptureToolVersion);
    }
    for (field, value) in [
        ("pack_id", &manifest.pack_id),
        ("merged_jdf_sha256", &manifest.merged_jdf_sha256),
        ("routing_pbf_sha256", &manifest.routing_pbf_sha256),
    ] {
        if !is_sha256(value) {
            return Err(PackError::Sha256 {
                field,
                value: value.clone(),
            });
        }
    }
    let expected = pack_id(
        &manifest.capture_tool_version,
        &manifest.merged_jdf_sha256,
        &manifest.routing_pbf_sha256,
        manifest.router(),
    );
    if !manifest.pack_id.eq_ignore_ascii_case(&expected) {
        return Err(PackError::PackId {
            stored: manifest.pack_id.clone(),
            expected,
        });
    }
    if manifest.capture_ceilings.routed_excess_metres != CAPTURE_ROUTED_EXCESS_METRES
        || manifest.capture_ceilings.maximum_corridor_variants != CAPTURE_MAXIMUM_CORRIDOR_VARIANTS
        || manifest.maximum_search_states != MAXIMUM_SEARCH_STATES
        || manifest.maximum_search_distance_metres != MAXIMUM_SEARCH_DISTANCE_METRES
    {
        return Err(PackError::Ceilings);
    }
    if [
        manifest.context_count,
        manifest.route_point_count,
        manifest.observation_count,
        manifest.corridor_variant_count,
        manifest.route_point_evidence_count,
    ]
    .iter()
    .any(|count| *count < 0)
    {
        return Err(PackError::NegativeCount);
    }
    Ok(())
}
