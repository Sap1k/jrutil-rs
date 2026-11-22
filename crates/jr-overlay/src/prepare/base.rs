//! The base view: the indexes over the base timetable the overlay's rules read (OVL-R025–R027,
//! R053). Only what the base publishes is in view: trips with running days, the routes, places
//! and boarding points they use.

use std::collections::{BTreeMap, HashMap};

use jr_model::handle::Handle;
use jr_model::timetable::{AssignmentKind, Entity, Mode, Precision, Scope, TimetableKind};
use jr_model::{Coord, DateSet, PlaceId, RouteId, Rule, Timetable, TripId};
use rayon::prelude::*;

use super::grid::Grid;
use super::names::Names;
use crate::profile::BaseProfile;

/// The base's stop places, grouped as the overlay matches them: a place with the boarding points
/// published calls use. Its name is the place's; its position the place's, else the mean of its
/// boarding points'; its points are the distinct positions of the place and those boarding
/// points (a boarding point without a position of its own stands at its place).
pub const OVL_R025: Rule = Rule {
    id: "OVL-R025",
    summary: "base stop places with their published boarding points, names, positions and points",
};

/// A base place whose position is estimated is "approximate": a source may name and place it
/// (R045, R046). The marker is the position's precision, never a name suffix.
///
/// The JDF compiler estimates a stop's position when no catalogue places it; such a stop's
/// name and position are provisional.
pub const OVL_R026: Rule = Rule {
    id: "OVL-R026",
    summary: "a base place with an estimated position is approximate",
};

/// Nearby base places are found through a grid of the places' points; the decision is the exact
/// distance (`jr_model::geo`), so the grid changes nothing but time.
pub const OVL_R027: Rule = Rule {
    id: "OVL-R027",
    summary: "base places are looked up near a point through a grid, decided by exact distance",
};

/// Per published base trip: its route, running days, the places of its passenger calls in order
/// and their times. These are the patterns and times every signature (R055) compares.
pub const OVL_R053: Rule = Rule {
    id: "OVL-R053",
    summary: "base trips indexed by route, call count and place pattern, with their times",
};

pub const RULES: &[Rule] = &[OVL_R025, OVL_R026, OVL_R027, OVL_R053];

/// A base stop place in view (R025).
#[derive(Clone, Debug)]
pub struct BasePlace {
    pub place: PlaceId,
    /// The place's name as the timetable holds it.
    pub name: Box<str>,
    /// R026.
    pub estimated: bool,
    /// The place's position, else the mean of its boarding points' (R025).
    pub position: Option<Coord>,
    /// Distinct positions of the place and its published boarding points (R025).
    pub points: Vec<Coord>,
    /// R032's stable name key (the words joined by U+001F).
    pub stable_key: String,
}

/// A base route (R049–R051, R058, R062).
#[derive(Clone, Debug)]
pub struct BaseRoute {
    pub route: RouteId,
    /// The national line key (`BaseProfile::line_namespace`); one per route.
    pub line: Option<Box<str>>,
    /// The public line number (`route_short_name`).
    pub public_number: Option<Box<str>>,
    pub mode: Mode,
    /// R058: a detour (výluka) timetable route.
    pub detour: bool,
    /// R062: the integrated systems the route is filed under, sorted.
    pub systems: Vec<Box<str>>,
    /// Some trip of it runs.
    pub published: bool,
}

/// An interned place pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PatternId(pub u32);

/// A published base trip (R053).
#[derive(Clone, Debug)]
pub struct BaseTrip {
    pub trip: TripId,
    pub route: RouteId,
    pub dates: DateSet,
    pub pattern: PatternId,
    /// Seconds from the service day's midnight, one per passenger call.
    pub arrivals: Box<[Option<i32>]>,
    pub departures: Box<[Option<i32>]>,
}

/// Index of a base trip in [`BaseView::trips`].
pub type BaseTripIx = u32;

#[derive(Debug)]
pub struct BaseView<'a> {
    pub timetable: &'a Timetable,
    pub profile: &'a BaseProfile,
    /// Stop places in view, in place handle order (R025).
    pub places: Vec<BasePlace>,
    place_slot: Vec<u32>,
    /// R040: places by stable name key, in place order.
    by_stable_key: HashMap<String, Vec<u32>>,
    grid: Grid,
    /// Every route of the timetable, by handle index.
    pub routes: Vec<BaseRoute>,
    /// Published routes in handle order: the candidates of R050, R051.
    pub published_routes: Vec<RouteId>,
    /// Routes by national line, each list in handle order.
    pub routes_by_line: BTreeMap<Box<str>, Vec<RouteId>>,
    /// Published trips in handle order (R053).
    pub trips: Vec<BaseTrip>,
    trip_slot: Vec<u32>,
    patterns: Vec<Box<[PlaceId]>>,
    pattern_index: HashMap<Box<[PlaceId]>, PatternId>,
    by_route: Vec<Vec<BaseTripIx>>,
    by_route_length: HashMap<(RouteId, usize), Vec<BaseTripIx>>,
    by_route_pattern: HashMap<(RouteId, PatternId), Vec<BaseTripIx>>,
}

