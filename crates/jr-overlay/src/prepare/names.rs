//! The overlay's name rules (OVL-R032–R035) over the one name machinery of `jr_geo::names`,
//! with their data in `data/names/overlay_names.toml` and the shared synonym table.

use jr_formats::geodata::synonyms;
use jr_geo::names::{HeadsignReason, HeadsignTables, Profile, RankTables, StopNameRanker};
use jr_model::Rule;
use serde::Deserialize;

const OVERLAY_NAMES: &str = include_str!("../../../../data/names/overlay_names.toml");

/// A stop name's words for keys: lower case, runs of letters and numbers. A flat source group's
/// registry key joins them by `-` (R039, section 4.2); a base place's stable name key, which
/// reviewed overrides name targets by (R040), joins them by U+001F.
///
/// The reviewed registry (`overlay_places.csv`) stores keys built this way; another
/// normalisation would unpin its places.
pub const OVL_R032: Rule = Rule {
    id: "OVL-R032",
    summary: "key words of a stop name: lower case, letters and numbers",
};

/// Two stop names say the same place when their words match in order: equal, a dotted
/// abbreviation beginning the other word, one of two words of three letters or more beginning
/// the other, or an abbreviation spelled out (R034); a rank counts the words spelled
/// differently. A junction's trailing distance is ignored, and the longer name may lead with
/// locality segments the other leaves out, each left-out word adding one.
///
/// Regional feeds and CIS spell one stop differently (`Bílovice nad Svitavou, železniční
/// stanice` and `Bílovice n.Svit.,žel.st.`; `ZOO` and `Brno,Bystrc,ZOO`).
pub const OVL_R033: Rule = Rule {
    id: "OVL-R033",
    summary: "stop names match word by word through abbreviations; the rank counts differences",
};

/// Abbreviations a prefix cannot find (acronyms such as `ZD` for `zemědělské družstvo`) come from
/// the data file and from the shared synonym table, whose first and dotted alternatives
/// abbreviate the others.
///
/// CIS abbreviates institutions by acronyms that regional feeds spell out.
pub const OVL_R034: Rule = Rule {
    id: "OVL-R034",
    summary: "acronym expansions from the data file and the shared synonym table",
};

/// A display headsign resolves to the one full destination name it abbreviates; a route
/// description (`via`, `přes`, `→`, `;`) and a headsign naming none or several stay as they are.
///
/// Regional feeds shorten terminal names on displays (`Černý Most` for `Praha,Černý Most`).
pub const OVL_R035: Rule = Rule {
    id: "OVL-R035",
    summary: "a headsign resolves to the one destination name it abbreviates",
};

pub const RULES: &[Rule] = &[OVL_R032, OVL_R033, OVL_R034, OVL_R035];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NameFile {
    group_name: Profile,
    rank: RankTables,
    headsigns: HeadsignTables,
}

/// The compiled name rules.
#[derive(Debug)]
pub struct Names {
    group_name: Profile,
    ranker: StopNameRanker,
    headsigns: HeadsignTables,
}

impl Names {
    pub fn new(synonym_lines: &[synonyms::Line]) -> Self {
        let file: NameFile =
            toml::from_str(OVERLAY_NAMES).expect("data/names/overlay_names.toml is valid");
        let lines = synonym_lines.iter().map(|line| line.alternatives.iter());
        Names {
            group_name: file.group_name,
            ranker: StopNameRanker::new(file.rank, lines),
            headsigns: file.headsigns,
        }
    }

    /// R032: the key words.
    pub fn key_words(&self, name: &str) -> Vec<String> {
        self.group_name.raw_tokens(name)
    }

    /// R032: the stable name key (words joined by U+001F).
    pub fn stable_key(&self, name: &str) -> String {
        self.key_words(name).join("\u{1f}")
    }

    /// R033.
    pub fn rank(&self, left: &str, right: &str) -> Option<u32> {
        self.ranker.rank(left, right)
    }

    /// R035.
    pub fn resolve_headsign(
        &self,
        raw: &str,
        destinations: &[&str],
    ) -> (Option<String>, HeadsignReason) {
        self.ranker
            .resolve_headsign(&self.headsigns, raw, destinations)
    }
}
