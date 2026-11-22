//! Reading a CZPTT source snapshot from disk.
//!
//! The snapshot is what the fetch step leaves: `sources.json` listing every object with its
//! size and SHA-256, the annual archive (`annual/JR<year>.zip`), the change objects
//! (`changes/<yyyy-mm>/*.xml.zip`, gzip despite the name), `kadr/catalog.json` and
//! `sr70/SR70.csv`. Every object is checked against the manifest before it is read: the
//! compiler builds from pinned inputs or not at all. Messages are decoded in parallel and
//! returned in manifest order; putting them in application order is the compiler's job.

use std::fs;
use std::io::Cursor;
use std::path::Path;

use jr_czptt::SourceMessage;
use jr_formats::snapshot::{Manifest, Object, ObjectKind};
use jr_formats::{archive, czptt, kadr, sr70};
use rayon::prelude::*;
use sha2::{Digest, Sha256};

use super::LoadError;

#[derive(Debug)]
pub struct Sources {
    pub timetable_year: i16,
    pub messages: Vec<SourceMessage>,
    pub catalog: kadr::Catalog,
    pub sr70: Vec<sr70::Row>,
    /// `(relative path, SHA-256)` of everything read, in path order: the snapshot's identity.
    pub inputs: Vec<(String, String)>,
}

const MANIFEST: &str = "sources.json";
const CATALOG: &str = "kadr/catalog.json";
const SR70: &str = "sr70/SR70.csv";

pub fn load(snapshot: &Path) -> Result<Sources, LoadError> {
    let manifest_text = read_text(&snapshot.join(MANIFEST))?;
    let manifest =
        Manifest::parse(&manifest_text).map_err(|error| invalid(snapshot, MANIFEST, error))?;

    let auxiliary = |name: &'static str| -> Result<Vec<u8>, LoadError> {
        let path = snapshot.join(name);
        let entry = manifest
            .auxiliary
            .iter()
            .find(|entry| entry.relative_path == name)
            .ok_or_else(|| invalid(snapshot, name, "not listed in the snapshot manifest"))?;
        let bytes = read(&path)?;
        verify(&path, &bytes, entry.bytes, &entry.sha256)?;
        Ok(bytes)
    };
    let catalog_bytes = auxiliary(CATALOG)?;
    let catalog_text =
        String::from_utf8(catalog_bytes).map_err(|error| invalid(snapshot, CATALOG, error))?;
    let catalog =
        kadr::Catalog::parse(&catalog_text).map_err(|error| invalid(snapshot, CATALOG, error))?;
    let sr70 = sr70::parse(&auxiliary(SR70)?).map_err(|error| invalid(snapshot, SR70, error))?;

    let per_object: Vec<Vec<SourceMessage>> = manifest
        .objects
        .par_iter()
        .map(|object| read_object(snapshot, object))
        .collect::<Result<_, _>>()?;
    let messages = per_object.into_iter().flatten().collect();

    let mut inputs: Vec<(String, String)> = manifest
        .objects
        .iter()
        .map(|object| (object.relative_path.clone(), object.sha256.clone()))
        .chain(
            manifest
                .auxiliary
                .iter()
                .map(|entry| (entry.relative_path.clone(), entry.sha256.clone())),
        )
        .collect();
    inputs.sort();

    Ok(Sources {
        timetable_year: manifest.timetable_year,
        messages,
        catalog,
        sr70,
        inputs,
    })
}

/// The messages of one object, in the order the object holds them.
fn read_object(snapshot: &Path, object: &Object) -> Result<Vec<SourceMessage>, LoadError> {
    let path = snapshot.join(&object.relative_path);
    let bytes = read(&path)?;
    verify(&path, &bytes, object.bytes, &object.sha256)?;
    match object.kind {
        ObjectKind::AnnualZip => {
            let members =
                archive::xml_members(Cursor::new(bytes)).map_err(|error| LoadError::Invalid {
                    path: path.clone(),
                    problem: error.to_string(),
                })?;
            Ok(members
                .into_par_iter()
                .map(|(name, xml)| message(format!("{}//{name}", object.relative_path), true, &xml))
                .collect())
        }
        ObjectKind::MonthlyGzip => {
            let xml = archive::gunzip(&bytes).map_err(|error| LoadError::Invalid {
                path,
                problem: error.to_string(),
            })?;
            Ok(vec![message(object.relative_path.clone(), false, &xml)])
        }
    }
}

/// A message that does not parse is still a message: the compiler reports it and moves on.
fn message(source_path: String, annual: bool, xml: &[u8]) -> SourceMessage {
    SourceMessage {
        source_path: source_path.into_boxed_str(),
        annual,
        digest: Sha256::digest(xml).into(),
        message: czptt::parse(xml).map_err(|error| error.to_string().into_boxed_str()),
    }
}

fn verify(path: &Path, bytes: &[u8], size: u64, sha256: &str) -> Result<(), LoadError> {
    if bytes.len() as u64 != size {
        return Err(LoadError::Changed {
            path: path.to_owned(),
            what: "size",
        });
    }
    let digest = Sha256::digest(bytes);
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    if !hex.eq_ignore_ascii_case(sha256) {
        return Err(LoadError::Changed {
            path: path.to_owned(),
            what: "SHA-256",
        });
    }
    Ok(())
}

fn read(path: &Path) -> Result<Vec<u8>, LoadError> {
    fs::read(path).map_err(|source| LoadError::Read {
        path: path.to_owned(),
        source,
    })
}

fn read_text(path: &Path) -> Result<String, LoadError> {
    fs::read_to_string(path).map_err(|source| LoadError::Read {
        path: path.to_owned(),
        source,
    })
}

fn invalid(snapshot: &Path, name: &str, problem: impl ToString) -> LoadError {
    LoadError::Invalid {
        path: snapshot.join(name),
        problem: problem.to_string(),
    }
}
