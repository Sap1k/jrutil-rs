//! The rail compiler: CZPTT messages to a Timetable. No I/O.
//!
//! This is the first half: from the reader's messages to the trip parts of each path
//! ([`paths`]). Every decision is a named rule of `docs/rules/czptt.md`; [`RULES`] lists them.
//!
//! See ARCHITECTURE.md.

pub mod connections;
pub mod ids_days;
pub mod path;
pub mod policy;

mod assemble;
mod merge;
mod normalize;
mod notes;
mod order;
mod segment;
mod timing;
mod transfers;

use jr_formats::czptt::ObjectType;
use jr_formats::kadr::Catalog;
use jr_model::{Diagnostics, Horizon, Rule};
use rayon::prelude::*;

pub use assemble::Summary;
pub use policy::Policy;

/// Catalogue (SR70) names of points: `(country, code)` to name.
pub type PointNames = std::collections::BTreeMap<(Box<str>, Box<str>), Box<str>>;

/// The compiled timetable and what went into it.
#[derive(Debug)]
pub struct Compiled {
    pub timetable: jr_model::Timetable,
    pub summary: Summary,
    /// Trips whose IDS zones differ by day, split by [`ids_days::edits`] (R120).
    pub ids_days: ids_days::IdsDays,
}

/// Messages to timetable: [`paths`], then the assembly of places, routes, trips, notes, zones
/// and in-path transfers. The replacement-bus connections are a separate pass
/// ([`connections::nad_connections`]).
pub fn compile(
    messages: Vec<SourceMessage>,
    catalog: &Catalog,
    point_names: &PointNames,
    timetable_year: i16,
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> Result<Compiled, CompileError> {
    let paths = paths(messages, catalog, timetable_year, policy, diagnostics)?;
    let (timetable, summary, ids_days) =
        assemble::assemble(&paths, catalog, point_names, policy, diagnostics);
    Ok(Compiled {
        timetable,
        summary,
        ids_days,
    })
}

/// Paths are processed in ordinal order of their PA identifier text, so every later
/// first-wins choice is deterministic.
const R026: Rule = Rule {
    id: "CZPTT-R026",
    summary: "Paths are processed in ordinal order of their PA identifier",
};

/// Every rule this crate implements.
pub const RULES: &[Rule] = &[
    order::R002,
    order::R005,
    order::R006,
    order::R010,
    order::R011,
    order::R012,
    merge::R013,
    merge::R014,
    merge::R015,
    merge::R016,
    R026,
    normalize::R027,
    normalize::R028,
    normalize::R029,
    normalize::R030,
    normalize::R031,
    normalize::R032,
    normalize::R033,
    normalize::R034,
    normalize::R035,
    normalize::R036,
    normalize::R037,
    normalize::R038,
    normalize::R039,
    normalize::R040,
    segment::R041,
    segment::R042,
    segment::R043,
    segment::R044,
    segment::R045,
    segment::R046,
    segment::R047,
    segment::R049,
    segment::R050,
    segment::R051,
    assemble::R052,
    assemble::R053,
    assemble::R054,
    assemble::R055,
    assemble::R056,
    assemble::R057,
    assemble::R058,
    assemble::R059,
    assemble::R060,
    assemble::R061,
    assemble::R062,
    assemble::R063,
    assemble::R064,
    merge::R065,
    normalize::R066,
    assemble::R067,
    assemble::R068,
    assemble::R069,
    notes::R070,
    notes::R071,
    notes::R072,
    notes::R073,
    notes::R074,
    notes::R075,
    notes::R076,
    notes::R077,
    notes::R078,
    notes::R079,
    assemble::R080,
    merge::R081,
    transfers::R082,
    connections::R083,
    notes::R085,
    notes::R086,
    notes::R119,
    ids_days::R120,
    assemble::R121,
    assemble::R122,
    merge::R116,
    segment::R117,
    assemble::R118,
];

/// One source message as the caller read it.
#[derive(Debug)]
pub struct SourceMessage {
    /// `annual/JR2026.zip//name.xml` or `changes/2026-01/x.xml.zip`.
    pub source_path: Box<str>,
    /// It came from the annual archive.
    pub annual: bool,
    /// SHA-256 of the raw XML bytes.
    pub digest: [u8; 32],
    /// The parsed message, or the reader's error text.
    pub message: Result<jr_formats::czptt::Message, Box<str>>,
}

/// Every surviving path with its trip parts, in PA order.
#[derive(Clone, Debug, PartialEq)]
pub struct Paths {
    /// The GVD window widened by the policy's margin (R116, Q2); every `DateSet` is over it.
    pub horizon: Horizon,
    pub paths: Vec<path::Path>,
    /// Paths left without running days (R016), in PA order.
    pub cancelled: Vec<Box<str>>,
    /// `czptt:<newest creation>` (R081); `None` without surviving paths.
    pub source_version: Option<Box<str>>,
}

#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    /// R012.
    #[error("conflicting timetable messages share path {pa}: {first} and {second}")]
    ConflictingPath {
        pa: Box<str>,
        first: Box<str>,
        second: Box<str>,
    },
    #[error("timetable year {year}: {source}")]
    Horizon {
        year: i16,
        #[source]
        source: jr_model::time::HorizonError,
    },
    #[error("policy time zone {zone:?}: {message}")]
    TimeZone { zone: Box<str>, message: String },
}