const NONE: u32 = u32::MAX;

/// A trip's passenger calls: places, arrivals, departures.
type Shaped = (TripId, Vec<PlaceId>, Vec<Option<i32>>, Vec<Option<i32>>);

impl<'a> BaseView<'a> {
    pub fn new(timetable: &'a Timetable, profile: &'a BaseProfile, names: &Names) -> Self {
        let t = timetable;
        let published: Vec<TripId> = t
            .trips
            .handles()
            .filter(|&id| !t.services.get(t.trips[id].service).is_empty())
            .collect();

        // Places and boarding points of published calls (R025).
        let mut place_used = vec![false; t.places.len()];
        let mut point_used = vec![false; t.boarding_points.len()];
        for &id in &published {
            for call in t.trips[id].calls.iter().filter(|call| call.passenger) {
                place_used[call.place.index()] = true;
                if let Some(point) = call.boarding_point {
                    point_used[point.index()] = true;
                }
            }
        }
        let mut members: Vec<Vec<Coord>> = vec![Vec::new(); t.places.len()];
        for (id, point) in t.boarding_points.iter() {
            if point_used[id.index()] {
                place_used[point.place.index()] = true;
                let position = point.position.or(t.places[point.place].position);
                if let Some(position) = position {
                    members[point.place.index()].push(position.coord);
                }
            }
        }
        let used: Vec<PlaceId> = t
            .places
            .handles()
            .filter(|id| place_used[id.index()])
            .collect();
        let places: Vec<BasePlace> = used
            .par_iter()
            .map(|&id| {
                let place = &t.places[id];
                let own = place.position.map(|position| position.coord);
                let position = own.or_else(|| jr_geo::plane::centroid(&members[id.index()]));
                let mut points: Vec<Coord> = Vec::new();
                for coord in own.iter().chain(&members[id.index()]) {
                    if !points.contains(coord) {
                        points.push(*coord);
                    }
                }
                BasePlace {
                    place: id,
                    name: place.name.clone(),
                    estimated: profile.estimated_places_correctable
                        && place
                            .position
                            .is_some_and(|position| position.precision == Precision::Estimated),
                    position,
                    points,
                    stable_key: names.stable_key(&place.name),
                }
            })
            .collect();
        let mut place_slot = vec![NONE; t.places.len()];
        let mut by_stable_key: HashMap<String, Vec<u32>> = HashMap::new();
        let mut grid = Grid::default();
        for (slot, place) in places.iter().enumerate() {
            let slot = slot as u32;
            place_slot[place.place.index()] = slot;
            by_stable_key
                .entry(place.stable_key.clone())
                .or_default()
                .push(slot);
            for &point in &place.points {
                grid.insert(point, slot);
            }
        }

        // Routes (R049–R051, R058, R062).
        let namespace = profile.line_namespace();
        let mut lines: Vec<Option<Box<str>>> = vec![None; t.routes.len()];
        for key in &t.keys {
            if key.namespace == namespace
                && let Entity::Route(route) = key.entity
            {
                lines[route.index()] = Some(key.identifier.clone());
            }
        }
        let mut systems: Vec<Vec<Box<str>>> = vec![Vec::new(); t.routes.len()];
        for assignment in &t.assignments {
            if let (Scope::Route(route), AssignmentKind::Feature(feature)) =
                (assignment.scope, assignment.kind)
                && feature == profile.integrated_system_feature
                && let Some(code) = &assignment.source_code
            {
                systems[route.index()].push(code.clone());
            }
        }
        let mut route_published = vec![false; t.routes.len()];
        for &id in &published {
            route_published[t.trips[id].route.index()] = true;
        }
        let routes: Vec<BaseRoute> = t
            .routes
            .iter()
            .map(|(id, route)| {
                let mut systems = std::mem::take(&mut systems[id.index()]);
                systems.sort();
                systems.dedup();
                BaseRoute {
                    route: id,
                    line: lines[id.index()].take(),
                    public_number: route.short_name.clone(),
                    mode: route.mode,
                    detour: route.timetable_kind == TimetableKind::Detour,
                    systems,
                    published: route_published[id.index()],
                }
            })
            .collect();
        let published_routes: Vec<RouteId> = routes
            .iter()
            .filter(|route| route.published)
            .map(|route| route.route)
            .collect();
        let mut routes_by_line: BTreeMap<Box<str>, Vec<RouteId>> = BTreeMap::new();
        for route in routes.iter().filter(|route| route.published) {
            if let Some(line) = &route.line {
                routes_by_line
                    .entry(line.clone())
                    .or_default()
                    .push(route.route);
            }
        }

        // Trips (R053).
        let shaped: Vec<Shaped> = published
            .par_iter()
            .map(|&id| {
                let calls = t.trips[id].calls.iter().filter(|call| call.passenger);
                let mut pattern = Vec::new();
                let mut arrivals = Vec::new();
                let mut departures = Vec::new();
                for call in calls {
                    pattern.push(call.place);
                    arrivals.push(call.arrival.get().map(|time| time.seconds()));
                    departures.push(call.departure.get().map(|time| time.seconds()));
                }
                (id, pattern, arrivals, departures)
            })
            .collect();
        let mut pattern_index: HashMap<Box<[PlaceId]>, PatternId> = HashMap::new();
        let mut patterns: Vec<Box<[PlaceId]>> = Vec::new();
        let mut trips = Vec::with_capacity(shaped.len());
        let mut trip_slot = vec![NONE; t.trips.len()];
        let mut by_route: Vec<Vec<BaseTripIx>> = vec![Vec::new(); t.routes.len()];
        let mut by_route_length: HashMap<(RouteId, usize), Vec<BaseTripIx>> = HashMap::new();
        let mut by_route_pattern: HashMap<(RouteId, PatternId), Vec<BaseTripIx>> = HashMap::new();
        for (id, pattern, arrivals, departures) in shaped {
            let length = pattern.len();
            let pattern: Box<[PlaceId]> = pattern.into();
            let pattern = match pattern_index.get(&pattern) {
                Some(&existing) => existing,
                None => {
                    let next = PatternId(patterns.len() as u32);
                    patterns.push(pattern.clone());
                    pattern_index.insert(pattern, next);
                    next
                }
            };
            let trip = &t.trips[id];
            let slot = trips.len() as BaseTripIx;
            trip_slot[id.index()] = slot;
            by_route[trip.route.index()].push(slot);
            by_route_length
                .entry((trip.route, length))
                .or_default()
                .push(slot);
            by_route_pattern
                .entry((trip.route, pattern))
                .or_default()
                .push(slot);
            trips.push(BaseTrip {
                trip: id,
                route: trip.route,
                dates: t.services.get(trip.service),
                pattern,
                arrivals: arrivals.into(),
                departures: departures.into(),
            });
        }
        BaseView {
            timetable,
            profile,
            places,
            place_slot,
            by_stable_key,
            grid,
            routes,
            published_routes,
            routes_by_line,
            trips,
            trip_slot,
            patterns,
            pattern_index,
            by_route,
            by_route_length,
            by_route_pattern,
        }
    }

