//! Line versions, their dates and keys (R238, R240–R249, R254, R267, R285), ported from the F#
//! `JdfMergerTests` version cases. Their dates are moved into one timetable year (GVD 2027,
//! 2026-12-13..2027-12-11) keeping their order; a "cut" version is one that keeps fewer dates,
//! and `:p2` chunks are the same version's trips running on the union.

mod support;

use jiff::civil::{Date, date};
use jr_formats::jdf::NoteType;
use jr_jdf::fixed::FixedBatch;
use jr_jdf::merge::{Merged, Registry};
use support::merge::*;

const LICENCE: &str = "586001";

fn d(month: i8, day: i8) -> Date {
    date(2027, month, day)
}

fn version(name: &str, detour: bool, from: Date, to: Option<Date>) -> FixedBatch {
    batch(name)
        .created(d(1, 1))
        .stop(stop("Zastávka", None, None))
        .route(LICENCE, from, to, detour)
        .trip(LICENCE, 1, &[])
        .build()
}

fn merge(batches: Vec<FixedBatch>) -> Merged {
    merge_in(2027, batches, &Registry::empty()).0
}

/// (detour, won intervals) per surviving version, by first won date.
fn won(merged: &Merged) -> Vec<(bool, Vec<(Date, Date)>)> {
    let mut rows: Vec<_> = merged
        .versions
        .iter()
        .map(|v| (v.route.detour, v.won.clone()))
        .collect();
    rows.sort_by_key(|(_, won)| won[0].0);
    rows
}

const END: Date = date(2027, 11, 30);

#[test]
fn r245_r247_later_versions_supersede_an_open_ended_older_detour() {
    // "Later versions supersede an open-ended older detour".
    let merged = merge(vec![
        version("b1", true, d(8, 29), Some(END)),
        version("b2", false, d(9, 12), Some(END)),
        version("b3", false, d(9, 26), Some(END)),
        version("b4", false, d(9, 19), Some(END)),
    ]);
    assert_eq!(
        won(&merged),
        [
            (true, vec![(d(8, 29), d(9, 11))]),
            (false, vec![(d(9, 12), d(9, 18))]),
            (false, vec![(d(9, 19), d(9, 25))]),
            (false, vec![(d(9, 26), END)]),
        ]
    );
}

#[test]
fn r245_r247_a_cut_open_ended_detour_does_not_delay_later_versions() {
    // "A cut open-ended detour does not delay later versions" (batch order is not validity).
    let merged = merge(vec![
        version("b1", false, d(8, 29), Some(END)),
        version("b2", true, d(9, 12), Some(END)),
        version("b3", false, d(9, 26), Some(END)),
        version("b4", false, d(9, 5), Some(END)),
        version("b5", false, d(9, 19), Some(END)),
    ]);
    assert_eq!(
        won(&merged),
        [
            (false, vec![(d(8, 29), d(9, 4))]),
            (false, vec![(d(9, 5), d(9, 11))]),
            (true, vec![(d(9, 12), d(9, 18))]),
            (false, vec![(d(9, 19), d(9, 25))]),
            (false, vec![(d(9, 26), END)]),
        ]
    );
}

#[test]
fn r245_a_bounded_detour_keeps_priority_over_a_later_regular_version() {
    // "A bounded detour keeps priority over a later regular version".
    let merged = merge(vec![
        version("b1", false, d(1, 1), Some(END)),
        version("b2", true, d(5, 4), Some(d(10, 22))),
        version("b3", false, d(6, 1), Some(END)),
    ]);
    assert_eq!(
        won(&merged),
        [
            (false, vec![(d(1, 1), d(5, 3))]),
            (true, vec![(d(5, 4), d(10, 22))]),
            (false, vec![(d(10, 23), END)]),
        ]
    );
}

#[test]
fn r247_a_year_long_special_day_version_does_not_hide_later_regular_versions() {
    // "A year-long special-day version does not hide later regular versions".
    let merged = merge(vec![
        version("b1", false, d(1, 1), Some(END)),
        version("b2", false, d(1, 24), Some(d(11, 24))),
        version("b3", false, d(1, 25), Some(d(11, 25))),
        version("b4", false, d(2, 3), Some(END)),
        version("b5", false, d(9, 1), Some(END)),
    ]);
    assert_eq!(
        won(&merged),
        [
            (false, vec![(d(1, 1), d(1, 23))]),
            (false, vec![(d(1, 24), d(1, 24))]),
            (false, vec![(d(1, 25), d(2, 2))]),
            (false, vec![(d(2, 3), d(8, 31))]),
            (false, vec![(d(9, 1), END)]),
        ]
    );
}

fn day_type(name: &str, code: &str) -> FixedBatch {
    batch(name)
        .created(d(1, 1))
        .stop(stop("Zastávka", None, None))
        .route(LICENCE, d(1, 1), Some(END), false)
        .trip(LICENCE, 1, &[code])
        .build()
}

