//! Edits: what a pass may ask for, and [`apply`], the only thing that changes a timetable.
//!
//! A pass reads a timetable and its evidence and returns an [`Edits`] batch. It never mutates.
//! The batch is built against the timetable's current shape with an [`EditBuilder`], which
//! hands out the handles new rows and new interned values will have, so edits can refer to
//! things they create. [`apply`] refuses a batch built against a different shape, applies the
//! edits in order, records which pass and rule made each change, lets the [`Precedence`] table
//! decide between sources, and re-checks every invariant.
//!
//! The vocabulary is closed and deliberately small. A pass that needs something it cannot say
//! here is a reason to extend the vocabulary on purpose, not to reach around it.

use std::borrow::Cow;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::dateset::DateSet;
use crate::diag::{Diagnostic, Diagnostics, Rule, Severity};
use crate::handle::{
    AgencyId, BoardingPointId, Handle, NoteId, PlaceId, RouteId, RouteStopId, ServiceId, ShapeId,
    Sym, TripId,
};
use crate::timetable::{
    Agency, Assignment, BoardingPoint, CallKey, ConnectionClaim, CoordSource, Note, Place,
    Position, Precision, Restriction, Route, RouteStop, Shape, SourceKey, Target, Timetable,
    Transfer, Trip, TripShape, Violation,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Edit {
    AddAgency(Agency),
    AddPlace(Place),
    AddBoardingPoint(BoardingPoint),
    AddRoute(Route),
    AddTrip(Trip),
    AddShape(Shape),
    AddNote(Note),
    AddAssignment(Assignment),
    AddConnectionClaim(ConnectionClaim),
    AddRestriction(Restriction),
    /// Subject to [`Precedence`]: a weaker source does not replace a stronger one.
    SetPlacePosition {
        place: PlaceId,
        position: Position,
    },
    SetPlaceName {
        place: PlaceId,
        name: Box<str>,
    },
    SetBoardingPointPosition {
        point: BoardingPointId,
        position: Option<Position>,
    },
    /// `call` is the position in the trip's call list.
    SetCallBoardingPoint {
        trip: TripId,
        call: u32,
        boarding_point: Option<BoardingPointId>,
    },
    /// The days a trip runs. An empty service retires the trip.
    SetTripService {
        trip: TripId,
        service: ServiceId,
    },
    SetTripShape {
        trip: TripId,
        shape: Option<TripShape>,
    },
    /// Replaces the route-stop table and every call's slot: the table is derived as a whole.
    /// `calls` lists, per trip, one slot per call; trips not listed lose their slots.
    SetRouteStops {
        route_stops: Vec<RouteStop>,
        calls: Vec<(TripId, Vec<Option<RouteStopId>>)>,
    },
    AddTransfer(Transfer),
    AddKey(SourceKey),
    AddCallKey(CallKey),
}

/// The table sizes a batch was built against: what its new handles are numbered from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Extent {
    strings: usize,
    services: usize,
    agencies: usize,
    places: usize,
    boarding_points: usize,
    routes: usize,
    trips: usize,
    shapes: usize,
    notes: usize,
}

impl Extent {
    fn of(timetable: &Timetable) -> Self {
        Self {
            strings: timetable.strings.len(),
            services: timetable.services.len(),
            agencies: timetable.agencies.len(),
            places: timetable.places.len(),
            boarding_points: timetable.boarding_points.len(),
            routes: timetable.routes.len(),
            trips: timetable.trips.len(),
            shapes: timetable.shapes.len(),
            notes: timetable.notes.len(),
        }
    }
}

/// A batch of edits from one pass, ready for [`apply`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edits {
    pass: Cow<'static, str>,
    base: Extent,
    strings: Vec<Box<str>>,
    services: Vec<DateSet>,
    items: Vec<(Cow<'static, str>, Edit)>,
}

impl Edits {
    pub fn pass(&self) -> &str {
        &self.pass
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The edits with the rule each was made under, in order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, &Edit)> {
        self.items.iter().map(|(rule, edit)| (rule.as_ref(), edit))
    }
}

/// Builds an [`Edits`] batch against one timetable.
#[derive(Debug)]
pub struct EditBuilder<'a> {
    timetable: &'a Timetable,
    edits: Edits,
    /// New strings and services, looked up and never iterated.
    string_index: HashMap<Box<str>, Sym>,
    service_index: HashMap<DateSet, ServiceId>,
    added: Extent,
}

impl<'a> EditBuilder<'a> {
    pub fn new(timetable: &'a Timetable, pass: &'static str) -> Self {
        let base = Extent::of(timetable);
        Self {
            timetable,
            edits: Edits {
                pass: Cow::Borrowed(pass),
                base,
                strings: Vec::new(),
                services: Vec::new(),
                items: Vec::new(),
            },
            string_index: HashMap::new(),
            service_index: HashMap::new(),
            added: base,
        }
    }

    pub fn finish(self) -> Edits {
        self.edits
    }

