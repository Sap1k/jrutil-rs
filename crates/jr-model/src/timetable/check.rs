//! The invariants every timetable holds between passes.
//!
//! Compilers establish them (repairing or dropping what the source gets wrong, each under a
//! named rule) and [`crate::apply`] re-checks them after every batch of edits, so no pass ever
//! has to defend against a malformed timetable.

use std::collections::BTreeSet;
use std::fmt;

use crate::geo::Coord;
use crate::handle::Handle;
use crate::identity::{Feed, PublicId};
use crate::time::ServiceTime;
use crate::timetable::{
    BoardingPointKind, Entity, LocationRef, Position, RestrictionScope, Scope, Timetable,
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Violation {
    /// The public ID (or table) the problem is in.
    pub subject: String,
    pub problem: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.subject, self.problem)
    }
}

impl Timetable {
    /// Every violated invariant, sorted. Empty for a well-formed timetable.
    pub fn check(&self) -> Vec<Violation> {
        let mut found = Checker {
            timetable: self,
            violations: Vec::new(),
        };
        found.identifiers();
        found.places();
        found.routes();
        found.trips();
        found.relations();
        found.violations.sort();
        found.violations
    }
}

struct Checker<'a> {
    timetable: &'a Timetable,
    violations: Vec<Violation>,
}