#[test]
fn r243_r244_r254_weekday_and_weekend_versions_of_one_validity_are_both_kept() {
    // "Weekday and weekend versions with one validity are both kept"; the second gets part 2.
    let merged = merge(vec![day_type("b1", "X"), day_type("b2", "+")]);
    assert_eq!(merged.versions.len(), 2);
    assert!(merged.versions.iter().all(|v| v.won == [(d(1, 1), END)]));
    let parts: Vec<_> = merged.versions.iter().map(|v| v.key.part).collect();
    assert_eq!(parts, [None, Some(2)]);
}

#[test]
fn r243_r244_versions_of_one_validity_sharing_days_keep_only_one() {
    // "Versions with one validity and shared days keep only one".
    let (merged, diagnostics) = merge_in(
        2027,
        vec![day_type("b1", "X"), day_type("b2", "1")],
        &Registry::empty(),
    );
    assert_eq!(merged.versions.len(), 1);
    assert_eq!(&*merged.versions[0].batch, "b1");
    assert_eq!(count(&diagnostics, "jdf_version_superseded"), 1);
}

fn keyed(created: Date, detour: bool, from: Date, to: Date) -> FixedBatch {
    batch(&format!("b-{from}-{to}-{detour}"))
        .created(created)
        .stop(stop("Zastávka", None, None))
        .route(LICENCE, from, Some(to), detour)
        .trip(LICENCE, 1, &[])
        .build()
}

#[test]
fn r248_r254_trip_keys_name_the_published_schedule_in_any_order() {
    // "Trip version keys name the published schedule in any merge order": the regular version
    // keeps January–March and July–November under one key, its trip running on both.
    let regular = || keyed(d(1, 1), false, d(1, 1), END);
    let detour = || keyed(d(2, 1), true, d(4, 1), d(6, 30));
    let forward = merge(vec![regular(), detour()]);
    let backward = merge(vec![detour(), regular()]);
    assert_eq!(forward, backward);
    let keys: Vec<_> = forward
        .versions
        .iter()
        .map(|v| {
            (
                v.key.valid_from,
                v.key.detour,
                v.key.hash.clone(),
                v.key.part,
            )
        })
        .collect();
    assert_eq!(
        keys,
        [(d(1, 1), false, None, None), (d(4, 1), true, None, None)]
    );
    let regular = &forward.versions[0];
    assert_eq!(regular.won, [(d(1, 1), d(3, 31)), (d(7, 1), END)]);
    let days = regular.trips[0].days;
    assert_eq!(days.count(), 90 + 153);
}

#[test]
fn r246_r254_versions_starting_on_one_day_get_a_hashed_key() {
    // "Versions starting on one day get a hashed trip version key": the newer short version
    // takes January–March; the longer one keeps April on and, ending later, is hashed.
    let merged = merge(vec![
        keyed(d(1, 2), false, d(1, 1), d(3, 31)),
        keyed(d(1, 1), false, d(1, 1), END),
    ]);
    let rows: Vec<_> = merged
        .versions
        .iter()
        .map(|v| (v.won[0].0, v.key.hash.clone()))
        .collect();
    assert_eq!(rows.len(), 2);
    let short = rows.iter().find(|r| r.0 == d(1, 1)).expect("short");
    let long = rows.iter().find(|r| r.0 == d(4, 1)).expect("long");
    assert_eq!(short.1, None);
    // SHA-256 of "270101|271130", first four bytes; the export date is not hashed.
    assert_eq!(long.1.as_deref(), Some("25449a9a"));
}

#[test]
fn r249_validity_is_bounded_to_the_timetable_year_from_the_reference_date() {
    // "Validity is bounded to the current GVD from the reference date".
    let batches = vec![
        version("b1", false, date(2025, 6, 1), Some(date(2026, 9, 28))),
        version("b2", false, date(2026, 9, 29), Some(date(2099, 12, 31))),
        version("b3", true, date(2026, 12, 13), Some(date(2027, 6, 30))),
    ];
    let (merged, diagnostics) = merge_at(2026, date(2026, 9, 29), batches, &Registry::empty());
    assert_eq!(
        won(&merged),
        [(false, vec![(date(2026, 9, 29), date(2026, 12, 12))])]
    );
    assert_eq!(count(&diagnostics, "jdf_version_expired"), 1);
    assert_eq!(count(&diagnostics, "jdf_version_next_gvd"), 1);
    assert_eq!(count(&diagnostics, "jdf_version_clamped"), 1);
}

#[test]
fn r249_past_starts_are_clamped_to_the_year_start_and_open_ends_to_its_end() {
    // "Past starts are clamped to the GVD start"; an open end (R030) runs to the year's end.
    let (merged, _) = merge_at(
        2026,
        date(2026, 1, 10),
        vec![version("b1", false, date(2024, 12, 15), None)],
        &Registry::empty(),
    );
    assert_eq!(
        won(&merged),
        [(false, vec![(date(2025, 12, 14), date(2026, 12, 12))])]
    );
}

