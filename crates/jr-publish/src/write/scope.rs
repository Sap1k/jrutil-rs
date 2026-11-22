//! Which rows of the timetable are published.
//!
//! A trip is published iff its service is not empty. Everything else follows from what the
//! published rows refer to, so every foreign key of the package resolves:
//!
//! - routes, agencies and shapes: those published trips use; route stops: those of published
//!   routes;
//! - transfers, assignments, source keys and call keys: those whose trips and routes are all
//!   published (a key on a place always is, a key on a boarding point when the point is);
//! - notes: those a published assignment links;
//! - boarding points: a published call's `boarding_point`, a published transfer's endpoint or a
//!   published `Scope::Location` (a point named only as a call's `track` is not a location);
//! - places: those a published call, boarding point, route stop, transfer, assignment or key
//!   names.

use jr_model::handle::Handle;
use jr_model::timetable::{Entity, LocationRef, Scope, Trip};
use jr_model::{BoardingPointId, PlaceId, Timetable, TripId};

use super::PublishError;

#[derive(Debug)]
pub struct Published<'t> {
    pub t: &'t Timetable,
    /// Published trips, by handle order.
    pub trips: Vec<TripId>,
    pub route: Vec<bool>,
    pub agency: Vec<bool>,
    pub shape: Vec<bool>,
    pub route_stop: Vec<bool>,
    pub note: Vec<bool>,
    pub place: Vec<bool>,
    pub boarding_point: Vec<bool>,
    /// Indexes into the timetable's vectors.
    pub transfers: Vec<usize>,
    pub assignments: Vec<usize>,
    pub keys: Vec<usize>,
    pub call_keys: Vec<usize>,
}

impl<'t> Published<'t> {
    pub fn of(t: &'t Timetable) -> Result<Published<'t>, PublishError> {
        let trip: Vec<bool> = t
            .trips
            .rows()
            .iter()
            .map(|trip| !t.services.get(trip.service).is_empty())
            .collect();
        let trips: Vec<TripId> = t.trips.handles().filter(|id| trip[id.index()]).collect();
        let mut route = vec![false; t.routes.len()];
        let mut shape = vec![false; t.shapes.len()];
        for &id in &trips {
            let row = &t.trips[id];
            route[row.route.index()] = true;
            if let Some(trip_shape) = &row.shape {
                shape[trip_shape.shape.index()] = true;
            }
        }
        let mut agency = vec![false; t.agencies.len()];
        for (id, row) in t.routes.iter() {
            if route[id.index()] {
                agency[row.agency.index()] = true;
            }
        }
        let route_stop: Vec<bool> = t
            .route_stops
            .rows()
            .iter()
            .map(|slot| route[slot.route.index()])
            .collect();

        let trip_ok = |id: Option<TripId>| id.is_none_or(|id| trip[id.index()]);
        let route_ok = |id: Option<jr_model::RouteId>| id.is_none_or(|id| route[id.index()]);
        let transfers: Vec<usize> = (0..t.transfers.len())
            .filter(|&i| {
                let row = &t.transfers[i];
                trip_ok(row.from_trip)
                    && trip_ok(row.to_trip)
                    && route_ok(row.from_route)
                    && route_ok(row.to_route)
            })
            .collect();
        let assignments: Vec<usize> = (0..t.assignments.len())
            .filter(|&i| match t.assignments[i].scope {
                Scope::Route(id) => route[id.index()],
                Scope::Trip(id)
                | Scope::Call { trip: id, .. }
                | Scope::CallRange { trip: id, .. } => trip[id.index()],
                Scope::Location(_) => true,
            })
            .collect();
        let mut note = vec![false; t.notes.len()];
        for &i in &assignments {
            if let Some(id) = t.assignments[i].note {
                note[id.index()] = true;
            }
        }

        let mut place = vec![false; t.places.len()];
        let mut boarding_point = vec![false; t.boarding_points.len()];
        let mut locate = |reference: LocationRef| match reference {
            LocationRef::Place(id) => place[id.index()] = true,
            LocationRef::BoardingPoint(id) => boarding_point[id.index()] = true,
        };
        for &id in &trips {
            for call in &t.trips[id].calls {
                locate(LocationRef::Place(call.place));
                if let Some(point) = call.boarding_point {
                    locate(LocationRef::BoardingPoint(point));
                }
            }
        }
        for &i in &transfers {
            let row = &t.transfers[i];
            row.from.into_iter().chain(row.to).for_each(&mut locate);
        }
        for &i in &assignments {
            if let Scope::Location(reference) = t.assignments[i].scope {
                locate(reference);
            }
        }
        for (id, slot) in t.route_stops.iter() {
            if route_stop[id.index()] {
                place[slot.place.index()] = true;
            }
        }
        for row in &t.keys {
            if let Entity::Place(id) = row.entity {
                place[id.index()] = true;
            }
        }
        for (id, point) in t.boarding_points.iter() {
            if boarding_point[id.index()] {
                place[point.place.index()] = true;
            }
        }

        let keys: Vec<usize> = (0..t.keys.len())
            .filter(|&i| match t.keys[i].entity {
                Entity::Trip(id) => trip[id.index()],
                Entity::Route(id) => route[id.index()],
                Entity::Place(id) => place[id.index()],
                Entity::BoardingPoint(id) => boarding_point[id.index()],
            })
            .collect();
        let call_keys: Vec<usize> = (0..t.call_keys.len())
            .filter(|&i| trip[t.call_keys[i].trip.index()])
            .collect();

        let published = Published {
            t,
            trips,
            route,
            agency,
            shape,
            route_stop,
            note,
            place,
            boarding_point,
            transfers,
            assignments,
            keys,
            call_keys,
        };
        published.check_route_stops()?;
        Ok(published)
    }

    /// A published call's route stop is published (it belongs to a published route).
    fn check_route_stops(&self) -> Result<(), PublishError> {
        for &id in &self.trips {
            let trip: &Trip = &self.t.trips[id];
            for call in &trip.calls {
                if let Some(slot) = call.route_stop
                    && !self.route_stop[slot.index()]
                {
                    return Err(PublishError::UnpublishedRouteStop {
                        trip: trip.id.to_string(),
                        sequence: call.sequence,
                        route_stop: self.t.route_stops[slot].id.to_string(),
                    });
                }
            }
        }
        Ok(())
    }

    pub fn place_ids(&self) -> impl Iterator<Item = PlaceId> + '_ {
        self.t.places.handles().filter(|id| self.place[id.index()])
    }

    pub fn boarding_point_ids(&self) -> impl Iterator<Item = BoardingPointId> + '_ {
        self.t
            .boarding_points
            .handles()
            .filter(|id| self.boarding_point[id.index()])
    }

    /// The published ID of a place or boarding point.
    pub fn location_id(&self, reference: LocationRef) -> &'t str {
        match reference {
            LocationRef::Place(id) => self.t.places[id].id.as_str(),
            LocationRef::BoardingPoint(id) => self.t.boarding_points[id].id.as_str(),
        }
    }
}
