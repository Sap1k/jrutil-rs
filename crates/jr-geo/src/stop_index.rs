//! The stop-name index: the replacement of the F# Lucene index (`docs/rules/jdf-read.md`,
//! verdict for R014): a token index with the matching semantics of JDF-R039–R042.
//!
//! An index holds named entries, numbered by their position in the list it was built from; the
//! caller keeps whatever the numbers stand for (a catalogue row, an OSM node, a municipality).
//! A query is pure in the name, so the caller can memoise it across batches.

use std::collections::HashMap;

use jr_model::Rule;
use rayon::prelude::*;

use crate::names::{Analyzer, NameMatch};

/// A candidate is retrieved when every token of the query (as written, not expanded) is one of
/// the words the candidate is found by: its tokens with their expansions (R040). A query
/// without tokens retrieves nothing. Candidates come in entry order, all of them (the F# cap of
/// 10,000 Lucene hits and Lucene's relevance order are gone: R014 orders candidates explicitly).
///
/// Every query token must be accounted for by the candidate; `praha dejvice` finds
/// `Praha-Dejvice`, never plain `Praha`.
pub const JDF_R041: Rule = Rule {
    id: "JDF-R041",
    summary: "a candidate contains every query token among its tokens and their synonyms",
};

pub const RULES: &[Rule] = &[JDF_R041];

/// A retrieved entry and how well its name matches the query (R042).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub entry: u32,
    pub quality: NameMatch,
}

/// The index. Built once, queried from any number of threads.
#[derive(Debug)]
pub struct StopIndex {
    analyzer: Analyzer,
    /// Every word the index knows, to its term number.
    terms: HashMap<Box<str>, u32>,
    /// Per term, the sorted term numbers of its expansion (R040).
    expansions: Vec<Box<[u32]>>,
    /// Per entry, its tokens as term numbers (`token_offsets` has one more element than there
    /// are entries).
    token_offsets: Vec<u32>,
    tokens: Vec<u32>,
    /// Per term, the sorted entries found by it.
    posting_offsets: Vec<u32>,
    postings: Vec<u32>,
}

impl StopIndex {
    /// Indexes the names; entry `i` is `names[i]`. The analyzer is the matching profile's
    /// (`data/names/road_names.toml` `[matching]`).
    pub fn new<S: AsRef<str> + Sync>(analyzer: Analyzer, names: &[S]) -> Self {
        let analysed: Vec<Vec<String>> = names
            .par_iter()
            .map(|name| analyzer.tokens(name.as_ref()))
            .collect();
        let mut builder = Builder::default();
        let mut token_offsets = Vec::with_capacity(names.len() + 1);
        let mut tokens = Vec::new();
        let mut entry_terms = Vec::new();
        token_offsets.push(0);
        for (entry, entry_tokens) in analysed.iter().enumerate() {
            let entry = u32::try_from(entry).expect("fewer than 2^32 entries");
            tokens.extend(entry_tokens.iter().map(|token| builder.intern(token)));
            token_offsets.push(u32::try_from(tokens.len()).expect("fewer than 2^32 tokens"));
            entry_terms.clear();
            entry_terms.extend(
                analyzer
                    .index_terms(entry_tokens)
                    .into_iter()
                    .map(|term| builder.intern(term)),
            );
            entry_terms.sort_unstable();
            entry_terms.dedup();
            for &term in &entry_terms {
                builder.postings[term as usize].push(entry);
            }
        }
        // Expansions of every term, interning the words they name (whose own expansions are
        // then computed in turn).
        let mut expansions = Vec::with_capacity(builder.words.len());
        let mut term = 0;
        while term < builder.words.len() {
            let word = builder.words[term].clone();
            let mut expansion: Vec<u32> = analyzer
                .expansion(&word)
                .into_iter()
                .map(|word| builder.intern(word))
                .collect();
            expansion.sort_unstable();
            expansion.dedup();
            expansions.push(expansion.into_boxed_slice());
            term += 1;
        }
        let mut posting_offsets = Vec::with_capacity(builder.postings.len() + 1);
        let mut postings = Vec::with_capacity(builder.postings.iter().map(Vec::len).sum());
        posting_offsets.push(0);
        for list in builder.postings {
            postings.extend(list);
            posting_offsets.push(u32::try_from(postings.len()).expect("fewer than 2^32 postings"));
        }
        StopIndex {
            analyzer,
            terms: builder.terms,
            expansions,
            token_offsets,
            tokens,
            posting_offsets,
            postings,
        }
    }

