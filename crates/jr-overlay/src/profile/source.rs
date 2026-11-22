//! The source profile: how one regional feed is read and what it may state. Every per-source
//! behaviour of `docs/rules/overlay.md` section 5 is a key here or a named hook with its data;
//! the engine never branches on a source's name.

use std::collections::BTreeMap;

use jr_model::timetable::Mode;
use regex::Regex;
use serde::Deserialize;

use super::ProfileError;
use super::vocabulary::{Capability, RouteTier, TripTier};

/// One source's profile (`policy/overlay/<name>.toml`).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceProfile {
    /// The source's ID (`pid-gtfs`): the namespace of its IDs (section 4) and its registry rows.
    pub source_id: Box<str>,
    /// The published key namespace prefix of its identifiers (`pid` gives `pid:gtfs_trip_id`,
    /// OVL-R112–R114).
    pub key_namespace: Box<str>,
    /// OVL-R008: the largest distance in days between the source's and the base's snapshots
    /// (checked by the build, which knows both).
    pub maximum_snapshot_skew_days: u32,
    pub routes: RoutePolicy,
    pub stops: StopPolicy,
    /// Read by trip matching (OVL-R065–R071).
    pub trips: TripPolicy,
    /// Read by trip-set authority (OVL-R057–R064) and native places (R047).
    pub authority: AuthorityPolicy,
    /// OVL-R077: what the source may state, each with the weakest trip tier a binding may have
    /// to state it (`None`: any tier).
    pub capabilities: BTreeMap<Capability, CapabilityGrant>,
    #[serde(default)]
    pub hooks: Hooks,
}

