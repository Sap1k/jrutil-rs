use std::path::PathBuf;

/// Why a package could not be written. A timetable the contract cannot express (a duplicate
/// key, an undeclared namespace) is an error, never a silent drop.
#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    #[error("{0} already exists")]
    OutputExists(PathBuf),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Parquet {
        path: PathBuf,
        #[source]
        source: parquet::errors::ParquetError,
    },
    #[error("{path}: {source}")]
    Arrow {
        path: PathBuf,
        #[source]
        source: arrow_schema::ArrowError,
    },
    #[error("{path}: {source}")]
    Zip {
        path: PathBuf,
        #[source]
        source: zip::result::ZipError,
    },
    #[error("{path}: {source}")]
    Csv {
        path: PathBuf,
        #[source]
        source: csv::Error,
    },
    #[error("{path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("{relation}: duplicate primary key ({key})")]
    DuplicateKey { relation: String, key: String },
    #[error("{relation}.{field}: null in a required column at row {row}")]
    NullInRequired {
        relation: String,
        field: String,
        row: usize,
    },
    #[error("{relation}.{field}: {value} does not fit the column type")]
    OutOfRange {
        relation: &'static str,
        field: &'static str,
        value: String,
    },
    #[error("source key namespace {0} is not declared by the serving contract")]
    UndeclaredNamespace(String),
    #[error("{namespace}={identifier}: the key names a {entity}, the namespace a {expected}")]
    EntityKind {
        namespace: String,
        identifier: String,
        entity: &'static str,
        expected: String,
    },
    #[error("{trip} call {sequence}: route stop {route_stop} belongs to an unpublished route")]
    UnpublishedRouteStop {
        trip: String,
        sequence: i32,
        route_stop: String,
    },
    #[error("diagnostics section {0} is written by the publisher itself")]
    ReservedSection(String),
    #[error("writer and contract disagree: {0}")]
    Contract(String),
}

impl PublishError {
    pub(crate) fn io(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> PublishError {
        let path = path.into();
        move |source| PublishError::Io { path, source }
    }
}
