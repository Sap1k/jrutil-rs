//! `connection_claim` and `travel_restriction` (serving.md sections 5.3, 5.4 and 7.6).
//!
//! Neither has a natural key; both get a content ID (`identity::content`) over these published
//! values, an absent value as the empty string, in this order:
//!
//! - connection claim (`connection`): `origin_trip_id`, `origin_sequence`, `direction`,
//!   `target_source_route_id`, `target_source_trip_id`, `target_source_stop_id`,
//!   `target_source_post_id`, `target_source_end_stop_id`, `target_source_end_post_id`,
//!   `wait_minutes`, `source_object_id`;
//! - travel restriction (`restriction`): `scope`, `route_id`, `trip_id`, `call_sequence`,
//!   `route_stop_id`, `group_code`, `source_object_id`.
//!
//! A claim's resolved targets are published only when the target itself is; otherwise the
//! column is null and the claim stays. A route-stop restriction becomes one row per distinct
//! route stop its published calls point at, or one row without a route stop when none does.

use std::collections::BTreeSet;

use jr_model::handle::Handle;
use jr_model::identity;
use jr_model::timetable::{LocationRef, RestrictionScope, Trip};
use jr_model::{RouteStopId, TripId};

use super::PublishError;
use super::assemble::{self, int32, text};
use super::calendar::Services;
use super::scope::Published;
use crate::table::Table;

fn opt(value: &Option<Box<str>>) -> &str {
    value.as_deref().unwrap_or("")
}

fn number(value: Option<i32>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}

pub fn connection_claim(p: &Published, services: &Services) -> Result<Table, PublishError> {
    let t = p.t;
    struct Row<'a> {
        id: String,
        claim: &'a jr_model::timetable::ConnectionClaim,
        origin: &'a str,
        service: Option<&'a str>,
        route: Option<&'a str>,
        trip: Option<&'a str>,
        location: Option<&'a str>,
    }
    let rows: Vec<Row> = p
        .claims
        .iter()
        .map(|&index| {
            let claim = &t.connection_claims[index];
            let origin = t.trips[claim.origin_trip].id.as_str();
            let sequence = claim.origin_sequence.to_string();
            let wait = number(claim.wait_minutes);
            let fields = [
                ("origin_trip_id", origin),
                ("origin_sequence", &sequence),
                ("direction", claim.direction.as_str()),
                ("target_source_route_id", opt(&claim.target_source_route)),
                ("target_source_trip_id", opt(&claim.target_source_trip)),
                ("target_source_stop_id", opt(&claim.target_source_stop)),
                ("target_source_post_id", opt(&claim.target_source_post)),
                (
                    "target_source_end_stop_id",
                    opt(&claim.target_source_end_stop),
                ),
                (
                    "target_source_end_post_id",
                    opt(&claim.target_source_end_post),
                ),
                ("wait_minutes", &wait),
                ("source_object_id", &claim.source_object),
            ];
            let location = claim.target_location.filter(|reference| match *reference {
                LocationRef::Place(id) => p.place[id.index()],
                LocationRef::BoardingPoint(id) => p.boarding_point[id.index()],
            });
            Row {
                id: identity::content(t.feed, "connection", &fields).to_string(),
                claim,
                origin,
                service: claim.dates.and_then(|dates| services.id(dates)),
                route: claim
                    .target_route
                    .filter(|id| p.route[id.index()])
                    .map(|id| t.routes[id].id.as_str()),
                trip: claim
                    .target_trip
                    .filter(|id| p.trip[id.index()])
                    .map(|id| t.trips[id].id.as_str()),
                location: location.map(|reference| p.location_id(reference)),
            }
        })
        .collect();
    let source = |field: fn(&jr_model::timetable::ConnectionClaim) -> &Option<Box<str>>| {
        text(rows.iter().map(move |r| field(r.claim).as_deref()))
    };
    assemble::finish(
        "connection_claim",
        vec![
            (
                "connection_id",
                text(rows.iter().map(|r| Some(r.id.as_str()))),
            ),
            (
                "direction",
                text(rows.iter().map(|r| Some(r.claim.direction.as_str()))),
            ),
            ("origin_trip_id", text(rows.iter().map(|r| Some(r.origin)))),
            (
                "origin_sequence",
                int32(rows.iter().map(|r| Some(r.claim.origin_sequence))),
            ),
            ("service_id", text(rows.iter().map(|r| r.service))),
            ("target_source_route_id", source(|c| &c.target_source_route)),
            ("target_source_trip_id", source(|c| &c.target_source_trip)),
            ("target_source_stop_id", source(|c| &c.target_source_stop)),
            ("target_source_post_id", source(|c| &c.target_source_post)),
            (
                "target_source_end_stop_id",
                source(|c| &c.target_source_end_stop),
            ),
            (
                "target_source_end_post_id",
                source(|c| &c.target_source_end_post),
            ),
            (
                "wait_minutes",
                int32(rows.iter().map(|r| r.claim.wait_minutes)),
            ),
            ("note", source(|c| &c.note)),
            ("target_public_line", source(|c| &c.target_public_line)),
            (
                "target_destination_text",
                source(|c| &c.target_destination_text),
            ),
            (
                "target_derivation",
                text(rows.iter().map(|r| Some(r.claim.derivation.as_str()))),
            ),
            (
                "resolution_status",
                text(rows.iter().map(|r| Some(r.claim.resolution.as_str()))),
            ),
            ("target_route_id", text(rows.iter().map(|r| r.route))),
            ("target_trip_id", text(rows.iter().map(|r| r.trip))),
            ("target_location_id", text(rows.iter().map(|r| r.location))),
            (
                "source_object_id",
                text(rows.iter().map(|r| Some(&*r.claim.source_object))),
            ),
        ],
    )
}

