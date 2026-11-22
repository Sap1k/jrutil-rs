//! The stop-name synonym table (`data/names/stop_synonyms.txt`), compiled for both of its uses:
//! expansion (JDF-R040) and canonical replacement (JDF-R215).

use std::collections::{BTreeSet, HashMap};

use super::profile::CharacterMap;

/// An alternative that two lines of the synonym table name. Verdict Q6 of `jdf-merge.md` keeps
/// such lines (changing them moves stop identities); the compiler reports them when it loads the
/// table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SynonymConflict {
    /// The alternative, as the profile maps it (lower-cased).
    pub alternative: Box<str>,
    /// The 0-based positions, in the lines handed to [`super::Analyzer::new`], of the first line
    /// naming it and of a later one.
    pub first_line: usize,
    pub line: usize,
}

/// A sequence of words.
type Words = Box<[Box<str>]>;

/// The compiled table.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Synonyms {
    /// A one-token alternative to its expansion: the sorted words of every alternative of every
    /// line naming it (itself included).
    expansions: HashMap<Box<str>, Words>,
    /// A multi-token alternative (`obecní úřad`) to its expansion; matched greedily, longest
    /// first, in a token sequence (Lucene's synonym filter at index time).
    phrases: HashMap<Words, Words>,
    longest_phrase: usize,
    /// A one-token alternative to the words of its line's canonical form, the last line naming
    /// it winning.
    canonical: HashMap<Box<str>, Words>,
    pub(super) conflicts: Vec<SynonymConflict>,
}

impl Synonyms {
    /// Compiles the lines (each a list of alternatives as written). Each alternative is mapped by
    /// `characters`, trimmed and split at white space into its words; blank alternatives and
    /// lines are skipped.
    pub(super) fn new<L, A>(lines: L, characters: &CharacterMap) -> Self
    where
        L: IntoIterator<Item = A>,
        A: IntoIterator,
        A::Item: AsRef<str>,
    {
        let mut expansions: HashMap<Box<str>, BTreeSet<Box<str>>> = HashMap::new();
        let mut phrases: HashMap<Words, BTreeSet<Box<str>>> = HashMap::new();
        let mut canonical = HashMap::new();
        let mut first_line: HashMap<Box<str>, usize> = HashMap::new();
        let mut conflicts = Vec::new();
        for (index, line) in lines.into_iter().enumerate() {
            let alternatives: Vec<(String, Box<[Box<str>]>)> = line
                .into_iter()
                .filter_map(|alternative| {
                    let mut mapped = String::new();
                    characters.map_into(alternative.as_ref().trim(), &mut mapped);
                    let mapped = mapped.trim().to_owned();
                    let words: Box<[Box<str>]> = mapped.split_whitespace().map(Box::from).collect();
                    (!words.is_empty()).then_some((mapped, words))
                })
                .collect();
            let Some((_, canonical_words)) = alternatives.iter().min_by(|left, right| {
                // JDF-R215: the longest alternative, ties broken by the smallest (ordinal).
                // Lengths count characters (the F# counted UTF-16 units: equal for Czech text).
                let length = |text: &str| text.chars().count();
                length(&right.0)
                    .cmp(&length(&left.0))
                    .then_with(|| left.0.cmp(&right.0))
            }) else {
                continue;
            };
            let canonical_words = canonical_words.clone();
            let line_words: BTreeSet<Box<str>> = alternatives
                .iter()
                .flat_map(|(_, words)| words.iter().cloned())
                .collect();
            for (mapped, words) in &alternatives {
                match &**words {
                    [single] => {
                        expansions
                            .entry(single.clone())
                            .or_default()
                            .extend(line_words.iter().cloned());
                        canonical.insert(single.clone(), canonical_words.clone());
                    }
                    _ => phrases
                        .entry(words.clone())
                        .or_default()
                        .extend(line_words.iter().cloned()),
                }
                let first = *first_line.entry(mapped.as_str().into()).or_insert(index);
                if first != index
                    && !conflicts.iter().any(|conflict: &SynonymConflict| {
                        conflict.line == index && *conflict.alternative == **mapped
                    })
                {
                    conflicts.push(SynonymConflict {
                        alternative: mapped.as_str().into(),
                        first_line: first,
                        line: index,
                    });
                }
            }
        }
        let collect = |set: BTreeSet<Box<str>>| set.into_iter().collect::<Box<[Box<str>]>>();
        Synonyms {
            longest_phrase: phrases.keys().map(|phrase| phrase.len()).max().unwrap_or(0),
            expansions: expansions
                .into_iter()
                .map(|(token, words)| (token, collect(words)))
                .collect(),
            phrases: phrases
                .into_iter()
                .map(|(phrase, words)| (phrase, collect(words)))
                .collect(),
            canonical,
            conflicts,
        }
    }

    /// The expansion of one token, `None` when no line names it as a one-token alternative.
    pub(super) fn expansion(&self, token: &str) -> Option<&[Box<str>]> {
        self.expansions.get(token).map(|words| &**words)
    }

    /// The canonical words of one token, `None` when no line names it.
    pub(super) fn canonical(&self, token: &str) -> Option<&[Box<str>]> {
        self.canonical.get(token).map(|words| &**words)
    }

    /// The longest phrase starting at `tokens[0]` with at least two tokens: its length and
    /// expansion.
    pub(super) fn phrase_at<S: AsRef<str>>(&self, tokens: &[S]) -> Option<(usize, &[Box<str>])> {
        let longest = self.longest_phrase.min(tokens.len());
        (2..=longest).rev().find_map(|length| {
            let key: Box<[Box<str>]> = tokens[..length]
                .iter()
                .map(|token| Box::from(token.as_ref()))
                .collect();
            self.phrases.get(&key).map(|words| (length, &**words))
        })
    }
}
