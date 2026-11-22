//! JDF-R021, R023, R024: each relation's fields in the three layouts (1.8/1.9, 1.10, 1.11).

use super::Problem;
use super::codes::{CompanyType, NoteType, RouteType, TransportMode};
use super::grammar::{Row, Skip, all, at, problem, records};
use super::records::*;

/// Reads every record of one relation. A record with fewer fields than the layout has is skipped;
/// extra fields are not read and reported once per file.
pub(super) fn relation<T>(
    text: &str,
    file: &'static str,
    counts: [u8; 3],
    layout: usize,
    problems: &mut Vec<Problem>,
    decode: fn(&mut Row<'_, '_>) -> Result<T, Skip>,
) -> Vec<T> {
    let expected = usize::from(counts[layout]);
    let mut out = Vec::new();
    let mut extra_reported = false;
    records(text, file, problems, |record, fields, problems| {
        if fields.len() < expected {
            problems.push(problem(
                file,
                record,
                None,
                format!(
                    "{} fields, {expected} expected (record skipped)",
                    fields.len()
                ),
            ));
            return;
        }
        if fields.len() > expected && !extra_reported {
            extra_reported = true;
            problems.push(problem(
                file,
                record,
                None,
                format!(
                    "{} fields, {expected} expected: the extra fields are not read (reported \
                     once per file)",
                    fields.len()
                ),
            ));
        }
        let mut row = Row {
            fields,
            layout,
            file,
            record,
            problems,
        };
        if let Ok(value) = decode(&mut row) {
            out.push(value);
        }
    });
    out
}

pub(super) const STOP: [u8; 3] = [12, 12, 12];
pub(super) fn stop(r: &mut Row<'_, '_>) -> Result<Stop, Skip> {
    Ok(Stop {
        id: r.req(all(1), "id")?,
        town: r.text(all(2), "town")?,
        district: r.opt_text(all(3)),
        nearby_place: r.opt_text(all(4)),
        okres: r.opt_text(all(5)),
        country: r.opt_text(all(6)),
        attributes: r.slots([all(7), all(8), all(9), all(10), all(11), all(12)]),
    })
}

pub(super) const STOP_POST: [u8; 3] = [7, 7, 7];
pub(super) fn stop_post(r: &mut Row<'_, '_>) -> Result<StopPost, Skip> {
    Ok(StopPost {
        stop_id: r.req(all(1), "stop_id")?,
        post_id: r.req(all(2), "post_id")?,
        name: r.opt_text(all(3)),
        description: r.opt_text(all(4)),
        post_name: r.opt_text(all(5)),
        reserved1: r.opt_text(all(6)),
        reserved2: r.opt_text(all(7)),
    })
}

pub(super) const AGENCY: [u8; 3] = [12, 13, 13];
pub(super) fn agency(r: &mut Row<'_, '_>) -> Result<Agency, Skip> {
    Ok(Agency {
        id: r.text(all(1), "id")?,
        tax_id: r.opt_text(all(2)),
        name: r.text(all(3), "name")?,
        company_type: r.code(all(4), CompanyType::from_code),
        person_name: r.text(all(5), "person_name")?,
        office_address: r.text(all(6), "office_address")?,
        office_phone: r.text(all(7), "office_phone")?,
        control_phone: r.opt_text(all(8)),
        info_phone: r.opt_text(all(9)),
        fax: r.opt_text(all(10)),
        email: r.opt_text(all(11)),
        website: r.opt_text(all(12)),
        distinction: r.req_where(at(0, 13, 13), "distinction")?,
    })
}

pub(super) const ROUTE: [u8; 3] = [10, 16, 17];
pub(super) fn route(r: &mut Row<'_, '_>) -> Result<Route, Skip> {
    let valid_to = at(10, 14, 15);
    Ok(Route {
        id: r.text(all(1), "id")?,
        name: r.text(all(2), "name")?,
        agency_id: r.text(all(3), "agency_id")?,
        route_type: r.code(all(4), RouteType::from_code),
        transport_mode: r.code(at(0, 5, 5), TransportMode::from_code),
        detour: r.opt(at(0, 6, 6), "detour"),
        grouped: r.opt(at(0, 7, 7), "grouped"),
        uses_stop_posts: r.opt(at(0, 8, 8), "uses_stop_posts"),
        one_way: r.opt(at(0, 0, 9), "one_way"),
        reserved1: r.opt_text(at(5, 9, 10)),
        licence_number: r.opt_text(at(6, 10, 11)),
        licence_valid_from: r.opt(at(7, 11, 12), "licence_valid_from"),
        licence_valid_to: r.opt(at(8, 12, 13), "licence_valid_to"),
        timetable_valid_from: r.req(at(9, 13, 14), "timetable_valid_from")?,
        timetable_valid_to: if r.layout == 2 {
            Some(r.req(valid_to, "timetable_valid_to")?)
        } else {
            r.opt(valid_to, "timetable_valid_to")
        },
        agency_distinction: r.req_where(at(0, 15, 16), "agency_distinction")?,
        distinction: r.req_where(at(0, 16, 17), "distinction")?,
    })
}

