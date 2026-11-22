//! Normalizing calls, repairing and checking times, projecting line boundaries (R026–R040,
//! R066).

mod support;

use jiff::civil::date;
use jr_czptt::path::RejectionReason;
use support::*;

#[test]
fn r026_paths_come_in_pa_order_whatever_the_input_order_and_thread_count() {
    let messages = || {
        vec![
            annual(3, simple("3")),
            annual(1, simple("10")),
            annual(2, simple("2")),
        ]
    };
    let run = |threads: usize| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .expect("pool")
            .install(|| compile(messages()))
    };
    let (one, one_diagnostics) = run(1);
    let (four, four_diagnostics) = run(4);
    let order: Vec<&str> = one.paths.iter().map(|path| &*path.pa).collect();
    assert_eq!(
        order,
        ["Pa:54:10:00:2026", "Pa:54:2:00:2026", "Pa:54:3:00:2026"]
    );
    assert_eq!(one, four);
    assert_eq!(one_diagnostics, four_diagnostics);
}

#[test]
fn r027_a_time_is_the_local_reading_plus_whole_days_of_offset() {
    let mut late = stop("57016", "08:20:00");
    late.timings = vec![timing("ALA", "08:20:00", 1), timing("ALD", "08:21:30", 1)];
    late.timings.push(timing("ELA", "07:00:00", 0));
    // The written UTC offset is ignored (Q4).
    late.timings[0].offset_seconds = Some(7200);
    let (path, diagnostics) = compile_one(timetable(vec![stop("57076", "08:00:00"), late]));
    assert_eq!(path.calls[1].arrival, Some(hms(32, 20, 0)));
    assert_eq!(path.calls[1].departure, Some(hms(32, 21, 30)));
    assert!(has(&diagnostics, "czptt_unknown_timing_qualifier"));
}

#[test]
fn r028_a_call_with_one_time_gets_it_on_both_sides() {
    let mut first = stop("57076", "08:00:00");
    first.timings.retain(|t| &*t.qualifier == "ALD");
    let mut last = stop("57016", "08:20:00");
    last.timings.retain(|t| &*t.qualifier == "ALA");
    let (path, _) = compile_one(timetable(vec![first, last]));
    assert_eq!(path.calls[0].arrival, Some(hms(8, 0, 0)));
    assert_eq!(path.calls[1].departure, Some(hms(8, 20, 0)));
}

#[test]
fn r029_a_passenger_call_has_activity_0001_and_cz13_does_not_suppress_it() {
    let (path, diagnostics) = compile_one(timetable(vec![
        location("57076", "Praha", Some("08:00:00"), &["0001", "CZ13"]),
        location("57050", "Výhybna", Some("08:05:00"), &["0040"]),
        stop("57016", "08:20:00"),
    ]));
    let passenger: Vec<bool> = path.calls.iter().map(|c| c.passenger).collect();
    assert_eq!(passenger, [true, false, true]);
    assert_eq!(messages_of(&diagnostics, "czptt_unpublished_stop").len(), 1);
}

#[test]
fn r030_an_unknown_activity_code_is_diagnosed_and_ignored() {
    let (path, diagnostics) = compile_one(timetable(vec![
        location("57076", "Praha", Some("08:00:00"), &["XX99", "0001"]),
        stop("57016", "08:20:00"),
    ]));
    assert!(path.calls[0].passenger);
    assert_eq!(path.parts.len(), 1);
    let unknown = messages_of(&diagnostics, "czptt_unknown_activity");
    assert_eq!(unknown.len(), 1);
    assert!(unknown[0].contains("XX99"));
}

#[test]
fn r031_the_last_value_of_a_repeated_parameter_wins() {
    let first = line(
        line(bus(bus(stop("57076", "08:00:00"), "1"), "0"), "101"),
        "112",
    );
    let (path, _) = compile_one(timetable(vec![
        first,
        line(stop("57016", "08:20:00"), "112"),
    ]));
    assert!(!path.calls[0].replacement_bus);
    assert_eq!(path.calls[0].line.as_deref(), Some("112"));
}

#[test]
fn r032_location_line_codes_clear_in_both_directions() {
    let (path, _) = compile_one(timetable(vec![
        line(stop("57076", "08:00:00"), "101"),
        line(stop("57050", "08:10:00"), " 101 "),
        stop("57016", "08:20:00"),
        line(stop("54357", "09:00:00"), " "),
    ]));
    let lines: Vec<Option<&str>> = path.calls.iter().map(|c| c.line.as_deref()).collect();
    assert_eq!(lines, [Some("101"), Some("101"), None, None]);

    // Without any location-level code, the root code applies everywhere.
    let mut root_only = timetable(vec![stop("57076", "08:00:00"), stop("57016", "08:20:00")]);
    root_only.parameters.push(jr_formats::czptt::Parameter {
        name: "CZPassengerServiceNumber".into(),
        value: "101".into(),
    });
    let (path, _) = compile_one(root_only);
    assert!(path.calls.iter().all(|c| c.line.as_deref() == Some("101")));
}

