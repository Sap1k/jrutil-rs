//! Counting differences exactly while keeping only a bounded, deterministic set of samples:
//! the smallest by (key, column, A value, B value) per category and ledger category.

use std::collections::BTreeMap;

use super::ledger::Ledger;

/// One recorded difference.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Sample {
    /// The row: primary-key values joined with `|`, or for a row without a comparable key
    /// its canonical content.
    pub key: String,
    /// The differing column of a `changed` difference.
    pub column: Option<String>,
    /// A's and B's values as text; `None` for null (and for a row only one side has).
    pub a: Option<String>,
    pub b: Option<String>,
}

/// The differences of one category assigned to one ledger category (or to none).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bucket {
    pub count: u64,
    /// Sorted, at most the profile's sample limit.
    pub samples: Vec<Sample>,
}

impl Bucket {
    fn keep(&mut self, sample: Sample, limit: usize) {
        if self.samples.len() >= limit
            && self.samples.last().is_none_or(|largest| sample >= *largest)
        {
            return;
        }
        let at = self.samples.partition_point(|kept| *kept < sample);
        self.samples.insert(at, sample);
        self.samples.truncate(limit);
    }

    fn merge(&mut self, other: Bucket, limit: usize) {
        self.count += other.count;
        for sample in other.samples {
            self.keep(sample, limit);
        }
    }
}

/// A relation's categories: their names and the ledger categories that may explain each.
#[derive(Debug)]
pub(crate) struct Categories {
    names: Vec<String>,
    candidates: Vec<Vec<usize>>,
}

/// Local category indexes of a relation.
pub(crate) const MISSING: usize = 0;
pub(crate) const EXTRA: usize = 1;

impl Categories {
    /// `missing`, `extra`, then `changed.<column>` per compared column.
    pub fn new<'c>(
        relation: &str,
        columns: impl IntoIterator<Item = &'c str>,
        ledger: &Ledger,
    ) -> Categories {
        let mut names = vec![format!("{relation}.missing"), format!("{relation}.extra")];
        names.extend(
            columns
                .into_iter()
                .map(|column| format!("{relation}.changed.{column}")),
        );
        let candidates = names.iter().map(|name| ledger.candidates(name)).collect();
        Categories { names, candidates }
    }

    /// The local index of `changed.<column>` for the `i`-th compared column.
    pub fn changed(i: usize) -> usize {
        2 + i
    }
}

/// Differences of (part of) one relation.
#[derive(Debug)]
pub(crate) struct Recorder<'l> {
    categories: &'l Categories,
    ledger: &'l Ledger,
    limit: usize,
    buckets: Vec<BTreeMap<Option<usize>, Bucket>>,
}

impl<'l> Recorder<'l> {
    pub fn new(categories: &'l Categories, ledger: &'l Ledger, limit: usize) -> Recorder<'l> {
        Recorder {
            categories,
            ledger,
            limit,
            buckets: vec![BTreeMap::new(); categories.names.len()],
        }
    }

    pub fn record(&mut self, category: usize, sample: Sample) {
        let assigned = self
            .ledger
            .assign(&self.categories.candidates[category], &sample);
        let bucket = self.buckets[category].entry(assigned).or_default();
        bucket.count += 1;
        bucket.keep(sample, self.limit);
    }

    pub fn merge(mut self, other: Recorder<'l>) -> Recorder<'l> {
        for (mine, theirs) in self.buckets.iter_mut().zip(other.buckets) {
            for (assigned, bucket) in theirs {
                mine.entry(assigned).or_default().merge(bucket, self.limit);
            }
        }
        self
    }

    /// Adds the counts and samples to `into`, ledger categories by name.
    pub fn finish(self, into: &mut BTreeMap<String, CategoryDiffs>) {
        for (name, buckets) in self.categories.names.iter().zip(self.buckets) {
            if buckets.is_empty() {
                continue;
            }
            let diffs = into.entry(name.clone()).or_default();
            for (assigned, bucket) in buckets {
                diffs.count += bucket.count;
                let ledger_name = assigned.map(|index| self.ledger.categories[index].name.clone());
                diffs
                    .groups
                    .entry(ledger_name)
                    .or_default()
                    .merge(bucket, self.limit);
            }
        }
    }
}

/// All differences of one default category.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CategoryDiffs {
    pub count: u64,
    /// By the ledger category that explains them; `None` for unexplained.
    pub groups: BTreeMap<Option<String>, Bucket>,
}
