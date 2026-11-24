//! Notes, features, claims and restrictions of the assembled timetable (R452–R465), the
//! assembly's policy and its rule ledger.

mod support;

use std::collections::BTreeSet;

use jr_formats::jdf::{NoteType, TimeCell};
use jr_jdf::batch::{Reservation, RouteInfo, RouteIntegration, Transfer};
use jr_jdf::fixed::FixedBatch;
use jr_model::timetable::{
    AssignmentKind, ClaimDirection, Feature, NoteKind, RestrictionGroup, RestrictionScope, Scope,
};
use jr_model::{Diagnostics, Timetable};
use support::merge::*;

const LICENCE: &str = "100001";

fn three_stops() -> B {
    year_route(
        batch("a")
            .stop(stop("Alfa", None, None))
            .stop(stop("Beta", None, None))
            .stop(stop("Gama", None, None)),
        LICENCE,
    )
}

fn notes(t: &Timetable) -> Vec<(&str, NoteKind, Option<&str>, Option<&str>)> {
    t.notes
        .rows()
        .iter()
        .map(|n| (n.id.as_str(), n.kind, n.label.as_deref(), n.text.as_deref()))
        .collect()
}

#[test]
fn r452_r455_route_information_is_a_route_note() {
    let mut batch = three_stops().trip(LICENCE, 1, &[]).build();
    for (id, text) in [(1, "Jede přes Alfu"), (2, "  ")] {
        batch.batch.route_info.push(RouteInfo {
            route_id: LICENCE.into(),
            id,
            text: text.into(),
            route_distinction: 1,
        });
    }
    let (assembled, diagnostics) = assemble(vec![batch]);
    let t = &assembled.timetable;
    assert_eq!(
        notes(t),
        [(
            "jdf:route:100001:notice:route:1",
            NoteKind::RouteInformation,
            None,
            Some("Jede přes Alfu")
        )]
    );
    let link = &t.assignments[0];
    assert_eq!(link.kind, AssignmentKind::Note);
    assert!(matches!(link.scope, Scope::Route(_)));
    assert_eq!(count(&diagnostics, "jdf_note_blank"), 1);
}

#[test]
fn r452_versions_of_one_route_share_an_equal_note_and_keep_a_differing_one() {
    let version = |name: &str, from, to, capacity: &str| {
        let mut batch = batch(name)
            .stop(stop("Alfa", None, None))
            .stop(stop("Beta", None, None))
            .stop(stop("Gama", None, None))
            .route(LICENCE, from, Some(to), false)
            .trip(LICENCE, 1, &[])
            .build();
        for (id, text) in [(1, "Platí tarif IDS"), (2, capacity)] {
            batch.batch.route_info.push(RouteInfo {
                route_id: LICENCE.into(),
                id,
                text: text.into(),
                route_distinction: 1,
            });
        }
        batch
    };
    let date = jiff::civil::date;
    let (assembled, _) = assemble(vec![
        version(
            "a",
            date(2025, 12, 14),
            date(2026, 5, 31),
            "Vozidla pro 70 cestujících",
        ),
        version(
            "b",
            date(2026, 6, 1),
            date(2026, 12, 12),
            "Vozidla pro 45 cestujících",
        ),
    ]);
    let found: Vec<(&str, Option<&str>)> = notes(&assembled.timetable)
        .into_iter()
        .map(|(id, _, _, text)| (id, text))
        .collect();
    assert_eq!(
        found,
        [
            ("jdf:route:100001:notice:route:1", Some("Platí tarif IDS")),
            (
                "jdf:route:100001:notice:route:2",
                Some("Vozidla pro 70 cestujících")
            ),
            (
                "jdf:route:100001:notice:route:2:2",
                Some("Vozidla pro 45 cestujících")
            ),
        ]
    );
    assert_eq!(assembled.timetable.assignments.len(), 3);
}

