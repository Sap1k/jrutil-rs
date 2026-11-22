//! Comparing two production packages relation by relation, canonically: generated
//! identifiers are never compared literally (rows keyed by one compare as a multiset of their
//! content, references to one compare by what they refer to), doubles compare within a
//! tolerance, and every difference lands in a default category (`<relation>.missing`,
//! `.extra`, `.changed.<column>`) that a parity ledger (`ledger`) may explain.
//!
//! A is the reference, B the candidate. Counts are exact; samples are bounded and
//! deterministic, so a comparison of millions of rows never holds millions of differences.

mod cells;
mod journey;
mod ledger;
mod profile;
mod report;
mod rows;
mod tally;
mod targets;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cells::Pair;
use journey::{Journeys, Renames};
use rows::Columns;
use tally::{Categories, Recorder};
use targets::{Target, TargetPair};

pub use journey::TripPairing;
pub use ledger::{
    CategoryVerdict, DefaultVerdict, Ledger, LedgerCategory, LedgerError, Status, Verdict,
};
pub use profile::{Content, GeneratedId, Profile, TripIdentity};
pub use report::report_json;
pub use tally::{Bucket, CategoryDiffs, Sample};

use crate::contract::{RelationSpec, contract};
use crate::error::PackageError;
use crate::package::Package;
use crate::table::{Column, Table};

/// The differences between two packages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comparison {
    /// Every contract relation, in contract order.
    pub relations: Vec<RelationSummary>,
    /// By default category.
    pub categories: BTreeMap<String, CategoryDiffs>,
    /// Generated content nothing references (informational), by `relation.field`.
    pub unreferenced: Vec<Unreferenced>,
    /// How trips were paired under `TripIdentity::Journey`; `None` under `TripId`.
    pub trip_pairing: Option<TripPairing>,
}

