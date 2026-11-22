//! Projections of a Timetable: serving relations, GTFS, manifest; package reading, validation and comparison.
//!
//! So far the reading side: the serving contract (`contract`), relations in memory (`table`),
//! a package on disk (`package`, `manifest`), its contract validation (`validate`) and the
//! canonical comparison of two packages against a parity ledger (`compare`). The writing side:
//! the package writer (`write`), a fixed projection of a `jr_model::Timetable`.

pub mod compare;
pub mod contract;
mod dates;
mod error;
pub mod manifest;
pub mod package;
mod parquet_read;
pub mod table;
pub mod validate;
pub mod write;

pub use compare::{Comparison, Ledger, Profile, Verdict, compare, compare_classified};
pub use contract::{
    Contract, FieldSpec, FieldType, ForeignKey, NamespaceSpec, RelationSpec, contract,
};
pub use error::PackageError;
pub use manifest::Manifest;
pub use package::{CsvTable, Package};
pub use table::{Column, StrColumn, StrColumnBuilder, Table};
pub use validate::{Finding, validate};
pub use write::{
    FeedInfo, PublishError, PublishOptions, SourceEntry, relations, write_package, write_relation,
};

/// Parquet `date32` day numbers, for writers and tests: days since 1970-01-01.
pub fn date_from_days(days: i32) -> Option<jiff::civil::Date> {
    dates::from_days(days)
}

pub fn date_to_days(date: jiff::civil::Date) -> i32 {
    dates::to_days(date)
}
