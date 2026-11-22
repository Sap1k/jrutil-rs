//! The closed vocabularies of the profiles: route tiers, trip tiers and capabilities. A name
//! outside them is a profile error (B18): serde refuses an unknown variant.

use serde::Deserialize;

/// How a source route finds its candidate base routes (OVL-R049, R050), in canonical order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteTier {
    /// The source asserts the national line (R049).
    CompanionAssertion,
    /// The public line number and the mode (R050).
    StructuralTripEvidence,
}

impl RouteTier {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CompanionAssertion => "companion_assertion",
            Self::StructuralTripEvidence => "structural_trip_evidence",
        }
    }
}

/// How a source trip matches a base trip (OVL-R066), in canonical order: earlier is stronger.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TripTier {
    FullSignature,
    PatternEndpoints,
    PatternFirst,
    PatternNearest,
    PatternEditNearest,
}

impl TripTier {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FullSignature => "full_signature",
            Self::PatternEndpoints => "pattern_endpoints",
            Self::PatternFirst => "pattern_first",
            Self::PatternNearest => "pattern_nearest",
            Self::PatternEditNearest => "pattern_edit_nearest",
        }
    }
}

/// What a source may state about a matched or added trip (OVL-R077); everything else stays
/// with the base. `shapes` is not a capability (verdict Q14); `headsigns` is (verdict Q12).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    StopCoordinates,
    BoardingPoints,
    CallBoardingPoints,
    RouteShortName,
    RouteLongName,
    RouteColor,
    RouteTextColor,
    Transfers,
    StopZones,
    Schedules,
    Headsigns,
}
