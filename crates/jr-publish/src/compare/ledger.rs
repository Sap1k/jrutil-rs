//! The parity ledger (`parity/<package>.toml`): named, reasoned categories of expected
//! differences, and the verdict of a comparison against them.

use regex::Regex;
use serde::Deserialize;

use super::Comparison;
use super::tally::Sample;
use crate::contract::contract;

#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    #[error("{0}")]
    Toml(#[from] toml::de::Error),
    #[error("category {name:?}: {problem}")]
    Category { name: String, problem: String },
}

/// Ledger categories in file order, and the columns the ledger says not to compare.
#[derive(Debug, Clone, Default)]
pub struct Ledger {
    pub categories: Vec<LedgerCategory>,
    pub labels: Vec<Label>,
}

/// A provenance label (`[[label]]`): a text column naming where a row came from, which the two
/// implementations word differently. It is left out of the comparison, with a reason.
#[derive(Debug, Clone)]
pub struct Label {
    pub relation: String,
    pub column: String,
    pub reason: String,
}

/// An explained difference.
#[derive(Debug, Clone)]
pub struct LedgerCategory {
    pub name: String,
    /// The default category (`<relation>.missing`, `.extra`, `.changed.<column>`) it explains.
    pub matches: String,
    pub reason: String,
    pub min: u64,
    pub max: Option<u64>,
    /// Filters on the row key and on A's and B's values (null as the empty string).
    pub key: Option<Regex>,
    pub a: Option<Regex>,
    pub b: Option<Regex>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLedger {
    #[serde(default)]
    category: Vec<RawCategory>,
    #[serde(default)]
    label: Vec<RawLabel>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLabel {
    relation: String,
    column: String,
    reason: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCategory {
    name: String,
    matches: String,
    reason: String,
    min: Option<u64>,
    max: Option<u64>,
    key: Option<String>,
    a: Option<String>,
    b: Option<String>,
}

impl Ledger {
    pub fn parse(text: &str) -> Result<Ledger, LedgerError> {
        let raw: RawLedger = toml::from_str(text)?;
        let mut categories: Vec<LedgerCategory> = Vec::with_capacity(raw.category.len());
        for raw in raw.category {
            let fail = |problem: String| LedgerError::Category {
                name: raw.name.clone(),
                problem,
            };
            if raw.name.trim().is_empty() {
                return Err(fail("empty name".to_owned()));
            }
            if categories.iter().any(|known| known.name == raw.name) {
                return Err(fail("name used twice".to_owned()));
            }
            if raw.reason.trim().is_empty() {
                return Err(fail("empty reason".to_owned()));
            }
            check_matches(&raw.matches).map_err(&fail)?;
            let min = raw.min.unwrap_or(0);
            if raw.max.is_some_and(|max| max < min) {
                return Err(fail(format!("max below min {min}")));
            }
            let regex = |text: &Option<String>, field: &str| {
                text.as_deref()
                    .map(Regex::new)
                    .transpose()
                    .map_err(|error| fail(format!("{field}: {error}")))
            };
            categories.push(LedgerCategory {
                key: regex(&raw.key, "key")?,
                a: regex(&raw.a, "a")?,
                b: regex(&raw.b, "b")?,
                name: raw.name,
                matches: raw.matches,
                reason: raw.reason,
                min,
                max: raw.max,
            });
        }
        let mut labels = Vec::with_capacity(raw.label.len());
        for raw in raw.label {
            let name = format!("{}.{}", raw.relation, raw.column);
            let fail = |problem: &str| LedgerError::Category {
                name: name.clone(),
                problem: problem.to_owned(),
            };
            let spec = contract()
                .relation(&raw.relation)
                .ok_or_else(|| fail("label of an unknown relation"))?;
            if spec.field(&raw.column).is_none() {
                return Err(fail("label of an unknown column"));
            }
            let is_key = spec.primary_key.contains(&raw.column)
                || spec
                    .foreign_keys
                    .iter()
                    .any(|key| key.fields.contains(&raw.column));
            if is_key {
                return Err(fail("a key column is not a label"));
            }
            if raw.reason.trim().is_empty() {
                return Err(fail("empty reason"));
            }
            labels.push(Label {
                relation: raw.relation,
                column: raw.column,
                reason: raw.reason,
            });
        }
        Ok(Ledger { categories, labels })
    }

    /// Indexes of the ledger categories that may explain `category`, in file order.
    pub(crate) fn candidates(&self, category: &str) -> Vec<usize> {
        (0..self.categories.len())
            .filter(|&index| self.categories[index].matches == category)
            .collect()
    }

    /// The first candidate whose filters all match.
    pub(crate) fn assign(&self, candidates: &[usize], sample: &Sample) -> Option<usize> {
        let test = |regex: &Option<Regex>, text: Option<&str>| {
            regex
                .as_ref()
                .is_none_or(|regex| regex.is_match(text.unwrap_or("")))
        };
        candidates.iter().copied().find(|&index| {
            let category = &self.categories[index];
            test(&category.key, Some(&sample.key))
                && test(&category.a, sample.a.as_deref())
                && test(&category.b, sample.b.as_deref())
        })
    }

    /// Judges a comparison classified with this ledger (`compare_classified`). Differences
    /// assigned to a category this ledger does not have count as unexplained.
    pub fn judge(&self, comparison: &Comparison) -> Verdict {
        let mut categories = Vec::with_capacity(self.categories.len());
        for category in &self.categories {
            let count = comparison
                .categories
                .get(&category.matches)
                .and_then(|diffs| diffs.groups.get(&Some(category.name.clone())))
                .map_or(0, |bucket| bucket.count);
            let in_range = count >= category.min && category.max.is_none_or(|max| count <= max);
            let status = match (in_range, count) {
                (false, _) => Status::OutOfRange,
                (true, 0) => Status::Stale,
                (true, _) => Status::Explained,
            };
            categories.push(CategoryVerdict {
                name: category.name.clone(),
                matches: category.matches.clone(),
                reason: category.reason.clone(),
                count,
                min: category.min,
                max: category.max,
                status,
            });
        }
        let defaults = comparison
            .categories
            .iter()
            .map(|(name, diffs)| {
                let mut unexplained = 0;
                let mut out_of_range = false;
                for (assigned, bucket) in &diffs.groups {
                    match categories
                        .iter()
                        .find(|c| Some(&c.name) == assigned.as_ref() && c.matches == *name)
                    {
                        Some(verdict) => out_of_range |= verdict.status == Status::OutOfRange,
                        None => unexplained += bucket.count,
                    }
                }
                let status = if unexplained > 0 {
                    Status::Unexplained
                } else if out_of_range {
                    Status::OutOfRange
                } else {
                    Status::Explained
                };
                DefaultVerdict {
                    category: name.clone(),
                    count: diffs.count,
                    unexplained,
                    status,
                }
            })
            .collect();
        Verdict {
            defaults,
            categories,
        }
    }
}

/// `<relation>.missing`, `<relation>.extra` or `<relation>.changed.<column>` of the contract.
fn check_matches(matches: &str) -> Result<(), String> {
    let contract = contract();
    let unknown = || format!("matches {matches:?} is not a category of the serving contract");
    let (relation, rest) = matches.split_once('.').ok_or_else(unknown)?;
    let spec = contract.relation(relation).ok_or_else(unknown)?;
    match rest.split_once('.') {
        None if rest == "missing" || rest == "extra" => Ok(()),
        Some(("changed", column)) if spec.field(column).is_some() => Ok(()),
        _ => Err(unknown()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Explained,
    Unexplained,
    OutOfRange,
    /// A ledger category that matched nothing: a warning.
    Stale,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Explained => "explained",
            Status::Unexplained => "UNEXPLAINED",
            Status::OutOfRange => "OUT OF RANGE",
            Status::Stale => "stale",
        }
    }

    pub fn fails(self) -> bool {
        matches!(self, Status::Unexplained | Status::OutOfRange)
    }
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// Per default category, sorted.
    pub defaults: Vec<DefaultVerdict>,
    /// Per ledger category, in file order.
    pub categories: Vec<CategoryVerdict>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultVerdict {
    pub category: String,
    pub count: u64,
    pub unexplained: u64,
    pub status: Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryVerdict {
    pub name: String,
    pub matches: String,
    pub reason: String,
    pub count: u64,
    pub min: u64,
    pub max: Option<u64>,
    pub status: Status,
}

impl Verdict {
    /// Everything explained and every ledger count in range.
    pub fn passed(&self) -> bool {
        !self.defaults.iter().any(|d| d.status.fails())
            && !self.categories.iter().any(|c| c.status.fails())
    }
}