#[test]
fn r453_caskody_rows_are_trip_notes_typed_designations_included() {
    let mut batch = three_stops()
        .trip(LICENCE, 1, &[])
        .note(
            LICENCE,
            1,
            NoteType::NoService,
            Some(jiff::civil::date(2026, 5, 1)),
            None,
        )
        .build();
    batch.batch.service_notes.push(jr_jdf::batch::ServiceNote {
        route_id: LICENCE.into(),
        trip_id: 1,
        id: 2,
        designation: "N".into(),
        note_type: None,
        date_from: None,
        date_to: None,
        note: Some("Nejede o svátcích".into()),
        route_distinction: 1,
    });
    let (assembled, _) = assemble(vec![batch]);
    let t = &assembled.timetable;
    // The typed row without text is the printed mark with its days (B4).
    assert_eq!(
        notes(t),
        [
            (
                "jdf:trip:100001:251214:1:notice:trip:1",
                NoteKind::TimetableNote,
                Some("1"),
                None
            ),
            (
                "jdf:trip:100001:251214:1:notice:trip:2",
                NoteKind::TimetableNote,
                Some("N"),
                Some("Nejede o svátcích")
            )
        ]
    );
    let typed = &t.notes.rows()[0];
    assert_eq!(
        (typed.note_type.as_deref(), typed.valid_from),
        (Some("no_service"), Some(jiff::civil::date(2026, 5, 1)))
    );
    assert!(matches!(
        t.assignments
            .iter()
            .find(|a| a.kind == AssignmentKind::Note)
            .map(|a| a.scope),
        Some(Scope::Trip(_))
    ));
}

#[test]
fn r454_mistenky_rows_are_reservation_notes_and_trip_zero_is_a_route_note() {
    let mut batch = three_stops().trip(LICENCE, 1, &[]).build();
    for trip in [1, 0] {
        batch.batch.reservations.push(Reservation {
            route_id: LICENCE.into(),
            trip_id: trip,
            note: "Místenky povinné".into(),
            route_distinction: 1,
        });
    }
    let (assembled, diagnostics) = assemble(vec![batch]);
    let t = &assembled.timetable;
    assert_eq!(
        notes(t),
        [
            (
                "jdf:trip:100001:251214:1:notice:reservation:1",
                NoteKind::ReservationNote,
                None,
                Some("Místenky povinné")
            ),
            (
                "jdf:route:100001:notice:reservation:1",
                NoteKind::ReservationNote,
                None,
                Some("Místenky povinné")
            )
        ]
    );
    assert!(
        t.assignments
            .iter()
            .any(|a| matches!(a.scope, Scope::Route(_)))
    );
    assert_eq!(count(&diagnostics, "jdf_reservation_without_trip"), 0);
}

#[test]
fn r457_trip_codes_are_calendar_designations_and_features() {
    let (assembled, _) = assemble(vec![
        three_stops()
            .trip(LICENCE, 1, &["X", "R", "§", "@"])
            .build(),
    ]);
    let kinds: Vec<(&str, Option<&str>)> = assembled
        .timetable
        .assignments
        .iter()
        .map(|a| (a.kind.as_str(), a.source_code.as_deref()))
        .collect();
    assert_eq!(
        kinds,
        [
            ("calendar_designation", Some("X")),
            ("reservation_available", Some("R")),
            ("wheelchair_accessible_vehicle", Some("@")),
        ]
    );
}

#[test]
fn r458_integrated_systems_are_route_features() {
    // "JDF bundle is deterministic and contains normalized sidecars" (30001 on the route).
    let mut batch = three_stops().trip(LICENCE, 1, &[]).build();
    for (system, preferential) in [(30001, Some(true)), (30621, None), (30001, None)] {
        batch.batch.route_integrations.push(RouteIntegration {
            route_id: LICENCE.into(),
            entry_number: 1,
            transport_system_id: system,
            route_name: "1".into(),
            preferential,
            reserved1: "".into(),
            route_distinction: 1,
        });
    }
    let (assembled, _) = assemble(vec![batch]);
    let systems: Vec<Option<&str>> = assembled
        .timetable
        .assignments
        .iter()
        .filter(|a| a.kind == AssignmentKind::Feature(Feature::IntegratedTransport))
        .map(|a| a.source_code.as_deref())
        .collect();
    assert_eq!(systems, [Some("30001"), Some("30621")]);
}

fn transfer(kind: &str, route_stop: u64) -> Transfer {
    Transfer {
        kind: kind.into(),
        route_id: LICENCE.into(),
        trip_id: 1,
        route_stop_id: route_stop,
        transfer_route_id: Some(1401),
        transfer_stop_id: Some(0),
        transfer_stop_post_id: None,
        transfer_end_stop_id: None,
        transfer_end_stop_post_id: None,
        wait_minutes: Some(5),
        note: None,
        route_distinction: 1,
    }
}

