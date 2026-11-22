//! Reading the road compiler's inputs from disk.
//!
//! Batches are one ZIP each in a directory and are named after their file (without `.zip`):
//! the name is the export's own identifier and the compiler's stated order. A batch that cannot
//! be decoded is still returned, with the reason: dropping it is the compiler's decision
//! (JDF-R004). The stop catalogues are the CSV files below a directory, in ordinal order of
//! their relative paths (JDF-R005). Every file read is listed with its SHA-256: together they
//! are the identity of the build's inputs.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use jr_formats::geodata::{catalogue, gazetteer, registry, route_rules};
use jr_formats::jdf::{self, JdfError};
use jr_formats::osm::Node;
use rayon::prelude::*;
use sha2::{Digest, Sha256};

use super::LoadError;
use crate::spec::JdfSpec;

const OKRESY: &str = "cz_regions.json.gz";
const TOWNS: &str = "cz_towns.json.gz";
const FOREIGN_TOWNS: &str = "eur_towns_countries.csv";
const REGISTRY_STOPS: &str = "stops.csv";
const TRANSPORT_MODES: &str = "transport-modes.csv";
const PRESENTATION: &str = "presentation.csv";

#[derive(Debug)]
pub struct LoadedBatch {
    pub name: String,
    pub read: Result<jdf::Read, JdfError>,
}

#[derive(Debug)]
pub struct Sources {
    /// In name order.
    pub batches: Vec<LoadedBatch>,
    /// `(path below the catalogue directory with / separators, rows)`, in path order.
    pub catalogues: Vec<(String, Vec<catalogue::Row>)>,
    pub osm_nodes: Vec<Node>,
    pub okresy: Vec<gazetteer::Okres>,
    pub towns: Vec<gazetteer::Town>,
    pub foreign_towns: Vec<gazetteer::ForeignTown>,
    pub registry: Option<Vec<registry::Stop>>,
    pub transport_modes: Vec<route_rules::TransportModeRule>,
    pub presentation: Vec<route_rules::PresentationRule>,
    /// `(group/relative path, SHA-256)` of every file read, sorted.
    pub inputs: Vec<(String, String)>,
}

