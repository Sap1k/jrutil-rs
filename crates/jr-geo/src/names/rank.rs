//! Stop-name equivalence for the regional overlay (`docs/rules/overlay.md`, OVL-R032–R035): the
//! overlay compares a regional feed's stop names with the national ones, which spell the same
//! place with other abbreviations, separators and leading locality segments.
//!
//! The tokens come from a [`Profile`] of the one name machinery (`data/names/overlay_names.toml`
//! `[rank.tokens]`: lower case, letters and numbers, a `.` ending the word it closes); the
//! expansion vocabulary is the data file's own short list plus the shared synonym table. What the
//! rules decide (which names say the same place, how close, which headsign a terminal name
//! resolves) is here; which rule fires when is the overlay's.

use std::collections::HashMap;

use serde::Deserialize;

use super::profile::Profile;

/// The tables of the overlay's name rules (`data/names/overlay_names.toml` `[rank]`).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RankTables {
    /// The token profile (OVL-R033 step 2). Its terminator marks a word as dotted.
    pub tokens: Profile,
    /// A segment containing this text (case-insensitively) loses a trailing junction distance
    /// (`rozc.4.0`, OVL-R033 step 1).
    pub junction_marker: Box<str>,
    /// Two undotted words of at least this many characters are the same word when one begins
    /// the other (OVL-R033 step 3).
    pub minimum_prefix: usize,
    /// Abbreviations a prefix cannot find, to their spelled-out forms (OVL-R034), before the
    /// shared synonym table.
    pub expansions: Vec<(Box<str>, Box<str>)>,
}

/// The tables of headsign resolution (`data/names/overlay_names.toml` `[headsigns]`, OVL-R035).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeadsignTables {
    /// Lower-case words that make a headsign a route description, not a destination.
    pub composite_words: Vec<Box<str>>,
    /// Characters that do the same.
    pub composite_characters: Vec<char>,
    /// Display abbreviations of a terminal name to a full word (one row per full word).
    pub terminal_abbreviations: Vec<(Box<str>, Box<str>)>,
}

/// A word of a stop name: `dotted` when written as an abbreviation (`žel.`), which then stands
/// for any word it begins.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Word {
    text: String,
    dotted: bool,
}

/// OVL-R033 and R034, compiled: the token profile and the expansion dictionary.
#[derive(Clone, Debug)]
pub struct StopNameRanker {
    tables: RankTables,
    terminator: Option<char>,
    /// Abbreviation to the word lists it expands to.
    expansions: HashMap<String, Vec<Vec<String>>>,
}

/// Why a headsign resolved as it did (OVL-R035), as the review reports name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HeadsignReason {
    ViaOrComposite,
    ExactFullName,
    LocalityPrefix,
    TerminalName,
    KnownAbbreviation,
    NoDestinationMatch,
    AmbiguousDestination,
}

impl HeadsignReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ViaOrComposite => "via_or_composite",
            Self::ExactFullName => "exact_full_name",
            Self::LocalityPrefix => "locality_prefix",
            Self::TerminalName => "terminal_name",
            Self::KnownAbbreviation => "known_abbreviation",
            Self::NoDestinationMatch => "no_destination_match",
            Self::AmbiguousDestination => "ambiguous_destination",
        }
    }
}

impl StopNameRanker {
    /// Compiles the tables and the shared synonym lines (each a list of alternatives as written,
    /// in file order). From a line of more than one alternative, the first alternative and every
    /// one ending in `.` abbreviate every alternative not ending in `.` (OVL-R034).
    pub fn new<L, A>(tables: RankTables, synonym_lines: L) -> Self
    where
        L: IntoIterator<Item = A>,
        A: IntoIterator,
        A::Item: AsRef<str>,
    {
        let terminator = tables.tokens.terminators.first().copied();
        let mut ranker = StopNameRanker {
            tables,
            terminator,
            expansions: HashMap::new(),
        };
        let builtin = ranker.tables.expansions.clone();
        for (abbreviation, expansion) in &builtin {
            ranker.add_expansion(abbreviation, expansion);
        }
        for line in synonym_lines {
            let members: Vec<String> = line
                .into_iter()
                .map(|member| ranker.lower(member.as_ref().trim()))
                .filter(|member| !member.is_empty())
                .collect();
            if members.len() < 2 {
                continue;
            }
            let full: Vec<&String> = members.iter().filter(|m| !m.ends_with('.')).collect();
            for (index, abbreviation) in members.iter().enumerate() {
                if index != 0 && !abbreviation.ends_with('.') {
                    continue;
                }
                for expansion in &full {
                    if *expansion != abbreviation {
                        ranker.add_expansion(abbreviation, expansion);
                    }
                }
            }
        }
        ranker
    }

