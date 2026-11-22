//! The canonical content of generated identifiers on both sides: a service's dates, a shape's
//! points, a note's columns. A reference compares by this content, never by identifier.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use rayon::prelude::*;

mod load;

use super::cells::Pair;
use super::profile::{Content, GeneratedId, Profile};
use super::type_mismatch;
use crate::contract::Contract;
use crate::dates;
use crate::error::PackageError;
use crate::package::Package;
use crate::table::{Column, Table};
use crate::validate::keys::{self, ValueHashes};

/// Dates and points listed in a value's text before it is abbreviated.
const LISTED: usize = 5;

/// One generated identifier's content in A (side 0) and B (side 1).
#[derive(Debug)]
pub(crate) struct Target {
    pub generated: GeneratedId,
    pub sides: [TargetSide; 2],
    /// Content index -> whether anything references it, per side.
    referenced: [Vec<AtomicBool>; 2],
}

#[derive(Debug)]
pub(crate) struct TargetSide {
    index: HashMap<Box<str>, u32>,
    ids: Vec<Box<str>>,
    hashes: Vec<u64>,
    body: Body,
}

#[derive(Debug)]
enum Body {
    /// Sorted day numbers per service.
    Dates(Vec<Vec<i32>>),
    Rows(Rows),
}

#[derive(Debug)]
struct Rows {
    /// The content columns of the target relation.
    own: Table,
    children: Option<Children>,
}

#[derive(Debug)]
struct Children {
    /// The content columns of the child relation.
    table: Table,
    /// Child rows grouped by content index, in key order.
    order: Vec<u32>,
    /// Content index -> range of `order`.
    ranges: Vec<(u32, u32)>,
}

impl Target {
    pub fn load(
        packages: [&Package; 2],
        contract: &Contract,
        generated: &GeneratedId,
        profile: &Profile,
    ) -> Result<Target, PackageError> {
        let (a, b) = rayon::join(
            || TargetSide::load(packages[0], contract, generated, profile),
            || TargetSide::load(packages[1], contract, generated, profile),
        );
        let sides = [a?, b?];
        let referenced = sides.each_ref().map(|side| {
            (0..side.ids.len())
                .map(|_| AtomicBool::new(false))
                .collect()
        });
        Ok(Target {
            generated: generated.clone(),
            sides,
            referenced,
        })
    }

    pub fn index_of(&self, side: usize, id: &str) -> Option<u32> {
        self.sides[side].index.get(id).copied()
    }

    pub fn mark(&self, side: usize, content: u32) {
        self.referenced[side][content as usize].store(true, Ordering::Relaxed);
    }

    pub fn is_referenced(&self, side: usize, content: u32) -> bool {
        self.referenced[side][content as usize].load(Ordering::Relaxed)
    }

    /// Contents nothing references, per side.
    pub fn unreferenced(&self) -> [u64; 2] {
        self.referenced.each_ref().map(|flags| {
            flags
                .iter()
                .filter(|flag| !flag.load(Ordering::Relaxed))
                .count() as u64
        })
    }
}

impl TargetSide {
    fn load(
        package: &Package,
        contract: &Contract,
        generated: &GeneratedId,
        profile: &Profile,
    ) -> Result<TargetSide, PackageError> {
        let spec = load::relation_spec(contract, &generated.relation)?;
        let field = generated.field.as_str();
        let mut side = match &generated.content {
            Content::Dates { exceptions } => {
                load::dates(package, &generated.relation, field, exceptions)?
            }
            Content::Columns => load::rows(package, spec, field, None, profile)?,
            Content::Children { relation } => {
                let child = load::relation_spec(contract, relation)?;
                load::rows(package, spec, field, Some(child), profile)?
            }
        };
        side.hashes = side.compute_hashes();
        Ok(side)
    }

    fn compute_hashes(&self) -> Vec<u64> {
        match &self.body {
            Body::Dates(sets) => sets
                .par_iter()
                .map(|days| keys::combine(days.iter().map(|&day| keys::int(day.into()))))
                .collect(),
            Body::Rows(rows) => {
                let own: Vec<ValueHashes> = column_hashes(&rows.own);
                let children = rows
                    .children
                    .as_ref()
                    .map(|children| (children, column_hashes(&children.table)));
                (0..self.ids.len())
                    .into_par_iter()
                    .map(|index| {
                        let mut values: Vec<u64> =
                            own.iter().map(|column| column.get_or_null(index)).collect();
                        if let Some((children, columns)) = &children {
                            for row in children.rows(index) {
                                values.extend(columns.iter().map(|column| column.get_or_null(row)));
                            }
                        }
                        keys::combine(values)
                    })
                    .collect()
            }
        }
    }
}

