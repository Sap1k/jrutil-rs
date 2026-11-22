use std::path::PathBuf;

/// A package file that cannot be read at all. Contract violations are `validate::Finding`s.
#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
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
    #[error("{path}: column {column} has unsupported type {data_type}")]
    UnsupportedColumn {
        path: PathBuf,
        column: String,
        data_type: String,
    },
    #[error("{path}: column {column} not found")]
    MissingColumn { path: PathBuf, column: String },
    #[error("{path}: {source}")]
    Zip {
        path: PathBuf,
        #[source]
        source: zip::result::ZipError,
    },
    #[error("{path}/{member}: {source}")]
    Csv {
        path: PathBuf,
        member: String,
        #[source]
        source: csv::Error,
    },
    #[error("{path}: no member {member}")]
    MissingMember { path: PathBuf, member: String },
}

impl PackageError {
    pub(crate) fn io(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> PackageError {
        let path = path.into();
        move |source| PackageError::Io { path, source }
    }

    pub(crate) fn parquet(
        path: impl Into<PathBuf>,
    ) -> impl FnOnce(parquet::errors::ParquetError) -> PackageError {
        let path = path.into();
        move |source| PackageError::Parquet { path, source }
    }

    pub(crate) fn zip(
        path: impl Into<PathBuf>,
    ) -> impl FnOnce(zip::result::ZipError) -> PackageError {
        let path = path.into();
        move |source| PackageError::Zip { path, source }
    }
}