    fn lower(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        self.tables.tokens.characters.map_into(text, &mut out);
        out
    }

    fn add_expansion(&mut self, abbreviation: &str, expansion: &str) {
        let key = abbreviation.trim().trim_end_matches('.').to_owned();
        let words: Vec<String> = self.words(expansion).into_iter().map(|w| w.text).collect();
        if key.is_empty()
            || key.contains(' ')
            || words.is_empty()
            || (words.len() == 1 && words[0] == key)
        {
            return;
        }
        self.expansions.entry(key).or_default().push(words);
    }

    /// OVL-R033 step 2: the words of a text, each marked dotted when a `.` closed it.
    fn words(&self, text: &str) -> Vec<Word> {
        self.tables
            .tokens
            .raw_tokens(text)
            .into_iter()
            .filter_map(|token| {
                let dotted = self.terminator.is_some_and(|dot| token.ends_with(dot));
                let text = match (dotted, self.terminator) {
                    (true, Some(dot)) => token[..token.len() - dot.len_utf8()].to_owned(),
                    _ => token,
                };
                (!text.is_empty()).then_some(Word { text, dotted })
            })
            .collect()
    }

    /// OVL-R033 step 1: the comma-separated segments of a name, trimmed, a junction distance
    /// removed, as words; segments without words dropped.
    fn segments(&self, name: &str) -> Vec<Vec<Word>> {
        name.split(',')
            .map(|segment| {
                let segment = segment.trim();
                let marker = self.lower(&self.tables.junction_marker);
                if self.lower(segment).contains(&marker) {
                    self.words(without_junction_distance(segment))
                } else {
                    self.words(segment)
                }
            })
            .filter(|words| !words.is_empty())
            .collect()
    }

    fn same_word(&self, left: &Word, right: &Word) -> bool {
        left.text == right.text
            || (left.dotted && right.text.starts_with(&left.text))
            || (right.dotted && left.text.starts_with(&right.text))
            || (left.text.chars().count().min(right.text.chars().count())
                >= self.tables.minimum_prefix
                && (left.text.starts_with(&right.text) || right.text.starts_with(&left.text)))
    }

    /// Word lengths of `short[index]`'s expansions that `long` spells out from `position`.
    fn expands_to(
        &self,
        short: &[Word],
        index: usize,
        long: &[Word],
        position: usize,
    ) -> Vec<usize> {
        let Some(lists) = self.expansions.get(&short[index].text) else {
            return Vec::new();
        };
        lists
            .iter()
            .filter(|words| {
                position + words.len() <= long.len()
                    && words
                        .iter()
                        .enumerate()
                        .all(|(offset, word)| long[position + offset].text == *word)
            })
            .map(Vec::len)
            .collect()
    }

    /// OVL-R033 step 4: how many words differ in spelling when the two word lists say the same.
    fn difference(&self, left: &[Word], right: &[Word]) -> Option<u32> {
        let columns = right.len() + 1;
        let mut known: Vec<Option<Option<u32>>> = vec![None; (left.len() + 1) * columns];
        self.difference_from(left, right, 0, 0, &mut known, columns)
    }

    fn difference_from(
        &self,
        left: &[Word],
        right: &[Word],
        i: usize,
        j: usize,
        known: &mut Vec<Option<Option<u32>>>,
        columns: usize,
    ) -> Option<u32> {
        if i == left.len() && j == right.len() {
            return Some(0);
        }
        if i == left.len() || j == right.len() {
            return None;
        }
        if let Some(value) = known[i * columns + j] {
            return value;
        }
        let mut best: Option<u32> = None;
        let mut consider = |value: Option<u32>| {
            if let Some(value) = value {
                best = Some(best.map_or(value, |b| b.min(value)));
            }
        };
        if self.same_word(&left[i], &right[j]) {
            let rest = self.difference_from(left, right, i + 1, j + 1, known, columns);
            consider(rest.map(|rest| rest + u32::from(left[i].text != right[j].text)));
        }
        for length in self.expands_to(left, i, right, j) {
            let rest = self.difference_from(left, right, i + 1, j + length, known, columns);
            consider(rest.map(|rest| rest + 1));
        }
        for length in self.expands_to(right, j, left, i) {
            let rest = self.difference_from(left, right, i + length, j + 1, known, columns);
            consider(rest.map(|rest| rest + 1));
        }
        known[i * columns + j] = Some(best);
        best
    }

