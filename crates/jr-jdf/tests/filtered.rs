//! The filtered road feed (R479–R482).

mod support;

use jiff::civil::date;
use jr_formats::geodata::filter_rules::FilterRules;
use jr_formats::jdf::RouteIntegration;
use jr_jdf::filtered::{FilterError, filtered_feed};
use jr_jdf::fixed::FixedBatch;
use jr_model::{Diagnostics, Precedence, Timetable, apply};
use support::merge::*;

/// One line with one trip over the given stops, run by the operator `name`.
fn line(batch_name: &str, licence: &str, ico: &str, name: &str, stops: &[(&str, bool)]) -> B {
    let mut builder = batch(batch_name);
    for (index, &(stop_name, placed)) in stops.iter().enumerate() {
        let fixed = stop(stop_name, None, None);
        builder = builder.stop(if placed {
            at(fixed, 50.0 + index as f64 / 100.0, 14.0)
        } else {
            fixed
        });
    }
    let mut builder = year_route(builder, licence).trip(licence, 1, &[]);
    builder.fixed.batch.agencies[0].id = ico.into();
    builder.fixed.batch.agencies[0].name = name.into();
    builder.fixed.batch.routes[0].agency_id = ico.into();
    builder
}

fn preferring(mut builder: B, licence: &str, system: u64, preferential: Option<bool>) -> B {
    builder
        .fixed
        .batch
        .route_integrations
        .push(RouteIntegration {
            route_id: licence.into(),
            entry_number: 1,
            transport_system_id: system,
            route_name: "1".into(),
            preferential,
            reserved1: "".into(),
            route_distinction: 1,
        });
    builder
}

const ABC: &[(&str, bool)] = &[("Alfa", true), ("Beta", true), ("Gama", true)];

fn feed() -> Vec<FixedBatch> {
    vec![
        line("a", "100001", "11111111", "FlixBus CZ s.r.o.", ABC).build(),
        preferring(
            line("b", "100002", "22222222", "Dopravce", ABC),
            "100002",
            30001,
            Some(true),
        )
        .build(),
        // Accepts the system's tariff without being its line.
        preferring(
            line("c", "100003", "22222222", "Dopravce", ABC),
            "100003",
            30001,
            None,
        )
        .build(),
        line("d", "199005", "22222222", "Dopravce", ABC).build(),
        line(
            "e",
            "300001",
            "22222222",
            "Dopravce",
            &[("Alfa", true), ("Neznámo", false), ("Gama", true)],
        )
        .build(),
        line(
            "f",
            "300002",
            "22222222",
            "Dopravce",
            &[("Alfa", true), ("Nikde", false), ("Jinde", false)],
        )
        .build(),
    ]
}

fn rules(operators: &[&str], ids_codes: &[u32]) -> FilterRules {
    FilterRules {
        schema_version: 1,
        source: None,
        operators: operators.iter().map(|name| (*name).into()).collect(),
        ids_codes: ids_codes.to_vec(),
        ids_names: Default::default(),
        line_prefixes: vec!["199".into()],
    }
}

/// The published trips as `route ID: call sequences`.
fn published(t: &Timetable) -> Vec<String> {
    let mut trips: Vec<String> = t
        .trips
        .rows()
        .iter()
        .filter(|trip| !t.services.get(trip.service).is_empty())
        .map(|trip| {
            let sequences: Vec<String> = trip
                .calls
                .iter()
                .map(|call| call.sequence.to_string())
                .collect();
            format!("{}: {}", t.routes[trip.route].id, sequences.join(" "))
        })
        .collect();
    trips.sort();
    trips
}

#[test]
fn r479_r480_r481_lines_of_other_feeds_and_unplaced_calls_are_removed() {
    let (assembled, _) = assemble(feed());
    let mut t = assembled.timetable;
    let filtered = filtered_feed(
        &t,
        &assembled.lines,
        &rules(&["FlixBus CZ s.r.o."], &[30001]),
        date(2026, 11, 21),
    )
    .unwrap();
    let queries: Vec<(&str, &str, Vec<&str>)> = filtered
        .queries
        .iter()
        .map(|q| {
            (
                q.kind,
                q.value.as_str(),
                q.lines.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    assert_eq!(
        queries,
        [
            ("operators", "FlixBus CZ s.r.o.", vec!["100001"]),
            ("ids", "30001", vec!["100002"]),
        ]
    );
    let report = &filtered.report;
    assert_eq!(
        (
            report.trips_removed_by_line,
            report.trips_removed_too_few_calls,
            report.calls_removed_missing_coordinates,
            report.trips_kept,
            report.routes_kept,
            report.routes_removed,
        ),
        (3, 1, 1, 2, 2, 3)
    );
    apply(
        &mut t,
        filtered.edits,
        &Precedence { position: vec![] },
        &mut Diagnostics::new(),
    )
    .unwrap();
    // The operator's line, the system's line and the prefixed line are gone; the trip through
    // the unplaced stop keeps its other calls and their sequences; the trip with one placed
    // stop is gone.
    assert_eq!(
        published(&t),
        ["jdf:route:100003: 0 1 2", "jdf:route:300001: 0 2"]
    );
}

#[test]
fn r479_a_version_that_ended_before_the_reference_date_selects_nothing() {
    let (assembled, _) = assemble(feed());
    let mut lines = assembled.lines.clone();
    for line in &mut lines {
        line.valid_to = Some(date(2026, 6, 30));
    }
    let result = filtered_feed(
        &assembled.timetable,
        &lines,
        &rules(&["FlixBus CZ s.r.o."], &[]),
        date(2026, 11, 21),
    );
    assert!(matches!(
        result,
        Err(FilterError::EmptySelection {
            kind: "operators",
            ..
        })
    ));
}

#[test]
fn r482_a_system_without_a_line_and_an_empty_feed_are_errors() {
    let (assembled, _) = assemble(feed());
    let t = &assembled.timetable;
    let reference = date(2026, 11, 21);
    assert!(matches!(
        filtered_feed(t, &assembled.lines, &rules(&[], &[30621]), reference),
        Err(FilterError::EmptySelection { kind: "ids", .. })
    ));
    let mut everything = rules(&[], &[30001]);
    everything.line_prefixes = vec!["1".into(), "3".into()];
    assert!(matches!(
        filtered_feed(t, &assembled.lines, &everything, reference),
        Err(FilterError::Empty)
    ));
}