/// Which source routes enter, and how they find base routes.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutePolicy {
    /// OVL-R013/R028: GTFS `route_type` codes that never enter.
    pub excluded_route_types: Vec<i32>,
    /// OVL-R013: modes that never enter (heavy rail: "No regional static rail data is used").
    pub excluded_modes: Vec<Mode>,
    /// OVL-R049, R050: the route tiers this source may use.
    pub tiers: Vec<RouteTier>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StopPolicy {
    /// OVL-R042: the name plus distance tier's radius.
    pub maximum_distance_metres: f64,
    /// OVL-R043: the coordinate identity tier; absent disables it.
    pub coordinate_identity: Option<CoordinateIdentity>,
    /// OVL-R039: a flat group (no parent station) is split by its stop names.
    pub split_flat_groups_by_name: bool,
    /// OVL-R044, R045.
    pub context: ContextPolicy,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoordinateIdentity {
    pub maximum_metres: f64,
    pub minimum_margin_metres: f64,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextPolicy {
    pub enabled: bool,
    /// Calls of the trip that must already be placed.
    pub minimum_mapped_calls: u32,
    /// Unresolved stop groups one trip may place (each called once).
    pub maximum_unresolved_groups_per_trip: u32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TripPolicy {
    /// OVL-R055: times compare rounded down to this many seconds.
    pub time_resolution_seconds: u32,
    /// OVL-R066: an exact pattern with other times is still a candidate (`pattern_nearest`).
    pub exact_pattern_proximity: bool,
    /// OVL-R065, R068, R080: bind to a base trip whose every call the source makes and keep the
    /// source's further calls.
    pub keep_source_calls: bool,
    /// OVL-R066, R071: the trip tiers this source may use. Their precedence is the canonical
    /// order ([`TripTier`]), not the order listed.
    pub tiers: Vec<TripTier>,
    /// OVL-R067.
    pub pattern_edit: PatternEdit,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternEdit {
    pub enabled: bool,
    pub maximum_edits: u32,
    pub minimum_agreement: f64,
    pub require_same_endpoints: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityPolicy {
    /// OVL-R060: modes in which the source runs the complete trip set of a line.
    pub modes: Vec<Mode>,
    /// OVL-R047, R057, R059: modes in which the source may add trips, routes and stop places
    /// of its own.
    pub source_native_modes: Vec<Mode>,
    /// OVL-R062: integrated systems whose lines run, inside the feed window, only what the
    /// source runs.
    #[serde(default)]
    pub integrated_systems: Vec<IntegratedSystemScope>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntegratedSystemScope {
    /// The system's code as the base states it (JDF LinExt, `30001` for PID).
    pub system: Box<str>,
    /// Lines filed under the system that the source does not publish.
    #[serde(default)]
    pub excluded_lines: Vec<Box<str>>,
    /// The rule is withheld when it would silence more lines than this.
    pub maximum_silenced_lines: u32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityGrant {
    pub minimum_tier: Option<TripTier>,
}

/// The source quirks (section 5), each a named hook with its data. An absent hook is the
/// generic behaviour.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hooks {
    /// OVL-R015: the revision of a trip variant, read from one of its columns.
    pub revision: Option<RevisionHook>,
    /// OVL-R016: the column that groups stops without a parent station.
    pub stop_group: Option<StopGroupHook>,
    /// OVL-R017: the column that keys a source post.
    pub post_key: Option<PostKeyHook>,
    /// OVL-R018: the companion table asserting a trip's national line.
    pub cis_line_claim: Option<CisLineClaimHook>,
    /// OVL-R019: the operator's line/course crosswalk to trip IDs.
    pub operational_crosswalk: Option<CrosswalkHook>,
    /// OVL-R101: the stops' `zone_id`s are zones of this tariff system.
    pub zones: Option<ZonesHook>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionHook {
    /// A `trips.txt` column.
    pub column: Box<str>,
    /// A regular expression with a group named `revision`; the first match counts.
    pub pattern: Box<str>,
    /// The `strptime` format of the captured text (`%y%m%d`).
    pub date_format: Box<str>,
    #[serde(skip)]
    pub(crate) compiled: Option<Regex>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StopGroupHook {
    /// A `stops.txt` column (`asw_node_id`); a blank cell falls back to the stop's own ID.
    pub key_column: Box<str>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostKeyHook {
    /// A `stops.txt` column (`asw_stop_id`); a blank cell falls back to the stop's own ID.
    pub column: Box<str>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CisLineClaimHook {
    /// The companion member (`route_sub_agencies.txt`).
    pub table: Box<str>,
    /// `trips.txt` columns forming the key, in order.
    pub source_keys: Vec<Box<str>>,
    /// The companion table's columns forming the same key.
    pub lookup_keys: Vec<Box<str>>,
    /// The companion column holding the national line.
    pub value_column: Box<str>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrosswalkHook {
    /// The verbatim member (`api.txt`).
    pub member: Box<str>,
    /// The grammar of one non-blank, trimmed line, with groups `line`, `course` and `trip`.
    pub pattern: Box<str>,
    #[serde(skip)]
    pub(crate) compiled: Option<Regex>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZonesHook {
    /// The tariff system the zones belong to (`ids-jmk`).
    pub system: Box<str>,
}

impl SourceProfile {
    /// Checks what serde cannot and compiles the hooks' patterns.
    pub(super) fn validate(mut self) -> Result<Self, ProfileError> {
        let invalid = |problem: String| ProfileError::Invalid {
            profile: self.source_id.to_string(),
            problem,
        };
        let mut problems = Vec::new();
        if self.source_id.trim().is_empty() {
            problems.push("source_id is blank".to_owned());
        }
        if self.key_namespace.trim().is_empty() {
            problems.push("key_namespace is blank".to_owned());
        }
        let stops = &self.stops;
        if stops.maximum_distance_metres.is_nan() || stops.maximum_distance_metres <= 0.0 {
            problems.push("stops.maximum_distance_metres must be positive".to_owned());
        }
        if let Some(identity) = stops.coordinate_identity
            && !(identity.maximum_metres > 0.0 && identity.minimum_margin_metres >= 0.0)
        {
            problems.push(
                "stops.coordinate_identity needs a positive maximum and a margin of at least 0"
                    .to_owned(),
            );
        }
        if stops.context.minimum_mapped_calls < 1 {
            problems.push("stops.context.minimum_mapped_calls must be at least 1".to_owned());
        }
        if stops.context.maximum_unresolved_groups_per_trip < 1 {
            problems.push(
                "stops.context.maximum_unresolved_groups_per_trip must be at least 1".to_owned(),
            );
        }
        let trips = &self.trips;
        if trips.time_resolution_seconds == 0 {
            problems.push("trips.time_resolution_seconds must be positive".to_owned());
        }
        if !(0.0..=1.0).contains(&trips.pattern_edit.minimum_agreement) {
            problems.push("trips.pattern_edit.minimum_agreement must lie in 0..=1".to_owned());
        }
        if trips.tiers.is_empty() || has_duplicates(&trips.tiers) {
            problems.push("trips.tiers must be non-empty and distinct".to_owned());
        }
        if self.routes.tiers.is_empty() || has_duplicates(&self.routes.tiers) {
            problems.push("routes.tiers must be non-empty and distinct".to_owned());
        }
        let authority = &self.authority;
        for mode in &authority.source_native_modes {
            if !authority.modes.contains(mode) {
                problems.push(format!(
                    "authority.source_native_modes: {mode:?} is not in authority.modes"
                ));
            }
        }
        for scope in &authority.integrated_systems {
            if scope.system.trim().is_empty() || scope.maximum_silenced_lines < 1 {
                problems.push(
                    "authority.integrated_systems needs a system and a maximum of at least 1"
                        .to_owned(),
                );
            }
        }
        for (capability, grant) in &self.capabilities {
            if let Some(tier) = grant.minimum_tier
                && !trips.tiers.contains(&tier)
            {
                problems.push(format!(
                    "capabilities.{capability:?}: minimum tier {} is not one of trips.tiers",
                    tier.as_str()
                ));
            }
        }
        if let Some(claim) = &self.hooks.cis_line_claim
            && (claim.source_keys.is_empty()
                || claim.source_keys.len() != claim.lookup_keys.len()
                || claim.table.trim().is_empty()
                || claim.value_column.trim().is_empty())
        {
            problems.push("hooks.cis_line_claim needs a table, a value column and keys of equal, non-zero length".to_owned());
        }
        if let Some(revision) = &mut self.hooks.revision {
            match Regex::new(&revision.pattern) {
                Ok(regex)
                    if regex
                        .capture_names()
                        .flatten()
                        .any(|name| name == "revision") =>
                {
                    revision.compiled = Some(regex);
                }
                Ok(_) => {
                    problems.push("hooks.revision.pattern has no group named revision".to_owned())
                }
                Err(error) => problems.push(format!("hooks.revision.pattern: {error}")),
            }
        }
        if let Some(crosswalk) = &mut self.hooks.operational_crosswalk {
            match Regex::new(&crosswalk.pattern) {
                Ok(regex)
                    if ["line", "course", "trip"].iter().all(|group| {
                        regex.capture_names().flatten().any(|name| name == *group)
                    }) =>
                {
                    crosswalk.compiled = Some(regex);
                }
                Ok(_) => problems.push(
                    "hooks.operational_crosswalk.pattern needs groups line, course and trip"
                        .to_owned(),
                ),
                Err(error) => {
                    problems.push(format!("hooks.operational_crosswalk.pattern: {error}"))
                }
            }
        }
        if problems.is_empty() {
            Ok(self)
        } else {
            Err(invalid(problems.join("; ")))
        }
    }

    /// Whether a binding of `tier` may state `capability` (OVL-R077).
    pub fn grants(&self, capability: Capability, tier: TripTier) -> bool {
        self.capabilities
            .get(&capability)
            .is_some_and(|grant| grant.minimum_tier.is_none_or(|minimum| tier <= minimum))
    }
}

fn has_duplicates<T: PartialEq>(values: &[T]) -> bool {
    values
        .iter()
        .enumerate()
        .any(|(index, value)| values[..index].contains(value))
}
