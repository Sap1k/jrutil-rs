//! The constants of the graph and the router (`policy/routing.toml`). Every field belongs to a
//! named rule; the TOML file says which.

use serde::Deserialize;

const BUILTIN: &str = include_str!("../../../../policy/routing.toml");

/// `policy/routing.toml`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingPolicy {
    pub graph: GraphPolicy,
    pub snap: SnapPolicy,
    pub search: SearchPolicy,
    pub evidence: EvidencePolicy,
}

impl RoutingPolicy {
    /// The policy shipped with the router.
    pub fn builtin() -> RoutingPolicy {
        RoutingPolicy::parse(BUILTIN).expect("policy/routing.toml")
    }

    pub fn parse(text: &str) -> Result<RoutingPolicy, toml::de::Error> {
        toml::from_str(text)
    }
}

/// A tag with one value.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TagValue {
    pub key: Box<str>,
    pub value: Box<str>,
}

/// POST-R011–R018.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphPolicy {
    pub highway_key: Box<str>,
    pub road_highways: Vec<Box<str>>,
    pub closed_highway: Box<str>,
    pub closed_keys: Vec<Box<str>>,
    pub general_access_keys: Vec<Box<str>>,
    pub transit_access_keys: Vec<Box<str>>,
    pub affirmative_access: Vec<Box<str>>,
    pub negative_access: Vec<Box<str>>,
    pub restricted_access_factor: f64,
    pub service_highway: Box<str>,
    pub tram: TagValue,
    pub oneway_key: Box<str>,
    pub oneway_forward: Vec<Box<str>>,
    pub oneway_backward: Vec<Box<str>>,
    pub roundabout: TagValue,
    pub barrier_key: Box<str>,
    pub restriction_key: Box<str>,
    pub restriction_only_prefix: Box<str>,
    pub restriction_from_role: Box<str>,
    pub restriction_via_role: Box<str>,
    pub restriction_to_role: Box<str>,
}

/// POST-R020.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapPolicy {
    pub radius_m: f64,
    pub candidates: usize,
    pub band_m: f64,
}

/// POST-R021.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchPolicy {
    pub max_expansions: usize,
    pub max_cost_m: f64,
    pub tolerance_m: f64,
}

/// POST-R050–R053.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidencePolicy {
    pub snaps_per_anchor: usize,
    pub max_anchor_snaps: usize,
    pub max_variants: usize,
    pub horizon_m: f64,
    pub heading_window_m: f64,
    pub face_centre_band_m: f64,
}
