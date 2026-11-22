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
}

impl Policy {
    /// The policy shipped with the compiler.
    pub fn builtin() -> Policy {
        Policy::parse(BUILTIN).expect("policy/czptt.toml is valid")
    }

    pub fn parse(text: &str) -> Result<Policy, toml::de::Error> {
        toml::from_str(text)
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