    /// The handle `value` has, or will have once the batch is applied.
    pub fn string(&mut self, value: &str) -> Sym {
        if let Some(sym) = self.timetable.strings.find(value) {
            return sym;
        }
        if let Some(&sym) = self.string_index.get(value) {
            return sym;
        }
        let sym = Sym::from_index(self.added.strings);
        self.added.strings += 1;
        self.edits.strings.push(value.into());
        self.string_index.insert(value.into(), sym);
        sym
    }

    /// The handle `set` has, or will have once the batch is applied.
    pub fn service(&mut self, set: DateSet) -> ServiceId {
        if let Some(id) = self.timetable.services.find(&set) {
            return id;
        }
        if let Some(&id) = self.service_index.get(&set) {
            return id;
        }
        let id = ServiceId::from_index(self.added.services);
        self.added.services += 1;
        self.edits.services.push(set);
        self.service_index.insert(set, id);
        id
    }

    pub fn push(&mut self, rule: Rule, edit: Edit) {
        debug_assert!(
            !matches!(
                edit,
                Edit::AddAgency(_)
                    | Edit::AddPlace(_)
                    | Edit::AddBoardingPoint(_)
                    | Edit::AddRoute(_)
                    | Edit::AddTrip(_)
                    | Edit::AddShape(_)
                    | Edit::AddNote(_)
            ),
            "new rows go through the add_* methods, which hand out their handles"
        );
        self.edits.items.push((Cow::Borrowed(rule.id), edit));
    }

    pub fn add_agency(&mut self, rule: Rule, agency: Agency) -> AgencyId {
        let id = AgencyId::from_index(self.added.agencies);
        self.added.agencies += 1;
        self.edits
            .items
            .push((Cow::Borrowed(rule.id), Edit::AddAgency(agency)));
        id
    }

    pub fn add_place(&mut self, rule: Rule, place: Place) -> PlaceId {
        let id = PlaceId::from_index(self.added.places);
        self.added.places += 1;
        self.edits
            .items
            .push((Cow::Borrowed(rule.id), Edit::AddPlace(place)));
        id
    }

    pub fn add_boarding_point(&mut self, rule: Rule, point: BoardingPoint) -> BoardingPointId {
        let id = BoardingPointId::from_index(self.added.boarding_points);
        self.added.boarding_points += 1;
        self.edits
            .items
            .push((Cow::Borrowed(rule.id), Edit::AddBoardingPoint(point)));
        id
    }

    pub fn add_route(&mut self, rule: Rule, route: Route) -> RouteId {
        let id = RouteId::from_index(self.added.routes);
        self.added.routes += 1;
        self.edits
            .items
            .push((Cow::Borrowed(rule.id), Edit::AddRoute(route)));
        id
    }

    pub fn add_trip(&mut self, rule: Rule, trip: Trip) -> TripId {
        let id = TripId::from_index(self.added.trips);
        self.added.trips += 1;
        self.edits
            .items
            .push((Cow::Borrowed(rule.id), Edit::AddTrip(trip)));
        id
    }

    pub fn add_note(&mut self, rule: Rule, note: Note) -> NoteId {
        let id = NoteId::from_index(self.added.notes);
        self.added.notes += 1;
        self.edits
            .items
            .push((Cow::Borrowed(rule.id), Edit::AddNote(note)));
        id
    }

    pub fn add_shape(&mut self, rule: Rule, shape: Shape) -> ShapeId {
        let id = ShapeId::from_index(self.added.shapes);
        self.added.shapes += 1;
        self.edits
            .items
            .push((Cow::Borrowed(rule.id), Edit::AddShape(shape)));
        id
    }
}

/// Which source of a contested value wins. Data, loaded from policy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Precedence {
    /// Coordinate sources, strongest first. A source not listed is weaker than all listed.
    pub position: Vec<CoordSource>,
}

impl Precedence {
    fn position_rank(&self, source: Option<CoordSource>) -> usize {
        source
            .and_then(|source| self.position.iter().position(|listed| *listed == source))
            .unwrap_or(self.position.len())
    }

