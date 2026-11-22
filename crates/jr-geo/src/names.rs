//! The one name normaliser (AGENTS.md rule 5) and the matching built on it: normalised forms,
//! railway name cores, edit similarity and protected qualifiers (`docs/rules/czptt.md`,
//! CZPTT-R090–R093).
//!
//! Word lists and tables are data ([`NameTables`], deserialised from a policy file), never
//! literals here. Lengths and edit distances count Unicode scalar values (`char`s).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use jr_model::Rule;
use serde::Deserialize;

/// A name's comparable form: transliterated (the table, for example `ß→ss`, `Ł→L` and the
/// decompositions of accented Latin letters), combining marks removed, every run of characters
/// that are neither letters nor digits replaced by one space, trimmed, upper-cased.
///
/// Railway point names cross borders with their diacritics and punctuation (`Głuchołazy`,
/// `Praha hl.n.`, `Ústí nad Labem-Střekov`); OSM spells the same place with or without them.
pub const CZPTT_R090: Rule = Rule {
    id: "CZPTT-R090",
    summary: "names compare transliterated, without marks, punctuation runs as one space, upper-case",
};

/// A name's railway core: a trailing parenthesised qualifier removed from the raw name, the rest
/// normalised (R090), then trailing railway suffix tokens (`Bf`, `Hp`, `zast.`, …) and route
/// section tokens (`R12`) stripped; a name that is nothing but such tokens keeps its full form.
/// Applied to the raw name on both sides of a comparison (the F# applied it to an already
/// normalised timetable name, so the parenthesis never came off there: B6, fixed).
///
/// Timetables and OSM disagree about operational suffixes (`Unterretzbach z` against
/// `Unterretzbach`) and district qualifiers (`Bärenstein (Annaberg)` against `Bärenstein`).
pub const CZPTT_R091: Rule = Rule {
    id: "CZPTT-R091",
    summary: "railway core: strip a trailing (qualifier), then trailing railway suffix and R<n> tokens",
};

/// Similarity of two names: the best of `1 − levenshtein / longer length` over the full forms,
/// the cores and the cores without spaces; 1 for equal forms, 0 when one is empty. Lengths count
/// characters (the F# counted UTF-16 units).
///
/// Abbreviated timetable names (`Głuchołas.F-ka Mebli`) only resemble their OSM names
/// (`Głuchołazy Fabryka Mebli`).
pub const CZPTT_R092: Rule = Rule {
    id: "CZPTT-R092",
    summary: "name similarity is the best edit similarity of full forms, cores and compact cores",
};

/// A fuzzy candidate is compatible with an expected name only if every protected qualifier
/// token of the expected name (`Ost`, `střed`, `Górna`, `město`, …) is a token of the candidate
/// or a prefix of one. Exact, core and code matches do not use it.
///
/// A qualifier names a different station: `Bad Schandau Ost` must not match `Bad Schandau`,
/// although the two are similar enough by edit distance.
pub const CZPTT_R093: Rule = Rule {
    id: "CZPTT-R093",
    summary: "fuzzy name matches keep every protected qualifier token of the expected name",
};

/// The word lists and tables of the name rules, compiled for lookup when deserialised.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(try_from = "RawNameTables")]
pub struct NameTables {
    transliteration: HashMap<char, Box<str>>,
    marks: Vec<(char, char)>,
    railway_suffixes: BTreeSet<String>,
    route_section_prefix: String,
    qualifier_open: char,
    qualifier_close: char,
    protected_qualifiers: BTreeSet<String>,
}

/// The policy form of [`NameTables`].
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNameTables {
    /// One character to its replacement (R090).
    transliteration: BTreeMap<String, String>,
    /// Inclusive ranges of combining marks that are dropped (R090).
    marks: Vec<(char, char)>,
    /// Trailing tokens a railway core drops, as written; normalised on load (R091).
    railway_suffix_tokens: Vec<String>,
    /// A route section token is this prefix followed by one or more ASCII digits (R091).
    route_section_prefix: String,
    /// The brackets of a trailing qualifier (R091).
    qualifier_open: char,
    qualifier_close: char,
    /// Protected qualifier tokens, as written; normalised on load (R093).
    protected_qualifiers: Vec<String>,
}

