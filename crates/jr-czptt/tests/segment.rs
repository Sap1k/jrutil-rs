//! Trip parts: selection, collapse, segmentation, edge timing and railway points (R041–R051,
//! R117).

mod support;

use jr_czptt::path::{Part, RejectionReason};
use support::*;

fn passenger_sequences(part: &Part) -> Vec<u32> {
    part.calls
        .iter()
        .filter(|call| call.passenger)
        .map(|call| call.sequence())
        .collect()
}

#[test]
fn r041_parts_are_cut_from_the_passenger_calls() {
    let (path, _) = compile_one(timetable(vec![
        line(stop("57076", "08:00:00"), "101"),
        line(point("57050", "08:05:00"), "101"),
        line(stop("57016", "08:20:00"), "101"),
        point("57017", "08:25:00"),
    ]));
    assert_eq!(path.parts.len(), 1);
    let part = &path.parts[0];
    assert_eq!(passenger_sequences(part), [1, 3]);
    // The line ends at the deadhead point after the terminus without splitting anything.
    assert_eq!(part.key.line.as_deref(), Some("101"));
}

#[test]
fn r042_a_whole_run_of_subsidiary_rows_collapses_into_one_call() {
    let (path, diagnostics) = compile_one(timetable(vec![
        stop("57076", "08:00:00"),
        track(stop("57050", "08:10:00"), "1"),
        track(stop("57050", "08:11:00"), "2"),
        track(stop("57050", "08:12:00"), "3"),
        stop("57016", "08:20:00"),
    ]));
    let part = &path.parts[0];
    assert_eq!(sequences(&part.calls), [1, 2, 5]);
    let station = &part.calls[1];
    assert_eq!(station.arrival, Some(hms(8, 10, 0)));
    assert_eq!(station.departure, Some(hms(8, 12, 0)));
    assert_eq!(station.subsidiary.as_ref().map(|s| &*s.code), Some("1"));
    assert_eq!(
        messages_of(&diagnostics, "czptt_subsidiary_rows_collapsed").len(),
        2
    );
}

#[test]
fn r043_a_path_with_one_passenger_call_is_rejected() {
    let (path, diagnostics) = compile_one(timetable(vec![
        stop("57076", "08:00:00"),
        point("57016", "08:20:00"),
    ]));
    assert!(path.parts.is_empty());
    assert_eq!(
        path.rejection.map(|r| r.reason),
        Some(RejectionReason::TooFewPassengerCalls)
    );
    assert!(has(&diagnostics, "czptt_path_too_few_passenger_calls"));
}

#[test]
fn r044_a_category_change_splits_parts_that_share_the_boundary_call() {
    let (path, _) = compile_one(timetable(vec![
        commercial(stop("57076", "08:00:00"), "Os"),
        commercial(stop("57050", "08:10:00"), "RJ"),
        commercial(stop("57016", "08:20:00"), "RJ"),
    ]));
    let parts: Vec<(u16, &str, Vec<u32>)> = path
        .parts
        .iter()
        .map(|p| (p.number, &*p.key.category, sequences(&p.calls)))
        .collect();
    assert_eq!(parts, [(1, "Os", vec![1, 2]), (2, "RJ", vec![2, 3])]);
    assert_eq!(path.parts[0].key.route_type, 106);
    assert_eq!(path.parts[1].key.route_type, 102);

    // A change at the final call does not split (Q8).
    let (path, diagnostics) = compile_one(timetable(vec![
        commercial(stop("57076", "08:00:00"), "Os"),
        commercial(stop("57050", "08:10:00"), "Os"),
        commercial(stop("57016", "08:20:00"), "RJ"),
    ]));
    assert_eq!(path.parts.len(), 1);
    assert_eq!(&*path.parts[0].key.category, "Os");
    assert!(has(&diagnostics, "czptt_terminal_change_ignored"));
}

