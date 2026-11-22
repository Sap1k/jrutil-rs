//! Each relation file against the contract: schema, footer metadata, row groups, row count and
//! primary key. Loads the columns the later checks need.

use std::collections::{BTreeMap, BTreeSet};

use rayon::prelude::*;

use super::keys::Keys;
use super::{Compatibility, Findings, code, gtfs, parts, relation_file};
use crate::contract::{Contract, RelationSpec, parse_version};
use crate::package::Package;
use crate::parquet_read::ParquetFile;
use crate::table::Table;

/// The loaded relations, by name. A relation is absent when its file is missing or broken.
pub type Tables = BTreeMap<String, Table>;

pub fn check(
    findings: &mut Findings,
    package: &Package,
    contract: &Contract,
    compat: &Compatibility,
    files: &BTreeMap<String, u64>,
) -> Tables {
    let needed = needed_columns(contract);
    let results: Vec<(Findings, Option<Table>)> = contract
        .relations
        .par_iter()
        .map(|relation| {
            let mut local = Findings::default();
            let table = if files.contains_key(&relation_file(&relation.name)) {
                check_relation(
                    &mut local,
                    package,
                    contract,
                    compat,
                    relation,
                    &needed[&relation.name],
                )
            } else {
                None
            };
            (local, table)
        })
        .collect();
    let mut tables = Tables::new();
    for (local, table) in results {
        findings.extend(local);
        if let Some(table) = table {
            tables.insert(table.name.clone(), table);
        }
    }
    tables
}

fn check_relation(
    findings: &mut Findings,
    package: &Package,
    contract: &Contract,
    compat: &Compatibility,
    relation: &RelationSpec,
    columns: &[&str],
) -> Option<Table> {
    let subject = relation_file(&relation.name);
    let file = match package.parquet(&relation.name) {
        Ok(file) => file,
        Err(error) => {
            findings.add(code::PARQUET_UNREADABLE, &subject, error.to_string());
            return None;
        }
    };
    let schema_ok = check_schema(findings, &subject, &file, relation, compat);
    check_metadata(findings, &subject, &file, contract, &relation.name);

    let metadata = file.metadata();
    for (index, group) in metadata.row_groups().iter().enumerate() {
        if u64::try_from(group.num_rows()).unwrap_or(u64::MAX) > contract.row_group_maximum {
            findings.add(
                code::ROW_GROUP_TOO_LARGE,
                &subject,
                format!(
                    "row group {index} has {} rows, over {}",
                    group.num_rows(),
                    contract.row_group_maximum
                ),
            );
        }
    }
    let rows: i64 = metadata
        .row_groups()
        .iter()
        .map(|group| group.num_rows())
        .sum();
    if let Some(declared) = package.manifest().relation(&relation.name)
        && u64::try_from(rows).ok() != Some(declared.row_count)
    {
        findings.add(
            code::ROW_COUNT_MISMATCH,
            &subject,
            format!(
                "manifest row count {}, physical row count {rows}",
                declared.row_count
            ),
        );
    }
    if !schema_ok {
        return None;
    }
    let table = match file.read(&relation.name, columns) {
        Ok(table) => table,
        Err(error) => {
            findings.add(code::PARQUET_UNREADABLE, &subject, error.to_string());
            return None;
        }
    };
    check_primary_key(findings, &subject, &table, relation);
    Some(table)
}

/// Exact field order, type and nullability; a later minor may append nullable fields.
fn check_schema(
    findings: &mut Findings,
    subject: &str,
    file: &ParquetFile,
    relation: &RelationSpec,
    compat: &Compatibility,
) -> bool {
    let actual = file.schema().fields();
    let expected = &relation.fields;
    let mut ok = true;
    if actual.len() < expected.len() || (actual.len() > expected.len() && !compat.newer_minor) {
        findings.add(
            code::SCHEMA_MISMATCH,
            subject,
            format!("expected {} fields, found {}", expected.len(), actual.len()),
        );
        ok = false;
    }
    for (field, spec) in actual.iter().zip(expected) {
        let ty = ParquetFile::field_type(field.data_type());
        if field.name() != &spec.name || ty != Some(spec.ty) || field.is_nullable() != spec.nullable
        {
            findings.add(
                code::SCHEMA_MISMATCH,
                subject,
                format!(
                    "field {} is {} {} nullable={}, contract says {} nullable={}",
                    spec.name,
                    field.name(),
                    field.data_type(),
                    field.is_nullable(),
                    spec.ty,
                    spec.nullable
                ),
            );
            ok = false;
        }
    }
    if actual.len() > expected.len() && compat.newer_minor {
        for field in actual
            .iter()
            .skip(expected.len())
            .filter(|field| !field.is_nullable())
        {
            findings.add(
                code::SCHEMA_MISMATCH,
                subject,
                format!(
                    "field {} added by a later minor must be nullable",
                    field.name()
                ),
            );
            ok = false;
        }
    }
    ok
}

