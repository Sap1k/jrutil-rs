//! The canonical comparison and the parity ledger on tiny packages written here.

// Shared with the validator tests; not every helper is used here.
#[allow(dead_code)]
mod support;

use jiff::civil::date;
use jr_publish::compare::{Sample, Status, TripIdentity};
use jr_publish::{Comparison, Ledger, Package, Profile, compare, compare_classified};
use support::{Cell, Fixture, Row, b, d, i, row, s};

fn open(fixture: &Fixture, name: &str) -> Package {
    Package::open(&fixture.write(name)).expect("open")
}

fn diff(name: &str, a: &Fixture, b: &Fixture) -> Comparison {
    let pa = open(a, &format!("compare-{name}-a"));
    let pb = open(b, &format!("compare-{name}-b"));
    compare(&pa, &pb, &Profile::default()).expect("compare")
}

/// Category -> count.
fn counts(comparison: &Comparison) -> Vec<(&str, u64)> {
    comparison
        .categories
        .iter()
        .map(|(name, diffs)| (name.as_str(), diffs.count))
        .collect()
}

fn set(rows: &mut [Row], index: usize, field: &str, cell: Cell) {
    rows[index].insert(field.to_owned(), cell);
}

/// The fixture's service, made short: 2026-01-05..=2026-01-16 on weekdays without 01-07.
fn short_service() -> Fixture {
    let mut fixture = Fixture::valid();
    set(
        fixture.rows_mut("service_calendar"),
        0,
        "valid_to",
        d(date(2026, 1, 16)),
    );
    set(
        fixture.rows_mut("service_exception"),
        0,
        "service_date",
        d(date(2026, 1, 7)),
    );
    fixture
}

/// The same dates as `short_service` under another id, split differently between the
/// calendar and its exceptions.
fn short_service_split(id: &str) -> Fixture {
    let mut fixture = short_service();
    *fixture.rows_mut("service_calendar") = vec![row(&[
        ("service_id", s(id)),
        ("valid_from", d(date(2026, 1, 8))),
        ("valid_to", d(date(2026, 1, 16))),
        ("weekday_mask", i(31)),
    ])];
    *fixture.rows_mut("service_exception") = [5, 6]
        .into_iter()
        .map(|day| {
            row(&[
                ("service_id", s(id)),
                ("service_date", d(date(2026, 1, day))),
                ("added", b(true)),
            ])
        })
        .collect();
    set(fixture.rows_mut("trip"), 0, "service_id", s(id));
    fixture
}

fn with_shape(id: &str, offset: f64) -> Fixture {
    let mut fixture = Fixture::valid();
    *fixture.rows_mut("shape") = vec![row(&[
        ("shape_id", s(id)),
        ("generation_method", s("routed")),
    ])];
    *fixture.rows_mut("shape_point") = (0..3)
        .map(|n| {
            row(&[
                ("shape_id", s(id)),
                // Sequence numbers are not content: only their order is.
                ("sequence", i(10 + n)),
                ("longitude", Cell::Double(14.4 + n as f64 * 0.01 + offset)),
                ("latitude", Cell::Double(50.0 + n as f64 * 0.01)),
            ])
        })
        .collect();
    set(fixture.rows_mut("trip"), 0, "shape_id", s(id));
    fixture
}

fn transfer(key: &str) -> Row {
    row(&[
        ("transfer_key", s(key)),
        ("from_location_id", s("jdf:stop:1")),
        ("to_location_id", s("jdf:stop:2")),
        ("transfer_type", i(2)),
        ("minimum_transfer_time", i(180)),
    ])
}

#[test]
fn identical_packages_have_no_differences() {
    let comparison = diff("identical", &Fixture::valid(), &Fixture::valid());
    assert_eq!(counts(&comparison), []);
    let trip_call = comparison
        .relations
        .iter()
        .find(|relation| relation.name == "trip_call");
    assert_eq!(trip_call.map(|relation| relation.rows), Some([2, 2]));
}

