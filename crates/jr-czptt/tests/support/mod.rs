//! A small builder for CZPTT messages, in the shape of the F# test fixture: every location has
//! one time for both ALA and ALD, operator `54` and train number `01234`; the message is PA/TR
//! `54/000000001234/00/2026`, created 2025-12-01, running on 2025-12-14.

#![allow(dead_code)]

use jiff::civil::{Date, DateTime, Time, date};
use jr_czptt::path::{Call, Path};
use jr_czptt::{Paths, Policy, SourceMessage};
use jr_formats::czptt::{
    Calendar, CancellationMessage, Identifier, Location, Message, ObjectType, Parameter,
    Subsidiary, Timestamp, TimetableMessage, Timing,
};
use jr_formats::kadr::{Catalog, CodeLabel};
use jr_model::Diagnostics;

pub const PA: &str = "Pa:54:000000001234:00:2026";

pub fn clock(text: &str) -> Time {
    text.parse().expect("hh:mm:ss")
}

pub fn timing(qualifier: &str, time: &str, day_offset: i32) -> Timing {
    Timing {
        qualifier: qualifier.into(),
        time: clock(time),
        offset_seconds: Some(3600),
        day_offset,
    }
}

/// A location at `time` (both qualifiers), or untimed with `None`.
pub fn location(code: &str, name: &str, time: Option<&str>, activities: &[&str]) -> Location {
    Location {
        country: "CZ".into(),
        primary_code: code.into(),
        primary_name: Some(name.into()),
        subsidiary: None,
        timings: time
            .map(|time| vec![timing("ALA", time, 0), timing("ALD", time, 0)])
            .unwrap_or_default(),
        responsible_ru: Some("54".into()),
        responsible_im: None,
        train_type: None,
        traffic_type: None,
        commercial_traffic_type: None,
        operational_train_number: Some("01234".into()),
        activities: activities.iter().map(|&a| a.into()).collect(),
        parameters: Vec::new(),
    }
}

/// A passenger location.
pub fn stop(code: &str, time: &str) -> Location {
    location(code, code, Some(time), &["0001"])
}

/// A railway point without passenger service.
pub fn point(code: &str, time: &str) -> Location {
    location(code, code, Some(time), &[])
}

pub fn with(mut location: Location, name: &str, value: &str) -> Location {
    location.parameters.push(Parameter {
        name: name.into(),
        value: value.into(),
    });
    location
}

pub fn line(location: Location, code: &str) -> Location {
    with(location, "CZPassengerServiceNumber", code)
}

pub fn bus(location: Location, flag: &str) -> Location {
    with(location, "CZAlternativeTransport", flag)
}

pub fn track(mut location: Location, code: &str) -> Location {
    location.subsidiary = Some(Subsidiary {
        code: code.into(),
        type_code: Some("1".into()),
        name: None,
        allocation_company: None,
    });
    location
}

pub fn operator(mut location: Location, ru: Option<&str>) -> Location {
    location.responsible_ru = ru.map(Box::from);
    location
}

pub fn commercial(mut location: Location, code: &str) -> Location {
    location.commercial_traffic_type = Some(code.into());
    location
}

pub fn untimed(mut location: Location) -> Location {
    location.timings.clear();
    location
}

pub fn identifier(object_type: ObjectType, core: &str) -> Identifier {
    Identifier {
        object_type,
        company: "54".into(),
        core: core.into(),
        variant: "00".into(),
        timetable_year: "2026".into(),
    }
}

pub fn stamp(text: &str) -> Timestamp {
    Timestamp {
        local: text.parse::<DateTime>().expect("timestamp"),
        offset_seconds: None,
    }
}

pub fn calendar(start: Date, end: Option<Date>, bitmap: &str) -> Calendar {
    let midnight = |day: Date| Timestamp {
        local: day.to_datetime(Time::midnight()),
        offset_seconds: None,
    };
    Calendar {
        start: midnight(start),
        end: end.map(midnight),
        bitmap: bitmap.into(),
    }
}

