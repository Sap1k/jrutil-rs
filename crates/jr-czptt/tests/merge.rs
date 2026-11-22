//! Ordering and merging the messages (R002–R016, R065, R081, R116).

mod support;

use jiff::civil::date;
use jr_czptt::{CompileError, Policy, SourceMessage};
use jr_formats::czptt::{Message, ObjectType};
use jr_model::Diagnostics;
use support::*;

fn dates(paths: &jr_czptt::Paths, index: usize) -> Vec<jiff::civil::Date> {
    paths.paths[index].dates.dates(paths.horizon).collect()
}

fn change(path: &str, seed: u8, message: Message) -> SourceMessage {
    source(path, false, seed, message)
}

fn two_days() -> jr_formats::czptt::TimetableMessage {
    let mut message = simple("1");
    message.calendar = calendar(date(2025, 12, 14), Some(date(2025, 12, 15)), "11");
    message
}

#[test]
fn r002_naive_event_time_is_prague_local_time() {
    // The timetable is stamped 10:00 UTC; the cancellation 10:30 without an offset, which is
    // 09:30 UTC in Prague. Read as Prague time, the cancellation comes first and finds nothing.
    let mut timetable = two_days();
    timetable.creation = stamp("2026-01-10T10:00:00");
    timetable.creation.offset_seconds = Some(0);
    let cancel = cancellation(
        "1",
        "2026-01-10T10:30:00",
        calendar(date(2025, 12, 14), None, "1"),
    );
    let (paths, diagnostics) = compile(vec![
        change("changes/b.xml.zip", 1, Message::Timetable(timetable)),
        change("changes/a.xml.zip", 2, Message::Cancellation(cancel)),
    ]);
    assert_eq!(dates(&paths, 0).len(), 2);
    assert!(has(&diagnostics, "czptt_unknown_cancellation_target"));
}

#[test]
fn r005_changes_follow_the_annual_archive_and_timetables_precede_cancellations_on_ties() {
    let mut timetable = two_days();
    timetable.creation = stamp("2026-01-10T10:00:00");
    let cancel = cancellation(
        "1",
        "2026-01-10T10:00:00",
        calendar(date(2025, 12, 15), None, "1"),
    );
    // The cancellation's path sorts first, but on an equal event time the timetable applies first.
    let (paths, _) = compile(vec![
        change(
            "changes/a.xml.zip",
            2,
            Message::Cancellation(cancel.clone()),
        ),
        change("changes/b.xml.zip", 1, Message::Timetable(timetable)),
    ]);
    assert_eq!(dates(&paths, 0), [date(2025, 12, 14)]);

    // An annual timetable applies before any change, however old the change's stamp.
    let mut early = cancel;
    early.cancellation = stamp("2025-01-01T00:00:00");
    let (paths, _) = compile(vec![
        change("changes/a.xml.zip", 2, Message::Cancellation(early)),
        annual(1, two_days()),
    ]);
    assert_eq!(dates(&paths, 0), [date(2025, 12, 14)]);
}

#[test]
fn r006_an_exact_duplicate_payload_is_dropped() {
    // Re-applying the same timetable after a cancellation would bring the day back.
    let cancel = cancellation(
        "1",
        "2026-01-10T10:00:00",
        calendar(date(2025, 12, 15), None, "1"),
    );
    let mut resent = two_days();
    resent.creation = stamp("2026-02-01T00:00:00");
    let (paths, diagnostics) = compile(vec![
        annual(1, two_days()),
        change("changes/a.xml.zip", 2, Message::Cancellation(cancel)),
        change("changes/b.xml.zip", 1, Message::Timetable(resent)),
    ]);
    assert_eq!(dates(&paths, 0), [date(2025, 12, 14)]);
    assert_eq!(
        messages_of(&diagnostics, "czptt_duplicate_message").len(),
        1
    );
}

#[test]
fn r010_an_unreadable_message_is_diagnosed_and_skipped() {
    let broken = SourceMessage {
        source_path: "changes/broken.xml.zip".into(),
        annual: false,
        digest: digest(9),
        message: Err("CZPTTCreation is missing".into()),
    };
    let (paths, diagnostics) = compile(vec![broken, annual(1, simple("1"))]);
    assert_eq!(paths.paths.len(), 1);
    let unreadable: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "czptt_message_unreadable")
        .collect();
    assert_eq!(unreadable.len(), 1);
    assert_eq!(unreadable[0].subject, "changes/broken.xml.zip");
    assert_eq!(unreadable[0].message, "CZPTTCreation is missing");
}

#[test]
fn r011_a_path_is_its_published_pa_identifier() {
    let mut without = simple("2");
    without
        .identifiers
        .retain(|id| id.object_type == ObjectType::Tr);
    let (paths, diagnostics) = compile(vec![annual(1, simple("000000001234")), annual(2, without)]);
    assert_eq!(paths.paths.len(), 1);
    let path = &paths.paths[0];
    assert_eq!(&*path.pa, PA);
    assert_eq!(path.tr.as_deref(), Some("Tr:54:000000001234:00:2026"));
    assert_eq!(path.tr_company.as_deref(), Some("54"));
    assert_eq!(path.tr_core.as_deref(), Some("000000001234"));
    assert!(has(&diagnostics, "czptt_message_without_path"));
}