#[test]
fn r460_r461_navaznosti_rows_are_claims_at_their_published_call() {
    let mut batch = three_stops().trip(LICENCE, 1, &[]).build();
    batch.batch.trip_stops[1].departure = Some(TimeCell::Passes);
    batch.batch.transfers = vec![
        transfer("m", 3),
        transfer("M", 2),
        transfer("q", 1),
        transfer("M", 9),
    ];
    let (assembled, diagnostics) = assemble(vec![batch]);
    let claims = &assembled.timetable.connection_claims;
    assert_eq!(claims.len(), 1);
    let claim = &claims[0];
    assert_eq!(
        (claim.direction, claim.origin_sequence),
        (ClaimDirection::WaitsFor, 2)
    );
    assert_eq!(claim.target_source_route.as_deref(), Some("001401"));
    assert_eq!(claim.target_source_stop, None);
    assert_eq!(claim.wait_minutes, Some(5));
    assert_eq!(&*claim.source_object, "jdf:trip:100001:251214:1:transfer:1");
    assert_eq!(count(&diagnostics, "jdf_claim_call_unserved"), 1);
    assert_eq!(count(&diagnostics, "jdf_claim_kind_unknown"), 1);
    assert_eq!(count(&diagnostics, "jdf_claim_call_unknown"), 1);
}

#[test]
fn r462_r463_r464_r465_restrictions_on_route_stops_and_calls() {
    let mut builder = three_stops().trip(LICENCE, 1, &[]).trip(LICENCE, 3, &[]);
    let a = builder.code("A");
    let b = builder.code("B");
    let mut batch: FixedBatch = builder.build();
    // Route stops 1 and 2 carry A: every published call there, both trips.
    batch.batch.route_stops[0].attributes[0] = Some(a);
    batch.batch.route_stops[1].attributes[0] = Some(a);
    // Trip 1's third call carries B (alone: a singleton); trip 3's second call is unserved.
    batch.batch.trip_stops[2].attributes[0] = Some(b);
    batch.batch.trip_stops[4].attributes[0] = Some(b);
    batch.batch.trip_stops[4].departure = Some(TimeCell::OtherRoute);
    let (assembled, diagnostics) = assemble(vec![batch]);
    let restrictions = &assembled.timetable.restrictions;
    let shapes: Vec<(RestrictionGroup, usize)> = restrictions
        .iter()
        .map(|r| {
            (
                r.group,
                match &r.scope {
                    RestrictionScope::RouteStop { calls, .. } => calls.len(),
                    RestrictionScope::Call { .. } => 0,
                },
            )
        })
        .collect();
    // Route stop 1: both trips; route stop 2: trip 1 only (trip 3 does not serve it); the call.
    assert_eq!(
        shapes,
        [
            (RestrictionGroup::A, 2),
            (RestrictionGroup::A, 1),
            (RestrictionGroup::B, 0)
        ]
    );
    assert!(matches!(
        restrictions[2].scope,
        RestrictionScope::Call { sequence: 2, .. }
    ));
    assert_eq!(&*restrictions[2].source_route_stop, "3");
    assert_eq!(count(&diagnostics, "jdf_restriction_call_unserved"), 1);
    // B reaches one call of trip 1; A reaches one served route stop of trip 3.
    assert_eq!(count(&diagnostics, "jdf_restriction_singleton"), 2);
}

#[test]
fn the_builtin_assembly_policy_loads() {
    let policy = jr_jdf::assemble::Policy::builtin();
    assert_eq!(
        policy.trip_features.get("@"),
        Some(&Feature::WheelchairAccessibleVehicle)
    );
    assert_eq!(
        policy.codes.restrictions.get("§"),
        Some(&RestrictionGroup::Section)
    );
    assert_eq!(policy.route_types.modes.get("E"), Some(&900));
    let _ = Diagnostics::new();
}

#[test]
fn every_assembly_rule_has_a_heading_in_the_ledger() {
    const CONVERT: &str = include_str!("../../../docs/rules/jdf-convert.md");
    let headings: BTreeSet<&str> = CONVERT
        .lines()
        .filter_map(|line| line.strip_prefix("#### "))
        .filter_map(|heading| heading.split_whitespace().next())
        .collect();
    let mut seen = BTreeSet::new();
    for rule in jr_jdf::assemble::RULES {
        assert!(headings.contains(rule.id), "{} is undocumented", rule.id);
        assert!(seen.insert(rule.id), "{} is declared twice", rule.id);
    }
}