#[test]
fn one_changed_cell_is_one_changed_difference_with_its_sample() {
    let mut candidate = Fixture::valid();
    set(
        candidate.rows_mut("trip_call"),
        1,
        "scheduled_arrival",
        i(90_060),
    );
    let comparison = diff("changed-cell", &Fixture::valid(), &candidate);
    assert_eq!(
        counts(&comparison),
        [("trip_call.changed.scheduled_arrival", 1)]
    );
    let bucket = &comparison.categories["trip_call.changed.scheduled_arrival"].groups[&None];
    assert_eq!(
        bucket.samples,
        [Sample {
            key: "jdf:trip:1|2".into(),
            column: Some("scheduled_arrival".into()),
            a: Some("90000".into()),
            b: Some("90060".into()),
        }]
    );
}

#[test]
fn a_row_only_in_one_package_is_missing_or_extra() {
    let mut candidate = Fixture::valid();
    set(
        candidate.rows_mut("location"),
        2,
        "location_id",
        s("jdf:stop:3"),
    );
    let comparison = diff("missing-extra", &Fixture::valid(), &candidate);
    assert_eq!(
        counts(&comparison),
        [("location.extra", 1), ("location.missing", 1)]
    );
    assert_eq!(
        comparison.categories["location.missing"].groups[&None].samples[0].key,
        "jdf:stop:2"
    );
}

#[test]
fn services_with_equal_dates_are_equal_whatever_their_ids_and_split() {
    let comparison = diff(
        "service-split",
        &short_service(),
        &short_service_split("jdf:service:other"),
    );
    assert_eq!(counts(&comparison), []);
}

#[test]
fn a_service_with_one_date_changed_changes_the_referencing_column() {
    let mut candidate = short_service_split("jdf:service:other");
    set(
        candidate.rows_mut("service_exception"),
        1,
        "service_date",
        d(date(2026, 1, 7)),
    );
    let comparison = diff("service-date", &short_service(), &candidate);
    assert_eq!(counts(&comparison), [("trip.changed.service_id", 1)]);
    let sample = &comparison.categories["trip.changed.service_id"].groups[&None].samples[0];
    assert_eq!(
        sample.b.as_deref(),
        Some("jdf:service:other {9 dates 2026-01-05..2026-01-16} +2026-01-07 -2026-01-06")
    );
}

#[test]
fn shapes_compare_by_their_points_within_the_coordinate_tolerance() {
    let reference = with_shape("jdf:shape:1", 0.0);
    let close = diff("shape-close", &reference, &with_shape("jdf:shape:9", 5e-8));
    assert_eq!(counts(&close), []);
    let far = diff("shape-far", &reference, &with_shape("jdf:shape:9", 1e-5));
    assert_eq!(counts(&far), [("trip.changed.shape_id", 1)]);
}

#[test]
fn a_relation_with_a_generated_key_is_a_multiset_of_its_rows() {
    let mut reference = Fixture::valid();
    *reference.rows_mut("transfer") = vec![transfer("t:1"), transfer("t:2")];
    let mut candidate = Fixture::valid();
    *candidate.rows_mut("transfer") = vec![transfer("other:7")];
    let comparison = diff("multiset", &reference, &candidate);
    assert_eq!(counts(&comparison), [("transfer.missing", 1)]);
    assert_eq!(
        comparison.categories["transfer.missing"].groups[&None].samples[0].key,
        "jdf:stop:1|jdf:stop:2|null|null|null|null|2|180|null"
    );
}

/// Arrival changed at both calls, a stop renamed.
fn ledger_case(ledger: &Ledger, name: &str) -> jr_publish::Verdict {
    let mut candidate = Fixture::valid();
    set(
        candidate.rows_mut("trip_call"),
        0,
        "scheduled_arrival",
        i(25_260),
    );
    set(
        candidate.rows_mut("trip_call"),
        1,
        "scheduled_arrival",
        i(90_060),
    );
    set(candidate.rows_mut("location"), 2, "name", s("Dolní Horní"));
    let a = open(&Fixture::valid(), &format!("ledger-{name}-a"));
    let b = open(&candidate, &format!("ledger-{name}-b"));
    let comparison = compare_classified(&a, &b, &Profile::default(), ledger).expect("compare");
    ledger.judge(&comparison)
}

