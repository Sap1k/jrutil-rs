//! Town names: looking a town up in the Czech municipality names and the European town names
//! (`docs/rules/jdf-read.md` R016 and the rules that query them, R012, R050, R051, R053, R056,
//! R058, R059, R063).
//!
//! The F# built one `StopMatcher` per gazetteer with the analyser of the stop matcher, so a
//! gazetteer here is a [`crate::stop_index::StopIndex`] over the gazetteer's names, entry `i`
//! being row or feature `i`, with the matching profile; every rule above only asks for the exact
//! entries (R042 score 1), except R012, which compares two names directly
//! ([`crate::names::Analyzer::name_match`]). The one step the F# applied to municipality lookups
//! only is the town-name table of R054 ([`TownSynonyms`]); the European gazetteer is queried with
//! the name as it is.

use std::collections::BTreeMap;

use jr_model::Rule;
use serde::Deserialize;

/// Before a municipality is looked up by name, a name the table lists is replaced by the
/// municipality name it stands for (exact, case-sensitive key). Applied to every municipality
/// lookup (the F# skipped it in R063, B14).
///
/// Timetables write some towns differently from the municipality register (`Dolní Malá Úpa`
/// for the municipality `Malá Úpa`).
pub const JDF_R054: Rule = Rule {
    id: "JDF-R054",
    summary: "a timetable town name listed in the town table is looked up as its municipality",
};

pub const RULES: &[Rule] = &[JDF_R054];

/// The town-name table of R054 (`data/names/road_names.toml` `[town_synonyms]`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct TownSynonyms(BTreeMap<Box<str>, Box<str>>);

impl TownSynonyms {
    pub fn new(pairs: impl IntoIterator<Item = (Box<str>, Box<str>)>) -> Self {
        TownSynonyms(pairs.into_iter().collect())
    }

    /// The name a municipality lookup uses for `town`.
    pub fn resolve<'a>(&'a self, town: &'a str) -> &'a str {
        self.0.get(town).map_or(town, |name| name)
    }
}