/// The contract's footer keys: serving major, relation name, bundle format.
fn check_metadata(
    findings: &mut Findings,
    subject: &str,
    file: &ParquetFile,
    contract: &Contract,
    name: &str,
) {
    for (key, template) in &contract.parquet_metadata {
        let actual = file.key_value(key);
        let ok = if key == "obehy.schema_version" {
            actual
                .and_then(parse_version)
                .is_some_and(|(major, _)| major == contract.major)
        } else if template == "<relation name>" {
            actual == Some(name)
        } else {
            actual == Some(template.as_str())
        };
        if !ok {
            findings.add(
                code::PARQUET_METADATA_MISMATCH,
                subject,
                format!(
                    "{key} is {}",
                    actual.map_or_else(|| "missing".to_owned(), |v| format!("{v:?}"))
                ),
            );
        }
    }
}

fn check_primary_key(
    findings: &mut Findings,
    subject: &str,
    table: &Table,
    relation: &RelationSpec,
) {
    let Some(keys) = Keys::new(table, &relation.primary_key) else {
        return;
    };
    let mut hashed: Vec<(u64, u32)> = Vec::with_capacity(table.rows());
    for row in 0..table.rows() {
        match keys.hash(row) {
            Some(hash) => hashed.push((hash, row as u32)),
            None => {
                findings.add(
                    code::NULL_PRIMARY_KEY,
                    subject,
                    format!("null primary key at row {row}"),
                );
                return;
            }
        }
    }
    hashed.par_sort_unstable();
    let mut duplicates = 0_u64;
    let mut example = None;
    for run in hashed
        .chunk_by(|left, right| left.0 == right.0)
        .filter(|run| run.len() > 1)
    {
        // Equal hashes are almost always equal keys; split off true collisions exactly.
        let mut rest: Vec<usize> = run.iter().map(|&(_, row)| row as usize).collect();
        while rest.len() > 1 {
            let first = rest[0];
            let (same, other): (Vec<usize>, Vec<usize>) =
                rest.iter().partition(|&&row| keys.same(first, row));
            if same.len() > 1 {
                duplicates += same.len() as u64 - 1;
                example = Some(example.map_or(first, |current: usize| current.min(first)));
            }
            rest = other;
        }
    }
    if let Some(row) = example {
        findings.add(
            code::DUPLICATE_PRIMARY_KEY,
            subject,
            format!(
                "{duplicates} duplicate primary keys, e.g. ({})",
                keys.display(row)
            ),
        );
    }
}

/// Per relation, the columns some check reads, in file order.
fn needed_columns(contract: &Contract) -> BTreeMap<String, Vec<&str>> {
    let mut wanted: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for relation in &contract.relations {
        let set = wanted.entry(&relation.name).or_default();
        set.extend(relation.primary_key.iter().map(String::as_str));
        set.extend(
            relation
                .fields
                .iter()
                .filter(|f| f.enumeration.is_some())
                .map(|f| f.name.as_str()),
        );
        for key in &relation.foreign_keys {
            set.extend(key.fields.iter().map(String::as_str));
        }
        for key in &relation.foreign_keys {
            wanted
                .entry(&key.relation)
                .or_default()
                .extend(key.target_fields.iter().map(String::as_str));
        }
    }
    let extra = parts::COLUMNS
        .iter()
        .chain(gtfs::COLUMNS)
        .chain(super::references::COLUMNS);
    for (relation, fields) in extra {
        wanted
            .entry(relation)
            .or_default()
            .extend(fields.iter().copied());
    }
    contract
        .relations
        .iter()
        .map(|relation| {
            let set = wanted.get(relation.name.as_str());
            let columns = relation
                .fields
                .iter()
                .map(|field| field.name.as_str())
                .filter(|name| set.is_some_and(|set| set.contains(name)))
                .collect();
            (relation.name.clone(), columns)
        })
        .collect()
}