const LEDGER: &str = r#"
[[category]]
name = "first-call"
matches = "trip_call.changed.scheduled_arrival"
reason = "The first call moved."
key = '\|1$'
max = 1

[[category]]
name = "also-first-call"
matches = "trip_call.changed.scheduled_arrival"
reason = "Shadowed by first-call."
key = '\|1$'

[[category]]
name = "late-arrivals"
matches = "trip_call.changed.scheduled_arrival"
reason = "Arrivals after midnight moved."
a = '^9'
b = '^90060$'
max = MAX

[[category]]
name = "pickups"
matches = "trip_call.changed.pickup_type"
reason = "Not seen here."
"#;

#[test]
fn the_ledger_assigns_in_file_order_through_filters_and_reports_what_is_left() {
    let ledger = Ledger::parse(&LEDGER.replace("MAX", "1")).expect("ledger");
    let verdict = ledger_case(&ledger, "in-range");
    let statuses: Vec<(&str, u64, Status)> = verdict
        .categories
        .iter()
        .map(|c| (c.name.as_str(), c.count, c.status))
        .collect();
    assert_eq!(
        statuses,
        [
            ("first-call", 1, Status::Explained),
            ("also-first-call", 0, Status::Stale),
            ("late-arrivals", 1, Status::Explained),
            ("pickups", 0, Status::Stale),
        ]
    );
    let defaults: Vec<(&str, u64, Status)> = verdict
        .defaults
        .iter()
        .map(|d| (d.category.as_str(), d.unexplained, d.status))
        .collect();
    assert_eq!(
        defaults,
        [
            ("location.changed.name", 1, Status::Unexplained),
            ("trip_call.changed.scheduled_arrival", 0, Status::Explained),
        ]
    );
    assert!(!verdict.passed());
}

#[test]
fn a_label_column_of_the_ledger_is_not_compared_and_must_not_be_a_key() {
    let label = |column: &str| {
        format!(
            "[[label]]
relation = \"location\"
column = \"{column}\"
reason = \"worded differently\"
"
        )
    };
    assert!(Ledger::parse(&label("location_id")).is_err());
    assert!(Ledger::parse(&label("parent_location_id")).is_err());
    assert!(Ledger::parse(&label("nonsense")).is_err());
    let ledger = Ledger::parse(&label("description")).expect("ledger");

    let mut candidate = Fixture::valid();
    set(
        candidate.rows_mut("location"),
        2,
        "description",
        s("from B"),
    );
    let a = open(&Fixture::valid(), "label-a");
    let b = open(&candidate, "label-b");
    let plain = compare(&a, &b, &Profile::default()).expect("compare");
    assert_eq!(counts(&plain), [("location.changed.description", 1)]);
    let profile = Profile {
        labels: vec![(
            ledger.labels[0].relation.clone(),
            ledger.labels[0].column.clone(),
        )],
        ..Profile::default()
    };
    assert_eq!(counts(&compare(&a, &b, &profile).expect("compare")), []);
}

#[test]
fn a_ledger_count_outside_its_range_fails() {
    let ledger = Ledger::parse(&LEDGER.replace("MAX", "0")).expect("ledger");
    let verdict = ledger_case(&ledger, "out-of-range");
    assert_eq!(verdict.categories[2].status, Status::OutOfRange);
    assert_eq!(verdict.defaults[1].status, Status::OutOfRange);
}