/// Orders and merges the messages, then normalizes and segments every surviving path.
pub fn paths(
    messages: Vec<SourceMessage>,
    catalog: &Catalog,
    timetable_year: i16,
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> Result<Paths, CompileError> {
    let horizon = Horizon::gvd(timetable_year)
        .and_then(|gvd| gvd.widened(policy.horizon.days_before, policy.horizon.days_after))
        .map_err(|source| CompileError::Horizon {
            year: timetable_year,
            source,
        })?;
    let ordered = order::order(messages, policy, diagnostics)?;
    let merged = merge::merge(ordered, horizon, policy, diagnostics);

    // R026: `surviving` is in PA order; each path reports into its own sink and the sinks are
    // appended in that order, so neither output nor diagnostics depend on scheduling.
    let compiled: Vec<(path::Path, Diagnostics)> = merged
        .surviving
        .into_par_iter()
        .map(|merged| {
            let mut sink = Diagnostics::new();
            let path = compile_path(merged, horizon, catalog, policy, &mut sink);
            (path, sink)
        })
        .collect();
    let mut paths = Vec::with_capacity(compiled.len());
    let mut cancelled = merged.cancelled;
    for (path, sink) in compiled {
        diagnostics.append(sink);
        // R016: a path left without days by anything after the merge (R066) is cancelled.
        if path.dates.is_empty() {
            diagnostics.push(
                jr_model::Diagnostic::new(
                    "czptt_path_cancelled",
                    jr_model::Severity::Info,
                    &*path.pa,
                    "no running day is left",
                )
                .rule(merge::R016),
            );
            cancelled.push(path.pa);
            continue;
        }
        paths.push(path);
    }
    cancelled.sort();
    Ok(Paths {
        horizon,
        paths,
        cancelled,
        source_version: merged.source_version,
    })
}

fn compile_path(
    merged: merge::Merged,
    horizon: Horizon,
    catalog: &Catalog,
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> path::Path {
    let message = merged.message;
    let tr = order::identifier_text(&message.identifiers, ObjectType::Tr, policy);
    let tr_company = tr
        .as_ref()
        .map(|(_, identifier)| identifier.company.clone());
    let tr_core = tr.as_ref().map(|(_, identifier)| identifier.core.clone());
    let tr = tr.map(|(text, _)| text);
    let normalized = normalize::normalize(
        &merged.pa,
        &message,
        tr_company.as_deref(),
        merged.dates,
        horizon,
        catalog,
        policy,
        diagnostics,
    );
    let (parts, rejection) = match normalized.rejection {
        Some(rejection) => (Vec::new(), Some(rejection)),
        // Cancelled by the caller (R016): no parts.
        None if normalized.dates.is_empty() => (Vec::new(), None),
        None => match segment::segment(&merged.pa, &normalized.calls, policy, diagnostics) {
            Ok(parts) => {
                normalize::report_adjustments(&merged.pa, &normalized.adjustments, diagnostics);
                (parts, None)
            }
            Err(rejection) => (Vec::new(), Some(rejection)),
        },
    };
    path::Path {
        pa: merged.pa,
        tr,
        tr_company,
        tr_core,
        dates: normalized.dates,
        first_date: message.calendar.start.local.date(),
        day_shift: normalized.day_shift,
        parameters: message.parameters,
        calls: normalized.calls,
        parts,
        rejection,
    }
}
