//! The tables of a [`Timetable`].
//!
//! One timetable per feed. Compilers build it, passes ask for changes through
//! [`crate::Edits`], projections read it. Field meanings follow the serving contract
//! (`contracts/serving-v5.json`) where a field is published as is; what the contract derives
//! (weekday masks, `location` rows, zone relations, GTFS) is not stored here.

use std::borrow::Cow;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::dateset::Services;
use crate::geo::Coord;
use crate::handle::{
    AgencyId, BoardingPointId, Handle, NoteId, PlaceId, RouteId, RouteStopId, ServiceId, ShapeId,
    Sym, Table, TripId, ZoneSetId,
};
use crate::identity::{Feed, PublicId};
use crate::text::Strings;
use crate::time::{Date, Horizon, OptTime};

mod check;
mod semantics;

pub use check::Violation;
pub use semantics::{Assignment, AssignmentKind, Feature, Note, NoteKind, Scope};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Timetable {
    pub feed: Feed,
    pub horizon: Horizon,
    pub strings: Strings,
    pub services: Services,
    pub zone_sets: ZoneSets,
    pub agencies: Table<AgencyId, Agency>,
    pub places: Table<PlaceId, Place>,
    pub boarding_points: Table<BoardingPointId, BoardingPoint>,
    pub routes: Table<RouteId, Route>,
    pub trips: Table<TripId, Trip>,
    pub shapes: Table<ShapeId, Shape>,
    /// Derived by the route-stop order pass; empty until it has run.
    pub route_stops: Table<RouteStopId, RouteStop>,
    pub notes: Table<NoteId, Note>,
    pub assignments: Vec<Assignment>,
    pub transfers: Vec<Transfer>,
    pub keys: Vec<SourceKey>,
    pub call_keys: Vec<CallKey>,
    /// The source's own version of what it gave us (the newest CZPTT message, a JDF
    /// export date), published as GTFS `feed_version`.
    pub source_version: Option<Box<str>>,
    /// Which pass and rule changed what, in application order ([`crate::apply`]).
    pub history: History,
}