/// The route stop of a trip's call, if the call exists and has one.
fn route_stop_of(trip: &Trip, sequence: i32) -> Option<RouteStopId> {
    let index = trip
        .calls
        .binary_search_by_key(&sequence, |call| call.sequence)
        .ok()?;
    trip.calls[index].route_stop
}

pub fn travel_restriction<'t>(p: &Published<'t>) -> Result<Table, PublishError> {
    let t = p.t;
    struct Row<'a> {
        id: String,
        scope: &'static str,
        route: Option<&'a str>,
        trip: Option<&'a str>,
        sequence: Option<i32>,
        route_stop: Option<&'a str>,
        group: &'static str,
        source_route_stop: &'a str,
        source_object: &'a str,
    }
    let route_stop_id = |slot: RouteStopId| t.route_stops[slot].id.as_str();
    let mut rows: Vec<Row> = Vec::new();
    for &index in &p.restrictions {
        let restriction = &t.restrictions[index];
        let mut push = |scope: &'static str,
                        route: Option<&'t str>,
                        trip: Option<&'t str>,
                        sequence: Option<i32>,
                        route_stop: Option<&'t str>| {
            let call_sequence = number(sequence);
            let fields = [
                ("scope", scope),
                ("route_id", route.unwrap_or("")),
                ("trip_id", trip.unwrap_or("")),
                ("call_sequence", &call_sequence),
                ("route_stop_id", route_stop.unwrap_or("")),
                ("group_code", restriction.group.as_str()),
                ("source_object_id", &restriction.source_object),
            ];
            rows.push(Row {
                id: identity::content(t.feed, "restriction", &fields).to_string(),
                scope,
                route,
                trip,
                sequence,
                route_stop,
                group: restriction.group.as_str(),
                source_route_stop: &restriction.source_route_stop,
                source_object: &restriction.source_object,
            });
        };
        match &restriction.scope {
            RestrictionScope::Call { trip, sequence } => {
                let row = &t.trips[*trip];
                push(
                    "trip_call",
                    None,
                    Some(row.id.as_str()),
                    Some(*sequence),
                    route_stop_of(row, *sequence).map(route_stop_id),
                );
            }
            RestrictionScope::RouteStop { route, calls } => {
                let slots: BTreeSet<RouteStopId> = calls
                    .iter()
                    .filter(|(trip, _)| p.trip[trip.index()])
                    .filter_map(|&(trip, sequence): &(TripId, i32)| {
                        route_stop_of(&t.trips[trip], sequence)
                    })
                    .collect();
                let route = Some(t.routes[*route].id.as_str());
                if slots.is_empty() {
                    push("route_stop", route, None, None, None);
                }
                for slot in slots {
                    push("route_stop", route, None, None, Some(route_stop_id(slot)));
                }
            }
        }
    }
    assemble::finish(
        "travel_restriction",
        vec![
            (
                "restriction_id",
                text(rows.iter().map(|r| Some(r.id.as_str()))),
            ),
            ("scope", text(rows.iter().map(|r| Some(r.scope)))),
            ("route_id", text(rows.iter().map(|r| r.route))),
            ("trip_id", text(rows.iter().map(|r| r.trip))),
            (
                "source_route_stop_id",
                text(rows.iter().map(|r| Some(r.source_route_stop))),
            ),
            ("route_stop_id", text(rows.iter().map(|r| r.route_stop))),
            ("call_sequence", int32(rows.iter().map(|r| r.sequence))),
            ("service_id", text(rows.iter().map(|_| None))),
            ("group_code", text(rows.iter().map(|r| Some(r.group)))),
            (
                "source_object_id",
                text(rows.iter().map(|r| Some(r.source_object))),
            ),
        ],
    )
}