pub fn timetable(locations: Vec<Location>) -> TimetableMessage {
    TimetableMessage {
        identifiers: vec![
            identifier(ObjectType::Tr, "000000001234"),
            identifier(ObjectType::Pa, "000000001234"),
        ],
        related: Vec::new(),
        creation: stamp("2025-12-01T00:00:00"),
        calendar: calendar(date(2025, 12, 14), Some(date(2025, 12, 14)), "1"),
        locations,
        parameters: Vec::new(),
    }
}

/// A timetable for path core `core` with two passenger calls.
pub fn simple(core: &str) -> TimetableMessage {
    let mut message = timetable(vec![stop("57076", "08:00:00"), stop("57016", "08:20:00")]);
    message.identifiers = vec![
        identifier(ObjectType::Tr, core),
        identifier(ObjectType::Pa, core),
    ];
    message
}

pub fn cancellation(core: &str, when: &str, calendar: Calendar) -> CancellationMessage {
    CancellationMessage {
        identifiers: vec![identifier(ObjectType::Pa, core)],
        cancellation: stamp(when),
        calendar,
    }
}

pub fn digest(seed: u8) -> [u8; 32] {
    [seed; 32]
}

pub fn source(path: &str, annual: bool, seed: u8, message: Message) -> SourceMessage {
    SourceMessage {
        source_path: path.into(),
        annual,
        digest: digest(seed),
        message: Ok(message),
    }
}

pub fn annual(seed: u8, message: TimetableMessage) -> SourceMessage {
    source(
        &format!("annual/JR2026.zip//{seed:03}.xml"),
        true,
        seed,
        Message::Timetable(message),
    )
}

pub fn catalog() -> Catalog {
    let label = |code: &str, abbreviation: &str| CodeLabel {
        code: code.into(),
        abbreviation: Some(abbreviation.into()),
    };
    let mut commercial = vec![label("84", "Os")];
    for code in ["EC", "IC", "RJ", "rj", "Ex", "R", "Os", "Sp"] {
        commercial.push(label(code, code));
    }
    Catalog {
        commercial_train_types: commercial,
        train_types: ["Os", "Sp", "R", "Ex"]
            .into_iter()
            .map(|code| label(code, code))
            .collect(),
        ..Catalog::default()
    }
}

pub fn compile(messages: Vec<SourceMessage>) -> (Paths, Diagnostics) {
    let mut diagnostics = Diagnostics::new();
    let paths = jr_czptt::paths(
        messages,
        &catalog(),
        2026,
        &Policy::builtin(),
        &mut diagnostics,
    )
    .expect("compiles");
    (paths, diagnostics)
}

/// The one path compiled from one annual timetable message.
pub fn compile_one(message: TimetableMessage) -> (Path, Diagnostics) {
    let (mut paths, diagnostics) = compile(vec![annual(1, message)]);
    assert_eq!(paths.paths.len(), 1, "one surviving path");
    (paths.paths.remove(0), diagnostics)
}

pub fn codes(diagnostics: &Diagnostics) -> Vec<&str> {
    diagnostics.iter().map(|d| d.code.as_ref()).collect()
}

pub fn has(diagnostics: &Diagnostics, code: &str) -> bool {
    diagnostics.iter().any(|d| d.code == code)
}

pub fn messages_of<'a>(diagnostics: &'a Diagnostics, code: &str) -> Vec<&'a str> {
    diagnostics
        .iter()
        .filter(|d| d.code == code)
        .map(|d| d.message.as_str())
        .collect()
}

pub fn sequences(calls: &[Call]) -> Vec<u32> {
    calls.iter().map(Call::sequence).collect()
}

pub fn hms(h: i32, m: i32, s: i32) -> i32 {
    h * 3600 + m * 60 + s
}