impl Timetable {
    pub fn new(feed: Feed, horizon: Horizon) -> Self {
        Self {
            feed,
            horizon,
            strings: Strings::new(),
            services: Services::new(),
            zone_sets: ZoneSets::default(),
            agencies: Table::new(),
            places: Table::new(),
            boarding_points: Table::new(),
            routes: Table::new(),
            trips: Table::new(),
            shapes: Table::new(),
            route_stops: Table::new(),
            notes: Table::new(),
            assignments: Vec::new(),
            transfers: Vec::new(),
            keys: Vec::new(),
            call_keys: Vec::new(),
            source_version: None,
            history: History::default(),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Coded values. The numeric codes are GTFS's, which the contract publishes unchanged.
// ---------------------------------------------------------------------------------------------

/// GTFS tri-state (`wheelchair_accessible`, `bikes_allowed`, `wheelchair_boarding`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tristate {
    #[default]
    Unknown = 0,
    Yes = 1,
    No = 2,
}

/// GTFS `pickup_type` / `drop_off_type`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Boarding {
    #[default]
    Regular = 0,
    None = 1,
    PhoneAgency = 2,
    /// On request to the driver (request stop).
    AskDriver = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Bus,
    Trolleybus,
    Tram,
    Metro,
    Rail,
    Water,
    Cable,
    Funicular,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimetableKind {
    #[default]
    Regular,
    /// A výluka (diversion) timetable.
    Detour,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceKind {
    /// A rider-facing stop or station.
    StopPlace,
    /// A railway point used for timing and progress, never a passenger stop.
    OperationalPoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoardingPointKind {
    /// "Somewhere at this place": the source names no post, platform or track.
    Unspecified,
    /// A road or tram stop post (označník, stanoviště).
    Post,
    /// A passenger platform (nástupiště).
    Platform,
    /// A railway track (kolej): the operational fact a rail source gives. Which platform it
    /// serves is a separate question.
    Track,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoordSource {
    Sr70,
    Osm,
    Gapfill,
    Catalogue,
    Overlay,
    /// Interpolated along a route by scheduled time.
    RouteTime,
    /// Placed past the last anchor of a route.
    RouteEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Precision {
    Exact,
    Estimated,
}

/// Where something is, and how we know. `None` in a `position` field is "missing".
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub coord: Coord,
    pub precision: Precision,
    pub source: Option<CoordSource>,
}

impl Position {
    pub const fn exact(coord: Coord, source: CoordSource) -> Self {
        Self {
            coord,
            precision: Precision::Exact,
            source: Some(source),
        }
    }

    pub const fn estimated(coord: Coord, source: CoordSource) -> Self {
        Self {
            coord,
            precision: Precision::Estimated,
            source: Some(source),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Entities
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Agency {
    pub id: PublicId,
    pub name: Box<str>,
    pub url: Option<Box<str>>,
    pub timezone: Box<str>,
    pub language: Option<Box<str>>,
    pub phone: Option<Box<str>>,
    pub fare_url: Option<Box<str>>,
    pub email: Option<Box<str>>,
}

/// A stop place, a station or a railway operational point.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub id: PublicId,
    pub kind: PlaceKind,
    pub name: Box<str>,
    /// The source's own code for the place: a CZPTT primary location code (with `country`, the
    /// PLC of a railway point), a CIS stop number. What matching against other catalogues uses.
    pub source_code: Option<Box<str>>,
    pub description: Option<Box<str>>,
    pub municipality: Option<Box<str>>,
    pub district_name: Option<Box<str>>,
    pub district_code: Option<Box<str>>,
    pub nearby_place: Option<Box<str>>,
    /// ISO 3166-1 alpha-2.
    pub country: Option<Box<str>>,
    pub position: Option<Position>,
    pub url: Option<Box<str>>,
    /// IANA zone of the place itself; `None` means the feed's (Europe/Prague).
    pub timezone: Option<Box<str>>,
    pub wheelchair_boarding: Tristate,
}

/// A post, platform or track of one place.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoardingPoint {
    pub id: PublicId,
    pub place: PlaceId,
    pub kind: BoardingPointKind,
    /// What a passenger is shown: the post or platform designation.
    pub public_code: Option<Box<str>>,
    /// The source's own code for it (a CZPTT subsidiary location code, a JDF post number).
    pub source_code: Option<Box<str>>,
    /// The source's own name for it, when it gives one beside the code.
    pub source_name: Option<Box<str>>,
    pub description: Option<Box<str>>,
    /// `None` means "at the place"; a point is positioned only when a source places it.
    pub position: Option<Position>,
    pub wheelchair_boarding: Tristate,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Route {
    pub id: PublicId,
    pub agency: AgencyId,
    pub mode: Mode,
    /// The (possibly extended) GTFS route type, e.g. 714 for rail replacement buses.
    pub gtfs_route_type: i32,
    pub short_name: Option<Box<str>>,
    pub long_name: Option<Box<str>>,
    pub description: Option<Box<str>>,
    pub url: Option<Box<str>>,
    pub color: Option<Box<str>>,
    pub text_color: Option<Box<str>>,
    pub sort_order: Option<i32>,
    pub timetable_kind: TimetableKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Trip {
    pub id: PublicId,
    pub route: RouteId,
    /// The days it runs. An empty set is a trip that no longer runs and is not published.
    pub service: ServiceId,
    pub direction: Option<u8>,
    pub headsign: Option<Sym>,
    pub short_name: Option<Sym>,
    pub block: Option<Sym>,
    /// For a trip that is one part of a longer run (a CZPTT path split at a line, operator or
    /// mode change).
    pub run: Option<Run>,
    pub wheelchair_accessible: Tristate,
    pub bikes_allowed: Tristate,
    pub shape: Option<TripShape>,
    /// In running order, at least two.
    pub calls: Vec<Call>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub key: Sym,
    /// Position of this trip within the run, from 1.
    pub part: u16,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TripShape {
    pub shape: ShapeId,
    /// Metres from the shape's first point, one entry per call; `None` for a call the shape
    /// was not matched through (its place has no exact position).
    pub offsets: Box<[Option<f64>]>,
}

/// One visit of a trip to a place. Kept small: there are millions.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Call {
    pub place: PlaceId,
    /// Where passengers board, as shown to them.
    pub boarding_point: Option<BoardingPointId>,
    /// The track a rail source assigns (a boarding point of kind `Track`). Equal to
    /// `boarding_point` until something maps tracks to platforms.
    pub track: Option<BoardingPointId>,
    /// The slot of the route's stop list this call belongs to (route-stop order pass).
    pub route_stop: Option<RouteStopId>,
    pub arrival: OptTime,
    pub departure: OptTime,
    /// The published call sequence: strictly increasing within a trip, gaps allowed (a rail
    /// trip keeps its source's numbering).
    pub sequence: i32,
    pub stop_headsign: Option<Sym>,
    /// The passenger line active when leaving this call (CZPTT).
    pub line: Option<Sym>,
    pub zones: Option<ZoneSetId>,
    /// The source's own distance figure (JDF tariff kilometres).
    pub kilometre: Option<f64>,
    pub pickup: Boarding,
    pub dropoff: Boarding,
    /// False for a railway point passed or stopped at without passenger service.
    pub passenger: bool,
    /// False when the time is approximate or was inferred.
    pub timepoint: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    pub id: PublicId,
    pub generation: ShapeGeneration,
    pub points: Vec<Coord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeGeneration {
    Source,
    Compiler,
    Motis,
}

/// One slot of a route direction's ordered stop list, as printed in a line timetable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RouteStop {
    pub id: PublicId,
    pub route: RouteId,
    pub direction: Option<u8>,
    pub sequence: i32,
    pub place: PlaceId,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Zone {
    pub code: Box<str>,
    /// The integrated transport system, where the source names one.
    pub system: Option<Box<str>>,
}

/// The interned ordered zone lists calls refer to.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(from = "Vec<Box<[Zone]>>", into = "Vec<Box<[Zone]>>")]
pub struct ZoneSets {
    sets: Vec<Box<[Zone]>>,
    /// Looked up, never iterated.
    index: HashMap<Box<[Zone]>, ZoneSetId>,
}

impl ZoneSets {
    /// `None` for an empty list, so "no zones" has one representation.
    pub fn intern(&mut self, zones: &[Zone]) -> Option<ZoneSetId> {
        if zones.is_empty() {
            return None;
        }
        if let Some(&id) = self.index.get(zones) {
            return Some(id);
        }
        let id = ZoneSetId::from_index(self.sets.len());
        self.sets.push(zones.into());
        self.index.insert(zones.into(), id);
        Some(id)
    }

    pub fn get(&self, id: ZoneSetId) -> &[Zone] {
        &self.sets[id.index()]
    }

    pub fn len(&self) -> usize {
        self.sets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sets.is_empty()
    }
}

impl From<Vec<Box<[Zone]>>> for ZoneSets {
    fn from(sets: Vec<Box<[Zone]>>) -> Self {
        let index = sets
            .iter()
            .enumerate()
            .map(|(position, set)| (set.clone(), ZoneSetId::from_index(position)))
            .collect();
        Self { sets, index }
    }
}

impl From<ZoneSets> for Vec<Box<[Zone]>> {
    fn from(zones: ZoneSets) -> Self {
        zones.sets
    }
}

impl PartialEq for ZoneSets {
    fn eq(&self, other: &Self) -> bool {
        self.sets == other.sets
    }
}

// ---------------------------------------------------------------------------------------------
// Relations between entities
// ---------------------------------------------------------------------------------------------

/// A place or one of its boarding points.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationRef {
    Place(PlaceId),
    BoardingPoint(BoardingPointId),
}

/// GTFS `transfer_type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferKind {
    Recommended = 0,
    Timed = 1,
    MinimumTime = 2,
    NotPossible = 3,
    InSeat = 4,
    InSeatNotAllowed = 5,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transfer {
    pub from: Option<LocationRef>,
    pub to: Option<LocationRef>,
    pub from_route: Option<RouteId>,
    pub to_route: Option<RouteId>,
    pub from_trip: Option<TripId>,
    pub to_trip: Option<TripId>,
    pub kind: TransferKind,
    /// Seconds.
    pub minimum_time: Option<i32>,
    /// Seconds a departing trip waits for a late arrival.
    pub maximum_wait: Option<i32>,
}

/// A source's identifier namespace (`cis:line_trip`, `czptt:train_number`, `sr70`, …). The
/// contract lists the published ones; the projection rejects any other.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Namespace(pub Cow<'static, str>);

impl Namespace {
    pub const CIS_LINE: Self = Self(Cow::Borrowed("cis:line"));
    pub const CIS_LINE_TRIP: Self = Self(Cow::Borrowed("cis:line_trip"));
    pub const CZPTT_TRAIN_NUMBER: Self = Self(Cow::Borrowed("czptt:train_number"));
    pub const CZPTT_PA: Self = Self(Cow::Borrowed("czptt:pa"));
    pub const CZPTT_TR: Self = Self(Cow::Borrowed("czptt:tr"));
    pub const SR70: Self = Self(Cow::Borrowed("sr70"));
    pub const SR70_TRACK: Self = Self(Cow::Borrowed("sr70:track"));

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Entity {
    Trip(TripId),
    Route(RouteId),
    Place(PlaceId),
    BoardingPoint(BoardingPointId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingMethod {
    /// The source identifier is a national identity (CIS, CZPTT).
    Identity,
    SourceNative,
    AuthoritativeSourceTripSet,
    StructuralMatch,
    CompanionAssertion,
    OperatorCrosswalk,
}

/// A source identifier that resolves to an entity: how realtime and other feeds find it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourceKey {
    pub namespace: Namespace,
    pub identifier: Box<str>,
    pub entity: Entity,
    pub valid_from: Date,
    pub valid_to: Date,
    pub binding: BindingMethod,
}

/// A source's call sequence where it differs from the published one.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CallKey {
    pub namespace: Namespace,
    pub identifier: Box<str>,
    pub source_sequence: Box<str>,
    pub trip: TripId,
    pub sequence: i32,
}

// ---------------------------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------------------------

/// What a change was made to. Call-level changes are recorded against their trip.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Agency(AgencyId),
    Place(PlaceId),
    BoardingPoint(BoardingPointId),
    Route(RouteId),
    Trip(TripId),
    Shape(ShapeId),
    Note(NoteId),
    /// A whole derived table (route stops, transfers, keys).
    Table,
}

/// The pass and rule behind every applied change, for `jrutil explain`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct History {
    /// `(pass, rule)` pairs, in order of first use.
    pub origins: Vec<(Box<str>, Box<str>)>,
    /// `(target, index into origins)`, in application order.
    pub changes: Vec<(Target, u32)>,
}

impl History {
    pub fn record(&mut self, pass: &str, rule: &str, target: Target) {
        let origin = match self
            .origins
            .iter()
            .position(|(p, r)| &**p == pass && &**r == rule)
        {
            Some(position) => position,
            None => {
                self.origins.push((pass.into(), rule.into()));
                self.origins.len() - 1
            }
        } as u32;
        // A pass that edits every call of a trip is one change to that trip.
        if self.changes.last() != Some(&(target, origin)) {
            self.changes.push((target, origin));
        }
    }

    /// The `(pass, rule)` pairs that changed `target`, in order.
    pub fn of(&self, target: Target) -> impl Iterator<Item = (&str, &str)> {
        self.changes
            .iter()
            .filter(move |(changed, _)| *changed == target)
            .map(|(_, origin)| {
                let (pass, rule) = &self.origins[*origin as usize];
                (&**pass, &**rule)
            })
    }
}