    /// OVL-R033: `Some(rank)` (lower is closer) when the names say the same place, else `None`.
    /// The whole names compare first; failing that, the longer name may lead with locality
    /// segments the other leaves out, each left-out word adding one.
    pub fn rank(&self, left: &str, right: &str) -> Option<u32> {
        let left = self.segments(left);
        let right = self.segments(right);
        let whole = |segments: &[Vec<Word>]| -> Vec<Word> { segments.concat() };
        if let Some(difference) = self.difference(&whole(&left), &whole(&right)) {
            return Some(difference);
        }
        let mut best: Option<u32> = None;
        for (longer, shorter) in [(&left, &right), (&right, &left)] {
            let shorter = whole(shorter);
            for dropped in 1..longer.len() {
                let kept = whole(&longer[dropped..]);
                if let Some(difference) = self.difference(&kept, &shorter) {
                    let words: usize = longer[..dropped].iter().map(Vec::len).sum();
                    let rank = difference + words as u32;
                    best = Some(best.map_or(rank, |b| b.min(rank)));
                }
            }
        }
        best
    }

    /// OVL-R035: the one full name among `destinations` that a display headsign abbreviates, and
    /// why; `None` when it says something else or names none or several of them.
    pub fn resolve_headsign(
        &self,
        tables: &HeadsignTables,
        raw: &str,
        destinations: &[&str],
    ) -> (Option<String>, HeadsignReason) {
        let tokens = |text: &str| -> Vec<String> {
            let lowered = self.lower(text);
            lowered
                .split(|c: char| !c.is_alphanumeric())
                .filter(|token| !token.is_empty())
                .map(str::to_owned)
                .collect()
        };
        let forms = |text: &str| -> Vec<Vec<String>> {
            let mut forms = vec![tokens(text)];
            if let Some(comma) = text.find(',') {
                forms.push(tokens(&text[comma + 1..]));
            }
            forms.retain(|form| !form.is_empty());
            forms.dedup();
            forms
        };
        let raw_tokens = tokens(raw);
        if raw
            .chars()
            .any(|c| tables.composite_characters.contains(&c))
            || raw_tokens
                .iter()
                .any(|token| tables.composite_words.iter().any(|word| **word == **token))
        {
            return (None, HeadsignReason::ViaOrComposite);
        }
        let abbreviated = forms(raw);
        let token_matches = |short: &str, long: &str| {
            short == long
                || tables
                    .terminal_abbreviations
                    .iter()
                    .any(|(abbreviation, full)| **abbreviation == *short && **full == *long)
        };
        let matches = |name: &str| {
            forms(name).iter().any(|full| {
                abbreviated.iter().any(|short| {
                    full.len() == short.len()
                        && short.iter().zip(full).all(|(s, l)| token_matches(s, l))
                })
            })
        };
        let mut unique: Vec<&str> = Vec::new();
        for destination in destinations {
            if !unique.contains(destination) {
                unique.push(destination);
            }
        }
        let exact: Vec<&str> = unique
            .iter()
            .copied()
            .filter(|name| tokens(name) == raw_tokens)
            .collect();
        if exact.len() == 1 {
            return (Some(exact[0].to_owned()), HeadsignReason::ExactFullName);
        }
        let locality: Vec<&str> = unique
            .iter()
            .copied()
            .filter(|name| self.rank(raw, name).is_some())
            .collect();
        if locality.len() == 1 {
            return (Some(locality[0].to_owned()), HeadsignReason::LocalityPrefix);
        }
        if let Some(last) = destinations.last()
            && matches(last)
        {
            return (Some((*last).to_owned()), HeadsignReason::TerminalName);
        }
        let known: Vec<&str> = unique
            .iter()
            .copied()
            .filter(|name| matches(name))
            .collect();
        match known.as_slice() {
            [name] => (Some((*name).to_owned()), HeadsignReason::KnownAbbreviation),
            [] => (None, HeadsignReason::NoDestinationMatch),
            _ => (None, HeadsignReason::AmbiguousDestination),
        }
    }
}

/// The segment without a trailing junction distance (`\s*\d+[.,]\d+\s*$`): `rozc.4.0` is
/// `rozc.`.
fn without_junction_distance(segment: &str) -> &str {
    fn digits(text: &str) -> Option<&str> {
        let kept = text.trim_end_matches(|c: char| c.is_ascii_digit());
        (kept.len() < text.len()).then_some(kept)
    }
    let text = segment.trim_end();
    let Some(rest) = digits(text) else {
        return segment;
    };
    let Some(rest) = rest.strip_suffix(['.', ',']) else {
        return segment;
    };
    match digits(rest) {
        Some(rest) => rest.trim_end(),
        None => segment,
    }
}
