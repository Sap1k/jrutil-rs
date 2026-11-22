//! `transfer`, `service_note` and `assignment`.
//!
//! Neither a transfer nor an assignment has a natural key; both get a content ID
//! (`identity::content`) over the published values that make the row what it is, an absent
//! value as the empty string, in this order:
//!
//! - transfer: `from_location_id`, `to_location_id`, `from_route_id`, `to_route_id`,
//!   `from_trip_id`, `to_trip_id`, `transfer_type` (the GTFS code);
//! - assignment: `scope`, `route_id`, `trip_id`, `call_sequence`, `call_sequence_to`,
//!   `location_id`, `kind`, `source_code`, `note_id`, `source_object_id`.

use jr_model::identity;
use jr_model::timetable::{Scope, TransferKind};

use super::PublishError;
use super::assemble::{self, date, int16, int32, text};
use super::scope::Published;
use crate::table::Table;

fn transfer_type(kind: TransferKind) -> i16 {
    match kind {
        TransferKind::Recommended => 0,
        TransferKind::Timed => 1,
        TransferKind::MinimumTime => 2,
        TransferKind::NotPossible => 3,
        TransferKind::InSeat => 4,
        TransferKind::InSeatNotAllowed => 5,
    }
}

pub fn transfer(p: &Published) -> Result<Table, PublishError> {
    let t = p.t;
    struct Row<'a> {
        key: String,
        refs: [Option<&'a str>; 6],
        kind: i16,
        minimum: Option<i32>,
        wait: Option<i32>,
    }
    let rows: Vec<Row> = p
        .transfers
        .iter()
        .map(|&index| {
            let row = &t.transfers[index];
            let refs = [
                row.from.map(|r| p.location_id(r)),
                row.to.map(|r| p.location_id(r)),
                row.from_route.map(|r| t.routes[r].id.as_str()),
                row.to_route.map(|r| t.routes[r].id.as_str()),
                row.from_trip.map(|r| t.trips[r].id.as_str()),
                row.to_trip.map(|r| t.trips[r].id.as_str()),
            ];
            let kind = transfer_type(row.kind);
            let kind_text = kind.to_string();
            let names = [
                "from_location_id",
                "to_location_id",
                "from_route_id",
                "to_route_id",
                "from_trip_id",
                "to_trip_id",
            ];
            let mut fields: Vec<(&str, &str)> = names
                .iter()
                .zip(&refs)
                .map(|(name, value)| (*name, value.unwrap_or("")))
                .collect();
            fields.push(("transfer_type", &kind_text));
            Row {
                key: identity::content(t.feed, "transfer", &fields).to_string(),
                refs,
                kind,
                minimum: row.minimum_time,
                wait: row.maximum_wait,
            }
        })
        .collect();
    let reference = |i: usize| text(rows.iter().map(move |r| r.refs[i]));
    assemble::finish(
        "transfer",
        vec![
            (
                "transfer_key",
                text(rows.iter().map(|r| Some(r.key.as_str()))),
            ),
            ("from_location_id", reference(0)),
            ("to_location_id", reference(1)),
            ("from_route_id", reference(2)),
            ("to_route_id", reference(3)),
            ("from_trip_id", reference(4)),
            ("to_trip_id", reference(5)),
            ("transfer_type", int16(rows.iter().map(|r| Some(r.kind)))),
            (
                "minimum_transfer_time",
                int32(rows.iter().map(|r| r.minimum)),
            ),
            ("maximum_waiting_time", int32(rows.iter().map(|r| r.wait))),
        ],
    )
}