#[test]
fn r248_an_undated_note_follows_each_won_interval() {
    // On an F# cut version an undated "does not run" note covered each chunk's first day.
    let regular = batch("a")
        .created(d(1, 1))
        .stop(stop("Zastávka", None, None))
        .route(LICENCE, d(1, 1), Some(END), false)
        .trip(LICENCE, 1, &[])
        .note(LICENCE, 1, NoteType::NoService, None, None)
        .build();
    let detour = keyed(d(1, 1), true, d(4, 1), d(6, 30));
    let merged = merge(vec![regular, detour]);
    let regular = merged
        .versions
        .iter()
        .find(|v| !v.route.detour)
        .expect("regular");
    let days: Vec<Date> = regular.trips[0].days.dates(merged.horizon).collect();
    assert!(!days.contains(&d(1, 1)) && !days.contains(&d(7, 1)));
    assert!(days.contains(&d(1, 2)) && days.contains(&d(7, 2)));
}

#[test]
fn r267_a_trip_that_never_runs_is_dropped_and_its_version_kept() {
    let closed = batch("a")
        .stop(stop("Zastávka", None, None))
        .route(LICENCE, d(1, 1), Some(END), false)
        .trip(LICENCE, 1, &[])
        .note(LICENCE, 1, NoteType::NoService, Some(d(1, 1)), Some(END))
        .build();
    let (merged, diagnostics) = merge_in(2027, vec![closed], &Registry::empty());
    assert_eq!(merged.versions.len(), 1);
    assert!(merged.versions[0].trips.is_empty());
    assert_eq!(count(&diagnostics, "jdf_trip_never_runs"), 1);
}

#[test]
fn r213_r241_trips_carry_codes_and_calls_refer_to_merged_stops() {
    let merged = merge(vec![
        batch("a")
            .stop(stop("Alfa", None, None))
            .stop(stop("Beta", None, None))
            .route(LICENCE, d(1, 1), Some(END), false)
            .trip(LICENCE, 1, &["X", "@"])
            .build(),
    ]);
    let trip = &merged.versions[0].trips[0];
    assert_eq!(
        merged.code_sets.get(trip.codes),
        [Box::from("X"), Box::from("@")]
    );
    let called: Vec<String> = trip
        .calls
        .iter()
        .map(|c| merged.stops[c.stop as usize].name.display_name())
        .collect();
    assert_eq!(called, ["Alfa", "Beta"]);
}

#[test]
fn r238_equal_agencies_are_one_and_differing_ones_numbered() {
    let with_phone = |name: &str, licence: &str, phone: &str| {
        batch(name)
            .agency_phone(phone)
            .stop(stop("Zastávka", None, None))
            .route(licence, d(1, 1), Some(END), false)
            .build()
    };
    let merged = merge(vec![
        with_phone("a", "100001", "1"),
        with_phone("b", "100002", "1"),
        with_phone("c", "100003", "2"),
    ]);
    let distinctions: Vec<u32> = merged.agencies.iter().map(|a| a.distinction).collect();
    assert_eq!(distinctions, [1, 2]);
    let agencies: Vec<usize> = merged.versions.iter().map(|v| v.agency).collect();
    assert_eq!(agencies, [0, 0, 1]);
}

#[test]
fn r240_a_version_keeps_its_batch_position_and_creation_date() {
    let merged = merge(vec![version("b1", false, d(1, 1), Some(END))]);
    let v = &merged.versions[0];
    assert_eq!((&*v.batch, v.position, v.created), ("b1", 0, Some(d(1, 1))));
    assert_eq!(v.route.timetable_valid_to, Some(END));
}

#[test]
fn r242_a_grouped_version_trip_without_a_group_is_diagnosed() {
    let grouped = batch("a")
        .stop(stop("Zastávka", None, None))
        .route(LICENCE, d(1, 1), Some(END), false)
        .grouped()
        .trip(LICENCE, 1, &[])
        .build();
    let (merged, diagnostics) = merge_in(2027, vec![grouped], &Registry::empty());
    assert_eq!(merged.versions[0].trips[0].group, None);
    assert_eq!(count(&diagnostics, "jdf_trip_group_missing"), 1);
}

#[test]
fn r285_rows_with_unresolvable_references_are_dropped() {
    let mut broken = version("a", false, d(1, 1), Some(END));
    broken.batch.trip_stops[0].trip_id = 99;
    broken.batch.route_stops[0].stop_id = 42;
    let (merged, diagnostics) = merge_in(2027, vec![broken], &Registry::empty());
    assert!(merged.versions[0].trips[0].calls.is_empty());
    assert!(merged.versions[0].route_stops.is_empty());
    assert_eq!(count(&diagnostics, "jdf_reference_unknown"), 2);
}

#[test]
fn r247_the_result_does_not_depend_on_the_input_order_of_batches() {
    // Batches are taken in name order (verdicts), whatever order they come in.
    let batches = || {
        vec![
            version("b1", false, d(8, 29), Some(END)),
            version("b2", true, d(9, 12), Some(END)),
            version("b3", false, d(9, 26), Some(END)),
        ]
    };
    let mut reversed = batches();
    reversed.reverse();
    assert_eq!(merge(batches()), merge(reversed));
}
