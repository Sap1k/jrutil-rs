//! `route_stop`, `route_stop_zone` and `call_zone` (serving.md section 7.5).
//!
//! A route stop is *unanimous* when every passenger call of a published trip at it has the same
//! non-empty zone list: its zones are then `route_stop_zone` rows. Every other call with zones
//! (at a slot that is not unanimous, without a slot, or not a passenger call) gets
//! `call_zone` rows. `source_order` is the zone's 0-based position in the list.

use jr_model::ZoneSetId;
use jr_model::handle::Handle;

use super::PublishError;
use super::assemble::{self, int16, int32, text};
use super::scope::Published;
use crate::table::Table;

pub fn route_stop(p: &Published) -> Result<Table, PublishError> {
    let t = p.t;
    let rows: Vec<_> = t
        .route_stops
        .iter()
        .filter(|(id, _)| p.route_stop[id.index()])
        .map(|(_, row)| row)
        .collect();
    assemble::finish(
        "route_stop",
        vec![
            (
                "route_stop_id",
                text(rows.iter().map(|r| Some(r.id.as_str()))),
            ),
            (
                "route_id",
                text(rows.iter().map(|r| Some(t.routes[r.route].id.as_str()))),
            ),
            (
                "direction",
                int16(rows.iter().map(|r| r.direction.map(i16::from))),
            ),
            ("sequence", int32(rows.iter().map(|r| Some(r.sequence)))),
            (
                "location_id",
                text(rows.iter().map(|r| Some(t.places[r.place].id.as_str()))),
            ),
        ],
    )
}

/// Per route stop: the zone list every passenger call at it shares, if they all share one.
fn unanimous(p: &Published) -> Vec<Option<ZoneSetId>> {
    #[derive(Clone, Copy, PartialEq)]
    enum Seen {
        Nothing,
        Same(Option<ZoneSetId>),
        Mixed,
    }
    let t = p.t;
    let mut seen = vec![Seen::Nothing; t.route_stops.len()];
    for &id in &p.trips {
        for call in t.trips[id].calls.iter().filter(|call| call.passenger) {
            let Some(slot) = call.route_stop else {
                continue;
            };
            let state = &mut seen[slot.index()];
            *state = match *state {
                Seen::Nothing => Seen::Same(call.zones),
                Seen::Same(zones) if zones == call.zones => Seen::Same(zones),
                _ => Seen::Mixed,
            };
        }
    }
    seen.into_iter()
        .map(|state| match state {
            Seen::Same(zones) => zones,
            _ => None,
        })
        .collect()
}

pub fn route_stop_zone(p: &Published) -> Result<Table, PublishError> {
    let t = p.t;
    let mut rows: Vec<(&str, i32, &str, Option<&str>)> = Vec::new();
    for (slot, zones) in unanimous(p).into_iter().enumerate() {
        let Some(zones) = zones else { continue };
        let id = t.route_stops.rows()[slot].id.as_str();
        for (order, zone) in t.zone_sets.get(zones).iter().enumerate() {
            rows.push((id, order as i32, &zone.code, zone.system.as_deref()));
        }
    }
    assemble::finish(
        "route_stop_zone",
        vec![
            ("route_stop_id", text(rows.iter().map(|r| Some(r.0)))),
            ("source_order", int32(rows.iter().map(|r| Some(r.1)))),
            ("zone_code", text(rows.iter().map(|r| Some(r.2)))),
            ("zone_system", text(rows.iter().map(|r| r.3))),
        ],
    )
}

pub fn call_zone(p: &Published) -> Result<Table, PublishError> {
    let t = p.t;
    let unanimous = unanimous(p);
    let mut rows: Vec<(&str, i32, i32, &str, Option<&str>)> = Vec::new();
    for &id in &p.trips {
        let trip = &t.trips[id];
        for call in &trip.calls {
            let Some(zones) = call.zones else { continue };
            let covered = call.passenger
                && call
                    .route_stop
                    .is_some_and(|slot| unanimous[slot.index()].is_some());
            if covered {
                continue;
            }
            for (order, zone) in t.zone_sets.get(zones).iter().enumerate() {
                rows.push((
                    trip.id.as_str(),
                    call.sequence,
                    order as i32,
                    &zone.code,
                    zone.system.as_deref(),
                ));
            }
        }
    }
    assemble::finish(
        "call_zone",
        vec![
            ("trip_id", text(rows.iter().map(|r| Some(r.0)))),
            ("sequence", int32(rows.iter().map(|r| Some(r.1)))),
            ("source_order", int32(rows.iter().map(|r| Some(r.2)))),
            ("zone_code", text(rows.iter().map(|r| Some(r.3)))),
            ("zone_system", text(rows.iter().map(|r| r.4))),
        ],
    )
}