pub fn service_note(p: &Published) -> Result<Table, PublishError> {
    let rows: Vec<_> =
        p.t.notes
            .iter()
            .filter(|(id, _)| p.note[jr_model::handle::Handle::index(*id)])
            .map(|(_, row)| row)
            .collect();
    assemble::finish(
        "service_note",
        vec![
            ("note_id", text(rows.iter().map(|r| Some(r.id.as_str())))),
            ("kind", text(rows.iter().map(|r| Some(r.kind.as_str())))),
            ("label", text(rows.iter().map(|r| r.label.as_deref()))),
            ("text", text(rows.iter().map(|r| r.text.as_deref()))),
            ("valid_from", date(rows.iter().map(|r| r.valid_from))),
            ("valid_to", date(rows.iter().map(|r| r.valid_to))),
            (
                "service_note_type",
                text(rows.iter().map(|r| r.note_type.as_deref())),
            ),
            (
                "source_object_id",
                text(rows.iter().map(|r| Some(&*r.source_object))),
            ),
        ],
    )
}

pub fn assignment(p: &Published) -> Result<Table, PublishError> {
    let t = p.t;
    struct Row<'a> {
        id: String,
        scope: &'static str,
        kind: &'static str,
        route: Option<&'a str>,
        trip: Option<&'a str>,
        from: Option<i32>,
        to: Option<i32>,
        location: Option<&'a str>,
        source_code: Option<&'a str>,
        note: Option<&'a str>,
        source_object: Option<&'a str>,
    }
    let rows: Vec<Row> = p
        .assignments
        .iter()
        .map(|&index| {
            let row = &t.assignments[index];
            let trip = |id: jr_model::TripId| Some(t.trips[id].id.as_str());
            let (scope, route, trip, from, to, location) = match row.scope {
                Scope::Route(id) => (
                    "route",
                    Some(t.routes[id].id.as_str()),
                    None,
                    None,
                    None,
                    None,
                ),
                Scope::Trip(id) => ("trip", None, trip(id), None, None, None),
                Scope::Call { trip: id, sequence } => {
                    ("call", None, trip(id), Some(sequence), None, None)
                }
                Scope::CallRange { trip: id, from, to } => {
                    ("call_range", None, trip(id), Some(from), Some(to), None)
                }
                Scope::Location(reference) => (
                    "location",
                    None,
                    None,
                    None,
                    None,
                    Some(p.location_id(reference)),
                ),
            };
            let kind = row.kind.as_str();
            let note = row.note.map(|id| t.notes[id].id.as_str());
            let number = |value: Option<i32>| value.map(|v| v.to_string()).unwrap_or_default();
            let (from_text, to_text) = (number(from), number(to));
            let fields = [
                ("scope", scope),
                ("route_id", route.unwrap_or("")),
                ("trip_id", trip.unwrap_or("")),
                ("call_sequence", &from_text),
                ("call_sequence_to", &to_text),
                ("location_id", location.unwrap_or("")),
                ("kind", kind),
                ("source_code", row.source_code.as_deref().unwrap_or("")),
                ("note_id", note.unwrap_or("")),
                (
                    "source_object_id",
                    row.source_object.as_deref().unwrap_or(""),
                ),
            ];
            Row {
                id: identity::content(t.feed, "assignment", &fields).to_string(),
                scope,
                kind,
                route,
                trip,
                from,
                to,
                location,
                source_code: row.source_code.as_deref(),
                note,
                source_object: row.source_object.as_deref(),
            }
        })
        .collect();
    assemble::finish(
        "assignment",
        vec![
            (
                "assignment_id",
                text(rows.iter().map(|r| Some(r.id.as_str()))),
            ),
            ("scope", text(rows.iter().map(|r| Some(r.scope)))),
            ("kind", text(rows.iter().map(|r| Some(r.kind)))),
            ("route_id", text(rows.iter().map(|r| r.route))),
            ("trip_id", text(rows.iter().map(|r| r.trip))),
            ("call_sequence", int32(rows.iter().map(|r| r.from))),
            ("call_sequence_to", int32(rows.iter().map(|r| r.to))),
            ("service_id", text(rows.iter().map(|_| None))),
            ("location_id", text(rows.iter().map(|r| r.location))),
            ("source_code", text(rows.iter().map(|r| r.source_code))),
            ("note_id", text(rows.iter().map(|r| r.note))),
            (
                "source_object_id",
                text(rows.iter().map(|r| r.source_object)),
            ),
        ],
    )
}