impl TryFrom<RawNameTables> for NameTables {
    type Error = String;

    fn try_from(raw: RawNameTables) -> Result<Self, String> {
        let mut transliteration = HashMap::new();
        for (from, to) in raw.transliteration {
            let mut chars = from.chars();
            let (Some(character), None) = (chars.next(), chars.next()) else {
                return Err(format!("transliteration key `{from}` is not one character"));
            };
            transliteration.insert(character, to.into_boxed_str());
        }
        let mut tables = NameTables {
            transliteration,
            marks: raw.marks,
            railway_suffixes: BTreeSet::new(),
            route_section_prefix: String::new(),
            qualifier_open: raw.qualifier_open,
            qualifier_close: raw.qualifier_close,
            protected_qualifiers: BTreeSet::new(),
        };
        let normalised = |words: &[String], tables: &NameTables| -> BTreeSet<String> {
            words
                .iter()
                .map(|word| normalize(word, tables))
                .filter(|word| !word.is_empty())
                .collect()
        };
        tables.railway_suffixes = normalised(&raw.railway_suffix_tokens, &tables);
        tables.protected_qualifiers = normalised(&raw.protected_qualifiers, &tables);
        tables.route_section_prefix = normalize(&raw.route_section_prefix, &tables);
        if tables.route_section_prefix.is_empty() {
            return Err("route_section_prefix normalises to nothing".to_owned());
        }
        Ok(tables)
    }
}

impl NameTables {
    fn is_mark(&self, character: char) -> bool {
        self.marks
            .iter()
            .any(|&(first, last)| (first..=last).contains(&character))
    }

    fn is_railway_suffix(&self, token: &str) -> bool {
        self.railway_suffixes.contains(token)
            || token
                .strip_prefix(self.route_section_prefix.as_str())
                .is_some_and(|digits| {
                    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
                })
    }
}

/// CZPTT-R090.
pub fn normalize(name: &str, tables: &NameTables) -> String {
    let mut transliterated = String::with_capacity(name.len());
    for character in name.chars() {
        match tables.transliteration.get(&character) {
            Some(replacement) => transliterated.push_str(replacement),
            None => transliterated.push(character),
        }
    }
    let mut out = String::with_capacity(transliterated.len());
    let mut gap = false;
    for character in transliterated.chars() {
        if tables.is_mark(character) {
            continue;
        }
        if !character.is_alphanumeric() {
            gap = true;
            continue;
        }
        if gap && !out.is_empty() {
            out.push(' ');
        }
        gap = false;
        // Simple (one-to-one) case mapping: a character whose upper case is several
        // characters stays as it is.
        let mut upper = character.to_uppercase();
        out.push(match (upper.next(), upper.next()) {
            (Some(single), None) => single,
            _ => character,
        });
    }
    out
}

/// CZPTT-R091, on a raw name.
pub fn railway_core(name: &str, tables: &NameTables) -> String {
    let full = normalize(without_trailing_qualifier(name, tables), tables);
    let tokens: Vec<&str> = full.split(' ').filter(|token| !token.is_empty()).collect();
    let mut kept = tokens.len();
    while kept > 0 && tables.is_railway_suffix(tokens[kept - 1]) {
        kept -= 1;
    }
    if kept == 0 {
        normalize(name, tables)
    } else {
        tokens[..kept].join(" ")
    }
}

/// The name without a trailing `\s*\([^)]*\)\s*$` (brackets from the tables): the qualifier
/// starts at the first opening bracket after the last closing one before the final bracket.
fn without_trailing_qualifier<'a>(name: &'a str, tables: &NameTables) -> &'a str {
    let Some(body) = name.trim_end().strip_suffix(tables.qualifier_close) else {
        return name;
    };
    let start = body
        .rfind(tables.qualifier_close)
        .map_or(0, |at| at + tables.qualifier_close.len_utf8());
    match body[start..].find(tables.qualifier_open) {
        Some(open) => name[..start + open].trim_end(),
        None => name,
    }
}