#[test]
fn r033_the_operator_starts_at_the_tr_company_and_carries_forward() {
    let (path, diagnostics) = compile_one(timetable(vec![
        operator(stop("57076", "08:00:00"), None),
        operator(stop("57050", "08:10:00"), Some("80")),
        operator(stop("57016", "08:20:00"), None),
    ]));
    let operators: Vec<&str> = path.calls.iter().map(|c| &*c.responsible_ru).collect();
    assert_eq!(operators, ["54", "80", "80"]);
    assert!(!has(&diagnostics, "czptt_responsible_ru_missing"));
}

#[test]
fn r034_the_category_is_the_catalogue_label_carried_forward() {
    let mut traffic = stop("57040", "08:15:00");
    traffic.traffic_type = Some("R".into());
    let (path, diagnostics) = compile_one(timetable(vec![
        stop("57076", "08:00:00"),
        commercial(stop("57050", "08:05:00"), "84"),
        stop("57060", "08:10:00"),
        traffic,
        commercial(stop("57016", "08:20:00"), "ZZ"),
    ]));
    let categories: Vec<&str> = path.calls.iter().map(|c| &*c.category).collect();
    assert_eq!(categories, ["Vlak", "Os", "Os", "R", "ZZ"]);
    let unknown = messages_of(&diagnostics, "czptt_unknown_category");
    assert_eq!(unknown.len(), 1);
    assert!(unknown[0].contains("ZZ"));
}

#[test]
fn r035_the_route_type_follows_the_category() {
    let (path, _) = compile_one(timetable(vec![
        stop("57076", "08:00:00"),
        commercial(stop("57050", "08:10:00"), "84"),
        commercial(stop("57016", "08:20:00"), "RJ"),
    ]));
    let types: Vec<i32> = path.calls.iter().map(|c| c.route_type).collect();
    assert_eq!(types, [100, 106, 102]);
}

#[test]
fn r036_replacement_bus_is_exactly_flag_one() {
    let (path, _) = compile_one(timetable(vec![
        bus(stop("57076", "08:00:00"), "1"),
        bus(stop("57050", "08:10:00"), "yes"),
        bus(stop("57016", "08:20:00"), "0"),
        stop("54357", "09:00:00"),
    ]));
    let flags: Vec<bool> = path.calls.iter().map(|c| c.replacement_bus).collect();
    assert_eq!(flags, [true, false, false, false]);
}

#[test]
fn r037_a_flagged_time_is_bounded_by_its_neighbours() {
    // Arrival 08:05 after the next call's 08:04: interpolated to 08:02, departure follows.
    let mut flagged = with(stop("53234", "08:05:00"), "CZInconsistentTime", "1");
    flagged.timings[1] = timing("ALD", "08:00:00", 0);
    let (path, diagnostics) = compile_one(timetable(vec![
        stop("53254", "08:00:00"),
        flagged,
        stop("53244", "08:04:00"),
    ]));
    assert_eq!(path.rejection, None);
    let call = &path.calls[1];
    assert_eq!(
        (call.arrival, call.departure),
        (Some(hms(8, 2, 0)), Some(hms(8, 2, 0)))
    );
    assert!(call.repaired);
    let repaired = messages_of(&diagnostics, "czptt_inconsistent_time_repaired");
    assert_eq!(repaired.len(), 1);
    assert!(repaired[0].contains("28800"));

    // A departure before a valid arrival is raised to it.
    let mut flagged = with(stop("53050", "08:03:00"), "CZInconsistentTime", "1");
    flagged.timings[1] = timing("ALD", "08:01:00", 0);
    let (path, _) = compile_one(timetable(vec![
        stop("58026", "08:00:00"),
        flagged,
        stop("53060", "08:06:00"),
    ]));
    assert_eq!(path.calls[1].departure, Some(hms(8, 3, 0)));

    // The root-level flag is not read.
    let mut root = timetable(vec![stop("57076", "08:00:00"), stop("57016", "08:20:00")]);
    root.parameters.push(jr_formats::czptt::Parameter {
        name: "CZInconsistentTime".into(),
        value: "1".into(),
    });
    let (path, diagnostics) = compile_one(root);
    assert_eq!(path.parts.len(), 1);
    assert!(!has(&diagnostics, "czptt_inconsistent_time_repaired"));
}