#[test]
fn a_ledger_category_needs_a_reason_and_a_real_category() {
    let entry = |matches: &str, reason: &str| {
        format!("[[category]]\nname = \"x\"\nmatches = \"{matches}\"\nreason = \"{reason}\"\n")
    };
    assert!(Ledger::parse(&entry("trip.changed.headsign", "why")).is_ok());
    assert!(Ledger::parse(&entry("trip.changed.headsign", " ")).is_err());
    assert!(Ledger::parse(&entry("trip.changed.nonsense", "why")).is_err());
    assert!(Ledger::parse(&entry("trips.missing", "why")).is_err());
    assert!(Ledger::parse(&format!("{}maximum = 3\n", entry("trip.extra", "why"))).is_err());
}

fn journey_profile() -> Profile {
    Profile {
        trip_identity: TripIdentity::Journey,
        ..Profile::default()
    }
}

fn diff_journeys(name: &str, a: &Fixture, b: &Fixture) -> Comparison {
    let pa = open(a, &format!("journey-{name}-a"));
    let pb = open(b, &format!("journey-{name}-b"));
    compare(&pa, &pb, &journey_profile()).expect("compare")
}

/// The fixture's trip with an assignment and its source key, all under `trip`.
fn trip_named(trip: &str) -> Fixture {
    let mut fixture = Fixture::valid();
    set(fixture.rows_mut("trip"), 0, "trip_id", s(trip));
    for call in fixture.rows_mut("trip_call") {
        call.insert("trip_id".into(), s(trip));
    }
    set(fixture.rows_mut("source_key"), 0, "public_id", s(trip));
    *fixture.rows_mut("assignment") = vec![row(&[
        ("assignment_id", s(&format!("{trip}:wheelchair"))),
        ("scope", s("trip")),
        ("kind", s("wheelchair_accessible_vehicle")),
        ("trip_id", s(trip)),
    ])];
    fixture
}

/// Another trip of the fixture's route, `id`, on `service`, arriving at `arrival`.
fn add_trip(fixture: &mut Fixture, id: &str, service: &str, arrival: i64) {
    let mut trip = fixture.rows_mut("trip")[0].clone();
    trip.insert("trip_id".into(), s(id));
    trip.insert("service_id".into(), s(service));
    fixture.rows_mut("trip").push(trip);
    let mut calls = fixture.rows_mut("trip_call")[..2].to_vec();
    for call in &mut calls {
        call.insert("trip_id".into(), s(id));
    }
    calls[1].insert("scheduled_arrival".into(), i(arrival));
    fixture.rows_mut("trip_call").extend(calls);
}

#[test]
fn a_renamed_trip_with_equal_content_pairs_with_everything_that_names_it() {
    let comparison = diff_journeys(
        "renamed",
        &trip_named("jdf:trip:1"),
        &trip_named("overlay:x:trip:9"),
    );
    assert_eq!(counts(&comparison), []);
    let pairing = comparison.trip_pairing.expect("journey pairing");
    assert_eq!(
        (
            pairing.by_id,
            pairing.by_content,
            pairing.near,
            pairing.unpaired
        ),
        (0, 1, 0, [0, 0])
    );
}

#[test]
fn under_the_default_identity_a_renamed_trip_is_missing_and_extra() {
    let a = open(&trip_named("jdf:trip:1"), "journey-default-a");
    let b = open(&trip_named("overlay:x:trip:9"), "journey-default-b");
    let comparison = compare(&a, &b, &Profile::default()).expect("compare");
    assert_eq!(
        counts(&comparison),
        [
            ("assignment.extra", 1),
            ("assignment.missing", 1),
            ("source_key.extra", 1),
            ("source_key.missing", 1),
            ("trip.extra", 1),
            ("trip.missing", 1),
            ("trip_call.extra", 2),
            ("trip_call.missing", 2),
        ]
    );
    assert_eq!(comparison.trip_pairing, None);
}