pub fn load(spec: &JdfSpec) -> Result<Sources, LoadError> {
    let mut inputs = Vec::new();

    let batch_files = files_below(&spec.batches, "zip", false)?;
    let batches: Vec<(LoadedBatch, String)> = batch_files
        .par_iter()
        .map(|(name, path)| {
            let bytes = read(path)?;
            let digest = sha256(&bytes);
            let name = name.strip_suffix(".zip").unwrap_or(name).to_owned();
            let read = jdf::read_zip(Cursor::new(bytes));
            Ok((LoadedBatch { name, read }, digest))
        })
        .collect::<Result<_, LoadError>>()?;
    let mut loaded = Vec::with_capacity(batches.len());
    for (batch, digest) in batches {
        inputs.push((format!("batches/{}.zip", batch.name), digest));
        loaded.push(batch);
    }
    loaded.sort_by(|a, b| a.name.cmp(&b.name));

    let catalogue_files = if spec.catalogues.is_file() {
        let name = spec
            .catalogues
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        vec![(name, spec.catalogues.clone())]
    } else {
        files_below(&spec.catalogues, "csv", true)?
    };
    if catalogue_files.is_empty() {
        return Err(invalid(&spec.catalogues, "no stop catalogue (*.csv) in it"));
    }
    let catalogues: Vec<(String, Vec<catalogue::Row>, String)> = catalogue_files
        .par_iter()
        .map(|(name, path)| {
            let bytes = read(path)?;
            let rows = catalogue::parse(&bytes).map_err(|error| invalid(path, error))?;
            Ok((name.clone(), rows, sha256(&bytes)))
        })
        .collect::<Result<_, LoadError>>()?;
    let catalogues = catalogues
        .into_iter()
        .map(|(name, rows, digest)| {
            inputs.push((format!("catalogues/{name}"), digest));
            (name, rows)
        })
        .collect();

    let mut auxiliary = |group: &str, path: PathBuf| -> Result<Vec<u8>, LoadError> {
        let bytes = read(&path)?;
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        inputs.push((format!("{group}/{name}"), sha256(&bytes)));
        Ok(bytes)
    };
    let path = spec.gazetteer.join(OKRESY);
    let okresy = gazetteer::parse_okresy(&*auxiliary("gazetteer", path.clone())?)
        .map_err(|error| invalid(&path, error))?;
    let path = spec.gazetteer.join(TOWNS);
    let towns = gazetteer::parse_towns(&*auxiliary("gazetteer", path.clone())?)
        .map_err(|error| invalid(&path, error))?;
    let path = spec.gazetteer.join(FOREIGN_TOWNS);
    let foreign_towns = gazetteer::parse_foreign_towns(&auxiliary("gazetteer", path.clone())?)
        .map_err(|error| invalid(&path, error))?;

    let registry = match &spec.registry {
        None => None,
        Some(directory) => {
            let path = directory.join(REGISTRY_STOPS);
            let bytes = auxiliary("registry", path.clone())?;
            Some(registry::parse_stops(&bytes).map_err(|error| invalid(&path, error))?)
        }
    };
    let (transport_modes, presentation) = match &spec.route_rules {
        None => (Vec::new(), Vec::new()),
        Some(directory) => {
            let path = directory.join(TRANSPORT_MODES);
            let modes = route_rules::parse_transport_modes(&auxiliary("routes", path.clone())?)
                .map_err(|error| invalid(&path, error))?;
            let path = directory.join(PRESENTATION);
            let presentation = route_rules::parse_presentation(&auxiliary("routes", path.clone())?)
                .map_err(|error| invalid(&path, error))?;
            (modes, presentation)
        }
    };

    let (osm_nodes, osm_digest) = osm_nodes(&spec.osm)?;
    inputs.push(("osm".to_owned(), osm_digest));
    inputs.sort();

    Ok(Sources {
        batches: loaded,
        catalogues,
        osm_nodes,
        okresy,
        towns,
        foreign_towns,
        registry,
        transport_modes,
        presentation,
        inputs,
    })
}

/// The tagged nodes of the extract, in file order, and the file's SHA-256.
fn osm_nodes(path: &Path) -> Result<(Vec<Node>, String), LoadError> {
    let bytes = read(path)?;
    let mut nodes = Vec::new();
    jr_formats::osm::tagged_nodes(Cursor::new(&bytes), |node| nodes.push(node))
        .map_err(|error| invalid(path, error))?;
    Ok((nodes, sha256(&bytes)))
}

/// The files with the extension below a directory as `(relative path with / separators, path)`,
/// in ordinal order of the relative path.
fn files_below(
    directory: &Path,
    extension: &str,
    recursive: bool,
) -> Result<Vec<(String, PathBuf)>, LoadError> {
    let mut found = Vec::new();
    let mut pending = vec![(String::new(), directory.to_owned())];
    while let Some((prefix, current)) = pending.pop() {
        let io = |source| LoadError::Read {
            path: current.clone(),
            source,
        };
        for entry in fs::read_dir(&current).map_err(io)? {
            let entry = entry.map_err(io)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = format!("{prefix}{name}");
            let path = entry.path();
            if path.is_dir() {
                if recursive {
                    pending.push((format!("{relative}/"), path));
                }
            } else if path
                .extension()
                .is_some_and(|found| found.eq_ignore_ascii_case(extension))
            {
                found.push((relative, path));
            }
        }
    }
    found.sort();
    Ok(found)
}

fn read(path: &Path) -> Result<Vec<u8>, LoadError> {
    fs::read(path).map_err(|source| LoadError::Read {
        path: path.to_owned(),
        source,
    })
}

fn invalid(path: &Path, problem: impl ToString) -> LoadError {
    LoadError::Invalid {
        path: path.to_owned(),
        problem: problem.to_string(),
    }
}

fn sha256(bytes: &[u8]) -> String {
    hex(Sha256::digest(bytes))
}

fn hex(digest: impl AsRef<[u8]>) -> String {
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
