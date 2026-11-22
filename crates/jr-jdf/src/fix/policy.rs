//! The constants of stop matching (`policy/jdf_fix.toml`) and the road name data
//! (`data/names/road_names.toml`). Every field belongs to a named rule; the TOML files say which.

use std::collections::BTreeMap;

use jr_geo::names::Profile;
use jr_geo::towns::TownSynonyms;
use serde::Deserialize;

const BUILTIN: &str = include_str!("../../../../policy/jdf_fix.toml");
const ROAD_NAMES: &str = include_str!("../../../../data/names/road_names.toml");

/// Everything stop matching is configured by.
#[derive(Clone, Debug)]
pub struct Policy {
    pub file: PolicyFile,
    pub names: RoadNames,
}

impl Policy {
    /// The policy shipped with the compiler.
    pub fn builtin() -> Policy {
        Policy::parse(BUILTIN, ROAD_NAMES).expect("policy/jdf_fix.toml and road_names.toml")
    }

    pub fn parse(policy: &str, road_names: &str) -> Result<Policy, toml::de::Error> {
        Ok(Policy {
            file: toml::from_str(policy)?,
            names: toml::from_str(road_names)?,
        })
    }
}

/// `data/names/road_names.toml`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoadNames {
    /// JDF-R039–R042.
    pub matching: Profile,
    /// JDF-R214–R215 (the merge's; not used here).
    pub identity: Profile,
    /// JDF-R054.
    pub town_synonyms: TownSynonyms,
}

/// `policy/jdf_fix.toml`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyFile {
    pub home_country: Box<str>,
    pub catalogues: Catalogues,
    pub osm: Osm,
    pub batch: BatchPolicy,
    pub matching: Matching,
    /// Verdict Q8: JDF country code to ISO alpha-2.
    pub countries: BTreeMap<Box<str>, Box<str>>,
    pub mhd: Mhd,
    pub geography: Geography,
    pub posts: Posts,
    pub plausibility: Plausibility,
    pub estimate: Estimate,
    pub diagnostics: KmCheck,
}

/// JDF-R006, R007.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalogues {
    pub gapfill: Vec<Box<str>>,
    pub source_prefix: Box<str>,
}

/// A tag with one of several values.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TagValues {
    pub key: Box<str>,
    pub values: Vec<Box<str>>,
}

/// JDF-R009–R013.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Osm {
    pub excluded_keys: Vec<Box<str>>,
    pub requires: Vec<(Box<str>, Box<str>)>,
    pub eligible: Vec<TagValues>,
    pub name_tags: Vec<Box<str>>,
    pub full_name_tag: Box<str>,
    pub name_joiner: Box<str>,
    pub source_prefix: Box<str>,
    pub post_evidence: PostEvidence,
}

/// JDF-R013.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostEvidence {
    pub affirmative: Vec<Box<str>>,
    pub negative: Vec<Box<str>>,
    pub lifecycle_keys: Vec<Box<str>>,
    pub lifecycle_tag: Box<str>,
    pub active: Box<str>,
    pub audit_keys: Vec<Box<str>>,
    pub modes: Vec<ModeEvidence>,
}

/// JDF-R013: the tags that declare a mode and the access keys that affirm or deny it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModeEvidence {
    pub mode: Box<str>,
    pub tags: Vec<TagValues>,
    pub access_keys: Vec<Box<str>>,
}

/// JDF-R033.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchPolicy {
    pub min_called_stops: usize,
}

/// JDF-R043–R052.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Matching {
    pub okres_aliases: BTreeMap<Box<str>, Box<str>>,
    pub okres_adjacency_m: f64,
    pub okres_border_tolerance_m: f64,
    pub max_group_radius_m: f64,
    pub secondary_km_tolerance: f64,
}

/// JDF-R056–R060.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mhd {
    pub min_supporting_stops: usize,
    pub town_groups: Vec<Vec<Box<str>>>,
    pub route_town_prefix: Box<str>,
    pub route_town_suffix: Box<str>,
    pub route_town_delimiters: Vec<char>,
    pub route_name_bonus: usize,
    pub max_name_parts: usize,
    pub min_match_gain: usize,
}

/// JDF-R063.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Geography {
    pub town_km_tolerance: f64,
}

/// JDF-R066.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Posts {
    pub candidate_radius_m: f64,
}

/// JDF-R068–R070.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plausibility {
    pub rollover_backstep_min: f64,
    pub day_min: f64,
    pub slack_m: f64,
    pub speed_kmh: f64,
}

/// JDF-R077.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Estimate {
    pub route_end_offset_m_per_call: f64,
    pub metres_per_degree_latitude: f64,
}

/// JDF-R072.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KmCheck {
    pub km_check_tolerance_km: f64,
    pub km_check_factor: f64,
}

impl PolicyFile {
    /// R045: the okres code compared in place of `code`.
    pub(crate) fn okres_alias<'a>(&'a self, code: &'a str) -> &'a str {
        self.matching
            .okres_aliases
            .get(code)
            .map_or(code, |alias| alias)
    }

    /// Verdict Q8: the ISO code a country code stands for.
    pub(crate) fn iso_country<'a>(&'a self, code: &'a str) -> &'a str {
        self.countries.get(code).map_or(code, |iso| iso)
    }

    /// Verdict Q8: the JDF code of an ISO code (the code itself when the table has none).
    pub(crate) fn jdf_country(&self, iso: &str) -> Box<str> {
        self.countries
            .iter()
            .find(|(_, value)| &***value == iso)
            .map_or_else(|| iso.into(), |(jdf, _)| jdf.clone())
    }

    pub(crate) fn same_country(&self, left: &str, right: &str) -> bool {
        self.iso_country(left) == self.iso_country(right)
    }

    pub(crate) fn is_home(&self, country: &str) -> bool {
        self.same_country(country, &self.home_country)
    }
}
