//! Matching rows of A and B and recording how they differ: by key, or as a multiset of
//! canonical rows. Keys are 64-bit hashes confirmed by exact comparison.

use rayon::prelude::*;

use super::cells::Pair;
use super::tally::{Categories, EXTRA, MISSING, Recorder, Sample};
use crate::validate::keys;

/// Rows compared per parallel task.
const CHUNK: usize = 1 << 16;

/// The columns of one relation, paired.
pub(crate) struct Columns<'a> {
    pub names: Vec<&'a str>,
    pub pairs: Vec<Pair<'a>>,
}

impl Columns<'_> {
    /// Row hashes of one side, sorted with their row numbers.
    fn sorted_hashes(&self, side: usize, rows: &[u32]) -> Vec<(u64, u32)> {
        let hashers: Vec<_> = self.pairs.iter().map(|pair| pair.hasher(side)).collect();
        let mut hashes: Vec<(u64, u32)> = rows
            .par_iter()
            .map(|&row| {
                let hash = keys::combine(hashers.iter().map(|hasher| hasher.get(row as usize)));
                (hash, row)
            })
            .collect();
        hashes.par_sort_unstable();
        hashes
    }

    pub fn eq(&self, ra: u32, rb: u32) -> bool {
        self.pairs
            .iter()
            .all(|pair| pair.eq(ra as usize, rb as usize))
    }

    /// The row's values joined with `|`, references by content.
    fn text(&self, side: usize, row: u32) -> String {
        let parts: Vec<String> = self
            .pairs
            .iter()
            .map(|pair| pair.canonical_text(side, row as usize))
            .collect();
        parts.join("|")
    }
}

/// Rows only in A, only in B, and matched pairs.
#[derive(Default)]
pub(crate) struct Matching {
    pub missing: Vec<u32>,
    pub extra: Vec<u32>,
    pub pairs: Vec<(u32, u32)>,
}

/// Merges two sorted hash lists; rows with equal hashes pair up when `same` confirms them,
/// first come first served (so duplicates pair one to one).
pub(crate) fn match_rows(
    a: &[(u64, u32)],
    b: &[(u64, u32)],
    same: impl Fn(u32, u32) -> bool,
) -> Matching {
    let mut matching = Matching::default();
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        let next_a = a.get(i).map(|entry| entry.0);
        let next_b = b.get(j).map(|entry| entry.0);
        match (next_a, next_b) {
            (Some(x), Some(y)) if x == y => {
                let end_a = i + a[i..].partition_point(|entry| entry.0 == x);
                let end_b = j + b[j..].partition_point(|entry| entry.0 == x);
                let mut taken = vec![false; end_b - j];
                for &(_, ra) in &a[i..end_a] {
                    let found = (j..end_b).find(|&k| !taken[k - j] && same(ra, b[k].1));
                    match found {
                        Some(k) => {
                            taken[k - j] = true;
                            matching.pairs.push((ra, b[k].1));
                        }
                        None => matching.missing.push(ra),
                    }
                }
                for k in j..end_b {
                    if !taken[k - j] {
                        matching.extra.push(b[k].1);
                    }
                }
                (i, j) = (end_a, end_b);
            }
            (Some(x), Some(y)) if x < y => {
                matching.missing.push(a[i].1);
                i += 1;
            }
            (Some(_), None) => {
                matching.missing.push(a[i].1);
                i += 1;
            }
            _ => {
                matching.extra.push(b[j].1);
                j += 1;
            }
        }
    }
    matching
}

/// Records rows that only one side has, keyed by `text`.
fn record_unmatched<'l>(
    recorder: Recorder<'l>,
    rows: &[u32],
    category: usize,
    text: &(impl Fn(u32) -> String + Sync),
    fresh: &(impl Fn() -> Recorder<'l> + Sync),
) -> Recorder<'l> {
    let local = rows
        .par_chunks(CHUNK)
        .map(|chunk| {
            let mut recorder = fresh();
            for &row in chunk {
                let sample = Sample {
                    key: text(row),
                    column: None,
                    a: None,
                    b: None,
                };
                recorder.record(category, sample);
            }
            recorder
        })
        .reduce(fresh, Recorder::merge);
    recorder.merge(local)
}

/// A relation matched by key: rows only in A are `missing`, only in B `extra`, and each
/// differing value column of a matched pair is `changed.<column>`. An unmatched row's sample
/// key is `describe(side, row)` where that says something, else its key.
pub(crate) fn keyed<'l>(
    row_counts: [usize; 2],
    key: &Columns,
    values: &Columns,
    describe: &(dyn Fn(usize, u32) -> Option<String> + Sync),
    fresh: impl Fn() -> Recorder<'l> + Sync,
) -> Recorder<'l> {
    let all = |rows: usize| (0..rows as u32).collect::<Vec<u32>>();
    let (a, b) = rayon::join(
        || key.sorted_hashes(0, &all(row_counts[0])),
        || key.sorted_hashes(1, &all(row_counts[1])),
    );
    let matching = match_rows(&a, &b, |ra, rb| key.eq(ra, rb));
    drop((a, b));
    let mut recorder = fresh();
    recorder = record_unmatched(
        recorder,
        &matching.missing,
        MISSING,
        &|row| describe(0, row).unwrap_or_else(|| key.text(0, row)),
        &fresh,
    );
    recorder = record_unmatched(
        recorder,
        &matching.extra,
        EXTRA,
        &|row| describe(1, row).unwrap_or_else(|| key.text(1, row)),
        &fresh,
    );
    let changed = matching
        .pairs
        .par_chunks(CHUNK)
        .map(|chunk| {
            let mut recorder = fresh();
            for &(ra, rb) in chunk {
                let (ra, rb) = (ra as usize, rb as usize);
                for (i, pair) in values.pairs.iter().enumerate() {
                    if pair.eq(ra, rb) {
                        continue;
                    }
                    let sample = Sample {
                        key: key.text(0, ra as u32),
                        column: Some(values.names[i].to_owned()),
                        a: pair.text_a(ra),
                        b: pair.text_b(rb, Some(ra)),
                    };
                    recorder.record(Categories::changed(i), sample);
                }
            }
            recorder
        })
        .reduce(&fresh, Recorder::merge);
    recorder.merge(changed)
}

/// A relation without a comparable key: the given rows of each side as a multiset of their
/// canonical values. Unpaired rows are `missing` or `extra`, keyed by their content.
pub(crate) fn multiset<'l>(
    rows: [Vec<u32>; 2],
    values: &Columns,
    fresh: impl Fn() -> Recorder<'l> + Sync,
) -> Recorder<'l> {
    let (a, b) = rayon::join(
        || values.sorted_hashes(0, &rows[0]),
        || values.sorted_hashes(1, &rows[1]),
    );
    let matching = match_rows(&a, &b, |ra, rb| values.eq(ra, rb));
    let mut recorder = fresh();
    recorder = record_unmatched(
        recorder,
        &matching.missing,
        MISSING,
        &|row| values.text(0, row),
        &fresh,
    );
    record_unmatched(
        recorder,
        &matching.extra,
        EXTRA,
        &|row| values.text(1, row),
        &fresh,
    )
}