#[test]
fn r038_a_path_without_a_passenger_call_is_rejected() {
    let (path, diagnostics) = compile_one(timetable(vec![
        location("57076", "Praha", Some("08:00:00"), &["0040"]),
        point("57016", "08:20:00"),
    ]));
    assert!(path.parts.is_empty());
    assert_eq!(
        path.rejection.map(|r| r.reason),
        Some(RejectionReason::NoPassengerCall)
    );
    assert_eq!(path.calls.len(), 2);
    assert!(has(&diagnostics, "czptt_path_without_passenger_call"));
}

#[test]
fn r039_times_that_go_backwards_reject_the_path() {
    let (path, diagnostics) = compile_one(timetable(vec![
        stop("57076", "08:10:00"),
        stop("57016", "08:05:00"),
    ]));
    let rejection = path.rejection.expect("rejected");
    assert_eq!(
        rejection.reason,
        RejectionReason::EventPrecedesPreviousEvent
    );
    assert_eq!(rejection.sequence, Some(2));
    assert_eq!(rejection.previous_seconds, Some(hms(8, 10, 0)));
    assert_eq!(rejection.current_seconds, Some(hms(8, 5, 0)));
    assert!(path.parts.is_empty());
    assert!(has(&diagnostics, "czptt_path_chronology"));

    // A flagged call between backward anchors is left alone and the path is rejected.
    let (path, _) = compile_one(timetable(vec![
        stop("53254", "08:10:00"),
        with(stop("53234", "08:08:00"), "CZInconsistentTime", "1"),
        stop("53244", "08:05:00"),
    ]));
    assert!(path.rejection.is_some());
    assert!(!path.calls[1].repaired);
}

#[test]
fn r040_a_line_change_at_a_railway_point_moves_to_a_passenger_call() {
    // Deferred to the next passenger call.
    let (path, diagnostics) = compile_one(timetable(vec![
        line(stop("57076", "08:00:00"), "101"),
        line(point("93001", "08:01:00"), "112"),
        line(stop("57050", "08:10:00"), "112"),
        line(stop("57016", "08:20:00"), "112"),
    ]));
    let projected: Vec<Option<&str>> = path
        .calls
        .iter()
        .map(|c| c.projected.line.as_deref())
        .collect();
    assert_eq!(
        projected,
        [Some("101"), Some("101"), Some("112"), Some("112")]
    );
    assert_eq!(path.calls[1].line.as_deref(), Some("112"));
    let moved = messages_of(&diagnostics, "czptt_line_boundary_moved");
    assert_eq!(
        moved,
        ["line change at sequence 2 to the next passenger call at sequence 3"]
    );

    // Coalesced with an operator change between the same passenger calls.
    let (path, diagnostics) = compile_one(timetable(vec![
        line(stop("57076", "08:00:00"), "101"),
        operator(line(point("93001", "08:05:00"), "101"), Some("80")),
        operator(line(point("93002", "08:06:00"), "112"), Some("80")),
        operator(line(stop("57050", "08:10:00"), "112"), Some("80")),
        operator(line(stop("57016", "08:20:00"), "112"), Some("80")),
    ]));
    assert_eq!(path.calls[1].projected.line.as_deref(), Some("112"));
    let moved = messages_of(&diagnostics, "czptt_line_boundary_moved");
    assert_eq!(
        moved,
        ["line change at sequence 3 to the operator change at sequence 2"]
    );
    assert_eq!(path.parts.len(), 2);

    // After the last passenger call, nothing moves.
    let (path, diagnostics) = compile_one(timetable(vec![
        line(stop("57076", "08:00:00"), "101"),
        line(stop("57016", "08:20:00"), "101"),
        point("57017", "08:25:00"),
    ]));
    assert_eq!(path.calls[2].projected.line.as_deref(), Some("101"));
    assert!(has(&diagnostics, "czptt_line_boundary_ignored"));
}

#[test]
fn r066_negative_times_move_forward_and_the_dates_back() {
    let mut first = stop("57076", "23:58:00");
    first.timings = vec![timing("ALA", "23:58:00", -1), timing("ALD", "23:58:00", -1)];
    let (path, diagnostics) = compile_one(timetable(vec![first, stop("57016", "00:10:00")]));
    assert_eq!(path.calls[0].arrival, Some(hms(23, 58, 0)));
    assert_eq!(path.calls[1].arrival, Some(hms(24, 10, 0)));
    let (paths, _) = compile(vec![annual(1, simple("1"))]);
    assert_eq!(
        path.dates.dates(paths.horizon).collect::<Vec<_>>(),
        [date(2025, 12, 13)]
    );
    assert_eq!(path.first_date, date(2025, 12, 14));
    assert_eq!(
        messages_of(&diagnostics, "czptt_negative_times_shifted").len(),
        1
    );
}
