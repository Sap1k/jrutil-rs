//! The first half's output: each surviving path (PA) with its normalized calls and its trip parts.

use jr_formats::czptt::{Parameter, Subsidiary};
use jr_model::{Date, DateSet};

/// One surviving path request.
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    /// The published PA identifier, `Pa:<company>:<core>:<variant>:<year>` (CZPTT-R011).
    pub pa: Box<str>,
    /// The published TR identifier, when the message has one.
    pub tr: Option<Box<str>>,
    pub tr_company: Option<Box<str>>,
    pub tr_core: Option<Box<str>>,
    /// The days it runs, after cancellations (R013) and after the negative-time shift (R066).
    pub dates: DateSet,
    /// The calendar's `StartDateTime` date as the source gives it (not shifted by R066): the
    /// date catalogue versions are chosen at (R056).
    pub first_date: Date,
    /// Root-level `NetworkSpecificParameter`s, in source order.
    pub parameters: Vec<Parameter>,
    /// Every location, normalized (R026–R040, R066), in source order.
    pub calls: Vec<Call>,
    /// The retained trip parts (R041–R051, R117), in order. Empty when the path was rejected or
    /// every part was dropped.
    pub parts: Vec<Part>,
    /// Why the path has no parts, when it was rejected as a whole (R038, R039, R043).
    pub rejection: Option<Rejection>,
}

/// One normalized `CZPTTLocation`.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    /// The 0-based source index; the published sequence is `index + 1`.
    pub index: u32,
    pub country: Box<str>,
    pub code: Box<str>,
    /// `PrimaryLocationName`.
    pub source_name: Option<Box<str>>,
    pub subsidiary: Option<Subsidiary>,
    /// Seconds from midnight of the service day; both present or both absent (R028).
    pub arrival: Option<i32>,
    pub departure: Option<i32>,
    /// R037 changed the times.
    pub repaired: bool,
    /// R048 or R049 supplied the times (only ever set on a part's own copy of a call).
    pub inferred: bool,
    /// Activity `0001` (R029).
    pub passenger: bool,
    /// `TrainActivityType` codes, verbatim, in source order.
    pub activities: Vec<Box<str>>,
    /// The operator, carried forward (R033).
    pub responsible_ru: Box<str>,
    /// The category label, carried forward (R034), and its route type (R035).
    pub category: Box<str>,
    pub route_type: i32,
    /// The raw active line code at this location (R032).
    pub line: Option<Box<str>>,
    /// Line, route type and category after boundary projection (R040).
    pub projected: ServiceKey,
    /// The location is served by a rail replacement bus (R036).
    pub replacement_bus: bool,
    /// `OperationalTrainNumber`, verbatim.
    pub train_number: Option<Box<str>>,
}

impl Call {
    /// The published sequence.
    pub fn sequence(&self) -> u32 {
        self.index + 1
    }
}

/// What a line boundary is projected on (R040). The operator is not part of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceKey {
    pub line: Option<Box<str>>,
    pub route_type: i32,
    pub category: Box<str>,
}

/// What splits a path into parts (R044).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentKey {
    pub line: Option<Box<str>>,
    pub responsible_ru: Box<str>,
    /// The part's route type: the category's (R035), or the replacement-bus type (R045).
    pub route_type: i32,
    pub category: Box<str>,
    pub replacement_bus: bool,
}

/// One trip part of a path.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    /// The segment's index + 1. Numbers of dropped parts (R047, R050) stay unused, so a part
    /// keeps its number.
    pub number: u16,
    pub key: SegmentKey,
    /// 1-based; increases where the path switches between rail and replacement bus (R046).
    pub mode_block: u16,
    /// The part's passenger calls (after R042, R047–R049) and its railway points (R117), by
    /// source index. Adjacent parts share their boundary call.
    pub calls: Vec<Call>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejection {
    pub reason: RejectionReason,
    /// The offending call's sequence (R039).
    pub sequence: Option<u32>,
    /// The earlier and the offending time (R039).
    pub previous_seconds: Option<i32>,
    pub current_seconds: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectionReason {
    /// No call with the passenger activity (R038).
    NoPassengerCall,
    /// A call departs before it arrives (R039).
    DeparturePrecedesArrival,
    /// A call is reached before the previous one is left (R039).
    EventPrecedesPreviousEvent,
    /// Fewer than two passenger calls after collapsing duplicates (R043).
    TooFewPassengerCalls,
}
