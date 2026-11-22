//! GTFS reader: hand-written feeds in, typed facts and problems out.

use std::io::{Cursor, Write as _};

use jiff::civil::date;
use jr_formats::gtfs::{Feed, GtfsError, IdRef, Options, Problem, read_files, read_zip};

const AGENCY: &str = "agency_id,agency_name,agency_url,agency_timezone\n\
    A1,Dopravce,https://example.cz,Europe/Prague\n";
const ROUTES: &str = "route_id,agency_id,route_short_name,route_type,route_color\n\
    R1,A1,10,3,FF0000\n\
    R2,A1,,700,\n";
const TRIPS: &str = "route_id,service_id,trip_id,trip_headsign,direction_id,sub_agency_id\n\
    R1,S1,T1,Centrum,0,X7\n\
    R2,S2,T2,,1,\n";
const STOPS: &str = "stop_id,stop_name,stop_lat,stop_lon,location_type,parent_station,asw_node_id\n\
    P1,Hlavní nádraží,49.19,16.61,1,,100\n\
    P1a,Hlavní nádraží,49.191,16.612,0,P1,100\n\
    P2,\"Česká, rozc.\",49.2,16.6,,,\n";
const STOP_TIMES: &str = "trip_id,arrival_time,departure_time,stop_id,stop_sequence,stop_headsign,pickup_type,drop_off_type,shape_dist_traveled\n\
    T1,4:32:00,4:32:30,P1a,1,Centrum,0,1,0\n\
    T1,24:05:00,24:05:00,P2,2,,,,1.5\n\
    T2,07:00:00,07:00:00,P2,1,Centrum,,,\n";
const CALENDAR: &str = "service_id,monday,tuesday,wednesday,thursday,friday,saturday,sunday,start_date,end_date\n\
    S1,1,1,1,1,1,0,0,20261214,20271211\n";
const CALENDAR_DATES: &str = "service_id,date,exception_type\n\
    S1,20261224,2\n\
    S2,20261225,1\n";

fn options() -> Options<'static> {
    Options {
        extra_columns: &[
            ("stops.txt", &["asw_node_id", "asw_stop_id"]),
            ("trips.txt", &["sub_agency_id"]),
        ],
        tables: &["route_sub_agencies.txt", "levels.txt"],
        raw: &["api.txt"],
        shapes: true,
    }
}

/// A feed of the standard members with `replace` substituted or added.
fn feed(replace: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static [u8])> {
    let mut files: Vec<(&str, &[u8])> = vec![
        ("agency.txt", AGENCY.as_bytes()),
        ("routes.txt", ROUTES.as_bytes()),
        ("trips.txt", TRIPS.as_bytes()),
        ("stops.txt", STOPS.as_bytes()),
        ("stop_times.txt", STOP_TIMES.as_bytes()),
        ("calendar.txt", CALENDAR.as_bytes()),
        ("calendar_dates.txt", CALENDAR_DATES.as_bytes()),
    ];
    for &(name, text) in replace {
        match files.iter_mut().find(|(file, _)| *file == name) {
            Some(entry) => entry.1 = text.as_bytes(),
            None => files.push((name, text.as_bytes())),
        }
    }
    files
}

fn without(name: &str) -> Vec<(&'static str, &'static [u8])> {
    feed(&[])
        .into_iter()
        .filter(|(file, _)| *file != name)
        .collect()
}

fn read(files: Vec<(&str, &[u8])>) -> Feed {
    read_files(files, &options()).expect("feed")
}

fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        if name.ends_with('/') {
            writer
                .add_directory(*name, zip::write::SimpleFileOptions::default())
                .expect("directory");
        } else {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .expect("member");
            writer.write_all(bytes).expect("write");
        }
    }
    writer.finish().expect("zip").into_inner()
}

fn messages(feed: &Feed) -> Vec<(String, u64, Option<String>)> {
    feed.problems
        .iter()
        .map(|problem| {
            (
                problem.file.to_string(),
                problem.line,
                problem.column.as_deref().map(str::to_owned),
            )
        })
        .collect()
}

