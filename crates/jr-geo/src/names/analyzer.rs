//! A [`Profile`] with the synonym table: the road name normalisations (JDF-R039–R042,
//! JDF-R214–R215).

use super::profile::{Profile, SynonymUse};
use super::synonyms::{SynonymConflict, Synonyms};

/// A name normalisation and its synonym table.
#[derive(Clone, Debug, PartialEq)]
pub struct Analyzer {
    profile: Profile,
    synonyms: Synonyms,
}

/// How well a candidate name matches a query (JDF-R042): of the candidate's `tokens`, `matched`
/// have an expansion that shares a word with the expansion of some query token.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NameMatch {
    pub matched: u32,
    pub tokens: u32,
}

impl NameMatch {
    /// The F# score, `matched / tokens` in single precision (NaN for a candidate without
    /// tokens, which retrieval never returns).
    pub fn score(self) -> f32 {
        self.matched as f32 / self.tokens as f32
    }

    /// JDF-R042: score 1, every token of the candidate is accounted for by the query.
    pub fn is_exact(self) -> bool {
        self.tokens > 0 && self.matched == self.tokens
    }
}

impl Analyzer {
    /// Compiles the synonym lines (each a list of alternatives as written, for example the
    /// `alternatives` of the `jr-formats` synonym reader's lines, in file order) for the profile.
    pub fn new<L, A>(profile: Profile, synonym_lines: L) -> Self
    where
        L: IntoIterator<Item = A>,
        A: IntoIterator,
        A::Item: AsRef<str>,
    {
        let synonyms = match profile.synonyms {
            SynonymUse::None => Synonyms::default(),
            SynonymUse::Expand | SynonymUse::Canonical => {
                Synonyms::new(synonym_lines, &profile.characters)
            }
        };
        Analyzer { profile, synonyms }
    }

    /// Alternatives named by more than one line of the table, in line order.
    pub fn synonym_conflicts(&self) -> &[SynonymConflict] {
        &self.synonyms.conflicts
    }

    /// The name's tokens: JDF-R039 for the matching profile (raw tokens, synonyms are applied
    /// when comparing), JDF-R214 for the identity profile (each token replaced by its canonical
    /// form, a canonical form of several words giving several tokens; other tokens lose their
    /// trailing `trim_unmapped` characters; empty tokens dropped).
    pub fn tokens(&self, name: &str) -> Vec<String> {
        let raw = self.profile.raw_tokens(name);
        if self.profile.synonyms != SynonymUse::Canonical {
            return raw;
        }
        let mut tokens = Vec::with_capacity(raw.len());
        for token in raw {
            match self.synonyms.canonical(&token) {
                Some(words) => tokens.extend(words.iter().map(|word| word.to_string())),
                None => {
                    let trimmed = token.trim_end_matches(&*self.profile.trim_unmapped);
                    if !trimmed.is_empty() {
                        tokens.push(trimmed.to_owned());
                    }
                }
            }
        }
        tokens
    }

    /// The tokens joined by one space.
    pub fn normalize(&self, name: &str) -> String {
        self.tokens(name).join(" ")
    }

    /// JDF-R040: the words a single token stands for, sorted: the token itself and, with
    /// `synonyms = "expand"`, every word of every line naming it. The relation is not transitive
    /// (`l.` expands to `labem` and `lázně`, `labem` only to `l.` and itself).
    pub fn expansion<'a>(&'a self, token: &'a str) -> Vec<&'a str> {
        if self.profile.synonyms == SynonymUse::Expand
            && let Some(words) = self.synonyms.expansion(token)
        {
            return words.iter().map(|word| &**word).collect();
        }
        vec![token]
    }

    /// JDF-R040–R041: the words a candidate name is found by. Each token adds its expansion,
    /// except that a sequence of tokens forming a multi-word alternative (`obecní úřad`) adds
    /// the expansion of that phrase instead, the longest phrase starting at a token winning
    /// (Lucene's synonym filter at index time). Unsorted, possibly repeated.
    pub fn index_terms<'a, S: AsRef<str>>(&'a self, tokens: &'a [S]) -> Vec<&'a str> {
        let mut terms = Vec::with_capacity(tokens.len() * 2);
        let mut at = 0;
        while at < tokens.len() {
            if self.profile.synonyms == SynonymUse::Expand
                && let Some((length, words)) = self.synonyms.phrase_at(&tokens[at..])
            {
                terms.extend(tokens[at..at + length].iter().map(AsRef::as_ref));
                terms.extend(words.iter().map(|word| &**word));
                at += length;
                continue;
            }
            terms.extend(self.expansion(tokens[at].as_ref()));
            at += 1;
        }
        terms
    }

    /// JDF-R042: how many tokens of `candidate` (repeats counted) have an expansion sharing a
    /// word with the union of the expansions of the `query` tokens. Order is ignored.
    pub fn name_match<Q: AsRef<str>, C: AsRef<str>>(
        &self,
        query: &[Q],
        candidate: &[C],
    ) -> NameMatch {
        let mut query_words: Vec<&str> = query
            .iter()
            .flat_map(|token| self.expansion(token.as_ref()))
            .collect();
        query_words.sort_unstable();
        query_words.dedup();
        let matched = candidate
            .iter()
            .filter(|token| {
                self.expansion(token.as_ref())
                    .iter()
                    .any(|word| query_words.binary_search(word).is_ok())
            })
            .count();
        NameMatch {
            matched: u32::try_from(matched).unwrap_or(u32::MAX),
            tokens: u32::try_from(candidate.len()).unwrap_or(u32::MAX),
        }
    }

    /// JDF-R042, the sequence predicate (`checkExactMatch`, used by the MHD rules): equal
    /// length and, position by position, equal expansions.
    pub fn same_sequence<A: AsRef<str>, B: AsRef<str>>(&self, left: &[A], right: &[B]) -> bool {
        left.len() == right.len()
            && left.iter().zip(right).all(|(left, right)| {
                self.expansion(left.as_ref()) == self.expansion(right.as_ref())
            })
    }
}