/// A name's normalised forms, computed once per name.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    full: String,
    core: String,
    full_chars: Box<[char]>,
    core_chars: Box<[char]>,
    compact_chars: Box<[char]>,
    tokens: Box<[Box<str>]>,
}

impl Prepared {
    pub fn new(name: &str, tables: &NameTables) -> Self {
        let full = normalize(name, tables);
        let core = railway_core(name, tables);
        Self {
            full_chars: full.chars().collect(),
            core_chars: core.chars().collect(),
            compact_chars: core.chars().filter(|&character| character != ' ').collect(),
            tokens: full
                .split(' ')
                .filter(|token| !token.is_empty())
                .map(Box::from)
                .collect(),
            full,
            core,
        }
    }

    /// The normalised name (R090).
    pub fn full(&self) -> &str {
        &self.full
    }

    /// The railway core (R091).
    pub fn core(&self) -> &str {
        &self.core
    }
}

/// CZPTT-R092.
pub fn similarity(a: &Prepared, b: &Prepared) -> f64 {
    edit_similarity(&a.full_chars, &b.full_chars)
        .max(edit_similarity(&a.core_chars, &b.core_chars))
        .max(edit_similarity(&a.compact_chars, &b.compact_chars))
}

/// [`similarity`] when it is at least `minimum`, `None` otherwise; skips the edit distances that
/// the length difference already rules out.
pub fn similarity_at_least(a: &Prepared, b: &Prepared, minimum: f64) -> Option<f64> {
    let full_bound = similarity_bound(&a.full_chars, &b.full_chars);
    let core_bound = similarity_bound(&a.core_chars, &b.core_chars);
    let compact_bound = similarity_bound(&a.compact_chars, &b.compact_chars);
    if full_bound.max(core_bound).max(compact_bound) < minimum {
        return None;
    }
    let refine = |best: f64, bound: f64, left: &[char], right: &[char]| {
        if bound < minimum || bound <= best {
            best
        } else {
            best.max(edit_similarity(left, right))
        }
    };
    let full = if full_bound < minimum {
        0.0
    } else {
        edit_similarity(&a.full_chars, &b.full_chars)
    };
    let core = refine(full, core_bound, &a.core_chars, &b.core_chars);
    let best = refine(core, compact_bound, &a.compact_chars, &b.compact_chars);
    (best >= minimum).then_some(best)
}

/// CZPTT-R093.
pub fn qualifiers_compatible(
    expected: &Prepared,
    candidate: &Prepared,
    tables: &NameTables,
) -> bool {
    expected
        .tokens
        .iter()
        .filter(|token| tables.protected_qualifiers.contains(&***token))
        .all(|required| {
            candidate
                .tokens
                .iter()
                .any(|token| token.starts_with(&**required))
        })
}

fn edit_similarity(left: &[char], right: &[char]) -> f64 {
    if left == right {
        return 1.0;
    }
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0; right.len() + 1];
    for (left_index, left_char) in left.iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_char) in right.iter().enumerate() {
            let substitution = usize::from(left_char != right_char);
            current[right_index + 1] = (current[right_index] + 1)
                .min(previous[right_index + 1] + 1)
                .min(previous[right_index] + substitution);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    1.0 - previous[right.len()] as f64 / left.len().max(right.len()) as f64
}

/// An upper bound of [`edit_similarity`]: the distance is at least the length difference.
fn similarity_bound(left: &[char], right: &[char]) -> f64 {
    if left == right {
        return 1.0;
    }
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    1.0 - left.len().abs_diff(right.len()) as f64 / left.len().max(right.len()) as f64
}

#[cfg(test)]
mod tests;
