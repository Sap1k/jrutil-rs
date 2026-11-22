//! JDF batch reader (JDF-R017–R024): real batches of each version, cut down, and the grammar's
//! tolerated and rejected deviations.
//!
//! `v1_11.zip` is `vld-4135` (trips 601, 602) under `Davka 4135/` with the file names' case
//! changed; `v1_10/` is `drahy-778` (trips 1002, 1003); `v1_8/` is `vld-544` (trips 1, 5).

use std::io::Cursor;

use jiff::civil::date;
use jr_formats::jdf::{
    JdfError, Km, NoteType, Problem, Read, RouteType, TimeCell, TransportMode, Version, read_files,
    read_zip,
};

macro_rules! batch_dir {
    ($dir:literal: $($file:literal),* $(,)?) => {
        vec![$(($file, include_bytes!(concat!("data/jdf/", $dir, "/", $file)).as_slice()),)*]
    };
}

fn v1_10() -> Vec<(&'static str, &'static [u8])> {
    batch_dir!("v1_10": "VerzeJDF.txt", "Zastavky.txt", "Dopravci.txt", "Linky.txt",
        "LinExt.txt", "Zaslinky.txt", "Spoje.txt", "Zasspoje.txt", "Udaje.txt", "Pevnykod.txt",
        "Caskody.txt")
}

fn v1_8() -> Vec<(&'static str, &'static [u8])> {
    batch_dir!("v1_8": "VerzeJDF.txt", "Zastavky.txt", "Dopravci.txt", "Linky.txt",
        "Zaslinky.txt", "Spoje.txt", "Zasspoje.txt", "Udaje.txt", "Pevnykod.txt", "Caskody.txt")
}

fn v1_11() -> Read {
    read_zip(Cursor::new(include_bytes!("data/jdf/v1_11.zip"))).expect("1.11 batch")
}

/// A 1.11 batch of one stop and nothing else, with `file` replaced by `bytes`.
fn minimal(file: &'static str, bytes: &'static [u8]) -> Result<Read, JdfError> {
    let mut files: Vec<(&str, &[u8])> = vec![
        (
            "VerzeJDF.txt",
            b"\"1.11\",\"\",\"\",\"\",\"20112026\",\"\";\r\n",
        ),
        (
            "Zastavky.txt",
            b"\"1\",\"A\",\"\",\"\",\"\",\"CZ\",\"\",\"\",\"\",\"\",\"\",\"\";\r\n",
        ),
        ("Dopravci.txt", b""),
        ("Linky.txt", b""),
        ("Zaslinky.txt", b""),
        ("Spoje.txt", b""),
        ("Zasspoje.txt", b""),
        ("Pevnykod.txt", b""),
        ("Caskody.txt", b""),
    ];
    match files.iter_mut().find(|(name, _)| *name == file) {
        Some(entry) => entry.1 = bytes,
        None => files.push((file, bytes)),
    }
    read_files(files)
}

fn at(hours: u16, minutes: u16) -> Option<TimeCell> {
    Some(TimeCell::At {
        minutes: hours * 60 + minutes,
    })
}

fn messages(problems: &[Problem]) -> Vec<(u32, Option<&str>, &str)> {
    problems
        .iter()
        .map(|p| (p.record, p.field, &*p.message))
        .collect()
}

#[test]
fn jdf_r017_zip_batch_in_a_subdirectory_with_any_name_case() {
    let read = v1_11();
    assert_eq!(read.batch.version, Version::V1_11);
    assert_eq!(read.batch.stops.len(), 19);
    assert_eq!(read.batch.trip_stops.len(), 38);
    assert_eq!(read.batch.transfers.len(), 4);
    assert!(read.problems.is_empty(), "{:?}", read.problems);
}

#[test]
fn jdf_r017_r018_r019_batch_errors() {
    let stop = b"\"1\",\"A\",\"\",\"\",\"\",\"CZ\",\"\",\"\",\"\",\"\",\"\",\"\";\r\n".as_slice();
    let mut files = v1_10();
    files.retain(|(name, _)| *name != "VerzeJDF.txt");
    assert!(matches!(read_files(files), Err(JdfError::NoVersionFile)));

    let mut files = v1_10();
    files.push(("other/verzejdf.txt", b"\"1.11\";\r\n"));
    assert!(matches!(
        read_files(files),
        Err(JdfError::SeveralBatches(_))
    ));

    let mut files = v1_10();
    files.push(("ZASTAVKY.TXT", stop));
    assert!(matches!(
        read_files(files),
        Err(JdfError::DuplicateFile("Zastavky.txt"))
    ));

    match minimal("VerzeJDF.txt", b"\"1.7\";\r\n") {
        Err(JdfError::UnknownVersion(version)) => assert_eq!(&*version, "1.7"),
        other => panic!("{other:?}"),
    }

    let mut files = v1_10();
    files.retain(|(name, _)| *name != "Zasspoje.txt");
    assert!(matches!(
        read_files(files),
        Err(JdfError::MissingFile {
            version: Version::V1_10,
            file: "Zasspoje.txt"
        })
    ));
}