#[test]
fn a_renamed_trip_with_one_changed_time_is_a_near_pair() {
    let mut candidate = trip_named("overlay:x:trip:9");
    set(
        candidate.rows_mut("trip_call"),
        1,
        "scheduled_arrival",
        i(90_060),
    );
    let comparison = diff_journeys("near", &trip_named("jdf:trip:1"), &candidate);
    assert_eq!(
        counts(&comparison),
        [("trip_call.changed.scheduled_arrival", 1)]
    );
    let bucket = &comparison.categories["trip_call.changed.scheduled_arrival"].groups[&None];
    assert_eq!(bucket.samples[0].key, "jdf:trip:1 → overlay:x:trip:9|2");
    assert_eq!(comparison.trip_pairing.map(|p| p.near), Some(1));
}

#[test]
fn a_near_pair_needs_a_single_candidate() {
    let mut candidate = trip_named("overlay:x:trip:8");
    set(
        candidate.rows_mut("trip_call"),
        1,
        "scheduled_arrival",
        i(90_060),
    );
    *candidate.rows_mut("assignment") = Vec::new();
    add_trip(&mut candidate, "overlay:x:trip:9", "jdf:service:1", 90_120);
    let mut reference = trip_named("jdf:trip:1");
    *reference.rows_mut("assignment") = Vec::new();
    let comparison = diff_journeys("two-candidates", &reference, &candidate);
    assert_eq!(
        counts(&comparison),
        [
            ("source_key.extra", 1),
            ("source_key.missing", 1),
            ("trip.extra", 2),
            ("trip.missing", 1),
            ("trip_call.extra", 4),
            ("trip_call.missing", 2),
        ]
    );
}

#[test]
fn a_trip_split_evenly_in_two_stays_unpaired_and_is_described() {
    // Two weeks as one trip, and as one trip per week: each week shares five dates with it.
    let comparison = diff_journeys(
        "split",
        &slices(&[("jdf:trip:1", [1, 5], [1, 16])]),
        &slices(&[
            ("overlay:x:trip:1", [1, 5], [1, 9]),
            ("overlay:x:trip:2", [1, 12], [1, 16]),
        ]),
    );
    assert_eq!(
        counts(&comparison),
        [
            ("trip.extra", 2),
            ("trip.missing", 1),
            ("trip_call.extra", 4),
            ("trip_call.missing", 2),
        ]
    );
    assert_eq!(
        comparison.categories["trip.missing"].groups[&None].samples[0].key,
        "jdf:trip:1 | jdf:route:1 | 2026-01-05..2026-01-16 (10 days) \
         | jdf:stop:1@07:00:00 → jdf:stop:2@25:00:00 | 2 calls"
    );
    assert_eq!(
        comparison.categories["trip.extra"].groups[&None].samples[1].key,
        "overlay:x:trip:2 | jdf:route:1 | 2026-01-12..2026-01-16 (5 days) \
         | jdf:stop:1@07:00:00 → jdf:stop:2@25:00:00 | 2 calls"
    );
}

#[test]
fn the_ledger_chooses_the_trip_identity() {
    let parse = |text: &str| Ledger::parse(text).map(|ledger| ledger.trip_identity);
    assert_eq!(parse("").expect("ledger"), TripIdentity::TripId);
    assert_eq!(
        parse("trip_identity = \"trip_id\"\n").expect("ledger"),
        TripIdentity::TripId
    );
    assert_eq!(
        parse("trip_identity = \"journey\"\n").expect("ledger"),
        TripIdentity::Journey
    );
    assert!(parse("trip_identity = \"source_key\"\n").is_err());
}

