//! The `nad_connections` pass (R083) and an end-to-end well-formedness check.

mod support;

use jr_czptt::Policy;
use jr_czptt::connections::{PASS, nad_connections};
use jr_model::timetable::{LocationRef, TransferKind};
use jr_model::{Diagnostics, Precedence, Timetable, apply};
use support::*;

fn connect(messages: Vec<jr_czptt::SourceMessage>) -> (Timetable, usize) {
    let (compiled, _) = compile_timetable(messages);
    let mut timetable = compiled.timetable;
    let before = timetable.transfers.len();
    let mut diagnostics = Diagnostics::new();
    let edits = nad_connections(&timetable, &Policy::builtin(), &mut diagnostics);
    assert_eq!(edits.pass(), PASS);
    apply(
        &mut timetable,
        edits,
        &Precedence {
            position: Vec::new(),
        },
        &mut diagnostics,
    )
    .expect("applies");
    (timetable, before)
}

/// `(from trip short name, from point, to trip short name, to point)` of the pass's transfers.
fn added(timetable: &Timetable, before: usize) -> Vec<(String, String, String, String)> {
    let name = |trip| {
        text(timetable, timetable.trips[trip].short_name)
            .unwrap_or("")
            .to_owned()
    };
    let point = |location| match location {
        Some(LocationRef::BoardingPoint(point)) => timetable.boarding_points[point].id.to_string(),
        other => format!("{other:?}"),
    };
    timetable.transfers[before..]
        .iter()
        .map(|t| {
            assert_eq!((t.kind, t.minimum_time), (TransferKind::Timed, Some(0)));
            (
                name(t.from_trip.expect("trip")),
                point(t.from),
                name(t.to_trip.expect("trip")),
                point(t.to),
            )
        })
        .collect()
}

fn connection(
    from: &str,
    from_point: &str,
    to: &str,
    to_point: &str,
) -> (String, String, String, String) {
    (from.into(), from_point.into(), to.into(), to_point.into())
}

#[test]
fn r083_a_separate_bus_meets_the_train_of_its_line() {
    let train = numbered(
        support::timetable(vec![
            line(stop("57076", "20:30:00"), "101"),
            line(stop("53001", "20:42:00"), "101"),
            line(stop("53002", "20:52:00"), "101"),
        ]),
        "000000006562",
        "06562",
    );
    let nad = numbered(
        support::timetable(vec![
            bus(line(stop("53001", "20:47:00"), "101"), "1"),
            bus(line(stop("53003", "20:58:00"), "101"), "1"),
        ]),
        "000000016162",
        "16162",
    );
    let (timetable, before) = connect(vec![annual(1, nad), annual(2, train)]);
    assert_eq!(
        added(&timetable, before),
        [connection(
            "Vlak 06562",
            "czptt:stop:CZ:53001:unspecified",
            "Vlak 16162",
            "czptt:stop:CZ:53001:platform:BUS"
        )]
    );
}

#[test]
fn r083_the_train_number_offset_wins_over_a_closer_train_of_the_line() {
    let train = numbered(
        support::timetable(vec![
            line(stop("57076", "07:00:00"), "101"),
            line(stop("53001", "07:41:00"), "101"),
        ]),
        "000000006802",
        "6802",
    );
    let closer = numbered(
        support::timetable(vec![
            line(stop("57076", "07:10:00"), "101"),
            line(stop("53001", "07:44:00"), "101"),
        ]),
        "000000009999",
        "9999",
    );
    let nad = numbered(
        support::timetable(vec![
            bus(line(stop("53001", "07:46:00"), "101"), "1"),
            bus(line(stop("53003", "07:58:00"), "101"), "1"),
        ]),
        "000000306802",
        "306802",
    );
    let (timetable, before) = connect(vec![annual(1, train), annual(2, closer), annual(3, nad)]);
    let found = added(&timetable, before);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, "Vlak 6802");
}