#[test]
fn r045_a_replacement_bus_part_has_the_bus_route_type() {
    let (path, _) = compile_one(timetable(vec![
        commercial(bus(stop("53001", "08:00:00"), "1"), "84"),
        commercial(bus(stop("53003", "08:10:00"), "1"), "84"),
    ]));
    let key = &path.parts[0].key;
    assert!(key.replacement_bus);
    assert_eq!(key.route_type, 714);
    assert_eq!(&*key.category, "Os");
}

#[test]
fn r046_mode_blocks_count_switches_between_rail_and_bus() {
    let (path, diagnostics) = compile_one(timetable(vec![
        line(stop("57076", "08:00:00"), "101"),
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
        bus(line(track(point("93001", "08:15:00"), "99"), "101"), "1"),
        bus(line(track(stop("57016", "08:20:00"), "3"), "101"), "0"),
        line(stop("54357", "09:00:00"), "101"),
    ]));
    let parts: Vec<(u16, bool, u16, Vec<u32>)> = path
        .parts
        .iter()
        .map(|p| {
            (
                p.number,
                p.key.replacement_bus,
                p.mode_block,
                sequences(&p.calls),
            )
        })
        .collect();
    assert_eq!(
        parts,
        [
            (1, false, 1, vec![1, 2]),
            (2, true, 2, vec![2, 4]),
            (3, false, 3, vec![4, 5]),
        ]
    );
    // The bus part's railway point belongs to no part.
    assert_eq!(
        messages_of(&diagnostics, "czptt_railway_points_unassigned"),
        ["railway points at sequence 3 belong to no rail part"]
    );
}

#[test]
fn r047_a_replacement_bus_part_holds_passenger_calls_only() {
    let (path, _) = compile_one(timetable(vec![
        stop("57076", "08:00:00"),
        bus(stop("57050", "08:10:00"), "1"),
        bus(point("93001", "08:15:00"), "1"),
        bus(stop("57016", "08:20:00"), "0"),
        stop("54357", "09:00:00"),
    ]));
    let bus_part = path
        .parts
        .iter()
        .find(|p| p.key.replacement_bus)
        .expect("bus");
    assert!(bus_part.calls.iter().all(|call| call.passenger));
    assert_eq!(sequences(&bus_part.calls), [2, 4]);

    // A bus flag between two railway points therefore never forms a part.
    let (path, _) = compile_one(timetable(vec![
        stop("57076", "08:00:00"),
        bus(point("93001", "08:05:00"), "1"),
        bus(point("93002", "08:10:00"), "0"),
        stop("57016", "08:20:00"),
    ]));
    assert_eq!(path.parts.len(), 1);
    assert!(!path.parts[0].key.replacement_bus);
    assert_eq!(sequences(&path.parts[0].calls), [1, 2, 3, 4]);
}

#[test]
fn r049_parts_that_share_an_untimed_boundary_call_carry_one_interpolated_time() {
    // The untimed boundary is one call of both parts (R048, copying a timed counterpart, is
    // gone); both copies are interpolated alike between 08:05 and 08:20.
    let (path, diagnostics) = compile_one(timetable(vec![
        stop("57076", "08:00:00"),
        bus(
            location("57050", "Poříčany operational", Some("08:05:00"), &[]),
            "1",
        ),
        bus(untimed(track(stop("57050", "08:10:00"), "1")), "1"),
        bus(stop("57016", "08:20:00"), "1"),
    ]));
    assert_eq!(path.parts.len(), 2);
    let left = path.parts[0].calls.last().expect("edge");
    let right = path.parts[1].calls.first().expect("edge");
    assert_eq!(left.sequence(), 3);
    assert_eq!(right.sequence(), 3);
    assert_eq!(left.arrival, Some(hms(8, 12, 30)));
    assert_eq!(
        (left.arrival, left.departure),
        (right.arrival, right.departure)
    );
    assert!(left.inferred && right.inferred);
    assert_eq!(
        messages_of(&diagnostics, "czptt_edge_time_interpolated").len(),
        2
    );
}