    pub fn len(&self) -> usize {
        self.token_offsets.len() - 1
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn analyzer(&self) -> &Analyzer {
        &self.analyzer
    }

    /// R041 and R042: every retrieved entry with its match quality, in entry order.
    pub fn candidates(&self, name: &str) -> Vec<Candidate> {
        let query = self.analyzer.tokens(name);
        let mut query_terms = Vec::with_capacity(query.len());
        for token in &query {
            match self.terms.get(token.as_str()) {
                Some(&term) => query_terms.push(term),
                // A word no entry is found by: nothing has all query tokens.
                None => return Vec::new(),
            }
        }
        let Some(entries) = self.intersection(&query_terms) else {
            return Vec::new();
        };
        let mut query_words: Vec<u32> = query_terms
            .iter()
            .flat_map(|&term| self.expansions[term as usize].iter().copied())
            .collect();
        query_words.sort_unstable();
        query_words.dedup();
        let mut related: HashMap<u32, bool> = HashMap::new();
        entries
            .into_iter()
            .map(|entry| {
                let tokens = self.entry_tokens(entry);
                let matched = tokens
                    .iter()
                    .filter(|&&token| {
                        *related.entry(token).or_insert_with(|| {
                            self.expansions[token as usize]
                                .iter()
                                .any(|word| query_words.binary_search(word).is_ok())
                        })
                    })
                    .count();
                Candidate {
                    entry,
                    quality: NameMatch {
                        matched: matched as u32,
                        tokens: tokens.len() as u32,
                    },
                }
            })
            .collect()
    }

    /// The entries that match the name exactly (R042 score 1), in entry order.
    pub fn exact(&self, name: &str) -> Vec<u32> {
        self.candidates(name)
            .into_iter()
            .filter(|candidate| candidate.quality.is_exact())
            .map(|candidate| candidate.entry)
            .collect()
    }

    fn entry_tokens(&self, entry: u32) -> &[u32] {
        let entry = entry as usize;
        &self.tokens[self.token_offsets[entry] as usize..self.token_offsets[entry + 1] as usize]
    }

    fn posting(&self, term: u32) -> &[u32] {
        let term = term as usize;
        &self.postings[self.posting_offsets[term] as usize..self.posting_offsets[term + 1] as usize]
    }

    /// The entries found by every term, sorted; `None` for no terms.
    fn intersection(&self, terms: &[u32]) -> Option<Vec<u32>> {
        let mut lists: Vec<&[u32]> = terms.iter().map(|&term| self.posting(term)).collect();
        lists.sort_by_key(|list| list.len());
        let (shortest, rest) = lists.split_first()?;
        let mut entries = shortest.to_vec();
        for list in rest {
            entries.retain(|entry| list.binary_search(entry).is_ok());
            if entries.is_empty() {
                break;
            }
        }
        Some(entries)
    }
}

#[derive(Default)]
struct Builder {
    terms: HashMap<Box<str>, u32>,
    words: Vec<Box<str>>,
    postings: Vec<Vec<u32>>,
}

impl Builder {
    fn intern(&mut self, word: &str) -> u32 {
        if let Some(&term) = self.terms.get(word) {
            return term;
        }
        let term = u32::try_from(self.words.len()).expect("fewer than 2^32 terms");
        self.terms.insert(word.into(), term);
        self.words.push(word.into());
        self.postings.push(Vec::new());
        term
    }
}

#[cfg(test)]
mod tests;
