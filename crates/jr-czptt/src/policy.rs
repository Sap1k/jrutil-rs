//! The compiler's constants, read from `policy/czptt.toml`. Every field belongs to a named rule
//! (`docs/rules/czptt.md`); the TOML file says which.

use serde::Deserialize;

const BUILTIN: &str = include_str!("../../../policy/czptt.toml");

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub identifiers: Identifiers,
    pub ordering: Ordering,
    pub horizon: HorizonPolicy,
    pub calendar: CalendarPolicy,
    pub source_version: SourceVersion,
    pub timing: TimingPolicy,
    pub activities: Activities,
    pub parameters: Parameters,
    pub category: CategoryPolicy,
    pub route_types: RouteTypes,
    pub parts: PartsPolicy,
    pub places: PlacesPolicy,
    pub tracks: TracksPolicy,
    pub replacement_bus_point: ReplacementBusPoint,
    pub agencies: AgenciesPolicy,
    pub routes: RoutesPolicy,
    pub keys: KeysPolicy,
    pub notes: NotesPolicy,
    pub features: FeaturesPolicy,
    pub zones: ZonesPolicy,
    pub connections: ConnectionsPolicy,
}

impl Policy {
    /// The policy shipped with the compiler.
    pub fn builtin() -> Policy {
        Policy::parse(BUILTIN).expect("policy/czptt.toml is valid")
    }

    pub fn parse(text: &str) -> Result<Policy, toml::de::Error> {
        toml::from_str(text)
    }

    /// The colour of a route type (CZPTT-R061).
    pub fn route_color(&self, route_type: i32) -> &str {
        self.routes
            .colors
            .iter()
            .find(|entry| entry.route_type == route_type)
            .map_or(&*self.routes.default_color, |entry| &*entry.color)
    }

    /// The feature kinds of a central note code (CZPTT-R077).
    pub fn central_features(&self, code: &str) -> &[jr_model::timetable::Feature] {
        self.features
            .central
            .iter()
            .find(|entry| &*entry.code == code)
            .map_or(&[], |entry| &entry.kinds)
    }

    /// The route type of a rail category (CZPTT-R035).
    pub fn route_type(&self, category: &str) -> i32 {
        self.route_types
            .classes
            .iter()
            .find(|class| class.categories.iter().any(|c| &**c == category))
            .map_or(self.route_types.default, |class| class.route_type)
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identifiers {
    pub path_object_type: Box<str>,
    pub train_object_type: Box<str>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ordering {
    pub naive_timestamp_zone: Box<str>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HorizonPolicy {
    pub days_before: u16,
    pub days_after: u16,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarPolicy {
    pub running_day: char,
    pub not_running_day: char,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceVersion {
    pub prefix: Box<str>,
    pub format: Box<str>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimingPolicy {
    pub arrival_qualifier: Box<str>,
    pub departure_qualifier: Box<str>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Activities {
    pub passenger: Box<str>,
    pub embark_only: Box<str>,
    pub disembark_only: Box<str>,
    pub request_stop: Box<str>,
    pub unpublished_stop: Box<str>,
    pub known: Vec<Box<str>>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameters {
    pub line: Box<str>,
    pub replacement_bus: Box<str>,
    pub replacement_bus_value: Box<str>,
    pub inconsistent_time: Box<str>,
    pub inconsistent_time_value: Box<str>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CategoryPolicy {
    pub default_label: Box<str>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteTypes {
    pub default: i32,
    pub replacement_bus: i32,
    pub classes: Vec<RouteClass>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteClass {
    pub route_type: i32,
    pub categories: Vec<Box<str>>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartsPolicy {
    pub minimum_calls: usize,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacesPolicy {
    pub timezone: Box<str>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TracksPolicy {
    /// Whether a track's name (else code) is published as its public code.
    pub public_code: bool,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplacementBusPoint {
    pub public_code: Box<str>,
    pub description: Box<str>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgenciesPolicy {
    pub timezone: Box<str>,
    pub language: Box<str>,
    pub qualifier_separator: Box<str>,
    pub unknown_name_prefix: Box<str>,
    pub default_scheme: Box<str>,
    pub schemes: Vec<Box<str>>,
    pub default_url: Box<str>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutesPolicy {
    pub replacement_bus_suffix: Box<str>,
    pub text_color: Box<str>,
    pub default_color: Box<str>,
    pub colors: Vec<RouteColor>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteColor {
    pub route_type: i32,
    pub color: Box<str>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeysPolicy {
    pub sr70_country: Box<str>,
    pub sr70_code_length: usize,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotesPolicy {
    pub central: Box<str>,
    pub local: Box<str>,
    pub calendar: Box<str>,
    pub field_separator: char,
    pub date_format: Box<str>,
    pub central_kind: Box<str>,
    pub local_kind: Box<str>,
    pub calendar_kind: Box<str>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeaturesPolicy {
    pub wheelchair_codes: Vec<Box<str>>,
    pub bicycle_codes: Vec<Box<str>>,
    pub bicycle_prohibited_codes: Vec<Box<str>>,
    pub central: Vec<CentralFeature>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CentralFeature {
    pub code: Box<str>,
    pub kinds: Vec<jr_model::timetable::Feature>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZonesPolicy {
    pub parameter: Box<str>,
    /// R119: the parameter holding a membership's calendar.
    pub calendar_parameter: Box<str>,
    pub fare_band_word: Box<str>,
    pub system_separator: char,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectionsPolicy {
    pub max_gap_seconds: i32,
    pub train_number_offset: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builtin_policy_loads() {
        let policy = Policy::builtin();
        assert_eq!(&*policy.ordering.naive_timestamp_zone, "Europe/Prague");
        assert_eq!(policy.route_type("rj"), 102);
        assert_eq!(policy.route_type("Vlak"), policy.route_types.default);
    }
}