#[test]
fn r083_disjoint_calendar_variants_each_connect() {
    let train = |core: &str, bitmap: &str| {
        let mut message = numbered(
            support::timetable(vec![stop("57076", "07:00:00"), stop("53001", "07:41:00")]),
            core,
            "6802",
        );
        message.calendar = calendar(
            jiff::civil::date(2025, 12, 14),
            Some(jiff::civil::date(2025, 12, 15)),
            bitmap,
        );
        message
    };
    let mut nad = numbered(
        support::timetable(vec![
            bus(stop("53001", "07:46:00"), "1"),
            bus(stop("53003", "07:58:00"), "1"),
        ]),
        "000000306802",
        "306802",
    );
    nad.calendar = calendar(
        jiff::civil::date(2025, 12, 14),
        Some(jiff::civil::date(2025, 12, 15)),
        "11",
    );
    let (timetable, before) = connect(vec![
        annual(1, train("000000006802", "10")),
        annual(2, train("000000106802", "01")),
        annual(3, nad),
    ]);
    assert_eq!(added(&timetable, before).len(), 2);
}

#[test]
fn r083_a_separate_bus_connects_onward() {
    let nad = numbered(
        support::timetable(vec![
            bus(line(stop("53003", "08:00:00"), "101"), "1"),
            bus(line(stop("53001", "08:10:00"), "101"), "1"),
        ]),
        "000000016163",
        "16163",
    );
    let train = numbered(
        support::timetable(vec![
            line(stop("53001", "08:15:00"), "101"),
            line(stop("53002", "08:22:00"), "101"),
        ]),
        "000000006563",
        "06563",
    );
    // A train leaving 11 minutes later is outside the window.
    let late = numbered(
        support::timetable(vec![
            line(stop("53001", "08:21:00"), "101"),
            line(stop("53002", "08:28:00"), "101"),
        ]),
        "000000006565",
        "06565",
    );
    let (timetable, before) = connect(vec![annual(1, nad), annual(2, train), annual(3, late)]);
    assert_eq!(
        added(&timetable, before),
        [connection(
            "Vlak 16163",
            "czptt:stop:CZ:53001:platform:BUS",
            "Vlak 06563",
            "czptt:stop:CZ:53001:unspecified"
        )]
    );
}

#[test]
fn a_path_with_rail_and_bus_parts_railway_points_notes_and_zones_is_well_formed() {
    let message = support::timetable(vec![
        support::point("93000", "07:50:00"),
        line(track(stop("57076", "08:00:00"), "1"), "101"),
        line(support::point("93001", "08:04:00"), "101"),
        bus(
            line(
                track(
                    location("57050", "Poříčany", Some("08:10:00"), &["0001", "0030"]),
                    "2",
                ),
                "101",
            ),
            "1",
        ),
        bus(
            line(
                untimed(track(support::point("93002", "08:15:00"), "9")),
                "101",
            ),
            "1",
        ),
        bus(line(track(stop("57016", "08:20:00"), "3"), "101"), "0"),
        line(support::point("93003", "08:40:00"), "112"),
        line(stop("54357", "09:00:00"), "112"),
        support::point("93004", "09:10:00"),
    ]);
    let message = root(message, "CZCentralPTTNote", "22|CZ57076||CZ54357||0|");
    let message = root(message, "CZNonCentralPTTNote", "CZ57050||CZ57016||Bus|||||");
    let message = root(message, "CZIPTS", "11|CZ57076||CZ57016||");
    // `compile_timetable` asserts `check()` is clean.
    let (compiled, _) = compile_timetable(vec![annual(1, message)]);
    let timetable = &compiled.timetable;
    assert_eq!(timetable.trips.len(), 3);
    assert!(timetable.trips.rows()[0].calls.iter().any(|c| !c.passenger));
    assert!(!timetable.notes.is_empty());
    assert!(!timetable.zone_sets.is_empty());
    assert_eq!(compiled.summary.paths_accepted, 1);
    assert_eq!(compiled.summary.trips, 3);
}
