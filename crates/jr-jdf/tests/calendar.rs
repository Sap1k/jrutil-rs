//! A trip's running days (R255–R264, R267, R030), over the 2026 timetable year. The validity
//! 2026-03-02 (Monday, ISO week 10) .. 2026-03-15 (Sunday, week 11) has no holiday; Easter 2026
//! is 5 April (Good Friday 3 April, Easter Monday 6 April).

mod support;

use jiff::civil::{Date, Weekday, date};
use jr_formats::jdf::NoteType;
use support::*;

fn weekday(d: Date) -> Weekday {
    d.weekday()
}

fn working(d: Date) -> bool {
    !matches!(weekday(d), Weekday::Saturday | Weekday::Sunday)
}

#[test]
fn r255_an_undated_note_covers_only_the_first_validity_day() {
    let no = NoteType::NoService;
    let (days, diagnostics) = run(&input(
        &[],
        &[typed(&no, None, None)],
        MONDAY,
        Some(SUNDAY_AFTER_NEXT),
    ));
    assert_eq!(days, span(date(2026, 3, 3), SUNDAY_AFTER_NEXT));
    assert_eq!(codes(&diagnostics), ["jdf_note_undated"]);
}

#[test]
fn r255_a_note_with_only_a_start_covers_that_day_and_untyped_notes_do_nothing() {
    let also = NoteType::ServiceAlso;
    let untyped = jr_jdf::calendar::CalendarNote {
        note_type: None,
        date_from: Some(MONDAY),
        date_to: Some(SUNDAY_AFTER_NEXT),
    };
    let saturday = date(2026, 3, 7);
    let (days, _) = run(&input(
        &["X"],
        &[typed(&also, Some(saturday), None), untyped],
        MONDAY,
        Some(SUNDAY_AFTER_NEXT),
    ));
    let mut expected = span_where(MONDAY, SUNDAY_AFTER_NEXT, working);
    expected.push(saturday);
    expected.sort();
    assert_eq!(days, expected);
}

#[test]
fn r255_a_note_ending_before_it_starts_covers_nothing() {
    let no = NoteType::NoService;
    let (days, diagnostics) = run(&input(
        &[],
        &[typed(&no, Some(date(2026, 3, 10)), Some(date(2026, 3, 5)))],
        MONDAY,
        Some(SUNDAY_AFTER_NEXT),
    ));
    assert_eq!(days, span(MONDAY, SUNDAY_AFTER_NEXT));
    assert_eq!(codes(&diagnostics), ["jdf_note_reversed"]);
}

#[test]
fn r256_runs_only_rows_decide_alone() {
    let only = NoteType::ServiceOnly;
    let no = NoteType::NoService;
    let saturday = date(2026, 3, 7);
    let (days, _) = run(&input(
        &["X"],
        &[
            typed(&only, Some(saturday), None),
            typed(&no, Some(saturday), None),
        ],
        MONDAY,
        Some(SUNDAY_AFTER_NEXT),
    ));
    assert_eq!(days, [saturday]);
}

#[test]
fn r257_r258_does_not_run_beats_runs_also() {
    let also = NoteType::ServiceAlso;
    let no = NoteType::NoService;
    let tuesday = date(2026, 3, 3);
    let sunday = date(2026, 3, 8);
    let (days, _) = run(&input(
        &["X"],
        &[
            typed(&also, Some(tuesday), None),
            typed(&no, Some(tuesday), None),
            typed(&also, Some(sunday), None),
        ],
        MONDAY,
        Some(SUNDAY_AFTER_NEXT),
    ));
    let mut expected = span_where(MONDAY, SUNDAY_AFTER_NEXT, |d| working(d) && d != tuesday);
    expected.push(sunday);
    expected.sort();
    assert_eq!(days, expected);
}

#[test]
fn r259_odd_and_even_week_rows_drop_days_of_the_other_parity() {
    let odd = NoteType::OddWeeksFromTo;
    let (days, _) = run(&input(
        &[],
        &[typed(&odd, Some(MONDAY), Some(SUNDAY_AFTER_NEXT))],
        MONDAY,
        Some(SUNDAY_AFTER_NEXT),
    ));
    assert_eq!(days, span(date(2026, 3, 9), SUNDAY_AFTER_NEXT));

    let even = NoteType::EvenWeeks;
    let (days, _) = run(&input(
        &[],
        &[typed(&even, Some(MONDAY), Some(SUNDAY_AFTER_NEXT))],
        MONDAY,
        Some(SUNDAY_AFTER_NEXT),
    ));
    assert_eq!(days, span(MONDAY, date(2026, 3, 8)));
}

