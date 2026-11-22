//! Reading inputs from disk. The library crates decode bytes; opening files, checking them
//! against their manifests and fanning the decoding out over threads happens here.

pub mod czptt;
pub mod osm;

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {problem}")]
    Invalid { path: PathBuf, problem: String },
    #[error("{path} does not match the snapshot manifest ({what})")]
    Changed { path: PathBuf, what: &'static str },
}
