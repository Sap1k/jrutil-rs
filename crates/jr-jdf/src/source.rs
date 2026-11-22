//! Admitting a batch as it was read (JDF-R004, R020 with verdict Q11).
//!
//! The reader reports facts: a batch, the problems it met, or why the batch cannot be read at
//! all. What follows from them is decided here, before the version defaults ([`crate::batch`]).

use jr_formats::jdf::{self, JdfError};
use jr_model::{Diagnostic, Diagnostics, Rule, Severity};

use crate::Policy;
use crate::batch::{self, Batch};

/// A batch that cannot be read is left out with an error diagnostic, and the caller must not
/// publish the package as complete: lines are missing from it. The build goes on.
///
/// F# aborted the whole run on the first such batch. One operator's broken export then stopped
/// the national build; a build that finishes and says what is missing can still be inspected
/// (verdict Q11).
pub const R004: Rule = Rule {
    id: "JDF-R004",
    summary: "a batch that cannot be read is dropped with an error and makes the package ineligible",
};

/// A record the grammar cannot place (no terminator, wrong field count, a value that does not
/// parse) is diagnosed with its file and record number; what the reader could still read of the
/// batch is used.
///
/// Exports end files without the final `;` or carry a stray field; F# dropped such records
/// without a word (B12).
pub const R020: Rule = Rule {
    id: "JDF-R020",
    summary: "a record the grammar cannot place is diagnosed, not silently dropped",
};

pub const RULES: &[Rule] = &[R004, R020];

/// The batch with its version's defaults applied, or `None` for one that cannot be read (R004).
pub fn admit(
    name: &str,
    read: Result<jdf::Read, JdfError>,
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> Option<Batch> {
    match read {
        Err(error) => {
            diagnostics.push(
                Diagnostic::new(
                    "jdf_batch_unreadable",
                    Severity::Error,
                    name,
                    format!("batch dropped: {error}"),
                )
                .rule(R004),
            );
            None
        }
        Ok(read) => {
            for problem in &read.problems {
                diagnostics.push(
                    Diagnostic::new(
                        "jdf_record_problem",
                        Severity::Warning,
                        name,
                        problem.to_string(),
                    )
                    .rule(R020),
                );
            }
            Some(batch::normalise(name, read.batch, policy, diagnostics))
        }
    }
}