impl Comparison {
    pub fn differences(&self) -> u64 {
        self.categories.values().map(|diffs| diffs.count).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationSummary {
    pub name: String,
    pub mode: Mode,
    /// Rows in A and in B.
    pub rows: [u64; 2],
}

/// How a relation's rows are compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Matched by primary key.
    Keyed,
    /// Its key is generated: a multiset of canonical rows.
    Multiset,
    /// It defines generated content and is compared through what references it.
    Definition,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Keyed => "keyed",
            Mode::Multiset => "multiset",
            Mode::Definition => "definition",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreferenced {
    pub target: String,
    /// In A and in B.
    pub counts: [u64; 2],
}

/// Compares `b` with the reference `a`; every difference is left unexplained.
pub fn compare(a: &Package, b: &Package, profile: &Profile) -> Result<Comparison, PackageError> {
    compare_classified(a, b, profile, &Ledger::default())
}

/// Compares `b` with the reference `a`, assigning each difference to the first ledger
/// category that matches it (for `Ledger::judge`).
pub fn compare_classified(
    a: &Package,
    b: &Package,
    profile: &Profile,
    ledger: &Ledger,
) -> Result<Comparison, PackageError> {
    let packages = [a, b];
    let plans = plan(profile);
    let mut last_use: HashMap<usize, usize> = HashMap::new();
    for (position, plan) in plans.iter().enumerate() {
        for target in plan.targets() {
            last_use.insert(target, position);
        }
    }
    let mut targets: BTreeMap<usize, Target> = BTreeMap::new();
    // Tables the trip pairing has already read, handed on to their relation's comparison.
    let mut preread: HashMap<&str, [Table; 2]> = HashMap::new();
    let journeys = match profile.trip_identity {
        TripIdentity::TripId => None,
        TripIdentity::Journey => Some(journey::pair_trips(
            &plans,
            packages,
            profile,
            &mut targets,
            &mut preread,
        )?),
    };
    let context = Context {
        packages,
        profile,
        ledger,
        journeys: journeys.as_ref(),
    };
    let mut summaries: BTreeMap<&str, RelationSummary> = BTreeMap::new();
    let mut categories = BTreeMap::new();
    let mut unreferenced = Vec::new();
    for (position, plan) in plans.iter().enumerate() {
        let name = plan.spec.name.as_str();
        let rows = if plan.mode == Mode::Definition {
            let count = |package: &Package| -> Result<u64, PackageError> {
                let rows = package.parquet(name)?.metadata().file_metadata().num_rows();
                Ok(u64::try_from(rows).unwrap_or(0))
            };
            [count(a)?, count(b)?]
        } else {
            load_targets(plan, packages, profile, &mut targets)?;
            let recorder_categories = plan.categories(ledger);
            let rows = compare_relation(
                plan,
                &context,
                &targets,
                &recorder_categories,
                preread.remove(name),
                &mut categories,
            )?;
            for target in plan.targets() {
                if last_use.get(&target) == Some(&position)
                    && let Some(done) = targets.remove(&target)
                {
                    unreferenced.push(Unreferenced {
                        target: format!("{}.{}", done.generated.relation, done.generated.field),
                        counts: done.unreferenced(),
                    });
                }
            }
            rows
        };
        summaries.insert(
            name,
            RelationSummary {
                name: name.to_owned(),
                mode: plan.mode,
                rows,
            },
        );
    }
    unreferenced.sort_by(|x, y| x.target.cmp(&y.target));
    let relations = contract()
        .relations
        .iter()
        .filter_map(|spec| summaries.remove(spec.name.as_str()))
        .collect();
    Ok(Comparison {
        relations,
        categories,
        unreferenced,
        trip_pairing: journeys.map(|journeys| journeys.counts),
    })
}

/// What every relation's comparison shares.
struct Context<'a> {
    packages: [&'a Package; 2],
    profile: &'a Profile,
    ledger: &'a Ledger,
    journeys: Option<&'a Journeys>,
}

/// Loads the generated identifiers `plan` references that are not loaded yet.
fn load_targets(
    plan: &Plan,
    packages: [&Package; 2],
    profile: &Profile,
    targets: &mut BTreeMap<usize, Target>,
) -> Result<(), PackageError> {
    for target in plan.targets() {
        if let std::collections::btree_map::Entry::Vacant(entry) = targets.entry(target) {
            entry.insert(Target::load(
                packages,
                contract(),
                &profile.generated[target],
                profile,
            )?);
        }
    }
    Ok(())
}

/// Both sides' tables of a relation, every contract field.
fn read_tables(plan: &Plan, packages: [&Package; 2]) -> Result<[Table; 2], PackageError> {
    let name = plan.spec.name.as_str();
    let fields: Vec<&str> = plan.spec.fields.iter().map(|f| f.name.as_str()).collect();
    let read = |package: &Package| package.parquet(name)?.read(name, &fields);
    let (ta, tb) = rayon::join(|| read(packages[0]), || read(packages[1]));
    Ok([ta?, tb?])
}

/// The targets `plan` references, paired.
fn target_pairs<'t>(
    plan: &Plan,
    targets: &'t BTreeMap<usize, Target>,
    packages: [&Package; 2],
    profile: &Profile,
) -> Result<BTreeMap<usize, TargetPair<'t>>, PackageError> {
    plan.references
        .values()
        .map(|&target| {
            Ok((
                target,
                TargetPair::new(&targets[&target], packages, profile)?,
            ))
        })
        .collect()
}

/// Pairs the columns of one relation's two tables: references to generated identifiers by
/// content, trip references through `renames`, everything else by value.
struct Builder<'a> {
    plan: &'a Plan<'static>,
    packages: [&'a Package; 2],
    tables: &'a [Table; 2],
    profile: &'a Profile,
    target_pairs: &'a BTreeMap<usize, TargetPair<'a>>,
    renames: Option<&'a Renames>,
}

