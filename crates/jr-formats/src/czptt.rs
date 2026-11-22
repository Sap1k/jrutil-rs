//! One CZPTT XML message (`CZPTTCISMessage` or `CZCanceledPTTMessage`), as written.
//!
//! Elements are matched by local name, so a namespace prefix is tolerated; unknown elements are
//! skipped. Text is trimmed of surrounding whitespace, and an element that is present but has no
//! text reads as absent (`None`, or no entry in a list) — except `NetworkSpecificParameter`
//! `Name`/`Value`, where an empty one is `""` so that names and values keep their positions.
//!
//! A message without one of its mandatory parts (identifiers, the creation or cancellation
//! timestamp, the calendar's start and bitmap, a location's country and primary code, a timing's
//! qualifier, time and offset) or with one that cannot be read is a [`CzpttError`] naming the
//! element; so is an element that may occur once but occurs twice. Nothing is interpreted:
//! qualifiers, activities and codes stay text.
//!
//! The parser is a pull parser over the input slice: no DOM, no global state.

mod cursor;
mod datetime;
mod parser;

use std::fmt;

/// A parsed message.
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    Timetable(TimetableMessage),
    Cancellation(CancellationMessage),
}

/// `ObjectType` of a `PlannedTransportIdentifiers`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObjectType {
    /// `PA`: the path request.
    Pa,
    /// `TR`: the train.
    Tr,
}

/// `PlannedTransportIdentifiers` (or `RelatedPlannedTransportIdentifiers`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identifier {
    pub object_type: ObjectType,
    pub company: Box<str>,
    pub core: Box<str>,
    pub variant: Box<str>,
    pub timetable_year: Box<str>,
}

/// An `xs:dateTime` as written: the local reading and the offset if one was given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timestamp {
    pub local: jiff::civil::DateTime,
    /// Seconds east of UTC.
    pub offset_seconds: Option<i32>,
}

/// `PlannedCalendar`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Calendar {
    /// `ValidityPeriod/StartDateTime`.
    pub start: Timestamp,
    /// `ValidityPeriod/EndDateTime`.
    pub end: Option<Timestamp>,
    /// `BitmapDays`, verbatim.
    pub bitmap: Box<str>,
}

/// One `Name[i]`/`Value[i]` pair of a `NetworkSpecificParameter`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parameter {
    pub name: Box<str>,
    pub value: Box<str>,
}

/// `CZPTTCISMessage`.
#[derive(Clone, Debug, PartialEq)]
pub struct TimetableMessage {
    /// `Identifiers/PlannedTransportIdentifiers`, in order.
    pub identifiers: Vec<Identifier>,
    /// `RelatedPlannedTransportIdentifiers` (under `Identifiers` or the root), in order.
    pub related: Vec<Identifier>,
    /// `CZPTTCreation`.
    pub creation: Timestamp,
    /// `CZPTTInformation/PlannedCalendar`.
    pub calendar: Calendar,
    /// `CZPTTInformation/CZPTTLocation`, in order.
    pub locations: Vec<Location>,
    /// Root-level `NetworkSpecificParameter`s, in order.
    pub parameters: Vec<Parameter>,
}

/// `CZCanceledPTTMessage`.
#[derive(Clone, Debug, PartialEq)]
pub struct CancellationMessage {
    /// `PlannedTransportIdentifiers`, in order.
    pub identifiers: Vec<Identifier>,
    /// `CZPTTCancelation`.
    pub cancellation: Timestamp,
    /// `PlannedCalendar`.
    pub calendar: Calendar,
}

/// `CZPTTLocation`.
#[derive(Clone, Debug, PartialEq)]
pub struct Location {
    /// `Location/CountryCodeISO`.
    pub country: Box<str>,
    /// `Location/LocationPrimaryCode`.
    pub primary_code: Box<str>,
    /// `Location/PrimaryLocationName`.
    pub primary_name: Option<Box<str>>,
    /// `Location/LocationSubsidiaryIdentification`.
    pub subsidiary: Option<Subsidiary>,
    /// `TimingAtLocation/Timing`, in order.
    pub timings: Vec<Timing>,
    pub responsible_ru: Option<Box<str>>,
    pub responsible_im: Option<Box<str>>,
    pub train_type: Option<Box<str>>,
    pub traffic_type: Option<Box<str>>,
    pub commercial_traffic_type: Option<Box<str>>,
    pub operational_train_number: Option<Box<str>>,
    /// `TrainActivity/TrainActivityType`, as text, in order.
    pub activities: Vec<Box<str>>,
    /// Location-level `NetworkSpecificParameter`s, in order.
    pub parameters: Vec<Parameter>,
}

/// `LocationSubsidiaryIdentification`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subsidiary {
    /// `LocationSubsidiaryCode`.
    pub code: Box<str>,
    /// `LocationSubsidiaryCode/@LocationSubsidiaryTypeCode`.
    pub type_code: Option<Box<str>>,
    /// `LocationSubsidiaryName`.
    pub name: Option<Box<str>>,
    /// `AllocationCompany`.
    pub allocation_company: Option<Box<str>>,
}

/// `TimingAtLocation/Timing`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Timing {
    /// `@TimingQualifierCode` as text (`ALA`, `ALD`, or anything else).
    pub qualifier: Box<str>,
    /// The local reading of `Time`.
    pub time: jiff::civil::Time,
    /// The UTC offset written with `Time`, in seconds east of UTC, if any.
    pub offset_seconds: Option<i32>,
    /// `Offset`: days after the calendar day.
    pub day_offset: i32,
}

#[derive(Debug, thiserror::Error)]
pub enum CzpttError {
    #[error("not UTF-8: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("malformed XML at byte {position}: {source}")]
    Xml {
        position: u64,
        #[source]
        source: quick_xml::Error,
    },
    #[error("undefined entity &{0};")]
    Entity(Box<str>),
    #[error("no root element")]
    NoRoot,
    #[error("unknown root element {0}")]
    UnknownRoot(Box<str>),
    #[error("{element} is missing{}", At(*location))]
    Missing {
        element: &'static str,
        /// 0-based index of the `CZPTTLocation`, for elements inside one.
        location: Option<usize>,
    },
    #[error("{element} occurs more than once{}", At(*location))]
    Repeated {
        element: &'static str,
        location: Option<usize>,
    },
    #[error("{element} cannot be read: {value:?}{}", At(*location))]
    Invalid {
        element: &'static str,
        location: Option<usize>,
        value: Box<str>,
    },
}

struct At(Option<usize>);

impl fmt::Display for At {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(index) => write!(f, " (CZPTTLocation {index})"),
            None => Ok(()),
        }
    }
}

/// Parses one message, dispatching on the root element's local name.
pub fn parse(xml: &[u8]) -> Result<Message, CzpttError> {
    let mut cursor = cursor::Cursor::new(std::str::from_utf8(xml)?);
    let root = cursor.root()?.ok_or(CzpttError::NoRoot)?;
    match root.name() {
        "CZPTTCISMessage" => parser::Parser::new(cursor)
            .timetable(&root)
            .map(Message::Timetable),
        "CZCanceledPTTMessage" => parser::Parser::new(cursor)
            .cancellation(&root)
            .map(Message::Cancellation),
        other => Err(CzpttError::UnknownRoot(other.into())),
    }
}
