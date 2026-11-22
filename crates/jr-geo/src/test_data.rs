//! The shipped road name data, read at compile time for tests.

use serde::Deserialize;

use crate::names::{Analyzer, Profile};
use crate::towns::TownSynonyms;

#[derive(Deserialize)]
struct RoadNames {
    matching: Profile,
    identity: Profile,
    town_synonyms: TownSynonyms,
}

fn road_names() -> RoadNames {
    toml::from_str(include_str!("../../../data/names/road_names.toml"))
        .expect("data/names/road_names.toml")
}

/// The synonym file's lines as the `jr-formats` reader returns them (text before `#`, split at
/// commas, trimmed, blanks dropped); a test stand-in, since this crate does not read files.
fn synonym_lines() -> Vec<Vec<&'static str>> {
    include_str!("../../../data/names/stop_synonyms.txt")
        .lines()
        .map(|line| {
            line.split('#')
                .next()
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|alternative| !alternative.is_empty())
                .collect::<Vec<_>>()
        })
        .filter(|alternatives| !alternatives.is_empty())
        .collect()
}

pub(crate) fn matching() -> Analyzer {
    Analyzer::new(road_names().matching, synonym_lines())
}

pub(crate) fn identity() -> Analyzer {
    Analyzer::new(road_names().identity, synonym_lines())
}

pub(crate) fn town_synonyms() -> TownSynonyms {
    road_names().town_synonyms
}
