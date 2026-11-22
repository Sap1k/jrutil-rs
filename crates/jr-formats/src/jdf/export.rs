//! A national export: one ZIP whose `*.zip` members are batches (CIS JŘ publishes the bus
//! lines and the urban and rail-replacement lines as two such archives).

use std::io::{Cursor, Read as _};
use std::sync::Arc;

use super::JdfError;

/// An export held in memory. Cloning shares the bytes and the directory, so any number of
/// threads can read members at once.
#[derive(Clone, Debug)]
pub struct Export {
    archive: zip::ZipArchive<Cursor<Arc<[u8]>>>,
}

/// A batch of an export.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Member {
    /// The member's path as the archive writes it, with `/` separators.
    pub path: String,
    /// Its index in the archive, for [`Export::read`].
    pub index: usize,
}

impl Member {
    /// The file name without its directories and without `.zip`.
    pub fn stem(&self) -> &str {
        let name = self.path.rsplit('/').next().unwrap_or(&self.path);
        &name[..name.len() - ".zip".len()]
    }
}

impl Export {
    pub fn open(bytes: Arc<[u8]>) -> Result<Export, JdfError> {
        Ok(Export {
            archive: zip::ZipArchive::new(Cursor::new(bytes))?,
        })
    }

    /// The members whose name ends in `.zip` (ignoring ASCII case), in path order.
    pub fn batches(&self) -> Result<Vec<Member>, JdfError> {
        let mut members = Vec::new();
        for index in 0..self.archive.len() {
            let Some(name) = self.archive.name_for_index(index).transpose()? else {
                continue;
            };
            let path = name.replace('\\', "/");
            let is_batch = !path.ends_with('/')
                && path
                    .len()
                    .checked_sub(".zip".len())
                    .and_then(|start| path.get(start..))
                    .is_some_and(|extension| extension.eq_ignore_ascii_case(".zip"));
            if is_batch {
                members.push(Member { path, index });
            }
        }
        members.sort();
        Ok(members)
    }

    /// The bytes of a member: a batch ZIP for [`super::read_zip`].
    pub fn read(&self, member: &Member) -> Result<Vec<u8>, JdfError> {
        let mut archive = self.archive.clone();
        let mut file = archive.by_index(member.index)?;
        let mut bytes = Vec::with_capacity(usize::try_from(file.size()).unwrap_or(0));
        file.read_to_end(&mut bytes)
            .map_err(|source| JdfError::Member {
                name: member.path.clone(),
                source,
            })?;
        Ok(bytes)
    }
}
