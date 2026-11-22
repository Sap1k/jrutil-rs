//! The rail compiler: CZPTT messages to a Timetable. No I/O.
//!
//! This is the first half: from the reader's messages to the trip parts of each path
//! ([`paths`]). Every decision is a named rule of `docs/rules/czptt.md`; [`RULES`] lists them.
//!
//! See ARCHITECTURE.md.

pub mod path;
pub mod policy;

mod merge;
mod normalize;
mod order;
mod segment;
mod timing;

use jr_formats::czptt::ObjectType;
use jr_formats::kadr::Catalog;
use jr_model::{Diagnostics, Horizon, Rule};
use rayon::prelude::*;

pub use policy::Policy;

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
    segment::R048,
    segment::R049,
    segment::R050,
    segment::R051,
    merge::R065,
    normalize::R066,
    merge::R081,
    merge::R116,
    segment::R117,
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
    for (path, sink) in compiled {
        diagnostics.append(sink);
        paths.push(path);
    }
    Ok(Paths {
        horizon,
        paths,
        cancelled: merged.cancelled,
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
        parameters: message.parameters,
        calls: normalized.calls,
        parts,
        rejection,
    }
}