    /// The place in view, if it is.
    pub fn place(&self, id: PlaceId) -> Option<&BasePlace> {
        match self.place_slot.get(id.index()) {
            Some(&slot) if slot != NONE => Some(&self.places[slot as usize]),
            _ => None,
        }
    }

    /// R040: the places in view whose stable name key is `key`.
    pub fn places_by_stable_key(&self, key: &str) -> impl Iterator<Item = &BasePlace> {
        self.by_stable_key
            .get(key)
            .into_iter()
            .flatten()
            .map(|&slot| &self.places[slot as usize])
    }

    /// R027: places with a point within `metres` of some of `points`, in place order, each once.
    pub fn places_near(&self, points: &[Coord], metres: f64) -> Vec<&BasePlace> {
        let mut slots: Vec<u32> = Vec::new();
        for &point in points {
            self.grid.near(point, metres, &mut slots);
        }
        slots.sort_unstable();
        slots.dedup();
        slots
            .into_iter()
            .map(|slot| &self.places[slot as usize])
            .collect()
    }

    pub fn route(&self, id: RouteId) -> &BaseRoute {
        &self.routes[id.index()]
    }

    pub fn trip(&self, id: TripId) -> Option<&BaseTrip> {
        match self.trip_slot.get(id.index()) {
            Some(&slot) if slot != NONE => Some(&self.trips[slot as usize]),
            _ => None,
        }
    }

    pub fn pattern(&self, id: PatternId) -> &[PlaceId] {
        &self.patterns[id.0 as usize]
    }

    /// Interned pattern of a place sequence, if some base trip has it.
    pub fn find_pattern(&self, places: &[PlaceId]) -> Option<PatternId> {
        self.pattern_index.get(places).copied()
    }

    /// Published trips of a route, in handle order.
    pub fn trips_of_route(&self, route: RouteId) -> &[BaseTripIx] {
        &self.by_route[route.index()]
    }

    /// R044: published trips of a route with this many passenger calls.
    pub fn trips_with_length(&self, route: RouteId, length: usize) -> &[BaseTripIx] {
        self.by_route_length
            .get(&(route, length))
            .map_or(&[], Vec::as_slice)
    }

    /// R065: published trips of a route with this pattern.
    pub fn trips_with_pattern(&self, route: RouteId, pattern: PatternId) -> &[BaseTripIx] {
        self.by_route_pattern
            .get(&(route, pattern))
            .map_or(&[], Vec::as_slice)
    }
}

/// The smallest distance between two point sets (R042, R043, R045), `None` when one is empty.
pub fn point_set_distance(left: &[Coord], right: &[Coord]) -> Option<f64> {
    let mut best: Option<f64> = None;
    for &a in left {
        for &b in right {
            let d = jr_model::geo::distance_m(a, b);
            best = Some(best.map_or(d, |best| best.min(d)));
        }
    }
    best
}
