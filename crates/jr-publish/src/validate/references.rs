//! Foreign keys, enumeration values, feed prefixes and source-key namespace encodings.

use std::collections::{BTreeMap, HashSet};

use rayon::prelude::*;

use super::keys::Keys;
use super::structure::Tables;
use super::{Findings, code, relation_file};
use crate::contract::{Contract, FEED_PREFIXES, FieldType, ForeignKey, RelationSpec};
use crate::table::{Column, Table};

/// Columns these checks read beyond keys and enumerations.
pub const COLUMNS: &[(&str, &[&str])] = &[
    ("source_key", &["namespace", "identifier", "entity_kind"]),
    ("call_key", &["namespace", "identifier"]),
];

pub fn check(findings: &mut Findings, contract: &Contract, tables: &Tables) {
    foreign_keys(findings, contract, tables);
    for relation in &contract.relations {
        let Some(table) = tables.get(&relation.name) else {
            continue;
        };
        enumerations(findings, contract, relation, table);
        feed_prefixes(findings, relation, table);
    }
    namespaces(findings, contract, tables, "source_key", true);
    namespaces(findings, contract, tables, "call_key", false);
}

fn report(
    findings: &mut Findings,
    code: &'static str,
    relation: &str,
    problem: &str,
    (count, example): (u64, Option<String>),
) {
    if count > 0 {
        let example = example.unwrap_or_default();
        findings.add(
            code,
            relation_file(relation),
            format!("{count} rows {problem}, e.g. {example}"),
        );
    }
}

/// Foreign keys by their target relation and fields.
type Uses<'a> = BTreeMap<(&'a str, &'a [String]), Vec<(&'a RelationSpec, &'a ForeignKey)>>;

/// Every non-null reference resolves; target key sets are built once per target.
fn foreign_keys(findings: &mut Findings, contract: &Contract, tables: &Tables) {
    let mut uses = Uses::new();
    for relation in &contract.relations {
        for key in &relation.foreign_keys {
            uses.entry((key.relation.as_str(), key.target_fields.as_slice()))
                .or_default()
                .push((relation, key));
        }
    }
    let results: Vec<Findings> = uses
        .into_par_iter()
        .map(|((target, target_fields), references)| {
            let mut local = Findings::default();
            let Some(keys) = tables
                .get(target)
                .and_then(|table| key_set(table, target_fields))
            else {
                return local;
            };
            for (relation, key) in references {
                let Some(table) = tables.get(&relation.name) else {
                    continue;
                };
                let Some(source) = Keys::new(table, &key.fields) else {
                    continue;
                };
                let mut missing = 0_u64;
                let mut example = None;
                for row in 0..table.rows() {
                    if let Some(hash) = source.hash(row)
                        && !keys.contains(&hash)
                    {
                        missing += 1;
                        example.get_or_insert(row);
                    }
                }
                if let Some(row) = example {
                    local.add(
                        code::FOREIGN_KEY_DANGLING,
                        relation_file(&relation.name),
                        format!(
                            "{missing} rows have ({}) missing from {target}, e.g. ({})",
                            key.fields.join(","),
                            source.display(row)
                        ),
                    );
                }
            }
            local
        })
        .collect();
    for local in results {
        findings.extend(local);
    }
}

fn key_set(table: &Table, fields: &[String]) -> Option<HashSet<u64>> {
    let keys = Keys::new(table, fields)?;
    Some((0..table.rows()).filter_map(|row| keys.hash(row)).collect())
}

/// Coded fields take only their enumeration's values (int16 as decimal text).
fn enumerations(
    findings: &mut Findings,
    contract: &Contract,
    relation: &RelationSpec,
    table: &Table,
) {
    for field in &relation.fields {
        let Some(name) = &field.enumeration else {
            continue;
        };
        let Some(column) = table.column(&field.name) else {
            continue;
        };
        let allowed: HashSet<&str> = contract.enumerations[name]
            .iter()
            .map(String::as_str)
            .collect();
        let result = match column {
            Column::Str(values) => {
                let valid: Vec<bool> = values
                    .pool()
                    .iter()
                    .map(|v| allowed.contains(&**v))
                    .collect();
                count_rows(
                    values.len(),
                    |row| values.code(row).is_some_and(|code| !valid[code as usize]),
                    |row| values.get(row).unwrap_or_default().to_owned(),
                )
            }
            Column::Int16(values) => {
                let invalid = |value: i16| !allowed.contains(value.to_string().as_str());
                count_rows(
                    values.len(),
                    |row| values[row].is_some_and(invalid),
                    |row| column.display(row),
                )
            }
            _ => continue,
        };
        report(
            findings,
            code::ENUM_VALUE_INVALID,
            &relation.name,
            &format!("have {} outside {name}", field.name),
            result,
        );
    }
}