#[test]
fn r259_an_undated_odd_week_row_restricts_only_the_first_day() {
    // Ported as written (B9): week 10 is even, so only 2 March is dropped.
    let odd = NoteType::OddWeeks;
    let (days, _) = run(&input(
        &[],
        &[typed(&odd, None, None)],
        MONDAY,
        Some(SUNDAY_AFTER_NEXT),
    ));
    assert_eq!(days, span(date(2026, 3, 3), SUNDAY_AFTER_NEXT));
}

#[test]
fn r260_runs_in_rows_limit_the_day_attributes() {
    let runs_in = NoteType::Service;
    let (days, _) = run(&input(
        &["X"],
        &[typed(
            &runs_in,
            Some(date(2026, 3, 7)),
            Some(date(2026, 3, 10)),
        )],
        MONDAY,
        Some(SUNDAY_AFTER_NEXT),
    ));
    // Saturday and Sunday inside the row stay off: the day attribute still decides.
    assert_eq!(days, [date(2026, 3, 9), date(2026, 3, 10)]);
}

#[test]
fn r261_plus_runs_on_sundays_and_holidays() {
    let (days, _) = run(&input(
        &["+"],
        &[],
        date(2026, 4, 1),
        Some(date(2026, 4, 12)),
    ));
    assert_eq!(
        days,
        [
            date(2026, 4, 3),
            date(2026, 4, 5),
            date(2026, 4, 6),
            date(2026, 4, 12)
        ]
    );
}

#[test]
fn r262_x_runs_on_working_days_only() {
    let (days, _) = run(&input(
        &["X"],
        &[],
        date(2026, 4, 1),
        Some(date(2026, 4, 12)),
    ));
    assert_eq!(
        days,
        [
            date(2026, 4, 1),
            date(2026, 4, 2),
            date(2026, 4, 7),
            date(2026, 4, 8),
            date(2026, 4, 9),
            date(2026, 4, 10)
        ]
    );
}

#[test]
fn r263_a_day_number_runs_on_that_weekday_holidays_included() {
    let (days, _) = run(&input(
        &["1", "6"],
        &[],
        date(2026, 4, 1),
        Some(date(2026, 4, 12)),
    ));
    // Easter Monday is a holiday and still a Monday.
    assert_eq!(
        days,
        [date(2026, 4, 4), date(2026, 4, 6), date(2026, 4, 11)]
    );
}

#[test]
fn r263_r264_r267_a_day_number_that_is_no_day_matches_nothing() {
    let (days, diagnostics) = run(&input(&["8"], &[], MONDAY, Some(SUNDAY_AFTER_NEXT)));
    assert!(days.is_empty());
    assert_eq!(codes(&diagnostics), ["jdf_trip_never_runs"]);
}

#[test]
fn r264_a_trip_without_day_attributes_runs_every_day() {
    let (days, diagnostics) = run(&input(&["@", "x"], &[], MONDAY, Some(SUNDAY_AFTER_NEXT)));
    assert_eq!(days, span(MONDAY, SUNDAY_AFTER_NEXT));
    assert!(diagnostics.is_empty());
}

#[test]
fn r267_a_trip_closed_over_its_whole_validity_never_runs() {
    // F# JdfToGtfsTests "Shared inactive schedules delete every source trip".
    let no = NoteType::NoService;
    let (days, diagnostics) = run(&input(
        &["X"],
        &[typed(&no, Some(MONDAY), Some(SUNDAY_AFTER_NEXT))],
        MONDAY,
        Some(SUNDAY_AFTER_NEXT),
    ));
    assert!(days.is_empty());
    assert_eq!(codes(&diagnostics), ["jdf_trip_never_runs"]);
    assert_eq!(
        diagnostics.iter().next().map(|d| d.subject.as_str()),
        Some("jdf:test:000001:0/1")
    );
}

#[test]
fn r030_an_open_validity_runs_to_the_end_of_the_horizon_and_days_outside_are_dropped() {
    let (days, _) = run(&input(&[], &[], date(2025, 6, 1), None));
    assert_eq!(days.len(), gvd().days());
    assert_eq!(days.first(), Some(&date(2025, 12, 14)));
    assert_eq!(days.last(), Some(&date(2026, 12, 12)));
}
