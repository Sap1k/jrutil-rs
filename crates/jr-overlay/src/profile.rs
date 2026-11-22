//! Source and base profiles (`policy/overlay/*.toml`, verdicts Q2 and Q8). Each source is
//! governed by its own profile; the shipped profiles state the values that are effective in a
//! combined F# run today (`docs/rules/overlay.md` section 2.3), so the engine decides as F# does
//! while the files say what really applies. A profile error is fatal (verdict Q11).

mod base;
mod source;
mod vocabulary;

pub use base::{BaseProfile, RouteTypeMode};
pub use source::{
    AuthorityPolicy, CapabilityGrant, CisLineClaimHook, ContextPolicy, CoordinateIdentity,
    CrosswalkHook, Hooks, IntegratedSystemScope, PatternEdit, PostKeyHook, RevisionHook,
    RoutePolicy, SourceProfile, StopGroupHook, StopPolicy, TripPolicy, ZonesHook,
};
pub use vocabulary::{Capability, RouteTier, TripTier};

const PID: &str = include_str!("../../../policy/overlay/pid.toml");
const IDS_JMK: &str = include_str!("../../../policy/overlay/ids-jmk.toml");
const ROAD_BASE: &str = include_str!("../../../policy/overlay/road-base.toml");

/// What the reader must keep of a feed beyond the standard columns.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReaderRequest {
    /// Per standard file, the non-standard columns to keep.
    pub extra_columns: Vec<(Box<str>, Vec<Box<str>>)>,
    /// Other CSV members to read as text tables.
    pub tables: Vec<Box<str>>,
    /// Members to return verbatim.
    pub raw: Vec<Box<str>>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("overlay profile: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("overlay profile {profile}: {problem}")]
    Invalid { profile: String, problem: String },
}

/// The standard `trips.txt` columns the reader types; any other column is an extra.
pub(crate) const TRIP_COLUMNS: [&str; 11] = [
    "route_id",
    "service_id",
    "trip_id",
    "trip_headsign",
    "trip_short_name",
    "direction_id",
    "block_id",
    "shape_id",
    "wheelchair_accessible",
    "bikes_allowed",
    "cars_allowed",
];

/// The standard `stops.txt` text columns the reader types.
pub(crate) const STOP_COLUMNS: [&str; 10] = [
    "stop_id",
    "stop_code",
    "stop_name",
    "tts_stop_name",
    "stop_desc",
    "zone_id",
    "stop_url",
    "parent_station",
    "stop_timezone",
    "level_id",
];

impl SourceProfile {
    /// A profile shipped with the compiler, by name (`pid`, `ids-jmk`).
    pub fn builtin(name: &str) -> Option<Result<SourceProfile, ProfileError>> {
        let text = match name {
            "pid" => PID,
            "ids-jmk" => IDS_JMK,
            _ => return None,
        };
        Some(SourceProfile::parse(text))
    }

    /// Reads and validates a profile.
    pub fn parse(text: &str) -> Result<SourceProfile, ProfileError> {
        toml::from_str::<SourceProfile>(text)?.validate()
    }

    /// The trips.txt columns its hooks read that the reader does not type.
    pub(crate) fn extra_trip_columns(&self) -> Vec<Box<str>> {
        let mut columns: Vec<Box<str>> = Vec::new();
        let mut want = |column: &str| {
            if !TRIP_COLUMNS.contains(&column) && !columns.iter().any(|c| &**c == column) {
                columns.push(column.into());
            }
        };
        if let Some(revision) = &self.hooks.revision {
            want(&revision.column);
        }
        if let Some(claim) = &self.hooks.cis_line_claim {
            claim.source_keys.iter().for_each(|key| want(key));
        }
        columns
    }

    /// The stops.txt columns its hooks read that the reader does not type.
    pub(crate) fn extra_stop_columns(&self) -> Vec<Box<str>> {
        let mut columns: Vec<Box<str>> = Vec::new();
        let hooks = [
            self.hooks.stop_group.as_ref().map(|hook| &*hook.key_column),
            self.hooks.post_key.as_ref().map(|hook| &*hook.column),
        ];
        for column in hooks.into_iter().flatten() {
            if !STOP_COLUMNS.contains(&column) && !columns.iter().any(|c| &**c == column) {
                columns.push(column.into());
            }
        }
        columns
    }

    pub fn reader(&self) -> ReaderRequest {
        let mut request = ReaderRequest::default();
        let trips = self.extra_trip_columns();
        if !trips.is_empty() {
            request.extra_columns.push(("trips.txt".into(), trips));
        }
        let stops = self.extra_stop_columns();
        if !stops.is_empty() {
            request.extra_columns.push(("stops.txt".into(), stops));
        }
        if let Some(claim) = &self.hooks.cis_line_claim {
            request.tables.push(claim.table.clone());
        }
        if let Some(crosswalk) = &self.hooks.operational_crosswalk {
            request.raw.push(crosswalk.member.clone());
        }
        request
    }
}

impl BaseProfile {
    /// The national road timetable compiled from JDF.
    pub fn road() -> Result<BaseProfile, ProfileError> {
        BaseProfile::parse(ROAD_BASE)
    }

    pub fn parse(text: &str) -> Result<BaseProfile, ProfileError> {
        toml::from_str::<BaseProfile>(text)?.validate()
    }
}
