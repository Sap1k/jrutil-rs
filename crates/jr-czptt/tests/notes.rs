//! Notes, features, trip flags, request stops and zones (R070–R079, R085, R086).

mod support;

use jr_model::Timetable;
use jr_model::timetable::{AssignmentKind, Feature, NoteKind, Scope, Tristate, Zone};
use support::*;

const CENTRAL: &str = "CZCentralPTTNote";

fn two_stops() -> jr_formats::czptt::TimetableMessage {
    support::timetable(vec![stop("57076", "08:00:00"), stop("57016", "08:20:00")])
}

fn features(timetable: &Timetable) -> Vec<(Feature, Scope)> {
    timetable
        .assignments
        .iter()
        .filter_map(|a| match a.kind {
            AssignmentKind::Feature(feature) => Some((feature, a.scope)),
            _ => None,
        })
        .collect()
}

fn feature_kinds(timetable: &Timetable) -> Vec<Feature> {
    features(timetable).into_iter().map(|(f, _)| f).collect()
}

fn note_scopes(timetable: &Timetable) -> Vec<Scope> {
    timetable
        .assignments
        .iter()
        .filter(|a| a.kind == AssignmentKind::Note)
        .map(|a| a.scope)
        .collect()
}

fn flags(timetable: &Timetable) -> (Tristate, Tristate) {
    let trip = &timetable.trips.rows()[0];
    (trip.wheelchair_accessible, trip.bikes_allowed)
}

#[test]
fn r070_r079_trip_flags_need_a_whole_part_claim() {
    let (timetable, _) = timetable_of(root(two_stops(), CENTRAL, "22|CZ57076||CZ57016||0|"));
    assert_eq!(flags(&timetable), (Tristate::Unknown, Tristate::Yes));
    let (timetable, _) = timetable_of(root(two_stops(), CENTRAL, "36|CZ57076||CZ57016||0|"));
    assert_eq!(flags(&timetable), (Tristate::Unknown, Tristate::No));
    let (timetable, _) = timetable_of(root(two_stops(), CENTRAL, "34|CZ57076||CZ57016||0|"));
    assert_eq!(flags(&timetable), (Tristate::Yes, Tristate::Unknown));

    // R079: contradictory claims stay unknown and are diagnosed.
    let both = root(
        root(two_stops(), CENTRAL, "22|CZ57076||CZ57016||0|"),
        CENTRAL,
        "36|CZ57076||CZ57016||0|",
    );
    let (timetable, diagnostics) = timetable_of(both);
    assert_eq!(flags(&timetable).1, Tristate::Unknown);
    assert!(has(&diagnostics, "czptt_conflicting_bicycle_notes"));

    // A replacement bus gets no flags.
    let bus_only = support::timetable(vec![
        bus(stop("57076", "08:00:00"), "1"),
        bus(stop("57016", "08:20:00"), "1"),
    ]);
    let (timetable, _) = timetable_of(root(bus_only, CENTRAL, "17|CZ57076||CZ57016||0|"));
    assert_eq!(flags(&timetable), (Tristate::Unknown, Tristate::Unknown));
    assert!(feature_kinds(&timetable).is_empty());
}

#[test]
fn r071_one_resolver_takes_first_matches_and_reads_occurrences_1_based() {
    let repeated = || {
        support::timetable(vec![
            stop("57076", "08:00:00"),
            stop("57016", "08:20:00"),
            stop("57076", "08:40:00"),
            stop("57017", "09:00:00"),
        ])
    };
    // A blank start is the first Praha: the whole trip.
    let (timetable, _) = timetable_of(root(repeated(), CENTRAL, "36|CZ57076||CZ57017||0|"));
    assert_eq!(flags(&timetable).1, Tristate::No);

    // Occurrence 1 is the second Praha (occurrences count from zero): calls 3 and 4 only.
    let (timetable, _) = timetable_of(root(repeated(), CENTRAL, "36|CZ57076|1|CZ57017||0|"));
    assert_eq!(flags(&timetable).1, Tristate::Unknown);
    let trip = timetable.trips.handles().next().expect("trip");
    assert_eq!(
        features(&timetable),
        [
            (
                Feature::BicycleTransportProhibited,
                Scope::Call { trip, sequence: 3 }
            ),
            (
                Feature::BicycleTransportProhibited,
                Scope::Call { trip, sequence: 4 }
            ),
        ]
    );

    // A blank end is the first match too: a station written as a passenger row and a following
    // non-passenger row ends the note at the passenger row.
    let doubled = support::timetable(vec![
        stop("57076", "08:00:00"),
        stop("57016", "08:20:00"),
        support::point("57016", "08:21:00"),
        stop("57017", "09:00:00"),
    ]);
    let (timetable, _) = timetable_of(root(doubled, CENTRAL, "44|CZ57076||CZ57016||0|"));
    let doubled_trip = timetable.trips.handles().next().expect("trip");
    assert_eq!(
        note_scopes(&timetable),
        [Scope::CallRange {
            trip: doubled_trip,
            from: 1,
            to: 2
        }]
    );

    // An end whose first match lies before the start takes the first match after it.
    let (timetable, _) = timetable_of(root(repeated(), CENTRAL, "44|CZ57016||CZ57076||0|"));
    assert_eq!(
        note_scopes(&timetable),
        [Scope::CallRange {
            trip,
            from: 2,
            to: 3
        }]
    );

    // The country prefix is honoured.
    let (timetable, diagnostics) =
        timetable_of(root(repeated(), CENTRAL, "36|AT57076||CZ57017||0|"));
    assert_eq!(note_scopes(&timetable), [Scope::Trip(trip)]);
    assert!(has(&diagnostics, "czptt_note_unresolved_interval"));
}