pub(super) const ROUTE_INTEGRATION: [u8; 3] = [7, 7, 7];
pub(super) fn route_integration(r: &mut Row<'_, '_>) -> Result<RouteIntegration, Skip> {
    Ok(RouteIntegration {
        route_id: r.text(all(1), "route_id")?,
        entry_number: r.req(all(2), "entry_number")?,
        transport_system_id: r.req(all(3), "transport_system_id")?,
        route_name: r.text(all(4), "route_name")?,
        preferential: r.opt(all(5), "preferential"),
        reserved1: r.text(all(6), "reserved1")?,
        route_distinction: r.req(all(7), "route_distinction")?,
    })
}

pub(super) const ROUTE_STOP: [u8; 3] = [7, 9, 9];
pub(super) fn route_stop(r: &mut Row<'_, '_>) -> Result<RouteStop, Skip> {
    Ok(RouteStop {
        route_id: r.text(all(1), "route_id")?,
        route_stop_id: r.req(all(2), "route_stop_id")?,
        zone: r.opt_text(all(3)),
        stop_id: r.req(all(4), "stop_id")?,
        approximate_minutes: r.opt(at(0, 5, 5), "approximate_minutes"),
        attributes: r.slots([at(5, 6, 6), at(6, 7, 7), at(7, 8, 8)]),
        route_distinction: r.req_where(at(0, 9, 9), "route_distinction")?,
    })
}

pub(super) const TRIP: [u8; 3] = [12, 14, 14];
pub(super) fn trip(r: &mut Row<'_, '_>) -> Result<Trip, Skip> {
    Ok(Trip {
        route_id: r.text(all(1), "route_id")?,
        id: r.req(all(2), "id")?,
        attributes: r.slots([
            all(3),
            all(4),
            all(5),
            all(6),
            all(7),
            all(8),
            all(9),
            all(10),
            all(11),
            all(12),
        ]),
        trip_group_id: r.opt(at(0, 13, 13), "trip_group_id"),
        route_distinction: r.req_where(at(0, 14, 14), "route_distinction")?,
    })
}

pub(super) const TRIP_GROUP: [u8; 3] = [5, 5, 5];
pub(super) fn trip_group(r: &mut Row<'_, '_>) -> Result<TripGroup, Skip> {
    Ok(TripGroup {
        id: r.req(all(1), "id")?,
        entry_number: r.req(all(2), "entry_number")?,
        name: r.text(all(3), "name")?,
        description: r.opt_text(all(4)),
        reserved1: r.text(all(5), "reserved1")?,
    })
}

pub(super) const TRIP_STOP: [u8; 3] = [10, 12, 15];
pub(super) fn trip_stop(r: &mut Row<'_, '_>) -> Result<TripStop, Skip> {
    Ok(TripStop {
        route_id: r.text(all(1), "route_id")?,
        trip_id: r.req(all(2), "trip_id")?,
        route_stop_id: r.req(all(3), "route_stop_id")?,
        stop_id: r.req(all(4), "stop_id")?,
        stop_post_id: r.opt(at(0, 5, 5), "stop_post_id"),
        stop_post_number: r.opt_text(at(5, 6, 6)),
        attributes: r.slots([at(6, 7, 7), at(7, 8, 8), at(0, 0, 9)]),
        km: r.opt(at(8, 9, 10), "km"),
        arrival: r.cell(at(9, 10, 11), "arrival"),
        departure: r.cell(at(10, 11, 12), "departure"),
        earliest_arrival: r.cell(at(0, 0, 13), "earliest_arrival"),
        latest_departure: r.cell(at(0, 0, 14), "latest_departure"),
        route_distinction: r.req_where(at(0, 12, 15), "route_distinction")?,
    })
}

