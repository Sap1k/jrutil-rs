//! The base profile: the facts of the base timetable the engine relies on (verdict Q8), so a
//! rail base can supply its own.

use jr_model::timetable::{Feature, Mode, Namespace};
use serde::Deserialize;

use super::ProfileError;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaseProfile {
    /// The namespace of a base route's national line key (`cis:line`): what a source's line
    /// claims (R018) and trip-set authority (R057–R064) speak of.
    pub line_namespace: Box<str>,
    /// The route feature naming the integrated systems a route belongs to, the system code
    /// being the assignment's `source_code` (R062).
    pub integrated_system_feature: Feature,
    /// OVL-R056: the base dates a trip by the day of its first departure, so a source trip
    /// departing at or after 24:00 of its service day is folded onto the next day.
    pub dated_by_departure: bool,
    /// OVL-R058: a line's detour routes yield to its regular route for authority.
    pub prefer_regular_routes: bool,
    /// OVL-R026, R045, R046: a base stop place with an estimated position may be named and
    /// placed by a source.
    pub estimated_places_correctable: bool,
    /// What the published name of a place with an estimated position ends with; used only to
    /// show base names in the review reports as the package shows them.
    pub estimated_name_suffix: Box<str>,
    /// OVL-R036: modes that are one another's compatible counterparts (JDF publishes
    /// trolleybus lines as bus routes).
    pub compatible_modes: Vec<Vec<Mode>>,
    /// OVL-R038: how a source's GTFS `route_type` is read as a mode; a code no row lists has
    /// none and is compatible with nothing.
    pub gtfs_route_types: Vec<RouteTypeMode>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteTypeMode {
    pub mode: Mode,
    #[serde(default)]
    pub codes: Vec<i32>,
    /// Inclusive ranges.
    #[serde(default)]
    pub ranges: Vec<(i32, i32)>,
}

impl BaseProfile {
    pub(super) fn validate(self) -> Result<Self, ProfileError> {
        let invalid = |problem: &str| ProfileError::Invalid {
            profile: "base".to_owned(),
            problem: problem.to_owned(),
        };
        if self.line_namespace.trim().is_empty() {
            return Err(invalid("line_namespace is blank"));
        }
        if self
            .gtfs_route_types
            .iter()
            .any(|row| row.ranges.iter().any(|(a, b)| a > b))
        {
            return Err(invalid("gtfs_route_types has a reversed range"));
        }
        Ok(self)
    }

    pub fn line_namespace(&self) -> Namespace {
        Namespace(std::borrow::Cow::Owned(self.line_namespace.to_string()))
    }

    /// OVL-R038.
    pub fn mode_of_route_type(&self, route_type: Option<i32>) -> Option<Mode> {
        let code = route_type?;
        self.gtfs_route_types
            .iter()
            .find(|row| {
                row.codes.contains(&code)
                    || row
                        .ranges
                        .iter()
                        .any(|&(first, last)| (first..=last).contains(&code))
            })
            .map(|row| row.mode)
    }

    /// OVL-R036: equal modes, or two modes of one compatibility class. A missing mode is
    /// compatible with nothing.
    pub fn compatible(&self, left: Option<Mode>, right: Option<Mode>) -> bool {
        match (left, right) {
            (Some(left), Some(right)) => {
                left == right
                    || self
                        .compatible_modes
                        .iter()
                        .any(|class| class.contains(&left) && class.contains(&right))
            }
            _ => false,
        }
    }
}
