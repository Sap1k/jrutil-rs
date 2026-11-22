//! The CZPTT source snapshot's manifest (`sources.json`), as text.
//!
//! The manifest lists the source objects in the order the snapshot recorded them and the
//! auxiliary files (KADR, SR70) with their sizes and digests. Fields the compiler does not need
//! (`schema_version`, `source_base_url`, `url`) are not read.

use serde::Deserialize;

/// `sources.json`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Manifest {
    pub timetable_year: i16,
    /// In manifest order.
    pub objects: Vec<Object>,
    /// In manifest order.
    pub auxiliary: Vec<Auxiliary>,
}

/// One CZPTT source object.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Object {
    pub relative_path: String,
    pub kind: ObjectKind,
    pub bytes: u64,
    pub sha256: String,
}

/// The container an object comes in, as the manifest declares it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    /// `annual_zip`: a ZIP of XML messages (`annual/JRyyyy.zip`).
    AnnualZip,
    /// `monthly_gzip`: one gzip-compressed XML message (`changes/yyyy-mm/*.xml.zip`, gzip despite
    /// the name).
    MonthlyGzip,
}

/// A file the snapshot carries besides the messages.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Auxiliary {
    pub relative_path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("sources.json: {0}")]
    Json(#[from] serde_json::Error),
}

impl Manifest {
    pub fn parse(json: &str) -> Result<Manifest, SnapshotError> {
        Ok(serde_json::from_str(json)?)
    }
}
