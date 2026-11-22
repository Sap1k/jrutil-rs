//! The merge's two passes: calls are placed per wanted batch, in any order, exactly once.

mod support;

use jiff::civil::date;
use jr_jdf::fixed::FixedBatch;
use jr_jdf::merge::{self, MergeContext, MergeError, Merger, Registry};
use jr_model::Diagnostics;
use support::merge::*;

fn batches() -> Vec<FixedBatch> {
    let version = |name: &str, licence: &str, from| {
        batch(name)
            .stop(stop("Alfa", None, None))
            .stop(stop(&format!("Beta {licence}"), None, None))
            .route(licence, from, Some(date(2026, 12, 12)), false)
            .trip(licence, 1, &[])
            .trip(licence, 3, &["X"])
            .build()
    };
    vec![
        version("a", "100001", date(2025, 12, 14)),
        version("b", "100002", date(2025, 12, 14)),
        // Superseded on every date by b (equal facts, the earlier batch wins): not wanted.
        version("c", "100002", date(2025, 12, 14)),
        version("d", "100003", date(2026, 1, 1)),
    ]
}

/// Pass 1 over `batches`, then the resolution; the batches kept aside for pass 2.
fn resolve_with<R>(
    batches: Vec<FixedBatch>,
    then: impl FnOnce(merge::Resolved<'_>, Vec<jr_jdf::batch::Batch>, &mut Diagnostics) -> R,
) -> R {
    let names = names();
    let jdf = jr_jdf::Policy::builtin();
    let registry = Registry::empty();
    let policy = merge::Policy::builtin();
    let context = MergeContext {
        registry: &registry,
        names: &names,
        jdf: &jdf,
        timetable_year: 2026,
        reference_date: date(2025, 12, 14),
    };
    let mut diagnostics = Diagnostics::new();
    let mut merger = Merger::new(context, &policy).expect("merger");
    let kept: Vec<_> = batches.iter().map(|b| b.batch.clone()).collect();
    for fixed in batches {
        merger.add(fixed, &mut diagnostics).expect("added");
    }
    let resolved = merger.resolve(&mut diagnostics);
    then(resolved, kept, &mut diagnostics)
}

#[test]
fn calls_placed_in_another_batch_order_give_the_same_merge() {
    let (expected, _) = merge_stops(batches());
    let merged = resolve_with(batches(), |mut resolved, kept, diagnostics| {
        let wanted: Vec<String> = resolved.wanted().map(str::to_owned).collect();
        assert_eq!(wanted, ["a", "b", "d"]);
        for batch in kept.into_iter().rev() {
            if wanted.iter().any(|w| **w == *batch.name) {
                resolved.add_calls(batch, diagnostics).expect("placed");
            }
        }
        resolved.finish(diagnostics).expect("finished")
    });
    assert_eq!(merged, expected);
    assert!(
        merged
            .versions
            .iter()
            .all(|v| v.trips.iter().all(|t| t.calls.len() == 2))
    );
}

#[test]
fn a_missing_unwanted_or_repeated_batch_is_an_error() {
    resolve_with(batches(), |mut resolved, kept, diagnostics| {
        let mut kept = kept.into_iter();
        let a = kept.next().expect("a");
        let c = kept.nth(1).expect("c");
        assert!(matches!(
            resolved.add_calls(c, diagnostics),
            Err(MergeError::Unwanted(name)) if &*name == "c"
        ));
        resolved.add_calls(a.clone(), diagnostics).expect("placed");
        assert!(matches!(
            resolved.add_calls(a, diagnostics),
            Err(MergeError::CallsTwice(name)) if &*name == "a"
        ));
        assert!(matches!(
            resolved.finish(diagnostics),
            Err(MergeError::MissingCalls(name)) if &*name == "b"
        ));
    });
}
