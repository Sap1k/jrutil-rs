//! The compiler's constants, read from `policy/jdf.toml`. Every field belongs to a named rule
//! (`docs/rules/jdf-read.md`, `docs/rules/jdf-merge.md`); the TOML file says which.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::international::Reason;

const BUILTIN: &str = include_str!("../../../policy/jdf.toml");

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub fixed_codes: FixedCodes,
    pub version_defaults: VersionDefaults,
    pub calendar: CalendarPolicy,
    pub holidays: HolidayPolicy,
    pub international: InternationalPolicy,
}

impl Policy {
    /// The policy shipped with the compiler.
    pub fn builtin() -> Policy {
        Policy::parse(BUILTIN).expect("policy/jdf.toml is valid")
    }

    pub fn parse(text: &str) -> Result<Policy, toml::de::Error> {
        toml::from_str(text)
    }
}

/// JDF-R025.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixedCodes {
    pub known: Vec<Box<str>>,
}

/// JDF-R028.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionDefaults {
    pub route_distinction: u32,
    pub agency_distinction: u32,
    /// A `Linky.txt` transport-mode code.
    pub transport_mode: Box<str>,
    pub detour: bool,
    pub grouped: bool,
}

/// JDF-R255–R264.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarPolicy {
    pub working_days: Box<str>,
    pub sundays_and_holidays: Box<str>,
    /// The day numbers of Monday … Sunday.
    pub days_of_week: [u32; 7],
    /// `Caskody.txt` type code to what it does.
    pub note_types: BTreeMap<Box<str>, NoteKind>,
}

/// What a `Caskody` type does to a trip's running days.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteKind {
    /// JDF-R260: the trip runs (by its day attributes) only inside such rows.
    RunsIn,
    /// JDF-R258.
    RunsAlso,
    /// JDF-R256.
    RunsOnly,
    /// JDF-R257.
    DoesNotRun,
    /// JDF-R259: inside the row, only in odd ISO weeks.
    OddWeeks,
    /// JDF-R259: inside the row, only in even ISO weeks.
    EvenWeeks,
}

/// JDF-R265, R266.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HolidayPolicy {
    pub fixed: Vec<FixedHoliday>,
    pub easter: Vec<EasterHoliday>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixedHoliday {
    pub month: i8,
    pub day: i8,
    /// The first year the date is a holiday; always when absent.
    #[serde(default)]
    pub since: Option<i16>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EasterHoliday {
    /// Days after Easter Sunday (negative: before).
    pub offset: i32,
    #[serde(default)]
    pub since: Option<i16>,
}

/// JDF-R269.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FilterPolicy {
    /// Classify and filter (R270–R281).
    RegionalAdjacent,
    /// No filter and no decisions.
    KeepAll,
}

/// JDF-R269–R281.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InternationalPolicy {
    pub policy: FilterPolicy,
    pub domestic_country: Box<str>,
    pub country_aliases: BTreeMap<Box<str>, Box<str>>,
    pub declared_international_route_types: Vec<Box<str>>,
    pub adjacent_countries: Vec<Box<str>>,
    pub max_span_km: f64,
    pub max_foreign_depth_km: f64,
    pub integrated_max_span_km: f64,
    pub integrated_max_foreign_depth_km: f64,
    pub reasons: Reasons,
}

impl InternationalPolicy {
    /// The recorded text of a reason.
    pub fn reason_text(&self, reason: Reason) -> &str {
        let r = &self.reasons;
        match reason {
            Reason::Domestic => &r.domestic,
            Reason::RegionalAdjacent => &r.regional_adjacent,
            Reason::InternationalMetadataWithoutForeignGeography => {
                &r.international_metadata_without_foreign_geography
            }
            Reason::ForeignOnlyTrip => &r.foreign_only_trip,
            Reason::UnknownStopCountry => &r.unknown_stop_country,
            Reason::NonAdjacentCountry => &r.non_adjacent_country,
            Reason::MissingTimetableKilometres => &r.missing_timetable_kilometres,
            Reason::TripSpanExceedsLimit => &r.trip_span_exceeds_limit,
            Reason::ForeignDepthExceedsLimit => &r.foreign_depth_exceeds_limit,
            Reason::NoQualifyingCrossBorderTrip => &r.no_qualifying_cross_border_trip,
        }
    }
}

/// The reason texts of R274 and R277.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reasons {
    pub domestic: Box<str>,
    pub regional_adjacent: Box<str>,
    pub international_metadata_without_foreign_geography: Box<str>,
    pub foreign_only_trip: Box<str>,
    pub unknown_stop_country: Box<str>,
    pub non_adjacent_country: Box<str>,
    pub missing_timetable_kilometres: Box<str>,
    pub trip_span_exceeds_limit: Box<str>,
    pub foreign_depth_exceeds_limit: Box<str>,
    pub no_qualifying_cross_border_trip: Box<str>,
}