fn count_rows(
    rows: usize,
    bad: impl Fn(usize) -> bool,
    show: impl Fn(usize) -> String,
) -> (u64, Option<String>) {
    let mut count = 0;
    let mut first = None;
    for row in 0..rows {
        if bad(row) {
            count += 1;
            first.get_or_insert(row);
        }
    }
    (count, first.map(show))
}

/// A single-field text primary key starts with a feed prefix.
fn feed_prefixes(findings: &mut Findings, relation: &RelationSpec, table: &Table) {
    let [key] = relation.primary_key.as_slice() else {
        return;
    };
    if relation.field(key).map(|field| field.ty) != Some(FieldType::String) {
        return;
    }
    let Some(Column::Str(values)) = table.column(key) else {
        return;
    };
    let valid: Vec<bool> = values
        .pool()
        .iter()
        .map(|value| FEED_PREFIXES.iter().any(|prefix| value.starts_with(prefix)))
        .collect();
    let result = count_rows(
        values.len(),
        |row| values.code(row).is_some_and(|code| !valid[code as usize]),
        |row| values.get(row).unwrap_or_default().to_owned(),
    );
    report(
        findings,
        code::FEED_PREFIX_MISSING,
        &relation.name,
        &format!("have {key} without a feed prefix"),
        result,
    );
}

/// Identifiers in a declared namespace follow its encoding; source keys also name the
/// namespace's entity kind. Undeclared namespaces (a later minor) are not checked.
fn namespaces(
    findings: &mut Findings,
    contract: &Contract,
    tables: &Tables,
    relation: &str,
    with_kind: bool,
) {
    let Some(table) = tables.get(relation) else {
        return;
    };
    let (Some(Column::Str(namespace)), Some(Column::Str(identifier))) =
        (table.column("namespace"), table.column("identifier"))
    else {
        return;
    };
    let kind = match table.column("entity_kind") {
        Some(Column::Str(kind)) if with_kind => Some(kind),
        _ => None,
    };
    let result = count_rows(
        table.rows(),
        |row| {
            let (Some(name), Some(id)) = (namespace.get(row), identifier.get(row)) else {
                return false;
            };
            let Some(spec) = contract.namespaces.get(name) else {
                return false;
            };
            let kind_ok = kind.is_none_or(|kind| kind.get(row) == Some(spec.entity_kind.as_str()));
            !kind_ok || !identifier_matches(name, id)
        },
        |row| {
            format!(
                "{} {}",
                namespace.get(row).unwrap_or_default(),
                identifier.get(row).unwrap_or_default()
            )
        },
    );
    report(
        findings,
        code::NAMESPACE_ENCODING_INVALID,
        relation,
        "do not match their namespace's entity kind or identifier encoding",
        result,
    );
}

/// The identifier encodings of the contract's namespaces (the F# validator's patterns). A
/// namespace without a structured encoding only needs a non-empty single-line identifier.
fn identifier_matches(namespace: &str, identifier: &str) -> bool {
    fn digits(text: &str) -> bool {
        !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
    }
    let line = !identifier.is_empty() && !identifier.contains('\n');
    match namespace {
        "cis:line" => identifier.len() == 6 && digits(identifier),
        "cis:line_trip" => identifier.split_once(':').is_some_and(|(line, trip)| {
            line.len() == 6 && digits(line) && digits(trip) && !trip.starts_with('0')
        }),
        "czptt:train_number" => digits(identifier),
        "sr70" => identifier.len() == 5 && digits(identifier),
        "sr70:track" => identifier.split_once(':').is_some_and(|(code, track)| {
            code.len() == 5 && digits(code) && !track.is_empty() && !track.contains('\n')
        }),
        "ids-jmk:line_course" => identifier.split_once('/').is_some_and(|(line, course)| {
            !line.is_empty()
                && !course.is_empty()
                && !course.contains('/')
                && !identifier.contains('\n')
        }),
        _ => line,
    }
}

#[cfg(test)]
mod tests {
    use super::identifier_matches;

    #[test]
    fn namespace_encodings_follow_the_contract_examples() {
        let contract = crate::contract::contract();
        assert_eq!(contract.namespaces.len(), 15);
        // The contract's own examples are valid.
        for (name, valid) in [
            ("cis:line", "582492"),
            ("cis:line_trip", "582492:143"),
            ("czptt:train_number", "16001"),
            ("ids-jmk:line_course", "104/25"),
            ("sr70", "53414"),
            ("sr70:track", "53414:102"),
            ("czptt:pa", "Pa:0054:KT----18872A:00:2026"),
        ] {
            assert!(identifier_matches(name, valid), "{name} {valid}");
        }
        for (name, invalid) in [
            ("cis:line", "58249"),
            ("cis:line_trip", "582492:043"),
            ("cis:line_trip", "582492:"),
            ("czptt:train_number", "16001a"),
            ("ids-jmk:line_course", "104/25/1"),
            ("sr70", "534140"),
            ("sr70:track", "53414:"),
            ("pid:gtfs_trip_id", ""),
        ] {
            assert!(!identifier_matches(name, invalid), "{name} {invalid}");
        }
    }
}
