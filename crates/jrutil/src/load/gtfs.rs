//! Reading a regional GTFS feed and the reviewed data that goes with it.

use std::fs;
use std::io::Cursor;
use std::path::Path;

use jr_formats::geodata::registry::{self, OverlayPlace};
use jr_formats::geodata::stop_overrides;
use jr_formats::gtfs::{self, Feed, Options};
use jr_overlay::profile::ReaderRequest;
use sha2::{Digest, Sha256};

use super::LoadError;

const OVERLAY_PLACES: &str = "overlay_places.csv";

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

/// The feed with what its profile asks the reader to keep, and the ZIP's SHA-256.
pub fn feed(path: &Path, request: &ReaderRequest) -> Result<(Feed, String), LoadError> {
    let bytes = read(path)?;
    let digest: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let columns: Vec<(&str, Vec<&str>)> = request
        .extra_columns
        .iter()
        .map(|(file, columns)| (&**file, columns.iter().map(|column| &**column).collect()))
        .collect();
    let extra_columns: Vec<(&str, &[&str])> = columns
        .iter()
        .map(|(file, columns)| (*file, columns.as_slice()))
        .collect();
    let tables: Vec<&str> = request.tables.iter().map(|name| &**name).collect();
    let raw: Vec<&str> = request.raw.iter().map(|name| &**name).collect();
    let options = Options {
        extra_columns: &extra_columns,
        tables: &tables,
        raw: &raw,
        shapes: false,
    };
    let feed =
        gtfs::read_zip(Cursor::new(bytes), &options).map_err(|error| invalid(path, error))?;
    Ok((feed, digest))
}

pub fn stop_overrides(path: &Path) -> Result<Vec<stop_overrides::Row>, LoadError> {
    stop_overrides::parse(&read(path)?).map_err(|error| invalid(path, error))
}

/// The registry's pinned stop places of sources; none when the registry has no such file.
pub fn overlay_places(registry: &Path) -> Result<Vec<OverlayPlace>, LoadError> {
    let path = registry.join(OVERLAY_PLACES);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    registry::parse_overlay_places(&read(&path)?).map_err(|error| invalid(&path, error))
}