/// The fixture's trip as slices `(trip, from, to)`: each runs on the weekdays of
/// 2026-`from`..=2026-`to` (month, day) on a service of its own, with the fixture's calls.
fn slices(trips: &[(&str, [i8; 2], [i8; 2])]) -> Fixture {
    let template = Fixture::valid();
    let mut fixture = Fixture::valid();
    *fixture.rows_mut("service_exception") = Vec::new();
    *fixture.rows_mut("source_key") = Vec::new();
    let (mut calendars, mut trip_rows, mut calls) = (Vec::new(), Vec::new(), Vec::new());
    for (id, from, to) in trips {
        let service = format!("{id}:service");
        calendars.push(row(&[
            ("service_id", s(&service)),
            ("valid_from", d(date(2026, from[0], from[1]))),
            ("valid_to", d(date(2026, to[0], to[1]))),
            ("weekday_mask", i(31)),
        ]));
        let mut trip = template.relations["trip"][0].clone();
        trip.insert("trip_id".into(), s(id));
        trip.insert("service_id".into(), s(&service));
        trip_rows.push(trip);
        for call in &template.relations["trip_call"] {
            let mut call = call.clone();
            call.insert("trip_id".into(), s(id));
            calls.push(call);
        }
    }
    *fixture.rows_mut("service_calendar") = calendars;
    *fixture.rows_mut("trip") = trip_rows;
    *fixture.rows_mut("trip_call") = calls;
    fixture
}

#[test]
fn a_renamed_trip_with_two_more_dates_pairs_by_dates() {
    let comparison = diff_journeys(
        "by-dates",
        &slices(&[("jdf:trip:1", [1, 5], [1, 16])]),
        &slices(&[("overlay:x:trip:9", [1, 5], [1, 20])]),
    );
    assert_eq!(counts(&comparison), [("trip.changed.service_id", 1)]);
    let sample = &comparison.categories["trip.changed.service_id"].groups[&None].samples[0];
    assert_eq!(sample.key, "jdf:trip:1 → overlay:x:trip:9");
    assert!(
        sample
            .b
            .as_deref()
            .is_some_and(|b| b.ends_with("+2026-01-19 +2026-01-20"))
    );
    assert_eq!(comparison.trip_pairing.map(|p| p.by_dates), Some(1));
}

#[test]
fn candidates_with_equal_overlap_pair_neither() {
    let comparison = diff_journeys(
        "dates-tie",
        &slices(&[("jdf:trip:1", [1, 5], [1, 16])]),
        &slices(&[
            ("overlay:x:trip:8", [1, 5], [1, 19]),
            ("overlay:x:trip:9", [1, 2], [1, 16]),
        ]),
    );
    assert_eq!(
        counts(&comparison),
        [
            ("trip.extra", 2),
            ("trip.missing", 1),
            ("trip_call.extra", 4),
            ("trip_call.missing", 2),
        ]
    );
}

#[test]
fn a_candidate_without_a_common_date_does_not_pair() {
    let comparison = diff_journeys(
        "dates-disjoint",
        &slices(&[("jdf:trip:1", [1, 5], [1, 16])]),
        &slices(&[("overlay:x:trip:9", [2, 2], [2, 13])]),
    );
    assert_eq!(
        counts(&comparison),
        [
            ("trip.extra", 1),
            ("trip.missing", 1),
            ("trip_call.extra", 2),
            ("trip_call.missing", 2),
        ]
    );
}

#[test]
fn slices_with_shifted_boundaries_pair_with_their_largest_overlap() {
    let comparison = diff_journeys(
        "dates-shifted",
        &slices(&[
            ("jdf:trip:a1", [1, 5], [1, 16]),
            ("jdf:trip:a2", [1, 19], [1, 30]),
        ]),
        &slices(&[
            ("overlay:x:trip:b1", [1, 5], [1, 20]),
            ("overlay:x:trip:b2", [1, 21], [1, 30]),
        ]),
    );
    assert_eq!(counts(&comparison), [("trip.changed.service_id", 2)]);
    let keys: Vec<&str> = comparison.categories["trip.changed.service_id"].groups[&None]
        .samples
        .iter()
        .map(|sample| sample.key.as_str())
        .collect();
    assert_eq!(
        keys,
        [
            "jdf:trip:a1 → overlay:x:trip:b1",
            "jdf:trip:a2 → overlay:x:trip:b2"
        ]
    );
}