#[test]
fn r049_an_untimed_part_edge_is_interpolated_between_timed_calls() {
    let (path, diagnostics) = compile_one(timetable(vec![
        bus(stop("57076", "08:00:00"), "1"),
        bus(track(stop("57050", "08:10:00"), "BUS"), "1"),
        bus(untimed(track(stop("57050", "08:10:00"), "1")), "0"),
        stop("57016", "08:20:00"),
    ]));
    assert_eq!(path.parts.len(), 2);
    for part in &path.parts {
        let edge = part.calls.iter().find(|c| c.sequence() == 3).expect("edge");
        assert_eq!(
            (edge.arrival, edge.departure),
            (Some(hms(8, 15, 0)), Some(hms(8, 15, 0)))
        );
        assert!(edge.inferred);
    }
    // The path's own call keeps the source's absence of a time.
    assert_eq!(path.calls[2].arrival, None);
    assert_eq!(
        messages_of(&diagnostics, "czptt_edge_time_interpolated").len(),
        2
    );
}

#[test]
fn r050_a_part_with_an_uninferable_edge_is_dropped() {
    let (path, diagnostics) = compile_one(timetable(vec![
        untimed(stop("57076", "08:00:00")),
        stop("57016", "08:20:00"),
    ]));
    assert!(path.parts.is_empty());
    assert_eq!(path.rejection, None);
    assert_eq!(
        messages_of(&diagnostics, "czptt_part_untimed_edge"),
        ["part 1 is dropped: no time can be inferred at sequence 1"]
    );
}

#[test]
fn r051_an_operator_change_at_a_railway_point_is_diagnosed() {
    let (path, diagnostics) = compile_one(timetable(vec![
        operator(point("93000", "07:55:00"), Some("80")),
        operator(line(stop("57076", "08:00:00"), "101"), Some("54")),
        operator(line(point("93001", "08:05:00"), "101"), Some("80")),
        operator(line(stop("57050", "08:10:00"), "101"), Some("80")),
        operator(line(stop("57016", "08:20:00"), "101"), Some("80")),
    ]));
    let operators: Vec<&str> = path.parts.iter().map(|p| &*p.key.responsible_ru).collect();
    assert_eq!(operators, ["54", "80"]);
    // The change before the first passenger call moves nothing and is not reported.
    assert_eq!(
        messages_of(&diagnostics, "czptt_operator_boundary_moved"),
        ["the operator change at sequence 3 moves to the next passenger call"]
    );
}

#[test]
fn r117_railway_points_join_the_rail_part_that_encloses_them() {
    let (path, diagnostics) = compile_one(timetable(vec![
        point("93000", "07:55:00"),
        line(stop("57076", "08:00:00"), "101"),
        line(point("93001", "08:05:00"), "101"),
        line(point("93002", "08:06:00"), "112"),
        line(stop("57050", "08:10:00"), "112"),
        line(point("93003", "08:15:00"), "112"),
        line(stop("57016", "08:20:00"), "112"),
        point("93004", "08:25:00"),
    ]));
    let parts: Vec<(Vec<u32>, Vec<u32>)> = path
        .parts
        .iter()
        .map(|p| (sequences(&p.calls), passenger_sequences(p)))
        .collect();
    assert_eq!(
        parts,
        [
            (vec![1, 2, 3, 4, 5], vec![2, 5]),
            (vec![5, 6, 7, 8], vec![5, 7]),
        ]
    );
    assert!(!has(&diagnostics, "czptt_railway_points_unassigned"));

    // A replacement bus that opens the path takes no leading railway point.
    let (path, diagnostics) = compile_one(timetable(vec![
        point("93000", "07:55:00"),
        bus(stop("57076", "08:00:00"), "1"),
        stop("57050", "08:10:00"),
        stop("57016", "08:20:00"),
    ]));
    assert_eq!(sequences(&path.parts[0].calls), [2, 3]);
    assert_eq!(sequences(&path.parts[1].calls), [3, 4]);
    assert_eq!(
        messages_of(&diagnostics, "czptt_railway_points_unassigned"),
        ["railway points at sequence 1 belong to no rail part"]
    );
}