#[test]
fn jdf_r022_r023_r024_v1_11_facts() {
    let batch = v1_11().batch;
    assert_eq!(batch.header.created, Some(date(2026, 11, 20)));
    assert_eq!(batch.header.batch_id, None);

    let stop = &batch.stops[1];
    assert_eq!(
        (
            &*stop.town,
            stop.district.as_deref(),
            stop.nearby_place.as_deref()
        ),
        ("Brno", Some("ÚAN Zvonařka"), None)
    );
    assert_eq!(stop.okres.as_deref(), Some("BM"));
    assert_eq!(stop.country.as_deref(), Some("CZ"));
    assert_eq!(
        batch.stops[18].attributes,
        [Some(19), None, None, None, None, None]
    );

    let route = &batch.routes[0];
    assert_eq!(route.route_type, Some(RouteType::Regional));
    assert_eq!(route.transport_mode, Some(TransportMode::Bus));
    assert_eq!((route.detour, route.one_way), (Some(false), Some(false)));
    assert_eq!(route.timetable_valid_from, date(2025, 12, 14));
    assert_eq!(route.timetable_valid_to, Some(date(2026, 12, 12)));
    assert_eq!(
        (route.agency_distinction, route.distinction),
        (Some(1), Some(1))
    );

    let call = |trip: u64, order: u64| {
        batch
            .trip_stops
            .iter()
            .find(|s| s.trip_id == trip && s.route_stop_id == order)
            .expect("call")
    };
    let passing = call(602, 3);
    assert_eq!(
        passing.km,
        Some(Km {
            units: 66,
            scale: 0
        })
    );
    assert_eq!(
        [
            passing.arrival,
            passing.departure,
            passing.earliest_arrival,
            passing.latest_departure
        ],
        [Some(TimeCell::Passes); 4]
    );
    let elsewhere = call(601, 2);
    assert_eq!(
        (elsewhere.km, elsewhere.arrival),
        (None, Some(TimeCell::OtherRoute))
    );
    let stopping = call(601, 9);
    assert_eq!(
        (stopping.arrival, stopping.departure),
        (at(4, 38), at(4, 40))
    );
    assert_eq!(stopping.earliest_arrival, Some(TimeCell::Empty));
    assert_eq!(stopping.route_distinction, Some(1));

    let fixed = batch
        .attribute_refs
        .iter()
        .find(|a| a.id == 19)
        .expect("19");
    assert_eq!(&*fixed.code, "~");
    let note = &batch.service_notes[0];
    assert_eq!(
        (&*note.designation, note.note_type.clone()),
        ("23", Some(NoteType::NoService))
    );
    assert_eq!(
        (note.date_from, note.date_to),
        (Some(date(2025, 12, 31)), None)
    );

    let transfer = &batch.transfers[2];
    assert_eq!(
        (&*transfer.kind, transfer.trip_id, transfer.route_stop_id),
        ("M", 601, 9)
    );
    assert_eq!(
        (transfer.wait_minutes, transfer.note.as_deref()),
        (None, None)
    );
    assert_eq!(transfer.route_distinction, 1);
    let integration = &batch.route_integrations[0];
    assert_eq!(integration.transport_system_id, 30621);
    assert_eq!(
        (&*integration.route_name, integration.preferential),
        ("108", Some(true))
    );
}

#[test]
fn jdf_r019_r023_v1_10_facts() {
    let read = read_files(v1_10()).expect("1.10 batch");
    assert!(read.problems.is_empty(), "{:?}", read.problems);
    let batch = read.batch;
    assert_eq!(batch.version, Version::V1_10);
    let route = &batch.routes[0];
    assert_eq!(route.transport_mode, Some(TransportMode::Tram));
    assert_eq!((route.detour, route.one_way), (Some(true), None));
    assert_eq!(route.timetable_valid_to, Some(date(2026, 12, 18)));
    assert_eq!(route.distinction, Some(1));
    assert_eq!(batch.stops[0].nearby_place.as_deref(), Some("Bulovka"));

    let first = &batch.trip_stops[0];
    assert_eq!(first.km, Some(Km { units: 8, scale: 0 }));
    assert_eq!(
        (first.arrival, first.departure),
        (at(0, 9), Some(TimeCell::Empty))
    );
    assert_eq!(
        (first.earliest_arrival, first.latest_departure),
        (None, None)
    );
    assert_eq!(first.attributes[2], None);
    assert_eq!(batch.trip_stops[1].departure, Some(TimeCell::OtherRoute));
    assert_eq!(
        batch.trips[1].attributes[..6],
        [Some(4), Some(5), Some(6), Some(7), Some(8), None]
    );
}

