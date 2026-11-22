//! Small builders: reader batches and records with neutral defaults, calendar inputs over the
//! 2026 timetable year, and international filter views.

#![allow(dead_code)]

use jiff::civil::{Date, date};
use jr_formats::jdf::{self, Header, Km, NoteType, RouteType, Version};
use jr_jdf::Policy;
use jr_jdf::calendar::{CalendarNote, TripCalendarInput, running_days};
use jr_jdf::holidays::Holidays;
use jr_jdf::international::{CallView, RouteView, StopCountry, TripView};
use jr_model::{Diagnostics, Horizon};

pub fn gvd() -> Horizon {
    Horizon::gvd(2026).expect("GVD 2026")
}

pub fn codes(diagnostics: &Diagnostics) -> Vec<&str> {
    diagnostics.iter().map(|d| d.code.as_ref()).collect()
}

// --- reader batches ---

pub fn source(version: Version) -> jdf::Batch {
    jdf::Batch {
        version,
        header: Header {
            version: version.as_str().into(),
            du_number: None,
            region: None,
            batch_id: None,
            created: None,
            generator: None,
        },
        stops: Vec::new(),
        stop_posts: Vec::new(),
        agencies: Vec::new(),
        routes: Vec::new(),
        route_integrations: Vec::new(),
        route_stops: Vec::new(),
        trips: Vec::new(),
        trip_groups: Vec::new(),
        trip_stops: Vec::new(),
        route_info: Vec::new(),
        attribute_refs: Vec::new(),
        service_notes: Vec::new(),
        transfers: Vec::new(),
        agency_alternations: Vec::new(),
        alternate_route_names: Vec::new(),
        reservations: Vec::new(),
    }
}

/// A route as a 1.8/1.9 batch writes it: no distinctions, mode or flags.
pub fn old_route(id: &str, from: Date, to: Option<Date>) -> jdf::Route {
    jdf::Route {
        id: id.into(),
        name: "Test line".into(),
        agency_id: "12345678".into(),
        route_type: Some(RouteType::Regional),
        transport_mode: None,
        detour: None,
        grouped: None,
        uses_stop_posts: None,
        one_way: None,
        reserved1: None,
        licence_number: None,
        licence_valid_from: None,
        licence_valid_to: None,
        timetable_valid_from: from,
        timetable_valid_to: to,
        agency_distinction: None,
        distinction: None,
    }
}

pub fn trip(route: &str, id: u64, attributes: &[u32], distinction: Option<u32>) -> jdf::Trip {
    let mut slots = [None; 10];
    for (slot, &attribute) in slots.iter_mut().zip(attributes) {
        *slot = Some(attribute);
    }
    jdf::Trip {
        route_id: route.into(),
        id,
        attributes: slots,
        trip_group_id: None,
        route_distinction: distinction,
    }
}

pub fn attribute(id: u32, code: &str) -> jdf::AttributeRef {
    jdf::AttributeRef {
        id,
        code: code.into(),
        reserved1: None,
    }
}

pub fn note(
    route: &str,
    trip: u64,
    note_type: Option<NoteType>,
    from: Option<Date>,
    to: Option<Date>,
    distinction: Option<u32>,
) -> jdf::ServiceNote {
    jdf::ServiceNote {
        route_id: route.into(),
        trip_id: trip,
        id: 1,
        designation: "1".into(),
        note_type,
        date_from: from,
        date_to: to,
        note: None,
        route_distinction: distinction,
    }
}

// --- calendars ---

/// A trip with these day codes and notes over `from..=to`.
pub fn input<'a>(
    codes: &[&'a str],
    notes: &[CalendarNote<'a>],
    from: Date,
    to: Option<Date>,
) -> TripCalendarInput<'a> {
    TripCalendarInput {
        subject: "jdf:test:000001:0",
        trip: 1,
        codes: codes.to_vec(),
        notes: notes.to_vec(),
        valid_from: from,
        valid_to: to,
    }
}

pub fn typed<'a>(
    note_type: &'a NoteType,
    from: Option<Date>,
    to: Option<Date>,
) -> CalendarNote<'a> {
    CalendarNote {
        note_type: Some(note_type),
        date_from: from,
        date_to: to,
    }
}

/// Running days and the diagnostics of computing them.
pub fn run(input: &TripCalendarInput<'_>) -> (Vec<Date>, Diagnostics) {
    let policy = Policy::builtin();
    let horizon = gvd();
    let holidays = Holidays::new(&policy, horizon);
    let mut diagnostics = Diagnostics::new();
    let set = running_days(input, horizon, &holidays, &policy, &mut diagnostics);
    (set.dates(horizon).collect(), diagnostics)
}

/// `from..=to`.
pub fn span(from: Date, to: Date) -> Vec<Date> {
    let mut days = Vec::new();
    let mut day = from;
    while day <= to {
        days.push(day);
        day = day.tomorrow().expect("in range");
    }
    days
}

/// The days of `from..=to` the predicate keeps.
pub fn span_where(from: Date, to: Date, keep: impl Fn(Date) -> bool) -> Vec<Date> {
    span(from, to).into_iter().filter(|&d| keep(d)).collect()
}

pub const MONDAY: Date = date(2026, 3, 2);
pub const SUNDAY_AFTER_NEXT: Date = date(2026, 3, 15);

// --- international views ---

pub fn cz() -> StopCountry<'static> {
    StopCountry {
        country: Some("CZ"),
        okres: Some("PB"),
    }
}

pub fn abroad(country: &str) -> StopCountry<'_> {
    StopCountry {
        country: Some(country),
        okres: None,
    }
}

pub fn km(value: u64) -> Option<Km> {
    Some(Km {
        units: value,
        scale: 0,
    })
}

pub fn call(stop: StopCountry<'_>, kilometre: Option<Km>) -> CallView<'_> {
    CallView {
        stop,
        km: kilometre,
        emitted: true,
    }
}

/// The F# `syntheticRoute`: Czech stops at kilometre 0 and `span - depth`, a foreign stop at
/// `span`; the third call passes without stopping when `passing`.
pub fn synthetic<'a>(
    foreign: &'a str,
    span: u64,
    depth: u64,
    integrated: bool,
    route_type: &'a RouteType,
    passing: bool,
) -> RouteView<'a> {
    let mut third = call(abroad(foreign), km(span));
    third.emitted = !passing;
    RouteView {
        subject: "jdf:route:586001:1",
        route_type: Some(route_type),
        integrated,
        route_stops: vec![cz(), cz(), abroad(foreign)],
        trips: vec![TripView {
            trip: 1,
            runs: true,
            calls: vec![call(cz(), km(0)), call(cz(), km(span - depth)), third],
        }],
    }
}

pub mod merge;
