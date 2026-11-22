//! The package writer: a fixed projection of a [`Timetable`] into a `jrutil-production`
//! package (`docs/rules/serving.md`). It serialises and decides nothing (AGENTS.md rule 2).
//!
//! - [`relations`] builds the 19 serving relations in memory, each sorted by primary key.
//! - [`write_package`] writes them as Parquet, projects `gtfs.zip` from them and writes
//!   `diagnostics.json` and `manifest.json`, all inside a sibling temporary directory that is
//!   renamed into place at the end.
//!
//! What is published: a trip whose service is not empty, and what published rows refer to
//! ([`scope`]). A transfer, assignment, source key or call key is published when every trip and
//! route it names is published; the places and boarding points it names are then published
//! with it. A key on a boarding point is published when the boarding point is.

mod assemble;
mod basic;
mod calendar;
mod error;
mod files;
mod gtfs;
mod keys;
mod places;
mod scope;
mod semantics;
mod trips;
mod zones;

use std::path::Path;

use jr_model::{Diagnostics, Timetable};
use rayon::prelude::*;

pub use error::PublishError;
pub use files::write_relation;

use crate::manifest::Manifest;
use crate::table::Table;

/// What the writer is told rather than reads from the timetable.
#[derive(Debug, Clone, PartialEq)]
pub struct PublishOptions {
    /// `manifest.compiler = {"tool": "jrutil-rs", "version": …}`.
    pub compiler_version: String,
    /// Computed by the caller (serving.md verdict 13).
    pub build_spec_sha256: String,
    /// `manifest.sources`, in this order.
    pub sources: Vec<SourceEntry>,
    /// GTFS `feed_info.txt` constants.
    pub feed_info: FeedInfo,
    /// Appended to the name of a place with an estimated position and to its boarding points'
    /// names (`" [?]"`).
    pub approximate_suffix: String,
    /// Write a timed transfer between two locations as a type-2, zero-minute stop pair in
    /// `transfers.txt` (the form MOTIS needs). The serving relation keeps the source fact.
    pub gtfs_timed_transfers_as_stop_pairs: bool,
    /// Further `diagnostics.json` sections (`route_stop_order`, `route_shapes`, …), in order.
    pub extra_sections: Vec<(String, serde_json::Value)>,
    pub publication_eligible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEntry {
    pub source_id: String,
    pub payload_sha256: String,
    pub payload_kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedInfo {
    pub publisher_name: String,
    pub publisher_url: String,
    pub lang: String,
    pub contact_email: Option<String>,
}

/// The 19 relations in contract order, each sorted by primary key. Pure.
pub fn relations(
    timetable: &Timetable,
    options: &PublishOptions,
) -> Result<Vec<Table>, PublishError> {
    let published = scope::Published::of(timetable)?;
    let services = calendar::Services::of(&published);
    let p = &published;
    let s = &services;
    type Build<'a> = Box<dyn Fn() -> Result<Table, PublishError> + Send + Sync + 'a>;
    let builders: Vec<(&str, Build)> = vec![
        ("agency", Box::new(|| basic::agency(p))),
        ("location", Box::new(|| places::location(p, options))),
        ("route", Box::new(|| basic::route(p))),
        ("service_calendar", Box::new(|| s.calendar())),
        ("service_exception", Box::new(|| s.exceptions())),
        ("trip", Box::new(|| trips::trip(p, s))),
        ("trip_call", Box::new(|| trips::trip_call(p))),
        ("route_stop", Box::new(|| zones::route_stop(p))),
        ("route_stop_zone", Box::new(|| zones::route_stop_zone(p))),
        ("call_zone", Box::new(|| zones::call_zone(p))),
        ("shape", Box::new(|| basic::shape(p))),
        ("shape_point", Box::new(|| basic::shape_point(p))),
        ("transfer", Box::new(|| semantics::transfer(p))),
        ("service_note", Box::new(|| semantics::service_note(p))),
        ("assignment", Box::new(|| semantics::assignment(p))),
        (
            "connection_claim",
            Box::new(|| assemble::empty("connection_claim")),
        ),
        (
            "travel_restriction",
            Box::new(|| assemble::empty("travel_restriction")),
        ),
        ("source_key", Box::new(|| keys::source_key(p))),
        ("call_key", Box::new(|| keys::call_key(p))),
    ];
    let contract = crate::contract::contract();
    let names: Vec<&str> = builders.iter().map(|(name, _)| *name).collect();
    let expected: Vec<&str> = contract.relations.iter().map(|r| r.name.as_str()).collect();
    if names != expected {
        return Err(PublishError::Contract(format!(
            "the writer builds {names:?}, the contract has {expected:?}"
        )));
    }
    // Results are collected in contract order, so the first error is the same on every run.
    builders
        .par_iter()
        .map(|(_, build)| build())
        .collect::<Vec<_>>()
        .into_iter()
        .collect()
}

/// Writes the package into `out`, which must not exist, through a sibling temporary
/// directory and one rename. Returns the manifest written.
pub fn write_package(
    timetable: &Timetable,
    diagnostics: &Diagnostics,
    options: &PublishOptions,
    out: &Path,
) -> Result<Manifest, PublishError> {
    if out.exists() {
        return Err(PublishError::OutputExists(out.to_path_buf()));
    }
    let tables = relations(timetable, options)?;
    files::write_atomically(out, |root| {
        files::write_contents(root, &tables, timetable, diagnostics, options)
    })
}