impl Checker<'_> {
    fn report(&mut self, subject: impl fmt::Display, problem: impl Into<String>) {
        self.violations.push(Violation {
            subject: subject.to_string(),
            problem: problem.into(),
        });
    }

    /// IDs carry an allowed prefix and are unique within what the contract publishes as one
    /// relation (places and boarding points are both `location`).
    fn identifiers(&mut self) {
        let t = self.timetable;
        let groups: [(&str, Vec<&PublicId>); 7] = [
            (
                "agency",
                t.agencies.rows().iter().map(|row| &row.id).collect(),
            ),
            (
                "location",
                t.places
                    .rows()
                    .iter()
                    .map(|row| &row.id)
                    .chain(t.boarding_points.rows().iter().map(|row| &row.id))
                    .collect(),
            ),
            ("route", t.routes.rows().iter().map(|row| &row.id).collect()),
            ("trip", t.trips.rows().iter().map(|row| &row.id).collect()),
            ("shape", t.shapes.rows().iter().map(|row| &row.id).collect()),
            (
                "service_note",
                t.notes.rows().iter().map(|row| &row.id).collect(),
            ),
            (
                "route_stop",
                t.route_stops.rows().iter().map(|row| &row.id).collect(),
            ),
        ];
        for (relation, ids) in groups {
            let mut seen = BTreeSet::new();
            for id in ids {
                if !has_feed_prefix(t.feed, id) {
                    self.report(id, format!("{relation} ID lacks the feed prefix"));
                }
                if !seen.insert(id) {
                    self.report(id, format!("duplicate {relation} ID"));
                }
            }
        }
    }

    fn position(&mut self, subject: &PublicId, position: Option<Position>) {
        if let Some(Position { coord, .. }) = position
            && !Coord::is_valid(coord)
        {
            self.report(subject, "position outside WGS84 bounds");
        }
    }

    fn places(&mut self) {
        let t = self.timetable;
        for place in t.places.rows() {
            self.position(&place.id, place.position);
        }
        for point in t.boarding_points.rows() {
            if !t.places.contains(point.place) {
                self.report(&point.id, "boarding point of an unknown place");
            }
            self.position(&point.id, point.position);
        }
    }

    fn routes(&mut self) {
        let t = self.timetable;
        for route in t.routes.rows() {
            if !t.agencies.contains(route.agency) {
                self.report(&route.id, "route of an unknown agency");
            }
        }
        for slot in t.route_stops.rows() {
            if !t.routes.contains(slot.route) || !t.places.contains(slot.place) {
                self.report(&slot.id, "route stop with an unknown route or place");
            }
        }
    }

    fn trips(&mut self) {
        let t = self.timetable;
        for trip in t.trips.rows() {
            let id = &trip.id;
            if !t.routes.contains(trip.route) {
                self.report(id, "trip of an unknown route");
            }
            if trip.service.index() >= t.services.len() {
                self.report(id, "trip with an unknown service");
            }
            if trip.calls.len() < 2 {
                self.report(id, format!("trip with {} calls", trip.calls.len()));
            }
            if let Some(shape) = &trip.shape
                && (!t.shapes.contains(shape.shape) || shape.offsets.len() != trip.calls.len())
            {
                self.report(
                    id,
                    "trip shape is unknown or does not have one offset per call",
                );
            }
            let mut previous_sequence = None;
            let mut latest: Option<ServiceTime> = None;
            for call in &trip.calls {
                let at = call.sequence;
                if previous_sequence.is_some_and(|previous| previous >= at) {
                    self.report(id, format!("call sequence {at} does not increase"));
                }
                previous_sequence = Some(at);
                for time in [call.arrival.get(), call.departure.get()]
                    .into_iter()
                    .flatten()
                {
                    if latest.is_some_and(|latest| time < latest) {
                        self.report(id, format!("time goes backwards at call {at}"));
                    }
                    latest = Some(latest.map_or(time, |latest| latest.max(time)));
                }
                if !t.places.contains(call.place) {
                    self.report(id, format!("call {at} at an unknown place"));
                }
                for (role, point) in [
                    ("boarding point", call.boarding_point),
                    ("track", call.track),
                ] {
                    let Some(point) = point else { continue };
                    match t.boarding_points.get(point) {
                        None => self.report(id, format!("call {at} has an unknown {role}")),
                        Some(row) if row.place != call.place => {
                            self.report(id, format!("call {at} has a {role} of another place"));
                        }
                        Some(row) if role == "track" && row.kind != BoardingPointKind::Track => {
                            self.report(id, format!("call {at} has a track that is not a track"));
                        }
                        Some(_) => {}
                    }
                }
                if call
                    .route_stop
                    .is_some_and(|slot| !t.route_stops.contains(slot))
                {
                    self.report(id, format!("call {at} has an unknown route stop"));
                }
                if call
                    .zones
                    .is_some_and(|zones| zones.index() >= t.zone_sets.len())
                {
                    self.report(id, format!("call {at} has an unknown zone set"));
                }
            }
        }
    }

    fn relations(&mut self) {
        let t = self.timetable;
        let location = |reference: Option<LocationRef>| match reference {
            None => true,
            Some(LocationRef::Place(place)) => t.places.contains(place),
            Some(LocationRef::BoardingPoint(point)) => t.boarding_points.contains(point),
        };
        for (index, transfer) in t.transfers.iter().enumerate() {
            let known = location(transfer.from)
                && location(transfer.to)
                && transfer
                    .from_route
                    .is_none_or(|route| t.routes.contains(route))
                && transfer
                    .to_route
                    .is_none_or(|route| t.routes.contains(route))
                && transfer.from_trip.is_none_or(|trip| t.trips.contains(trip))
                && transfer.to_trip.is_none_or(|trip| t.trips.contains(trip));
            if !known {
                self.report(format!("transfer #{index}"), "refers to an unknown entity");
            }
        }
        for (index, assignment) in t.assignments.iter().enumerate() {
            let call = |trip, sequence: i32| {
                t.trips
                    .get(trip)
                    .is_some_and(|row| row.calls.iter().any(|call| call.sequence == sequence))
            };
            let known = match assignment.scope {
                Scope::Route(route) => t.routes.contains(route),
                Scope::Trip(trip) => t.trips.contains(trip),
                Scope::Call { trip, sequence } => call(trip, sequence),
                Scope::CallRange { trip, from, to } => {
                    from <= to && call(trip, from) && call(trip, to)
                }
                Scope::Location(reference) => location(Some(reference)),
            };
            if !known || assignment.note.is_some_and(|note| !t.notes.contains(note)) {
                self.report(
                    format!("assignment #{index}"),
                    "refers to an unknown entity",
                );
            }
        }
        let call_exists = |trip, sequence: i32| {
            t.trips
                .get(trip)
                .is_some_and(|row| row.calls.iter().any(|call| call.sequence == sequence))
        };
        for (index, claim) in t.connection_claims.iter().enumerate() {
            let known = call_exists(claim.origin_trip, claim.origin_sequence)
                && claim
                    .dates
                    .is_none_or(|dates| dates.index() < t.services.len())
                && claim
                    .target_route
                    .is_none_or(|route| t.routes.contains(route))
                && claim.target_trip.is_none_or(|trip| t.trips.contains(trip))
                && location(claim.target_location);
            if !known {
                self.report(
                    format!("connection claim #{index}"),
                    "refers to an unknown entity",
                );
            }
        }
        for (index, restriction) in t.restrictions.iter().enumerate() {
            let known = match &restriction.scope {
                RestrictionScope::RouteStop { route, calls } => {
                    t.routes.contains(*route)
                        && calls
                            .iter()
                            .all(|(trip, sequence)| call_exists(*trip, *sequence))
                }
                RestrictionScope::Call { trip, sequence } => call_exists(*trip, *sequence),
            };
            if !known {
                self.report(
                    format!("restriction #{index}"),
                    "refers to an unknown entity",
                );
            }
        }
        for key in &t.keys {
            let subject = format!("key {}={}", key.namespace.as_str(), key.identifier);
            let known = match key.entity {
                Entity::Trip(trip) => t.trips.contains(trip),
                Entity::Route(route) => t.routes.contains(route),
                Entity::Place(place) => t.places.contains(place),
                Entity::BoardingPoint(point) => t.boarding_points.contains(point),
            };
            if !known {
                self.report(&subject, "refers to an unknown entity");
            }
            if key.valid_to < key.valid_from {
                self.report(&subject, "validity ends before it starts");
            }
        }
        for key in &t.call_keys {
            let has_call = t
                .trips
                .get(key.trip)
                .is_some_and(|trip| trip.calls.iter().any(|call| call.sequence == key.sequence));
            if !has_call {
                let subject = format!("call key {}={}", key.namespace.as_str(), key.identifier);
                self.report(subject, "refers to an unknown call");
            }
        }
    }
}

/// `overlay:` marks entities a regional overlay brought into the road feed.
fn has_feed_prefix(feed: Feed, id: &PublicId) -> bool {
    let rest = |prefix: &str| {
        id.as_str()
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with(':'))
    };
    rest(feed.prefix()) || (feed == Feed::Jdf && rest("overlay"))
}