#[test]
fn jdf_r028_r029_v1_8_absent_fields_and_note_type() {
    let mut files = v1_8();
    files.push((
        "Navaznosti.txt",
        b"\"m\",\"100307\",\"1\",\"9\",\"\",\"\",\"\",\"\",\"\",\"\",\"1\";\r\n",
    ));
    let read = read_files(files).expect("1.8 batch");
    assert_eq!(
        messages(&read.problems),
        [(0, None, "not part of JDF 1.8: not read")]
    );
    let batch = read.batch;
    assert_eq!(batch.version, Version::V1_8);
    assert!(batch.transfers.is_empty());
    assert_eq!(&*batch.header.version, "1.8");
    assert_eq!((batch.header.created, batch.header.du_number), (None, None));
    assert_eq!(batch.agencies[0].distinction, None);

    let route = &batch.routes[0];
    assert_eq!(route.route_type, Some(RouteType::ExtraRegional));
    assert_eq!((route.transport_mode.clone(), route.detour), (None, None));
    assert_eq!((route.agency_distinction, route.distinction), (None, None));
    assert_eq!(route.timetable_valid_to, Some(date(2026, 12, 12)));

    let route_stop = &batch.route_stops[0];
    assert_eq!(route_stop.zone.as_deref(), Some("B"));
    assert_eq!(route_stop.attributes, [Some(18), None, None]);
    assert_eq!(
        (route_stop.approximate_minutes, route_stop.route_distinction),
        (None, None)
    );

    let call = &batch.trip_stops[9];
    assert_eq!((call.stop_id, call.stop_post_id), (21, None));
    assert_eq!(
        (call.arrival, call.departure),
        (Some(TimeCell::OtherRoute), Some(TimeCell::OtherRoute))
    );
    assert_eq!(
        (call.earliest_arrival, call.route_distinction),
        (None, None)
    );
    assert_eq!(batch.trip_stops[0].departure, at(5, 4));

    // Real 1.8 batches leave field 5 empty; the fixture's first note was given type 4 and a date.
    let note = &batch.service_notes[0];
    assert_eq!(note.note_type, Some(NoteType::NoService));
    assert_eq!(note.date_from, Some(date(2026, 12, 24)));
    assert_eq!(note.route_distinction, None);
    assert!(
        note.note
            .as_deref()
            .unwrap()
            .contains("\"Unhošť, Náměstí\"")
    );
    assert_eq!(batch.service_notes[1].note_type, None);
}

#[test]
fn jdf_r020_lf_line_ends_and_a_missing_terminator_are_read_and_reported() {
    let read = minimal(
        "Zastavky.txt",
        b"\"1\",\"A\",\"\",\"\",\"\",\"CZ\",\"\",\"\",\"\",\"\",\"\",\"\";\n\
          \"2\",\"B\",\"\",\"\",\"\",\"CZ\",\"\",\"\",\"\",\"\",\"\",\"\";\n\
          \"3\",\"C\",\"\",\"\",\"\",\"CZ\",\"\",\"\",\"\",\"\",\"\",\"\"\n",
    )
    .expect("batch");
    let towns: Vec<&str> = read.batch.stops.iter().map(|s| &*s.town).collect();
    assert_eq!(towns, ["A", "B", "C"]);
    assert_eq!(read.batch.stops[2].attributes, [None; 6]);
    assert_eq!(
        messages(&read.problems),
        [
            (
                1,
                None,
                "record ended by LF without CR (reported once per file)"
            ),
            (3, None, "record without terminator"),
        ]
    );
    assert!(read.problems.iter().all(|p| &*p.file == "Zastavky.txt"));
}

#[test]
fn jdf_r021_r023_short_records_and_bad_values() {
    let read = minimal(
        "Zastavky.txt",
        b"\"1\",\"A\",\"\",\"\",\"\",\"CZ\",\"\",\"\",\"\",\"\",\"\",\"\";\r\n\
          \"2\",\"B\",\"\",\"\",\"\",\"CZ\",\"\",\"\",\"\",\"\",\"\";\r\n\
          \"x3\",\"C\",\"\",\"\",\"\",\"CZ\",\"\",\"\",\"\",\"\",\"\",\"\";\r\n\
          \"4\",\"D\",\"\",\"\",\"\",\"CZ\",\"1a\",\"\",\"\",\"\",\"\",\"\";\r\n",
    )
    .expect("batch");
    let ids: Vec<u64> = read.batch.stops.iter().map(|s| s.id).collect();
    assert_eq!(ids, [1, 4]);
    assert_eq!(read.batch.stops[1].attributes[0], None);
    assert_eq!(
        messages(&read.problems),
        [
            (2, None, "11 fields, 12 expected (record skipped)"),
            (3, Some("id"), "not a number (record skipped): \"x3\""),
            (4, Some("attributes"), "not a number: \"1a\""),
        ]
    );

    let read = minimal(
        "Zasspoje.txt",
        b"\"1\",\"1\",\"1\",\"1\",\"\",\"\",\"\",\"\",\"\",\"1.5\",\"2460\",\"0001\",\"\",\"\",\"1\";\r\n",
    )
    .expect("batch");
    let call = &read.batch.trip_stops[0];
    assert_eq!(
        call.km,
        Some(Km {
            units: 15,
            scale: 1
        })
    );
    assert_eq!((call.arrival, call.departure), (None, at(0, 1)));
    assert_eq!(
        messages(&read.problems),
        [(1, Some("arrival"), "not a time (HHMM, | or <): \"2460\"")]
    );
}
