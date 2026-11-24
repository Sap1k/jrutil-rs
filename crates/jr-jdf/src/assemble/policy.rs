//! The assembly's constants, read from `policy/jdf_convert.toml`. Every field belongs to a named
//! rule of `docs/rules/jdf-convert.md`; the TOML file says which.

use std::collections::BTreeMap;

use jr_model::timetable::Mode;
use jr_model::timetable::{Feature, RestrictionGroup};
use serde::Deserialize;

const BUILTIN: &str = include_str!("../../../../policy/jdf_convert.toml");

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub feed: FeedPolicy,
    pub agencies: AgenciesPolicy,
    pub public_line: PublicLinePolicy,
    pub modes: ModesPolicy,
    pub route_types: RouteTypesPolicy,
    pub colours: ColoursPolicy,
    pub routes: RoutesPolicy,
    pub codes: CodesPolicy,
    pub trip_features: BTreeMap<Box<str>, Feature>,
    pub location_features: BTreeMap<Box<str>, Feature>,
    pub times: TimesPolicy,
    pub notes: NotesPolicy,
    pub claims: ClaimsPolicy,
    pub keys: KeysPolicy,
}

impl Policy {
    /// The policy shipped with the compiler.
    pub fn builtin() -> Policy {
        Policy::parse(BUILTIN).expect("policy/jdf_convert.toml is valid")
    }

    pub fn parse(text: &str) -> Result<Policy, toml::de::Error> {
        toml::from_str(text)
    }
}

/// JDF-R469.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedPolicy {
    pub source_format_version: Box<str>,
}

/// JDF-R426.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgenciesPolicy {
    pub timezone: Box<str>,
    pub language: Box<str>,
    pub url_schemes: Vec<Box<str>>,
    pub url_prefix: Box<str>,
}

/// JDF-R408.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicLinePolicy {
    pub licence_digits: usize,
    pub licence_suffix: usize,
}

/// JDF-R427, R478.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModesPolicy {
    pub bus: Box<str>,
    pub fallback: Box<str>,
    pub letters: BTreeMap<Box<str>, Mode>,
}

/// JDF-R427.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteTypesPolicy {
    pub bus: BTreeMap<Box<str>, i32>,
    pub bus_cross_border: i32,
    pub bus_default: i32,
    pub modes: BTreeMap<Box<str>, i32>,
}

/// JDF-R428.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColoursPolicy {
    pub text: Box<str>,
    pub bus_default: Box<str>,
    pub bus_cross_border: Box<str>,
    pub bus_by_route_type: BTreeMap<Box<str>, Box<str>>,
    pub modes: BTreeMap<Box<str>, [Box<str>; 2]>,
    pub metro: BTreeMap<Box<str>, [Box<str>; 2]>,
    pub metro_default: [Box<str>; 2],
}

/// JDF-R430.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutesPolicy {
    pub detour_description: Box<str>,
}

/// JDF-R417, R437, R438, R444, R457, R462.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodesPolicy {
    pub wheelchair_trip: Vec<Box<str>>,
    pub bikes_trip: Vec<Box<str>>,
    pub wheelchair_stop: Box<str>,
    pub ask_driver: Vec<Box<str>>,
    pub phone_agency: Vec<Box<str>>,
    /// R444: no pickup and no drop-off at any level (`$`).
    pub both_directions_none: Vec<Box<str>>,
    /// R444: these and `no_dropoff` exclude each other; the most specific level decides.
    pub no_pickup: Vec<Box<str>>,
    pub no_dropoff: Vec<Box<str>>,
    pub restrictions: BTreeMap<Box<str>, RestrictionGroup>,
    pub working_days: Box<str>,
    pub sundays_and_holidays: Box<str>,
}

impl CodesPolicy {
    /// R457: `X`, `+` and any all-digit code are calendar codes.
    pub fn is_calendar(&self, code: &str) -> bool {
        code == &*self.working_days
            || code == &*self.sundays_and_holidays
            || (!code.is_empty() && code.bytes().all(|b| b.is_ascii_digit()))
    }
}

/// JDF-R442.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimesPolicy {
    pub max_midnight_gap_minutes: i32,
}

/// JDF-R452–R454.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotesPolicy {
    pub route_kind: Box<str>,
    pub trip_kind: Box<str>,
    pub reservation_kind: Box<str>,
    pub note_types: BTreeMap<Box<str>, Box<str>>,
}

/// JDF-R460.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimsPolicy {
    pub waits_for: Box<str>,
    pub connects_to: Box<str>,
}

/// JDF-R466.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeysPolicy {
    pub licence_digits: usize,
}

fn contains(list: &[Box<str>], code: &str) -> bool {
    list.iter().any(|c| &**c == code)
}

impl CodesPolicy {
    pub fn any(&self, list: &[Box<str>], codes: &[Box<str>]) -> bool {
        codes.iter().any(|code| contains(list, code))
    }
}