fn column_hashes(table: &Table) -> Vec<ValueHashes<'_>> {
    table
        .columns
        .iter()
        .map(|(_, column)| ValueHashes::new(column))
        .collect()
}

impl Children {
    fn rows(&self, index: usize) -> impl Iterator<Item = usize> + '_ {
        let (start, end) = self.ranges[index];
        self.order[start as usize..end as usize]
            .iter()
            .map(|&row| row as usize)
    }

    fn len(&self, index: usize) -> usize {
        let (start, end) = self.ranges[index];
        (end - start) as usize
    }
}

/// A target's two sides with their content columns paired for comparison.
pub(crate) struct TargetPair<'a> {
    target: &'a Target,
    own: Vec<(&'a str, Pair<'a>)>,
    children: Option<(&'a str, Vec<Pair<'a>>)>,
    /// No double among the content columns: equal content has equal hashes.
    exact: bool,
}

impl<'a> TargetPair<'a> {
    pub fn new(
        target: &'a Target,
        packages: [&Package; 2],
        profile: &Profile,
    ) -> Result<TargetPair<'a>, PackageError> {
        let [a, b] = &target.sides;
        let pairs =
            |ta: &'a Table, tb: &'a Table| -> Result<Vec<(&'a str, Pair<'a>)>, PackageError> {
                ta.columns
                    .iter()
                    .zip(&tb.columns)
                    .map(|((name, ca), (_, cb))| {
                        Pair::new(ca, cb, profile.tolerance(name))
                            .map(|pair| (name.as_str(), pair))
                            .map_err(|_| type_mismatch(packages[1], &tb.name, name, cb))
                    })
                    .collect()
            };
        let (own, children) = match (&a.body, &b.body) {
            (Body::Rows(ra), Body::Rows(rb)) => {
                let own = pairs(&ra.own, &rb.own)?;
                let children = match (&ra.children, &rb.children) {
                    (Some(ca), Some(cb)) => Some((
                        ca.table.name.as_str(),
                        pairs(&ca.table, &cb.table)?
                            .into_iter()
                            .map(|(_, pair)| pair)
                            .collect(),
                    )),
                    _ => None,
                };
                (own, children)
            }
            _ => (Vec::new(), None),
        };
        let has_double = |table: &Table| {
            table
                .columns
                .iter()
                .any(|(_, column)| matches!(column, Column::Double(_)))
        };
        let exact = match &a.body {
            Body::Dates(_) => true,
            Body::Rows(rows) => {
                !has_double(&rows.own)
                    && !rows.children.as_ref().is_some_and(|c| has_double(&c.table))
            }
        };
        Ok(TargetPair {
            target,
            own,
            children,
            exact,
        })
    }

    pub fn resolve(&self, side: usize, id: &str) -> Option<u32> {
        self.target.index_of(side, id)
    }

    pub fn id(&self, side: usize, index: u32) -> &str {
        &self.target.sides[side].ids[index as usize]
    }

    pub fn hash(&self, side: usize, index: u32) -> u64 {
        self.target.sides[side].hashes[index as usize]
    }

    /// Whether A's content `x` equals B's content `y` (doubles within tolerance).
    pub fn eq(&self, x: u32, y: u32) -> bool {
        let (a, b) = (&self.target.sides[0], &self.target.sides[1]);
        let same_hash = a.hashes[x as usize] == b.hashes[y as usize];
        if self.exact && !same_hash {
            return false;
        }
        match (&a.body, &b.body) {
            (Body::Dates(da), Body::Dates(db)) => da[x as usize] == db[y as usize],
            (Body::Rows(ra), Body::Rows(rb)) => self.first_difference(ra, rb, x, y).is_none(),
            _ => false,
        }
    }

    /// Where A's content `x` and B's content `y` first differ: a column name, or a child
    /// position.
    fn first_difference(&self, ra: &Rows, rb: &Rows, x: u32, y: u32) -> Option<String> {
        let (x, y) = (x as usize, y as usize);
        if let Some((name, _)) = self.own.iter().find(|(_, pair)| !pair.eq(x, y)) {
            return Some((*name).to_owned());
        }
        let (Some((child, pairs)), Some(ca), Some(cb)) =
            (&self.children, &ra.children, &rb.children)
        else {
            return None;
        };
        let position = ca
            .rows(x)
            .zip(cb.rows(y))
            .position(|(p, q)| pairs.iter().any(|pair| !pair.eq(p, q)));
        match position {
            Some(position) => Some(format!("{child} {}", position + 1)),
            None if ca.len(x) != cb.len(y) => Some(format!("{child} count")),
            None => None,
        }
    }

    /// The content as text, without its identifier.
    pub fn text(&self, side: usize, index: u32) -> String {
        let target_side = &self.target.sides[side];
        let index = index as usize;
        match &target_side.body {
            Body::Dates(sets) => dates_text(&sets[index]),
            Body::Rows(rows) => {
                let mut parts: Vec<String> = rows
                    .own
                    .columns
                    .iter()
                    .filter(|(_, column)| !column.is_null(index))
                    .map(|(name, column)| format!("{name}={}", column.display(index)))
                    .collect();
                if let Some(children) = &rows.children {
                    parts.push(format!("{} {}", children.len(index), children.table.name));
                }
                format!("{{{}}}", parts.join(", "))
            }
        }
    }

    /// A service's sorted day numbers; `None` for content that is not dates.
    pub fn days(&self, side: usize, index: u32) -> Option<&'a [i32]> {
        match &self.target.sides[side].body {
            Body::Dates(sets) => Some(&sets[index as usize]),
            Body::Rows(_) => None,
        }
    }

    /// A service's dates as `first..last (n days)`; `None` for content that is not dates.
    pub fn date_span(&self, side: usize, index: u32) -> Option<String> {
        let Body::Dates(sets) = &self.target.sides[side].body else {
            return None;
        };
        let days = &sets[index as usize];
        Some(match (days.first(), days.last()) {
            (Some(&first), Some(&last)) => format!(
                "{}..{} ({} days)",
                day_text(first),
                day_text(last),
                days.len()
            ),
            _ => "no dates".to_owned(),
        })
    }

    /// The content as a key part: its text, plus a digest where the text abbreviates it.
    pub fn canonical_text(&self, side: usize, index: u32) -> String {
        let text = self.text(side, index);
        match &self.target.sides[side].body {
            Body::Rows(Rows { children: None, .. }) => text,
            _ => format!("{text} #{:08x}", self.hash(side, index) >> 32),
        }
    }

    /// How B's content `y` differs from A's content `x`, as a suffix to B's text.
    pub fn difference(&self, x: u32, y: u32) -> String {
        let (a, b) = (&self.target.sides[0], &self.target.sides[1]);
        match (&a.body, &b.body) {
            (Body::Dates(da), Body::Dates(db)) => {
                let (da, db) = (&da[x as usize], &db[y as usize]);
                let added: Vec<i32> = db
                    .iter()
                    .filter(|d| da.binary_search(d).is_err())
                    .copied()
                    .collect();
                let removed: Vec<i32> = da
                    .iter()
                    .filter(|d| db.binary_search(d).is_err())
                    .copied()
                    .collect();
                let mut text = String::new();
                for (sign, days) in [('+', &added), ('-', &removed)] {
                    for &day in days.iter().take(LISTED) {
                        text.push_str(&format!(" {sign}{}", day_text(day)));
                    }
                    if days.len() > LISTED {
                        text.push_str(&format!(" {sign}…{} more", days.len() - LISTED));
                    }
                }
                text
            }
            (Body::Rows(ra), Body::Rows(rb)) => self
                .first_difference(ra, rb, x, y)
                .map(|at| format!(" (differs at {at})"))
                .unwrap_or_default(),
            _ => String::new(),
        }
    }
}

fn day_text(day: i32) -> String {
    dates::from_days(day).map_or_else(|| day.to_string(), |date| date.to_string())
}

fn dates_text(days: &[i32]) -> String {
    match (days.first(), days.last()) {
        (Some(&first), Some(&last)) => {
            format!(
                "{{{} dates {}..{}}}",
                days.len(),
                day_text(first),
                day_text(last)
            )
        }
        _ => "{no dates}".to_owned(),
    }
}
