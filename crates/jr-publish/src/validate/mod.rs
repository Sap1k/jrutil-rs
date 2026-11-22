//! Contract validation of a production package: the checks of the F# `validate-package`
//! (`Serving/Validation.fs`), plus the consumer's declared-schema check.
//!
//! Order matters as in F#: inventory, manifest and the structure of every relation first;
//! the reference and semantic checks (foreign keys, values, trip parts, the GTFS projection)
//! run only on a package without findings so far, because they need structurally valid
//! relation files.

mod gtfs;
mod inventory;
pub(crate) mod keys;
mod parts;
mod references;
mod structure;

use std::collections::BTreeSet;

use crate::contract::{Contract, contract, parse_version};
use crate::error::PackageError;
use crate::package::{DIAGNOSTICS, Package};

/// One contract violation. Findings sort by code, subject, detail.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub code: &'static str,
    /// The file (or `file/member`) the finding is about.
    pub subject: String,
    pub detail: String,
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}: {}", self.code, self.subject, self.detail)
    }
}

/// Stable finding codes.
pub mod code {
    pub const MISSING_FILE: &str = "missing_file";
    pub const UNDECLARED_FILE: &str = "undeclared_file";
    pub const MANIFEST_FIELD_INVALID: &str = "manifest_field_invalid";
    pub const MANIFEST_FILES_MISSING: &str = "manifest_files_missing";
    pub const MANIFEST_RELATIONS_MISSING: &str = "manifest_relations_missing";
    pub const MANIFEST_SOURCES_MISSING: &str = "manifest_sources_missing";
    pub const SOURCE_DIGEST_MISSING: &str = "source_digest_missing";
    pub const INVENTORY_MISSING: &str = "inventory_missing";
    pub const INVENTORY_UNDECLARED: &str = "inventory_undeclared";
    pub const INVENTORY_DUPLICATE: &str = "inventory_duplicate";
    pub const FILE_SIZE_MISMATCH: &str = "file_size_mismatch";
    pub const FILE_HASH_MISMATCH: &str = "file_hash_mismatch";
    pub const RELATION_UNDECLARED: &str = "relation_undeclared";
    pub const RELATION_UNKNOWN: &str = "relation_unknown";
    pub const MANIFEST_SCHEMA_MISMATCH: &str = "manifest_schema_mismatch";
    pub const PARQUET_UNREADABLE: &str = "parquet_unreadable";
    pub const SCHEMA_MISMATCH: &str = "schema_mismatch";
    pub const PARQUET_METADATA_MISMATCH: &str = "parquet_metadata_mismatch";
    pub const ROW_GROUP_TOO_LARGE: &str = "row_group_too_large";
    pub const ROW_COUNT_MISMATCH: &str = "row_count_mismatch";
    pub const NULL_PRIMARY_KEY: &str = "null_primary_key";
    pub const DUPLICATE_PRIMARY_KEY: &str = "duplicate_primary_key";
    pub const FOREIGN_KEY_DANGLING: &str = "foreign_key_dangling";
    pub const ENUM_VALUE_INVALID: &str = "enum_value_invalid";
    pub const FEED_PREFIX_MISSING: &str = "feed_prefix_missing";
    pub const NAMESPACE_ENCODING_INVALID: &str = "namespace_encoding_invalid";
    pub const TRIP_PART_NUMBER_MISSING: &str = "trip_part_number_missing";
    pub const TRIP_PART_BUS_CALLS: &str = "trip_part_bus_calls";
    pub const TRIP_PART_BOUNDARY: &str = "trip_part_boundary";
    pub const GTFS_UNREADABLE: &str = "gtfs_unreadable";
    pub const GTFS_ENTRY_NOT_FLAT: &str = "gtfs_entry_not_flat";
    pub const GTFS_ENTRY_ORDER: &str = "gtfs_entry_order";
    pub const GTFS_NONSTANDARD_COLUMN: &str = "gtfs_nonstandard_column";
    pub const GTFS_PROJECTION_MISMATCH: &str = "gtfs_projection_mismatch";
    pub const DIAGNOSTICS_TOO_LARGE: &str = "diagnostics_too_large";
    pub const PACKAGE_TOO_LARGE: &str = "package_too_large";
}

/// The bounded production summary's limit.
const DIAGNOSTICS_MAXIMUM: u64 = 1024 * 1024;
/// The acceptance limit for a whole package.
const PACKAGE_MAXIMUM: u64 = 2 * 1024 * 1024 * 1024;

/// Every contract violation of `package`, sorted; empty when it is valid. `Err` only when the
/// package cannot be read at all (its directory cannot be listed).
pub fn validate(package: &Package) -> Result<Vec<Finding>, PackageError> {
    let contract = contract();
    let mut findings = Findings::default();
    let files = inventory::list_files(package.root())?;
    let compat = Compatibility::of(package, contract);

    inventory::check(&mut findings, package, contract, &compat, &files);
    let tables = structure::check(&mut findings, package, contract, &compat, &files);
    if findings.is_empty() {
        references::check(&mut findings, contract, &tables);
        parts::check(&mut findings, &tables);
        gtfs::check_projection(&mut findings, package, &tables);
    }
    drop(tables);
    if files.contains_key(crate::package::GTFS) {
        gtfs::check_archive(&mut findings, package);
    }
    if files
        .get(DIAGNOSTICS)
        .is_some_and(|&size| size > DIAGNOSTICS_MAXIMUM)
    {
        findings.add(
            code::DIAGNOSTICS_TOO_LARGE,
            DIAGNOSTICS,
            "bounded production summary exceeds 1 MiB",
        );
    }
    let total: u64 = files.values().sum();
    if total > PACKAGE_MAXIMUM {
        findings.add(
            code::PACKAGE_TOO_LARGE,
            ".",
            format!("{total} bytes exceed the 2 GiB acceptance limit"),
        );
    }
    Ok(findings.finish())
}

#[derive(Debug, Default)]
pub(crate) struct Findings(Vec<Finding>);

impl Findings {
    pub fn add(
        &mut self,
        code: &'static str,
        subject: impl Into<String>,
        detail: impl Into<String>,
    ) {
        self.0.push(Finding {
            code,
            subject: subject.into(),
            detail: detail.into(),
        });
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn extend(&mut self, other: Findings) {
        self.0.extend(other.0);
    }

    fn finish(mut self) -> Vec<Finding> {
        self.0.sort();
        self.0.dedup();
        self.0
    }
}

/// What the contract's compatibility rule lets this package's serving version add.
#[derive(Debug)]
pub(crate) struct Compatibility {
    /// A later minor of our major: may add relations and trailing nullable fields.
    pub newer_minor: bool,
    /// Relations the manifest declares that the contract does not know.
    pub extra_relations: BTreeSet<String>,
}

impl Compatibility {
    fn of(package: &Package, contract: &Contract) -> Compatibility {
        let manifest = package.manifest();
        let newer_minor = manifest
            .serving_schema_version
            .as_deref()
            .and_then(parse_version)
            .is_some_and(|(major, minor)| major == contract.major && minor > contract.minor);
        let extra_relations = manifest
            .relations
            .iter()
            .flatten()
            .filter(|relation| contract.relation(&relation.name).is_none())
            .map(|relation| relation.name.clone())
            .collect();
        Compatibility {
            newer_minor,
            extra_relations,
        }
    }
}

/// `serving/<name>.parquet`, as package-relative path.
pub(crate) fn relation_file(name: &str) -> String {
    format!("serving/{name}.parquet")
}
