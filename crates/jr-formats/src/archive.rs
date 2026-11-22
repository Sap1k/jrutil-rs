//! Containers: ZIP archives and gzip streams.

use std::io::{Read, Seek};

#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("ZIP: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("ZIP member {name}: {source}")]
    Member {
        name: String,
        #[source]
        source: std::io::Error,
    },
    #[error("gzip: {0}")]
    Gzip(#[source] std::io::Error),
}

/// The `.xml` members (name not a directory, ending `.xml` case-insensitively) of a ZIP, sorted by
/// name. Members with equal names keep their entry order.
pub fn xml_members<R: Read + Seek>(zip: R) -> Result<Vec<(String, Vec<u8>)>, ArchiveError> {
    let mut archive = zip::ZipArchive::new(zip)?;
    let mut members = Vec::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        let name = file.name()?.into_owned();
        if file.is_dir() || !name.to_ascii_lowercase().ends_with(".xml") {
            continue;
        }
        let mut bytes = Vec::with_capacity(usize::try_from(file.size()).unwrap_or(0));
        if let Err(source) = file.read_to_end(&mut bytes) {
            return Err(ArchiveError::Member { name, source });
        }
        members.push((name, bytes));
    }
    members.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(members)
}

/// Decompresses a whole gzip stream (every member, if there are several).
pub fn gunzip(bytes: &[u8]) -> Result<Vec<u8>, ArchiveError> {
    let mut out = Vec::with_capacity(bytes.len() * 8);
    flate2::read::MultiGzDecoder::new(bytes)
        .read_to_end(&mut out)
        .map_err(ArchiveError::Gzip)?;
    Ok(out)
}
