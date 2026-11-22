//! The merge's constants, read from `policy/jdf_merge.toml`. Every field belongs to a named rule
//! of `docs/rules/jdf-merge.md`; the TOML file says which.

use serde::Deserialize;

const BUILTIN: &str = include_str!("../../../../policy/jdf_merge.toml");

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub stops: StopsPolicy,
    pub candidates: CandidatesPolicy,
}

impl Policy {
    /// The policy shipped with the compiler.
    pub fn builtin() -> Policy {
        Policy::parse(BUILTIN).expect("policy/jdf_merge.toml is valid")
    }

    pub fn parse(text: &str) -> Result<Policy, toml::de::Error> {
        toml::from_str(text)
    }
}

/// JDF-R207, R217–R231.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StopsPolicy {
    pub match_distance_m: f64,
    pub grid_max_latitude: f64,
    pub min_suffix_components: usize,
    pub fuzzy_min_tokens: usize,
    pub fuzzy_min_levenshtein: f64,
    pub fuzzy_min_token_dice: f64,
    pub provisional_base: u64,
    pub provisional_span: u64,
    pub location_conflict_m: f64,
}

/// JDF-R232–R234.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidatesPolicy {
    pub reason_new: Box<str>,
    pub reason_ambiguous: Box<str>,
    pub reason_alias: Box<str>,
    pub suggestion_min_similarity: f64,
    pub suggestion_max: usize,
}
