//! Admitting a batch as read (R004, R020).

mod support;

use jr_formats::jdf::{self, JdfError, Problem, Version};
use jr_jdf::Policy;
use jr_jdf::source::admit;
use jr_model::{Diagnostics, Severity};
use support::*;

#[test]
fn r004_an_unreadable_batch_is_dropped_with_an_error() {
    let mut diagnostics = Diagnostics::new();
    let batch = admit(
        "vld-1",
        Err(JdfError::NoVersionFile),
        &Policy::builtin(),
        &mut diagnostics,
    );
    assert!(batch.is_none());
    let found: Vec<_> = diagnostics.iter().collect();
    assert_eq!(found.len(), 1);
    assert_eq!(
        (
            found[0].code.as_ref(),
            found[0].severity,
            &*found[0].subject
        ),
        ("jdf_batch_unreadable", Severity::Error, "vld-1")
    );
}

#[test]
fn r020_a_record_problem_is_diagnosed_and_the_batch_kept() {
    let read = jdf::Read {
        batch: source(Version::V1_11),
        problems: vec![Problem {
            file: "Zasspoje.txt".into(),
            record: 7,
            field: None,
            message: "no terminator".into(),
        }],
    };
    let mut diagnostics = Diagnostics::new();
    let batch = admit("vld-1", Ok(read), &Policy::builtin(), &mut diagnostics);
    assert!(batch.is_some());
    assert!(codes(&diagnostics).contains(&"jdf_record_problem"));
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message == "Zasspoje.txt record 7: no terminator")
    );
}