    /// An exact position beats an estimated one; within the same precision the stronger or
    /// equal source wins, so a pass may refine its own earlier value.
    fn replaces(&self, current: Option<Position>, new: Position) -> bool {
        let Some(current) = current else { return true };
        match (current.precision, new.precision) {
            (Precision::Estimated, Precision::Exact) => true,
            (Precision::Exact, Precision::Estimated) => false,
            _ => self.position_rank(new.source) <= self.position_rank(current.source),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error("edits of pass `{pass}` were built against a different timetable")]
    Stale { pass: String },
    #[error("pass `{pass}`, rule {rule}: {problem}")]
    Invalid {
        pass: String,
        rule: String,
        problem: String,
    },
    #[error("pass `{pass}` left {} invariant violation(s), first: {}", .violations.len(), .violations[0])]
    Invariant {
        pass: String,
        violations: Vec<Violation>,
    },
}

/// Applies a batch. On an error the timetable may be partly changed and must be discarded.
pub fn apply(
    timetable: &mut Timetable,
    edits: Edits,
    precedence: &Precedence,
    diagnostics: &mut Diagnostics,
) -> Result<(), ApplyError> {
    let Edits {
        pass,
        base,
        strings,
        services,
        items,
    } = edits;
    if base != Extent::of(timetable) {
        return Err(ApplyError::Stale {
            pass: pass.into_owned(),
        });
    }
    for value in &strings {
        timetable.strings.intern(value);
    }
    for set in services {
        timetable.services.intern(set);
    }
    for (rule, edit) in items {
        let target =
            apply_one(timetable, edit, precedence).map_err(|problem| ApplyError::Invalid {
                pass: pass.to_string(),
                rule: rule.to_string(),
                problem,
            })?;
        match target {
            Applied::Changed(target) => timetable.history.record(&pass, &rule, target),
            Applied::Superseded(subject) => diagnostics.push(Diagnostic {
                code: Cow::Borrowed("edit_superseded"),
                subject,
                message: format!("pass `{pass}` offered a value from a weaker source"),
                severity: Severity::Info,
                rule: Some(rule),
            }),
        }
    }
    let violations = timetable.check();
    if violations.is_empty() {
        Ok(())
    } else {
        Err(ApplyError::Invariant {
            pass: pass.into_owned(),
            violations,
        })
    }
}

enum Applied {
    Changed(Target),
    /// The precedence table kept the current value of this subject.
    Superseded(String),
}

fn apply_one(
    timetable: &mut Timetable,
    edit: Edit,
    precedence: &Precedence,
) -> Result<Applied, String> {
    let changed = match edit {
        Edit::AddAgency(agency) => Target::Agency(timetable.agencies.push(agency)),
        Edit::AddPlace(place) => Target::Place(timetable.places.push(place)),
        Edit::AddBoardingPoint(point) => {
            Target::BoardingPoint(timetable.boarding_points.push(point))
        }
        Edit::AddRoute(route) => Target::Route(timetable.routes.push(route)),
        Edit::AddTrip(trip) => Target::Trip(timetable.trips.push(trip)),
        Edit::AddShape(shape) => Target::Shape(timetable.shapes.push(shape)),
        Edit::AddNote(note) => Target::Note(timetable.notes.push(note)),
        Edit::AddAssignment(assignment) => {
            timetable.assignments.push(assignment);
            Target::Table
        }
        Edit::AddConnectionClaim(claim) => {
            timetable.connection_claims.push(claim);
            Target::Table
        }
        Edit::AddRestriction(restriction) => {
            timetable.restrictions.push(restriction);
            Target::Table
        }
        Edit::SetPlacePosition { place, position } => {
            let row = timetable.places.get_mut(place).ok_or("unknown place")?;
            if !precedence.replaces(row.position, position) {
                return Ok(Applied::Superseded(row.id.to_string()));
            }
            row.position = Some(position);
            Target::Place(place)
        }
        Edit::SetPlaceName { place, name } => {
            timetable.places.get_mut(place).ok_or("unknown place")?.name = name;
            Target::Place(place)
        }
        Edit::SetBoardingPointPosition { point, position } => {
            timetable
                .boarding_points
                .get_mut(point)
                .ok_or("unknown boarding point")?
                .position = position;
            Target::BoardingPoint(point)
        }
        Edit::SetCallBoardingPoint {
            trip,
            call,
            boarding_point,
        } => {
            let row = timetable.trips.get_mut(trip).ok_or("unknown trip")?;
            let call = row.calls.get_mut(call as usize).ok_or("unknown call")?;
            call.boarding_point = boarding_point;
            Target::Trip(trip)
        }
        Edit::SetTripService { trip, service } => {
            timetable.trips.get_mut(trip).ok_or("unknown trip")?.service = service;
            Target::Trip(trip)
        }
        Edit::SetTripShape { trip, shape } => {
            timetable.trips.get_mut(trip).ok_or("unknown trip")?.shape = shape;
            Target::Trip(trip)
        }
        Edit::SetRouteStops { route_stops, calls } => {
            for trip in timetable.trips.rows_mut() {
                for call in &mut trip.calls {
                    call.route_stop = None;
                }
            }
            timetable.route_stops = route_stops.into_iter().collect();
            for (trip, slots) in calls {
                let row = timetable.trips.get_mut(trip).ok_or("unknown trip")?;
                if slots.len() != row.calls.len() {
                    return Err(format!("{}: one route stop per call expected", row.id));
                }
                for (call, slot) in row.calls.iter_mut().zip(slots) {
                    call.route_stop = slot;
                }
            }
            Target::Table
        }
        Edit::AddTransfer(transfer) => {
            timetable.transfers.push(transfer);
            Target::Table
        }
        Edit::AddKey(key) => {
            timetable.keys.push(key);
            Target::Table
        }
        Edit::AddCallKey(key) => {
            timetable.call_keys.push(key);
            Target::Table
        }
    };
    Ok(Applied::Changed(changed))
}

#[cfg(test)]
mod tests;