pub(super) const ROUTE_INFO: [u8; 3] = [3, 4, 4];
pub(super) fn route_info(r: &mut Row<'_, '_>) -> Result<RouteInfo, Skip> {
    Ok(RouteInfo {
        route_id: r.text(all(1), "route_id")?,
        id: r.req(all(2), "id")?,
        text: r.text(all(3), "text")?,
        route_distinction: r.req_where(at(0, 4, 4), "route_distinction")?,
    })
}

pub(super) const ATTRIBUTE_REF: [u8; 3] = [3, 3, 3];
pub(super) fn attribute_ref(r: &mut Row<'_, '_>) -> Result<AttributeRef, Skip> {
    Ok(AttributeRef {
        id: r.req(all(1), "id")?,
        code: r.text(all(2), "code")?,
        reserved1: r.opt_text(all(3)),
    })
}

pub(super) const SERVICE_NOTE: [u8; 3] = [8, 9, 9];
pub(super) fn service_note(r: &mut Row<'_, '_>) -> Result<ServiceNote, Skip> {
    Ok(ServiceNote {
        route_id: r.text(all(1), "route_id")?,
        trip_id: r.req(all(2), "trip_id")?,
        id: r.req(all(3), "id")?,
        designation: r.text(all(4), "designation")?,
        note_type: r.code(all(5), NoteType::from_code),
        date_from: r.opt(all(6), "date_from"),
        date_to: r.opt(all(7), "date_to"),
        note: r.opt_text(all(8)),
        route_distinction: r.req_where(at(0, 9, 9), "route_distinction")?,
    })
}

pub(super) const TRANSFER: [u8; 3] = [0, 11, 12];
pub(super) fn transfer(r: &mut Row<'_, '_>) -> Result<Transfer, Skip> {
    Ok(Transfer {
        kind: r.text(all(1), "kind")?,
        route_id: r.text(all(2), "route_id")?,
        trip_id: r.req(all(3), "trip_id")?,
        route_stop_id: r.req(all(4), "route_stop_id")?,
        transfer_route_id: r.opt(all(5), "transfer_route_id"),
        transfer_stop_id: r.opt(all(6), "transfer_stop_id"),
        transfer_stop_post_id: r.opt(all(7), "transfer_stop_post_id"),
        transfer_end_stop_id: r.opt(all(8), "transfer_end_stop_id"),
        transfer_end_stop_post_id: r.opt(all(9), "transfer_end_stop_post_id"),
        wait_minutes: r.opt(all(10), "wait_minutes"),
        note: r.opt_text(at(0, 0, 11)),
        route_distinction: r.req(at(0, 11, 12), "route_distinction")?,
    })
}

pub(super) const AGENCY_ALTERNATION: [u8; 3] = [13, 15, 15];
pub(super) fn agency_alternation(r: &mut Row<'_, '_>) -> Result<AgencyAlternation, Skip> {
    Ok(AgencyAlternation {
        route_id: r.text(all(1), "route_id")?,
        trip_id: r.req(all(2), "trip_id")?,
        agency_id: r.text(all(3), "agency_id")?,
        attributes: r.slots([all(4), all(5), all(6), all(7), all(8), all(9)]),
        time_type: r.opt_text(all(10)),
        reserved1: r.opt_text(all(11)),
        date_from: r.opt(all(12), "date_from"),
        date_to: r.opt(all(13), "date_to"),
        agency_distinction: r.req_where(at(0, 14, 14), "agency_distinction")?,
        route_distinction: r.req_where(at(0, 15, 15), "route_distinction")?,
    })
}

pub(super) const ALTERNATE_ROUTE_NAME: [u8; 3] = [3, 4, 4];
pub(super) fn alternate_route_name(r: &mut Row<'_, '_>) -> Result<AlternateRouteName, Skip> {
    Ok(AlternateRouteName {
        route_id: r.text(all(1), "route_id")?,
        alt_route_number: r.req(all(2), "alt_route_number")?,
        country: r.text(all(3), "country")?,
        route_distinction: r.req_where(at(0, 4, 4), "route_distinction")?,
    })
}

pub(super) const RESERVATION: [u8; 3] = [3, 4, 4];
pub(super) fn reservation(r: &mut Row<'_, '_>) -> Result<Reservation, Skip> {
    Ok(Reservation {
        route_id: r.text(all(1), "route_id")?,
        trip_id: r.req(all(2), "trip_id")?,
        note: r.text(all(3), "note")?,
        route_distinction: r.req_where(at(0, 4, 4), "route_distinction")?,
    })
}