impl<'a> Builder<'a> {
    fn pair(&self, field: &str) -> Result<Pair<'a>, PackageError> {
        let tables: &'a [Table; 2] = self.tables;
        let column = |side: usize| typed(self.packages[side], &tables[side], field, Some);
        let (ca, cb) = (column(0)?, column(1)?);
        let renames = self
            .renames
            .filter(|_| self.plan.trip_refs.contains(&field));
        let paired = match (self.plan.references.get(field), renames) {
            (Some(target), _) => Pair::reference(ca, cb, &self.target_pairs[target]),
            (None, Some(renames)) => Pair::renamed(ca, cb, renames),
            (None, None) => Pair::new(ca, cb, self.profile.tolerance(field)),
        };
        paired.map_err(|_| type_mismatch(self.packages[1], &self.plan.spec.name, field, cb))
    }

    fn columns(&self, fields: &[&'static str]) -> Result<Columns<'a>, PackageError> {
        Ok(Columns {
            names: fields.to_vec(),
            pairs: fields
                .iter()
                .map(|field| self.pair(field))
                .collect::<Result<_, _>>()?,
        })
    }
}

/// How one relation is compared.
#[derive(Debug)]
struct Plan<'c> {
    spec: &'c RelationSpec,
    mode: Mode,
    /// Compared fields; for a keyed relation the key fields come first.
    key_fields: Vec<&'c str>,
    value_fields: Vec<&'c str>,
    /// Field -> the generated identifier it references (index into `Profile::generated`).
    references: BTreeMap<&'c str, usize>,
    /// For a multiset keyed by a referenced generated identifier: only the rows nothing
    /// references are compared (the others are compared through their referrers).
    unreferenced_only: Option<(&'c str, usize)>,
    /// Fields naming a trip (`trip.trip_id`, foreign keys reaching it, `Profile::trip_columns`):
    /// compared through the trip pairing under `TripIdentity::Journey`.
    trip_refs: Vec<&'c str>,
}

impl Plan<'_> {
    fn targets(&self) -> BTreeSet<usize> {
        let mut targets: BTreeSet<usize> = self.references.values().copied().collect();
        targets.extend(self.unreferenced_only.map(|(_, target)| target));
        targets
    }

    fn categories(&self, ledger: &Ledger) -> Categories {
        Categories::new(&self.spec.name, self.value_fields.iter().copied(), ledger)
    }
}

/// Every contract relation's plan; multisets filtered by references come last, after
/// everything that references them.
fn plan(profile: &Profile) -> Vec<Plan<'static>> {
    let contract = contract();
    let generated_index = |relation: &str, field: &str| {
        profile
            .generated
            .iter()
            .position(|id| id.relation == relation && id.field == field)
    };
    let trip_refs = journey::trip_references(profile);
    let mut plans: Vec<Plan> = contract
        .relations
        .iter()
        .map(|spec| {
            let own_generated: Vec<&str> = spec
                .primary_key
                .iter()
                .map(String::as_str)
                .filter(|field| generated_index(&spec.name, field).is_some())
                .collect();
            let references: BTreeMap<&str, usize> = spec
                .foreign_keys
                .iter()
                .filter(|key| key.fields.len() == 1)
                .filter_map(|key| {
                    let target = generated_index(&key.relation, &key.target_fields[0])?;
                    Some((key.fields[0].as_str(), target))
                })
                .collect();
            let mode = if profile.is_definition(&spec.name) {
                Mode::Definition
            } else if spec.primary_key.iter().any(|field| {
                own_generated.contains(&field.as_str()) || references.contains_key(field.as_str())
            }) {
                Mode::Multiset
            } else {
                Mode::Keyed
            };
            let fields = spec.fields.iter().map(|field| field.name.as_str());
            let key_fields: Vec<&str> = match mode {
                Mode::Keyed => spec.primary_key.iter().map(String::as_str).collect(),
                _ => Vec::new(),
            };
            let value_fields = fields
                .filter(|field| !key_fields.contains(field) && !own_generated.contains(field))
                .filter(|field| !profile.is_label(&spec.name, field))
                .collect();
            let unreferenced_only = own_generated
                .first()
                .and_then(|field| Some((*field, generated_index(&spec.name, field)?)));
            Plan {
                spec,
                mode,
                key_fields,
                value_fields,
                references: if mode == Mode::Definition {
                    BTreeMap::new()
                } else {
                    references
                },
                unreferenced_only,
                trip_refs: spec
                    .fields
                    .iter()
                    .map(|field| field.name.as_str())
                    .filter(|field| trip_refs.contains(&(spec.name.as_str(), *field)))
                    .collect(),
            }
        })
        .collect();
    let referenced: BTreeSet<usize> = plans
        .iter()
        .flat_map(|plan| plan.references.values().copied())
        .collect();
    for plan in &mut plans {
        plan.unreferenced_only = plan
            .unreferenced_only
            .filter(|(_, target)| plan.mode == Mode::Multiset && referenced.contains(target));
    }
    plans.sort_by_key(|plan| plan.unreferenced_only.is_some());
    plans
}

