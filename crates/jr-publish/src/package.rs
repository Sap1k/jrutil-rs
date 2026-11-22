//! A production package on disk: `manifest.json`, `diagnostics.json`, `serving/*.parquet`,
//! `gtfs.zip`.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use crate::error::PackageError;
use crate::manifest::Manifest;
use crate::parquet_read::ParquetFile;
use crate::table::Table;

pub const MANIFEST: &str = "manifest.json";
pub const DIAGNOSTICS: &str = "diagnostics.json";
pub const GTFS: &str = "gtfs.zip";

#[derive(Debug, Clone)]
pub struct Package {
    root: PathBuf,
    manifest: Manifest,
}

/// One CSV member of `gtfs.zip`, all cells as text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CsvTable {
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl Package {
    /// Reads `manifest.json` only.
    pub fn open(root: &Path) -> Result<Package, PackageError> {
        let path = root.join(MANIFEST);
        let bytes = std::fs::read(&path).map_err(PackageError::io(&path))?;
        let raw = serde_json::from_slice(&bytes).map_err(|source| PackageError::Json {
            path: path.clone(),
            source,
        })?;
        Ok(Package {
            root: root.to_path_buf(),
            manifest: Manifest::from_value(raw),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// `serving/<name>.parquet`, every column, unchecked.
    pub fn relation(&self, name: &str) -> Result<Table, PackageError> {
        self.parquet(name)?.read_all(name)
    }

    pub(crate) fn relation_path(&self, name: &str) -> PathBuf {
        self.root.join("serving").join(format!("{name}.parquet"))
    }

    pub(crate) fn parquet(&self, name: &str) -> Result<ParquetFile, PackageError> {
        ParquetFile::open(&self.relation_path(name))
    }

    pub fn diagnostics(&self) -> Result<serde_json::Value, PackageError> {
        let path = self.root.join(DIAGNOSTICS);
        let bytes = std::fs::read(&path).map_err(PackageError::io(&path))?;
        serde_json::from_slice(&bytes).map_err(|source| PackageError::Json { path, source })
    }

    /// Member names of `gtfs.zip`, in archive order.
    pub fn gtfs_files(&self) -> Result<Vec<String>, PackageError> {
        let archive = self.gtfs_archive()?;
        archive
            .file_names()
            .map(|name| name.map(|name| name.into_owned()))
            .collect::<Result<_, _>>()
            .map_err(PackageError::zip(self.root.join(GTFS)))
    }

    /// One member of `gtfs.zip`, read whole. Use for small members; large ones are streamed
    /// inside the crate.
    pub fn gtfs(&self, file: &str) -> Result<CsvTable, PackageError> {
        let table = self.gtfs_csv(file, |mut reader| {
            let header = reader.headers()?.iter().map(str::to_owned).collect();
            let rows = reader
                .records()
                .map(|record| record.map(|record| record.iter().map(str::to_owned).collect()))
                .collect::<Result<_, _>>()?;
            Ok(CsvTable { header, rows })
        })?;
        table.ok_or_else(|| PackageError::MissingMember {
            path: self.root.join(GTFS),
            member: file.to_owned(),
        })
    }

    fn gtfs_archive(&self) -> Result<zip::ZipArchive<BufReader<File>>, PackageError> {
        let path = self.root.join(GTFS);
        let file = File::open(&path).map_err(PackageError::io(&path))?;
        zip::ZipArchive::new(BufReader::new(file)).map_err(PackageError::zip(&path))
    }

    /// Runs `read` over a CSV reader of one member; `None` when the member is absent.
    pub(crate) fn gtfs_csv<T>(
        &self,
        member: &str,
        read: impl FnOnce(csv::Reader<&mut dyn Read>) -> Result<T, csv::Error>,
    ) -> Result<Option<T>, PackageError> {
        let path = self.root.join(GTFS);
        let mut archive = self.gtfs_archive()?;
        let mut entry = match archive.by_name(member) {
            Ok(entry) => entry,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(source) => return Err(PackageError::Zip { path, source }),
        };
        let mut buffered = BufReader::with_capacity(1 << 20, &mut entry);
        let reader = csv::ReaderBuilder::new()
            .flexible(true)
            .from_reader(&mut buffered as &mut dyn Read);
        read(reader).map(Some).map_err(|source| PackageError::Csv {
            path,
            member: member.to_owned(),
            source,
        })
    }
}