#[test]
fn every_table_and_option_from_a_zip() {
    let mut files = feed(&[
        (
            "transfers.txt",
            "from_stop_id,to_stop_id,from_trip_id,to_trip_id,transfer_type,min_transfer_time,max_waiting_time\n\
             P2,P2,T1,T2,1,,300\n",
        ),
        (
            "feed_info.txt",
            "feed_publisher_name,feed_publisher_url,feed_lang,feed_start_date,feed_end_date\n\
             KORDIS,https://kordis-jmk.cz,cs,20261214,20270131\n\
             other,https://x,en,,\n",
        ),
        (
            "shapes.txt",
            "shape_id,shape_pt_lat,shape_pt_lon,shape_pt_sequence\nH1,49.1,16.6,1\nH2,49.2,16.7,1\nH1,49.3,16.8,2\n",
        ),
        (
            "route_sub_agencies.txt",
            "route_id,sub_agency_id,route_licence_number\nR1, X7 ,100010\n",
        ),
        ("api.txt", "Linka/CVlaku = trip_id: 10/42 = T1\r\n"),
        ("fare_rules.txt", "fare_id\nF\n"),
    ]);
    files.insert(0, ("docs/", b""));
    let bytes = zip(&files);
    let feed = read_zip(Cursor::new(bytes), &options()).expect("feed");
    assert_eq!(feed.problems, []);

    assert_eq!(feed.agencies.len(), 1);
    assert_eq!(feed.agencies[0].agency_id.as_deref(), Some("A1"));
    assert_eq!(
        feed.agencies[0].agency_timezone.as_deref(),
        Some("Europe/Prague")
    );
    assert_eq!(feed.agencies[0].agency_lang, None);

    let r2 = &feed.routes[1];
    assert_eq!(&*r2.route_id, "R2");
    assert_eq!(r2.route_short_name, None);
    assert_eq!(r2.route_type, Some(700));
    assert_eq!(r2.route_color, None);
    assert_eq!(r2.line, 3);
    assert_eq!(feed.routes[0].route_color.as_deref(), Some("FF0000"));

    assert_eq!(feed.trips[0].extra, vec![Some("X7".into())].into());
    assert_eq!(feed.trips[1].extra, vec![None].into());
    assert_eq!(feed.trips[1].direction_id, Some(1));
    assert_eq!(feed.trips[1].trip_headsign, None);

    let p2 = &feed.stops[2];
    assert_eq!(p2.stop_name.as_deref(), Some("Česká, rozc."));
    assert_eq!((p2.stop_lat, p2.stop_lon), (Some(49.2), Some(16.6)));
    assert_eq!(p2.location_type, None);
    assert_eq!(p2.extra, vec![None, None].into());
    assert_eq!(feed.stops[1].parent_station.as_deref(), Some("P1"));
    assert_eq!(feed.stops[1].extra, vec![Some("100".into()), None].into());

    let calendar = &feed.calendars[0];
    assert_eq!(calendar.weekdays, [1, 1, 1, 1, 1, 0, 0].map(Some));
    assert_eq!(calendar.start_date, Some(date(2026, 12, 14)));
    assert_eq!(calendar.end_date, Some(date(2027, 12, 11)));
    assert_eq!(feed.calendar_dates[1].date, Some(date(2026, 12, 25)));
    assert_eq!(feed.calendar_dates[1].exception_type, Some(1));

    let transfer = &feed.transfers[0];
    assert_eq!(transfer.from_trip_id.as_deref(), Some("T1"));
    assert_eq!(transfer.transfer_type, Some(1));
    assert_eq!(transfer.min_transfer_time, None);

    let info = feed.feed_info.as_ref().expect("feed_info");
    assert_eq!(info.feed_publisher_name.as_deref(), Some("KORDIS"));
    assert_eq!(info.feed_end_date, Some(date(2027, 1, 31)));

    let shapes = feed.shapes.as_ref().expect("shapes");
    assert_eq!(shapes.ids, ["H1".into(), "H2".into()]);
    let shape_of: Vec<u32> = shapes.points.iter().map(|point| point.shape).collect();
    assert_eq!(shape_of, [0, 1, 0]);
    assert_eq!(shapes.points[2].shape_pt_sequence, Some(2));

    let first = feed.stop_times.get(0);
    assert_eq!((first.trip, first.stop), (IdRef::Index(0), IdRef::Index(1)));
    assert_eq!(first.arrival_time, Some(4 * 3600 + 32 * 60));
    assert_eq!(first.departure_time, Some(4 * 3600 + 32 * 60 + 30));
    assert_eq!(first.stop_headsign, Some("Centrum"));
    assert_eq!((first.pickup_type, first.drop_off_type), (Some(0), Some(1)));
    assert_eq!(first.timepoint, None);
    assert_eq!(first.shape_dist_traveled, Some(0.0));
    assert_eq!(first.line, 2);
    let second = feed.stop_times.get(1);
    assert_eq!(second.arrival_time, Some(24 * 3600 + 5 * 60));
    assert_eq!(second.stop_headsign, None);
    assert_eq!(second.pickup_type, None);
    assert_eq!(second.shape_dist_traveled, Some(1.5));
    assert_eq!(feed.stop_times.get(2).stop_headsign, Some("Centrum"));

    let table = &feed.other["route_sub_agencies.txt"];
    let licence = table.column("route_licence_number").expect("column");
    assert_eq!(&*table.rows[0][licence], "100010");
    assert_eq!(&*table.rows[0][1], "X7");
    assert_eq!(table.lines, [2]);
    assert!(!feed.other.contains_key("levels.txt"));
    assert_eq!(
        feed.raw["api.txt"],
        b"Linka/CVlaku = trip_id: 10/42 = T1\r\n"
    );
    assert!(feed.members.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(feed.members.contains(&"fare_rules.txt".into()));
    assert_eq!(feed.members.len(), files.len() - 1);
}

#[test]
fn shapes_and_unrequested_members_are_not_read_by_default() {
    let feed = read_files(
        feed(&[("shapes.txt", "shape_id\nH1\n"), ("api.txt", "x")]),
        &Options::default(),
    )
    .expect("feed");
    assert_eq!(feed.shapes, None);
    assert!(feed.raw.is_empty() && feed.other.is_empty());
    assert!(feed.trips[0].extra.is_empty());
    assert_eq!(feed.problems, []);
}

#[test]
fn bom_and_free_column_order() {
    let feed = read(feed(&[(
        "routes.txt",
        "\u{feff}route_type, route_id ,agency_id\n3,R1,A1\n",
    )]));
    assert_eq!(&*feed.routes[0].route_id, "R1");
    assert_eq!(feed.routes[0].route_type, Some(3));
    assert_eq!(feed.problems, []);
}

#[test]
fn invalid_cells_are_problems_and_the_row_is_kept() {
    let feed = read(feed(&[(
        "stop_times.txt",
        "trip_id,stop_id,stop_sequence,arrival_time,departure_time,pickup_type\n\
         T1,P1a,1,4:60:00,4:5:00,0\n\
         T1,P2,x,,,200\n",
    )]));
    assert_eq!(feed.stop_times.len(), 2);
    let first = feed.stop_times.get(0);
    assert_eq!((first.arrival_time, first.departure_time), (None, None));
    assert_eq!(first.stop_sequence, Some(1));
    let second = feed.stop_times.get(1);
    assert_eq!((second.stop_sequence, second.pickup_type), (None, None));
    let at = |line, column: &str| ("stop_times.txt".to_owned(), line, Some(column.to_owned()));
    assert_eq!(
        messages(&feed),
        [
            at(2, "arrival_time"),
            at(2, "departure_time"),
            at(3, "stop_sequence"),
            at(3, "pickup_type"),
        ]
    );
    assert_eq!(
        &*feed.problems[0].message,
        "not an H:MM:SS time: \"4:60:00\""
    );
}

#[test]
fn a_row_with_another_number_of_fields_is_skipped() {
    let feed = read(feed(&[(
        "stops.txt",
        "stop_id,stop_name,stop_lat\nP1,A\nP1a,B,49.1\nP2,C,49.2,extra\n",
    )]));
    let ids: Vec<&str> = feed.stops.iter().map(|stop| &*stop.stop_id).collect();
    assert_eq!(ids, ["P1a"]);
    assert_eq!(
        feed.problems[..2],
        [
            Problem {
                file: "stops.txt".into(),
                line: 2,
                column: None,
                message: "2 fields, the header has 3; row skipped".into(),
            },
            Problem {
                file: "stops.txt".into(),
                line: 4,
                column: None,
                message: "4 fields, the header has 3; row skipped".into(),
            },
        ]
    );
}

#[test]
fn unknown_trip_and_stop_ids_are_kept_addressable() {
    let feed = read(feed(&[(
        "stop_times.txt",
        "trip_id,stop_id,stop_sequence\n\
         T9,P1a,1\n\
         T1,P9,1\n\
         T9,P2,2\n\
         ,P2,1\n",
    )]));
    let st = &feed.stop_times;
    assert_eq!(st.len(), 4);
    assert_eq!(st.get(0).trip, IdRef::Unknown("T9"));
    assert_eq!(st.get(1).stop, IdRef::Unknown("P9"));
    assert_eq!(st.get(2).trip, IdRef::Unknown("T9"));
    assert_eq!(st.get(3).trip, IdRef::Unknown(""));
    assert_eq!(st.unknown_trip_ids(), ["T9".into(), "".into()]);
    assert_eq!(st.unknown_stop_ids(), ["P9".into()]);
    let at = |line, column: &str| ("stop_times.txt".to_owned(), line, Some(column.to_owned()));
    // One problem per distinct unknown ID; the empty one is also an empty required value.
    assert_eq!(
        messages(&feed),
        [
            at(2, "trip_id"),
            at(3, "stop_id"),
            at(5, "trip_id"),
            at(5, "trip_id")
        ]
    );
    let trips: Vec<(IdRef, Vec<usize>)> = st
        .trips()
        .map(|(trip, rows)| (trip, rows.collect()))
        .collect();
    assert_eq!(
        trips,
        [
            (IdRef::Index(0), vec![1]),
            (IdRef::Unknown("T9"), vec![0, 2]),
            (IdRef::Unknown(""), vec![3]),
        ]
    );
}

#[test]
fn per_trip_rows_by_sequence_ties_in_file_order() {
    let feed = read(feed(&[(
        "stop_times.txt",
        "trip_id,stop_id,stop_sequence\n\
         T2,P2,5\n\
         T1,P1a,3\n\
         T2,P1a,2\n\
         T1,P2,1\n\
         T1,P1,3\n\
         T1,P2,\n",
    )]));
    let st = &feed.stop_times;
    assert_eq!(st.trip_rows(0).collect::<Vec<_>>(), [5, 3, 1, 4]);
    assert_eq!(st.trip_rows(1).collect::<Vec<_>>(), [2, 0]);
    assert_eq!(st.trip_rows(1).len(), 2);
    assert_eq!(st.trip_rows(7).count(), 0);
    let order: Vec<IdRef> = st.trips().map(|(trip, _)| trip).collect();
    assert_eq!(order, [IdRef::Index(0), IdRef::Index(1)]);
}

#[test]
fn per_trip_rows_of_an_ordered_file() {
    let feed = read(feed(&[(
        "stop_times.txt",
        "trip_id,stop_id,stop_sequence\nT2,P2,1\nT2,P1a,2\nT1,P2,1\nT1,P1,1\n",
    )]));
    assert_eq!(feed.stop_times.trip_rows(0).collect::<Vec<_>>(), [2, 3]);
    assert_eq!(feed.stop_times.trip_rows(1).collect::<Vec<_>>(), [0, 1]);
}

#[test]
fn duplicate_ids_refer_to_the_first() {
    let feed = read(feed(&[("stops.txt", "stop_id\nP1\nP1a\nP2\nP2\n")]));
    assert_eq!(feed.stop_times.get(1).stop, IdRef::Index(2));
    assert_eq!(
        messages(&feed),
        [("stops.txt".to_owned(), 5, Some("stop_id".to_owned()))]
    );
}

#[test]
fn missing_required_member_or_column_is_an_error() {
    assert!(matches!(
        read_files(without("stops.txt"), &options()),
        Err(GtfsError::MissingMember("stops.txt"))
    ));
    let error = read_files(
        feed(&[("trips.txt", "route_id,trip_id\nR1,T1\n")]),
        &options(),
    );
    assert!(matches!(
        error,
        Err(GtfsError::MissingColumn { ref file, column: "service_id" }) if &**file == "trips.txt"
    ));
    let error = read_files(feed(&[("stop_times.txt", "")]), &options());
    assert!(matches!(
        error,
        Err(GtfsError::MissingColumn {
            column: "trip_id",
            ..
        })
    ));
    let neither: Vec<_> = without("calendar.txt")
        .into_iter()
        .filter(|(name, _)| *name != "calendar_dates.txt")
        .collect();
    assert!(matches!(
        read_files(neither, &options()),
        Err(GtfsError::NoCalendar)
    ));
    let mut bad = feed(&[]);
    bad[0].1 = b"agency_id\n\xff\n";
    assert!(matches!(
        read_files(bad, &options()),
        Err(GtfsError::Utf8 { line: 2, .. })
    ));
    assert!(matches!(
        read_zip(Cursor::new(b"not a zip".to_vec()), &options()),
        Err(GtfsError::Zip(_))
    ));
}

#[test]
fn calendar_only_and_calendar_dates_only() {
    let only_calendar = read(without("calendar_dates.txt"));
    assert_eq!(only_calendar.calendars.len(), 1);
    assert!(only_calendar.calendar_dates.is_empty());
    let only_dates = read(without("calendar.txt"));
    assert!(only_dates.calendars.is_empty());
    assert_eq!(only_dates.calendar_dates.len(), 2);
    assert_eq!(only_dates.problems, []);
}

#[test]
fn a_feed_in_one_directory_is_read_and_said() {
    let files: Vec<(String, &[u8])> = feed(&[])
        .into_iter()
        .map(|(name, bytes)| (format!("jmk/{name}"), bytes))
        .collect();
    let feed = read_files(
        files.iter().map(|(name, bytes)| (name.as_str(), *bytes)),
        &options(),
    )
    .expect("feed");
    assert_eq!(feed.routes.len(), 2);
    assert!(feed.members.contains(&"stops.txt".into()));
    assert_eq!(messages(&feed), [("jmk/".to_owned(), 0, None)]);
}

#[test]
fn a_feed_nested_beside_other_members_is_an_error() {
    let mut files: Vec<(String, &[u8])> = feed(&[])
        .into_iter()
        .map(|(name, bytes)| (format!("jmk/{name}"), bytes))
        .collect();
    files.push(("readme.txt".into(), b"x"));
    let bytes = zip(&files
        .iter()
        .map(|(name, bytes)| (name.as_str(), *bytes))
        .collect::<Vec<_>>());
    assert!(matches!(
        read_zip(Cursor::new(bytes), &options()),
        Err(GtfsError::Nested { ref directory }) if &**directory == "jmk/"
    ));
}

#[test]
fn member_names_are_matched_exactly() {
    let mut files = feed(&[]);
    files[3].0 = "Stops.txt";
    assert!(matches!(
        read_files(files, &options()),
        Err(GtfsError::MissingMember("stops.txt"))
    ));
}

#[test]
fn an_empty_optional_member_reads_as_empty() {
    let feed = read(feed(&[("transfers.txt", "")]));
    assert!(feed.transfers.is_empty());
    assert_eq!(messages(&feed), [("transfers.txt".to_owned(), 0, None)]);
}

#[test]
fn same_bytes_same_feed() {
    let files = feed(&[(
        "stop_times.txt",
        "trip_id,stop_id,stop_sequence,shape_dist_traveled\nT2,P2,5,\nT1,P9,3,2.5\nT9,P1,1,\n",
    )]);
    assert_eq!(read(files.clone()), read(files));
}
