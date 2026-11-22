//! The GTFS overlay engine: what a regional (or foreign) GTFS feed knows better than the
//! timetable compiled from national sources, as edits to that timetable. No I/O.
//!
//! The engine is not tied to a road base or to particular sources. A source is a
//! [`jr_formats::gtfs::Feed`] with a [`profile::SourceProfile`] that says how to read it (its
//! quirks are named hooks of the profile, never branches on a source's name); the base is a
//! `Timetable` with a [`profile::BaseProfile`] that says which of its facts the engine may
//! rely on. The stages follow `docs/rules/overlay.md`:
//!
//! 1. [`prepare`]: each source against its profile and the timetable's horizon, the base's
//!    indexes, source stops matched to base stop places, source routes given their candidate
//!    base routes, call patterns (OVL-R013–R056);
//! 2. authority: on which lines and dates a source runs the complete trip set (R057–R064);
//! 3. matching: each source trip, date by date, to a base trip (R065–R076);
//! 4. projection: what a matched base trip takes from the source, which national trips retire,
//!    which source trips are added, as edits, with keys (R077–R116).
//!
//! The review reports the stages produce are returned as tables ([`report::ReportTable`]) for
//! the command line to write next to the package.
//!
//! See ARCHITECTURE.md.

pub mod prepare;
pub mod profile;
pub mod report;

use jr_formats::geodata::registry::OverlayPlace;
use jr_formats::geodata::{stop_overrides, synonyms};
use jr_formats::gtfs::Feed;
use jr_model::{Diagnostics, EditBuilder, Edits, Timetable};

use profile::{BaseProfile, SourceProfile};
use report::ReportTable;

/// The pass name recorded in the timetable's history.
pub const PASS: &str = "overlay";

/// One source: its profile, its feed as read, and the reviewed stop overrides for it.
#[derive(Debug)]
pub struct SourceInput<'a> {
    pub profile: &'a SourceProfile,
    pub feed: Feed,
    pub overrides: Vec<stop_overrides::Row>,
}

#[derive(Debug)]
pub struct OverlayInput<'a> {
    pub base: &'a Timetable,
    pub base_profile: &'a BaseProfile,
    /// In ordinal order of their source IDs: where a rule says "first source", it means that
    /// order.
    pub sources: Vec<SourceInput<'a>>,
    /// The registry's pinned stop places of sources (`overlay_places.csv`).
    pub places: &'a [OverlayPlace],
    /// The stop-name synonym table every name matcher shares.
    pub synonyms: &'a [synonyms::Line],
}

#[derive(Debug)]
pub struct Overlaid {
    pub edits: Edits,
    /// Review reports, each a table the command line writes as `<name>.csv`.
    pub reports: Vec<ReportTable>,
}

#[derive(Debug, thiserror::Error)]
pub enum OverlayError {
    #[error(transparent)]
    Profile(#[from] profile::ProfileError),
    #[error(transparent)]
    Prepare(#[from] prepare::PrepareError),
}

/// Every rule the engine implements, stage by stage.
pub fn rules() -> impl Iterator<Item = jr_model::Rule> {
    [prepare::RULES].into_iter().flatten().copied()
}

pub fn overlay(
    input: OverlayInput<'_>,
    diagnostics: &mut Diagnostics,
) -> Result<Overlaid, OverlayError> {
    let base = input.base;
    let prepared = prepare::prepare(input, diagnostics)?;
    // Authority, matching and projection are not built yet: nothing is changed.
    Ok(Overlaid {
        edits: EditBuilder::new(base, PASS).finish(),
        reports: prepared.reports(),
    })
}