#[test]
fn r003_r007_r012_different_payloads_for_one_pa_stop_the_build() {
    let mut other = simple("1");
    other.creation = stamp("2026-01-01T00:00:00");
    let mut diagnostics = Diagnostics::new();
    let result = jr_czptt::paths(
        vec![
            annual(1, simple("1")),
            change("changes/x.xml.zip", 2, Message::Timetable(other)),
        ],
        &catalog(),
        2026,
        &Policy::builtin(),
        &mut diagnostics,
    );
    match result {
        Err(CompileError::ConflictingPath { pa, first, second }) => {
            assert_eq!(&*pa, "Pa:54:1:00:2026");
            assert_eq!(&*first, "annual/JR2026.zip//001.xml");
            assert_eq!(&*second, "changes/x.xml.zip");
        }
        other => panic!("expected a conflict, got {other:?}"),
    }
}

#[test]
fn r013_a_cancellation_removes_its_marked_days() {
    let mut three = simple("1");
    three.calendar = calendar(date(2025, 12, 14), Some(date(2025, 12, 16)), "111");
    let cancel = cancellation(
        "1",
        "2026-01-10T10:00:00",
        calendar(date(2025, 12, 14), Some(date(2025, 12, 16)), "010"),
    );
    let (paths, _) = compile(vec![
        annual(1, three),
        change("changes/a.xml.zip", 2, Message::Cancellation(cancel)),
    ]);
    assert_eq!(dates(&paths, 0), [date(2025, 12, 14), date(2025, 12, 16)]);
}

#[test]
fn r014_cancellations_of_an_unknown_path_are_diagnosed_once() {
    let cancel = |seed: u8| {
        change(
            &format!("changes/{seed}.xml.zip"),
            seed,
            Message::Cancellation(cancellation(
                "7",
                "2026-01-10T10:00:00",
                calendar(date(2025, 12, 14), None, "1"),
            )),
        )
    };
    let (_, diagnostics) = compile(vec![annual(1, simple("1")), cancel(2), cancel(3)]);
    let unknown: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "czptt_unknown_cancellation_target")
        .collect();
    assert_eq!(unknown.len(), 1);
    assert_eq!(unknown[0].subject, "Pa:54:7:00:2026");
    assert!(unknown[0].message.starts_with("2 "));
}

#[test]
fn r015_a_cancellation_past_the_validity_applies_its_overlap() {
    let cancel = cancellation(
        "1",
        "2026-01-10T10:00:00",
        calendar(date(2025, 12, 15), Some(date(2025, 12, 16)), "11"),
    );
    let (paths, diagnostics) = compile(vec![
        annual(1, two_days()),
        change("changes/a.xml.zip", 2, Message::Cancellation(cancel)),
    ]);
    assert_eq!(dates(&paths, 0), [date(2025, 12, 14)]);
    assert!(has(&diagnostics, "czptt_cancellation_outside_validity"));
}

#[test]
fn r015_a_bitmap_that_does_not_fit_its_validity_drops_the_message() {
    let mut long = simple("1");
    long.calendar = calendar(date(2025, 12, 14), None, "11");
    let mut odd = simple("2");
    odd.calendar = calendar(date(2025, 12, 14), Some(date(2025, 12, 15)), "1x");
    let (paths, diagnostics) = compile(vec![
        annual(1, long),
        annual(2, odd),
        annual(3, simple("3")),
    ]);
    assert_eq!(paths.paths.len(), 1);
    assert_eq!(&*paths.paths[0].pa, "Pa:54:3:00:2026");
    assert_eq!(
        messages_of(&diagnostics, "czptt_calendar_malformed").len(),
        2
    );
}

#[test]
fn r016_a_path_without_days_is_listed_as_cancelled() {
    let cancel = cancellation(
        "1",
        "2026-01-10T10:00:00",
        calendar(date(2025, 12, 14), None, "1"),
    );
    let (paths, diagnostics) = compile(vec![
        annual(1, simple("1")),
        annual(2, simple("2")),
        change("changes/a.xml.zip", 3, Message::Cancellation(cancel)),
    ]);
    assert_eq!(paths.cancelled, [Box::from("Pa:54:1:00:2026")]);
    assert_eq!(paths.paths.len(), 1);
    assert!(has(&diagnostics, "czptt_path_cancelled"));
}

#[test]
fn r065_running_bits_count_from_the_start_date() {
    let mut message = simple("1");
    message.calendar = calendar(date(2025, 12, 20), Some(date(2025, 12, 22)), "101");
    let (path, _) = compile_one(message);
    let (paths, _) = compile(vec![annual(1, simple("1"))]);
    assert_eq!(
        path.dates.dates(paths.horizon).collect::<Vec<_>>(),
        [date(2025, 12, 20), date(2025, 12, 22)]
    );
    assert_eq!(path.first_date, date(2025, 12, 20));
}

#[test]
fn r081_the_source_version_is_the_newest_creation() {
    let mut newer = simple("2");
    newer.creation = stamp("2025-12-02T10:15:00");
    let (paths, _) = compile(vec![annual(1, simple("1")), annual(2, newer)]);
    assert_eq!(
        paths.source_version.as_deref(),
        Some("czptt:20251202T101500")
    );
}

#[test]
fn r116_running_days_outside_the_widened_horizon_are_dropped() {
    // GVD 2026 starts 2025-12-14; the horizon keeps one day before it.
    let mut message = simple("1");
    message.calendar = calendar(date(2025, 12, 12), Some(date(2025, 12, 14)), "111");
    let (paths, diagnostics) = compile(vec![annual(1, message)]);
    assert_eq!(paths.horizon.start(), date(2025, 12, 13));
    assert_eq!(dates(&paths, 0), [date(2025, 12, 13), date(2025, 12, 14)]);
    let outside = messages_of(&diagnostics, "czptt_dates_outside_horizon");
    assert_eq!(outside.len(), 1);
    assert!(outside[0].starts_with("1 "));
}