#[test]
fn r072_r075_a_calendar_limited_note_is_not_a_whole_trip_claim() {
    let mut message = root(two_stops(), CENTRAL, "22|CZ57076||CZ57016||0|1");
    message = root(message, "CZCalendarPTTNote", "1|20251214|20251215|10|");
    message.calendar = calendar(
        jiff::civil::date(2025, 12, 14),
        Some(jiff::civil::date(2025, 12, 15)),
        "11",
    );
    let (timetable, _) = timetable_of(message);
    assert_eq!(flags(&timetable).1, Tristate::Unknown);
    assert!(feature_kinds(&timetable).contains(&Feature::BicycleTransport));
    let notes = timetable.notes.rows();
    let central = &notes[0];
    assert_eq!(central.valid_from, Some(jiff::civil::date(2025, 12, 14)));
    assert_eq!(central.valid_to, Some(jiff::civil::date(2025, 12, 14)));
    let calendar_note = &notes[1];
    assert_eq!(calendar_note.kind, NoteKind::CzpttCalendar);
    assert_eq!(calendar_note.note_type.as_deref(), Some("1"));
    assert_eq!(
        (calendar_note.valid_from, calendar_note.valid_to),
        (
            Some(jiff::civil::date(2025, 12, 14)),
            Some(jiff::civil::date(2025, 12, 15))
        )
    );
    assert_eq!(
        calendar_note.id.as_str(),
        "czptt:note:Pa%3A54%3A000000001234%3A00%3A2026:calendar:1"
    );
}

#[test]
fn r073_a_central_note_is_kept_even_when_its_interval_does_not_resolve() {
    let (timetable, diagnostics) = timetable_of(root(two_stops(), CENTRAL, "36|missing"));
    let note = &timetable.notes.rows()[0];
    assert_eq!(note.kind, NoteKind::CzpttCentral);
    assert_eq!(note.text.as_deref(), Some("36|missing"));
    assert_eq!(note.label.as_deref(), Some("Bicycles prohibited"));
    assert_eq!(note.note_type.as_deref(), Some("36"));
    assert_eq!(note.source_object.as_ref(), note.id.as_str());
    let trip = timetable.trips.handles().next().expect("trip");
    assert_eq!(note_scopes(&timetable), [Scope::Trip(trip)]);
    assert_eq!(flags(&timetable).1, Tristate::Unknown);
    assert!(has(&diagnostics, "czptt_note_unresolved_interval"));
}

#[test]
fn r074_a_local_note_carries_its_own_label() {
    let message = root(
        support::timetable(vec![
            stop("57076", "08:00:00"),
            stop("57050", "08:10:00"),
            stop("57016", "08:20:00"),
        ]),
        "CZNonCentralPTTNote",
        "CZ57050||CZ57016||Výluka|||||",
    );
    let (timetable, _) = timetable_of(message);
    let note = &timetable.notes.rows()[0];
    assert_eq!(note.kind, NoteKind::CzpttLocal);
    assert_eq!(note.label.as_deref(), Some("Výluka"));
    let trip = timetable.trips.handles().next().expect("trip");
    assert_eq!(
        note_scopes(&timetable),
        [Scope::CallRange {
            trip,
            from: 2,
            to: 3
        }]
    );
}