/// Compares one keyed or multiset relation; returns its row counts.
fn compare_relation(
    plan: &Plan<'static>,
    context: &Context,
    targets: &BTreeMap<usize, Target>,
    categories: &Categories,
    preread: Option<[Table; 2]>,
    out: &mut BTreeMap<String, CategoryDiffs>,
) -> Result<[u64; 2], PackageError> {
    let Context {
        packages,
        profile,
        ledger,
        journeys,
    } = *context;
    let tables = match preread {
        Some(tables) => tables,
        None => read_tables(plan, packages)?,
    };
    let target_pairs = target_pairs(plan, targets, packages, profile)?;
    let builder = Builder {
        plan,
        packages,
        tables: &tables,
        profile,
        target_pairs: &target_pairs,
        renames: journeys
            .map(|journeys| &journeys.renames)
            .filter(|renames| !renames.is_empty()),
    };
    let key = builder.columns(&plan.key_fields)?;
    let values = builder.columns(&plan.value_fields)?;
    for (field, pair) in values.names.iter().zip(&values.pairs) {
        if let Some(target) = plan.references.get(field) {
            for side in 0..2 {
                for content in pair.referenced(side) {
                    targets[target].mark(side, content);
                }
            }
        }
    }
    let fresh = || Recorder::new(categories, ledger, profile.samples);
    let counts = [tables[0].rows(), tables[1].rows()];
    // Trips left unpaired are described so that a person can find the journey.
    let unpaired = journeys
        .filter(|_| plan.spec.name == journey::TRIP)
        .map(|journeys| &journeys.unpaired);
    let describe = |side: usize, row: u32| unpaired?[side].get(&row).cloned();
    let recorder = match plan.mode {
        Mode::Keyed => rows::keyed(counts, &key, &values, &describe, fresh),
        _ => {
            let selected = |side: usize| -> Result<Vec<u32>, PackageError> {
                let all = 0..counts[side] as u32;
                let Some((field, target)) = plan.unreferenced_only else {
                    return Ok(all.collect());
                };
                let target = &targets[&target];
                let ids = typed(packages[side], &tables[side], field, Column::as_str)?;
                Ok(all
                    .filter(|&row| {
                        ids.get(row as usize)
                            .and_then(|id| target.index_of(side, id))
                            .is_none_or(|content| !target.is_referenced(side, content))
                    })
                    .collect())
            };
            rows::multiset([selected(0)?, selected(1)?], &values, fresh)
        }
    };
    recorder.finish(out);
    Ok(counts.map(|rows| rows as u64))
}

/// A column of the wrong type.
pub(crate) fn type_mismatch(
    package: &Package,
    relation: &str,
    column: &str,
    found: &Column,
) -> PackageError {
    PackageError::UnsupportedColumn {
        path: package.relation_path(relation),
        column: column.to_owned(),
        data_type: found.field_type().to_string(),
    }
}

/// A column of `table`, as the type `pick` accepts.
pub(crate) fn typed<'t, T>(
    package: &Package,
    table: &'t Table,
    name: &str,
    pick: impl Fn(&'t Column) -> Option<T>,
) -> Result<T, PackageError> {
    let column = table
        .column(name)
        .ok_or_else(|| PackageError::MissingColumn {
            path: package.relation_path(&table.name),
            column: name.to_owned(),
        })?;
    pick(column).ok_or_else(|| type_mismatch(package, &table.name, name, column))
}
