//! Curated geodata the road compiler reads: stop catalogues, the stop registry, gazetteers,
//! name synonyms and route rules.
//!
//! Each reader returns the rows as the file writes them, with the 1-based line (or the feature
//! index of a GeoJSON file) so diagnostics can point at them. Which rows are usable, how names
//! compare and what wins are the compiler's rules (`docs/rules/jdf-read.md`, `jdf-merge.md`,
//! `jdf-convert.md`). A file that is not what it claims is an error; so is a row that breaks a
//! file grammar the documents state as a reviewed contract (the registry, the route rules).

pub mod catalogue;
pub mod filter_rules;
pub mod gazetteer;
pub mod registry;
pub mod route_rules;
pub mod stop_overrides;
pub mod synonyms;

/// The 1-based line a CSV record starts on.
fn line_of(record: &csv::StringRecord) -> u64 {
    record.position().map_or(0, csv::Position::line)
}

/// A decimal number as the invariant-culture readers of the F# JrUtil accept it, surrounding
/// whitespace allowed; `None` for anything else, including `NaN` and infinities.
fn finite_number(text: &str) -> Option<f64> {
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

/// `None` for an empty or all-whitespace cell, otherwise the trimmed text.
fn trimmed(text: &str) -> Option<Box<str>> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.into())
}