#[test]
fn r076_notes_belong_to_accepted_paths_and_link_to_each_part_inside() {
    let rejected = root(
        numbered(
            support::timetable(vec![stop("57076", "08:10:00"), stop("57016", "08:05:00")]),
            "2",
            "2",
        ),
        CENTRAL,
        "36|CZ57076||CZ57016||0|",
    );
    let split = root(
        support::timetable(vec![
            commercial(stop("57076", "08:00:00"), "Os"),
            commercial(support::point("57040", "08:05:00"), "Os"),
            commercial(stop("57050", "08:10:00"), "RJ"),
            commercial(stop("57016", "08:20:00"), "RJ"),
        ]),
        CENTRAL,
        "44|CZ57040||CZ57016||0|",
    );
    let (compiled, _) = compile_timetable(vec![annual(1, split), annual(2, rejected)]);
    let timetable = &compiled.timetable;
    assert_eq!(timetable.notes.len(), 1);
    let trips: Vec<_> = timetable.trips.handles().collect();
    assert_eq!(
        note_scopes(timetable),
        [
            Scope::CallRange {
                trip: trips[0],
                from: 2,
                to: 3
            },
            Scope::CallRange {
                trip: trips[1],
                from: 3,
                to: 4
            },
        ]
    );
}

#[test]
fn r077_note_features_cover_whole_parts_or_single_calls_of_rail_parts() {
    let message = support::timetable(vec![
        stop("57076", "08:00:00"),
        stop("57050", "08:10:00"),
        bus(stop("57016", "08:20:00"), "1"),
        bus(stop("54357", "09:00:00"), "1"),
    ]);
    let (timetable, _) = timetable_of(root(message, CENTRAL, "26|CZ57076||CZ54357||0|"));
    let rail = timetable.trips.handles().next().expect("trip");
    let scopes: Vec<(Feature, Scope)> = features(&timetable);
    assert_eq!(
        scopes,
        [
            (Feature::BicycleTransport, Scope::Trip(rail)),
            (Feature::BicycleStorage, Scope::Trip(rail)),
            (Feature::BicycleReservationAvailable, Scope::Trip(rail)),
        ]
    );
    let source = &timetable
        .assignments
        .iter()
        .find(|a| a.kind != AssignmentKind::Note);
    let source = source.expect("feature");
    assert_eq!(source.source_code.as_deref(), Some("26"));
    assert!(source.note.is_some());
}

#[test]
fn r078_on_request_only_on_passenger_calls() {
    let (timetable, _) = timetable_of(support::timetable(vec![
        location("57076", "Praha", Some("08:00:00"), &["0001", "0030"]),
        location("57060", "Výhybna", Some("08:05:00"), &["0030"]),
        stop("57016", "08:20:00"),
    ]));
    let trip = timetable.trips.handles().next().expect("trip");
    assert_eq!(
        features(&timetable),
        [(Feature::OnRequest, Scope::Call { trip, sequence: 1 })]
    );
    let assignment = &timetable.assignments[0];
    assert_eq!(assignment.source_code.as_deref(), Some("0030"));
    assert_eq!(
        assignment.source_object.as_deref(),
        Some("Pa:54:000000001234:00:2026:sequence:1:activity:0030")
    );
}

#[test]
fn r085_r086_a_fare_band_puts_passenger_calls_in_its_zone() {
    let message = support::timetable(vec![
        stop("57076", "08:00:00"),
        support::point("57060", "08:05:00"),
        stop("57050", "08:10:00"),
        stop("57016", "08:20:00"),
    ]);
    let message = root(message, "CZIPTS", "11|CZ57076||CZ57050||");
    let message = root(message, "CZIPTS", "12|CZ57050||CZ57016||");
    let message = root(message, "CZIPTS", "99|CZ57050||CZ57016||");
    let message = root(message, "CZIPTS", "11|CZ57076");
    let (timetable, diagnostics) = timetable_of(message);
    let zone = |code: &str| Zone {
        code: code.into(),
        system: Some("PID".into()),
    };
    let zones: Vec<Vec<Zone>> = timetable.trips.rows()[0]
        .calls
        .iter()
        .map(|call| {
            call.zones
                .map(|z| timetable.zone_sets.get(z).to_vec())
                .unwrap_or_default()
        })
        .collect();
    assert_eq!(
        zones,
        [
            vec![zone("P")],
            vec![],
            vec![zone("P"), zone("B")],
            vec![zone("B")],
        ]
    );
    assert!(has(&diagnostics, "czptt_ids_malformed"));
    assert!(has(&diagnostics, "czptt_ids_unresolved"));
}
