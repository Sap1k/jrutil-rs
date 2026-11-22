//! Transfers between the parts of one path (R082).

use jr_model::timetable::{LocationRef, Transfer, TransferKind};
use jr_model::{Diagnostic, Diagnostics, Rule, Severity, Timetable, TripId};

use crate::path::Part;

/// Consecutive parts of one path are one train: the rider stays seated between rail parts and
/// changes vehicle at the shared station between rail and replacement bus. Parts that do not
/// meet at one call get no transfer (fixes B11).
pub(crate) const R082: Rule = Rule {
    id: "CZPTT-R082",
    summary: "Consecutive parts sharing their boundary call are linked in seat, or timed across modes",
};

fn boarding_point(timetable: &Timetable, trip: TripId, sequence: i32) -> Option<LocationRef> {
    let call = timetable.trips[trip]
        .calls
        .iter()
        .find(|call| call.sequence == sequence)?;
    Some(match call.boarding_point {
        Some(point) => LocationRef::BoardingPoint(point),
        None => LocationRef::Place(call.place),
    })
}

pub(crate) fn in_path(
    pa: &str,
    trips: &[(TripId, &Part)],
    timetable: &mut Timetable,
    diagnostics: &mut Diagnostics,
) {
    for pair in trips.windows(2) {
        let [(left_trip, left), (right_trip, right)] = pair else {
            continue;
        };
        let last = left.passenger_calls().last().map(|call| call.index);
        let first = right.passenger_calls().next().map(|call| call.index);
        let (Some(last), Some(first)) = (last, first) else {
            continue;
        };
        if last != first {
            diagnostics.push(
                Diagnostic::new(
                    "czptt_parts_do_not_meet",
                    Severity::Warning,
                    pa,
                    format!(
                        "part {} ends at sequence {} and part {} starts at sequence {}; no transfer",
                        left.number,
                        last + 1,
                        right.number,
                        first + 1
                    ),
                )
                .rule(R082),
            );
            continue;
        }
        let sequence = last as i32 + 1;
        let transfer = if left.key.replacement_bus == right.key.replacement_bus {
            Transfer {
                from: None,
                to: None,
                from_route: None,
                to_route: None,
                from_trip: Some(*left_trip),
                to_trip: Some(*right_trip),
                kind: TransferKind::InSeat,
                minimum_time: None,
                maximum_wait: None,
            }
        } else {
            Transfer {
                from: boarding_point(timetable, *left_trip, sequence),
                to: boarding_point(timetable, *right_trip, sequence),
                from_route: None,
                to_route: None,
                from_trip: Some(*left_trip),
                to_trip: Some(*right_trip),
                kind: TransferKind::Timed,
                minimum_time: Some(0),
                maximum_wait: None,
            }
        };
        timetable.transfers.push(transfer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::{Call, SegmentKey, ServiceKey};
    use jr_model::handle::Handle;
    use jr_model::{Feed, Horizon};

    fn call(index: u32) -> Call {
        Call {
            index,
            country: "CZ".into(),
            code: format!("5700{index}").into(),
            source_name: None,
            subsidiary: None,
            arrival: Some(0),
            departure: Some(0),
            repaired: false,
            inferred: false,
            passenger: true,
            activities: Vec::new(),
            responsible_ru: "54".into(),
            category: "Os".into(),
            route_type: 106,
            line: None,
            projected: ServiceKey {
                line: None,
                route_type: 106,
                category: "Os".into(),
            },
            replacement_bus: false,
            train_number: None,
        }
    }

    fn part(number: u16, indices: &[u32]) -> Part {
        Part {
            number,
            key: SegmentKey {
                line: None,
                responsible_ru: "54".into(),
                route_type: 106,
                category: "Os".into(),
                replacement_bus: false,
            },
            mode_block: 1,
            calls: indices.iter().map(|&index| call(index)).collect(),
        }
    }

    #[test]
    fn r082_parts_that_do_not_meet_get_no_in_seat_transfer() {
        let mut timetable = Timetable::new(Feed::Czptt, Horizon::gvd(2026).expect("gvd"));
        let (left, right) = (part(1, &[0, 1]), part(3, &[2, 3]));
        let trips = [
            (TripId::from_index(0), &left),
            (TripId::from_index(1), &right),
        ];
        let mut diagnostics = Diagnostics::new();
        in_path("Pa:x", &trips, &mut timetable, &mut diagnostics);
        assert!(timetable.transfers.is_empty());
        assert_eq!(
            diagnostics
                .iter()
                .map(|d| d.code.as_ref())
                .collect::<Vec<_>>(),
            ["czptt_parts_do_not_meet"]
        );
    }
}
