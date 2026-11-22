//! Typed semantics: what a timetable says beyond stops and times.
//!
//! Notes are texts the source attaches to something; assignments say what a note, a feature or
//! a calendar designation applies to. The published names of the coded values are the serving
//! contract's (`note_kind`, `assignment_kind`); [`Feature::as_str`] and [`NoteKind::as_str`]
//! give them.

use serde::{Deserialize, Serialize};

use crate::handle::{NoteId, RouteId, ServiceId, TripId};
use crate::identity::PublicId;
use crate::time::Date;
use crate::timetable::LocationRef;

/// An enum whose variants are published under a fixed snake_case name.
macro_rules! coded {
    ($(#[$doc:meta])* $name:ident { $($(#[$about:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        pub enum $name {
            $($(#[$about])* #[serde(rename = $text)] $variant),+
        }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }
        }
    };
}

coded!(
    /// The contract's `note_kind`.
    NoteKind {
        RouteInformation => "route_information",
        TimetableNote => "timetable_note",
        ReservationNote => "reservation_note",
        CzpttCentral => "czptt_central_note",
        CzpttLocal => "czptt_local_note",
        CzpttCalendar => "czptt_calendar_note",
    }
);

coded!(
    /// The feature kinds of the contract's `assignment_kind`.
    Feature {
        ReservationAvailable => "reservation_available",
        ReservationRequired => "reservation_required",
        WheelchairAccessibleVehicle => "wheelchair_accessible_vehicle",
        PartlyWheelchairAccessibleVehicle => "partly_wheelchair_accessible_vehicle",
        WheelchairBookingRequired => "wheelchair_booking_required",
        WheelchairBookingRecommended => "wheelchair_booking_recommended",
        RefreshmentsOnVehicle => "refreshments_on_vehicle",
        LuggageTransport => "luggage_transport",
        BicycleTransport => "bicycle_transport",
        BicycleCarryOn => "bicycle_carry_on",
        BicycleStorage => "bicycle_storage",
        BicycleReservationAvailable => "bicycle_reservation_available",
        BicycleReservationRequired => "bicycle_reservation_required",
        BicycleTransportProhibited => "bicycle_transport_prohibited",
        OnRequest => "on_request",
        Conditional => "conditional",
        SelfServiceTicketing => "self_service_ticketing",
        IntegratedTransport => "integrated_transport",
        NotStopping => "not_stopping",
        Diversion => "diversion",
        RequestStop => "request_stop",
        ExitOnly => "exit_only",
        BoardingOnly => "boarding_only",
        WheelchairAccessible => "wheelchair_accessible",
        Refreshments => "refreshments",
        Toilet => "toilet",
        AccessibleToilet => "accessible_toilet",
        UrbanTransportInterchange => "urban_transport_interchange",
        BorderControlOnly => "border_control_only",
        VisuallyImpairedAccessible => "visually_impaired_accessible",
        AccessibilityTerminal => "accessibility_terminal",
        RailInterchange => "rail_interchange",
        LineInterchange => "line_interchange",
        MetroInterchange => "metro_interchange",
        ShipTerminal => "ship_terminal",
        AirportNearby => "airport_nearby",
        ParkAndRide => "park_and_ride",
    }
);

/// A text the source attaches to a route, a trip or part of one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub id: PublicId,
    pub kind: NoteKind,
    /// What to show.
    pub label: Option<Box<str>>,
    /// The source's text, verbatim.
    pub text: Option<Box<str>>,
    pub valid_from: Option<Date>,
    pub valid_to: Option<Date>,
    /// The source's own code for the kind of note (a CZPTT central note code).
    pub note_type: Option<Box<str>>,
    pub source_object: Box<str>,
}

/// What an assignment applies to. Call positions are published call sequences.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Route(RouteId),
    Trip(TripId),
    Call {
        trip: TripId,
        sequence: i32,
    },
    /// The calls `from..=to` of a trip.
    CallRange {
        trip: TripId,
        from: i32,
        to: i32,
    },
    Location(LocationRef),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssignmentKind {
    /// A link to a [`Note`].
    Note,
    /// A source calendar code kept for display and audit.
    CalendarDesignation,
    Feature(Feature),
}

impl AssignmentKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::CalendarDesignation => "calendar_designation",
            Self::Feature(feature) => feature.as_str(),
        }
    }
}

/// A note, a feature or a calendar designation applied to something.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Assignment {
    pub scope: Scope,
    pub kind: AssignmentKind,
    /// The source's code for the fact (a CZPTT activity or note code, a JDF fixed code).
    pub source_code: Option<Box<str>>,
    pub note: Option<NoteId>,
    pub source_object: Option<Box<str>>,
}

coded!(
    /// The contract's `connection_direction`.
    ClaimDirection {
        /// This trip waits for the other (JDF `m`).
        WaitsFor => "waits_for",
        /// The other trip continues from this one (JDF `M`).
        ConnectsTo => "connects_to",
    }
);

coded!(
    /// How a claim's target was found (`connection_derivation`).
    ClaimDerivation {
        None => "none",
        /// The source's own target columns.
        Structured => "structured",
        /// Parsed from the note form the specification defines.
        SpecNote => "spec_note",
    }
);

coded!(
    /// How far a claim's target is resolved (`connection_resolution`).
    ClaimResolution {
        Unresolved => "unresolved",
        /// To a route and stop, not to one trip.
        Pattern => "pattern",
        /// To one trip.
        Resolved => "resolved",
    }
);

/// A source's claim that a trip connects with another service at one of its calls. Kept at
/// the specificity the source gives: the `target_source_*` fields are the source's own
/// identifiers, verbatim, and the resolved targets are filled only when resolution is certain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionClaim {
    pub direction: ClaimDirection,
    pub origin_trip: TripId,
    /// The published sequence of the origin call.
    pub origin_sequence: i32,
    /// The days the claim holds, when narrower than the trip's.
    pub dates: Option<ServiceId>,
    pub target_source_route: Option<Box<str>>,
    pub target_source_trip: Option<Box<str>>,
    pub target_source_stop: Option<Box<str>>,
    pub target_source_post: Option<Box<str>>,
    pub target_source_end_stop: Option<Box<str>>,
    pub target_source_end_post: Option<Box<str>>,
    pub wait_minutes: Option<i32>,
    pub note: Option<Box<str>>,
    pub target_public_line: Option<Box<str>>,
    pub target_destination_text: Option<Box<str>>,
    pub derivation: ClaimDerivation,
    pub resolution: ClaimResolution,
    pub target_route: Option<RouteId>,
    pub target_trip: Option<TripId>,
    pub target_location: Option<LocationRef>,
    pub source_object: Box<str>,
}

coded!(
    /// The contract's `restriction_group`: calls of one trip sharing a group may not be used as
    /// both the boarding and the alighting end of a journey.
    RestrictionGroup {
        Section => "§",
        A => "A",
        B => "B",
        C => "C",
    }
);

/// What a travel restriction is stated for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestrictionScope {
    /// A stop of a line's stop list: every listed call (trip, published sequence) of the
    /// route's trips at that stop.
    RouteStop {
        route: RouteId,
        calls: Box<[(TripId, i32)]>,
    },
    /// One call of one trip.
    Call { trip: TripId, sequence: i32 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Restriction {
    pub group: RestrictionGroup,
    pub scope: RestrictionScope,
    /// The source's own number of the route stop the restriction is written on.
    pub source_route_stop: Box<str>,
    pub source_object: Box<str>,
}
