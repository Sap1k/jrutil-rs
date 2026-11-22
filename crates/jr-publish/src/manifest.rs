//! `manifest.json`: the raw document plus typed views of what it holds.
//!
//! Parsing is lenient: a field that is missing or has the wrong shape is `None`, and
//! `validate` reports it. Only unparsable JSON is an error.

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub raw: Value,
    pub bundle_format: Option<String>,
    pub bundle_version: Option<i64>,
    pub serving_schema_version: Option<String>,
    pub diagnostics_schema_version: Option<i64>,
    pub identity_contract: Option<String>,
    /// `sha256:<hex>` digest of the build inputs.
    pub feed_version: Option<String>,
    pub build_spec_sha256: Option<String>,
    /// Absent in packages built without `--converter-version`.
    pub compiler: Option<Compiler>,
    pub contract_valid: Option<bool>,
    pub publication_eligible: Option<bool>,
    pub calibration: Option<bool>,
    /// Absent in CZPTT packages.
    pub service_horizon: Option<ServiceHorizon>,
    pub sources: Option<Vec<Source>>,
    pub namespaces: Option<Vec<ManifestNamespace>>,
    pub relations: Option<Vec<ManifestRelation>>,
    /// Every payload except `manifest.json`.
    pub files: Option<Vec<FileEntry>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Compiler {
    pub tool: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ServiceHorizon {
    /// The timetable year, where the compiler records one.
    pub year: Option<i32>,
    /// `yyyymmdd`.
    pub start_date: String,
    pub end_date: String,
}

/// One input snapshot. Sources differ in what else they record, so `raw` keeps all of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub source_id: Option<String>,
    pub payload_sha256: Option<String>,
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ManifestNamespace {
    pub name: String,
    pub entity_kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ManifestRelation {
    pub name: String,
    pub path: String,
    pub schema: Vec<ManifestField>,
    pub primary_key: Vec<String>,
    pub foreign_keys: Vec<crate::contract::ForeignKey>,
    pub row_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ManifestField {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub nullable: bool,
    #[serde(rename = "enum")]
    pub enumeration: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

impl Manifest {
    pub fn from_value(raw: Value) -> Manifest {
        let field = |name: &str| raw.get(name);
        fn typed<T: DeserializeOwned>(value: Option<&Value>) -> Option<T> {
            value.and_then(|value| T::deserialize(value).ok())
        }
        let sources = field("sources").and_then(Value::as_array).map(|sources| {
            sources
                .iter()
                .map(|source| Source {
                    source_id: typed(source.get("source_id")),
                    payload_sha256: typed(source.get("payload_sha256")),
                    raw: source.clone(),
                })
                .collect()
        });
        Manifest {
            bundle_format: typed(field("bundle_format")),
            bundle_version: typed(field("bundle_version")),
            serving_schema_version: typed(field("serving_schema_version")),
            diagnostics_schema_version: typed(field("diagnostics_schema_version")),
            identity_contract: typed(field("identity_contract")),
            feed_version: typed(field("feed_version")),
            build_spec_sha256: typed(field("build_spec_sha256")),
            compiler: typed(field("compiler")),
            contract_valid: typed(field("contract_valid")),
            publication_eligible: typed(field("publication_eligible")),
            calibration: typed(field("calibration")),
            service_horizon: typed(field("service_horizon")),
            sources,
            namespaces: typed(field("namespaces")),
            relations: typed(field("relations")),
            files: typed(field("files")),
            raw,
        }
    }

    /// The declaration of one relation.
    pub fn relation(&self, name: &str) -> Option<&ManifestRelation> {
        self.relations
            .as_ref()?
            .iter()
            .find(|relation| relation.name == name)
    }
}
