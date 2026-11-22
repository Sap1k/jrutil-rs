//! Reading a generated identifier's content from one package.

use std::collections::HashMap;

use rayon::prelude::*;

use super::{Body, Children, Rows, TargetSide};
use crate::compare::cells::cmp_rows;
use crate::compare::typed;
use crate::contract::{Contract, RelationSpec};
use crate::dates;
use crate::error::PackageError;
use crate::package::Package;
use crate::table::{Column, Table};

/// Calendar and exception columns (contract `service_calendar`, `service_exception`).
const VALID_FROM: &str = "valid_from";
const VALID_TO: &str = "valid_to";
const WEEKDAY_MASK: &str = "weekday_mask";
const SERVICE_DATE: &str = "service_date";
const ADDED: &str = "added";

pub(super) fn relation_spec<'c>(
    contract: &'c Contract,
    name: &str,
) -> Result<&'c RelationSpec, PackageError> {
    contract
        .relation(name)
        .ok_or_else(|| PackageError::MissingMember {
            path: "serving".into(),
            member: format!("{name} (not in the contract)"),
        })
}

/// Services: calendar rows expanded over their weekdays, then the exceptions applied.
pub(super) fn dates(
    package: &Package,
    calendar_name: &str,
    field: &str,
    exceptions: &str,
) -> Result<TargetSide, PackageError> {
    let calendar = package
        .parquet(calendar_name)?
        .read(calendar_name, &[field, VALID_FROM, VALID_TO, WEEKDAY_MASK])?;
    let changes = package
        .parquet(exceptions)?
        .read(exceptions, &[field, SERVICE_DATE, ADDED])?;
    let services = typed(package, &calendar, field, Column::as_str)?;
    let from = typed(package, &calendar, VALID_FROM, Column::as_date)?;
    let to = typed(package, &calendar, VALID_TO, Column::as_date)?;
    let mask = typed(package, &calendar, WEEKDAY_MASK, Column::as_int16)?;
    let exception_services = typed(package, &changes, field, Column::as_str)?;
    let day = typed(package, &changes, SERVICE_DATE, Column::as_date)?;
    let added = typed(package, &changes, ADDED, Column::as_bool)?;

    let mut index: HashMap<Box<str>, u32> = HashMap::new();
    let mut ids: Vec<Box<str>> = Vec::new();
    let mut sets: Vec<Vec<i32>> = Vec::new();
    let mut removed: Vec<Vec<i32>> = Vec::new();
    let mut intern = |id: &str, sets: &mut Vec<Vec<i32>>, removed: &mut Vec<Vec<i32>>| {
        if let Some(&code) = index.get(id) {
            return code as usize;
        }
        let code = ids.len();
        ids.push(id.into());
        index.insert(id.into(), code as u32);
        sets.push(Vec::new());
        removed.push(Vec::new());
        code
    };
    for row in 0..calendar.rows() {
        let Some(id) = services.get(row) else {
            continue;
        };
        let code = intern(id, &mut sets, &mut removed);
        if let (Some(from), Some(to), Some(mask)) = (from[row], to[row], mask[row]) {
            for day in dates::to_days(from)..=dates::to_days(to) {
                // 1970-01-01, day 0, was a Thursday: Monday-based weekday 3.
                if mask >> (day + 3).rem_euclid(7) & 1 == 1 {
                    sets[code].push(day);
                }
            }
        }
    }
    for row in 0..changes.rows() {
        let (Some(id), Some(date), Some(added)) =
            (exception_services.get(row), day[row], added[row])
        else {
            continue;
        };
        let code = intern(id, &mut sets, &mut removed);
        let list = if added { &mut sets } else { &mut removed };
        list[code].push(dates::to_days(date));
    }
    sets.par_iter_mut()
        .zip(removed)
        .for_each(|(set, mut removed)| {
            set.sort_unstable();
            set.dedup();
            removed.sort_unstable();
            set.retain(|day| removed.binary_search(day).is_err());
        });
    Ok(TargetSide {
        index,
        ids,
        hashes: Vec::new(),
        body: Body::Dates(sets),
    })
}

/// A relation's content columns (every field but the identifier), plus, for a parent, its
/// children's content columns in key order.
pub(super) fn rows(
    package: &Package,
    spec: &RelationSpec,
    field: &str,
    child: Option<&RelationSpec>,
) -> Result<TargetSide, PackageError> {
    let mut columns: Vec<&str> = vec![field];
    columns.extend(
        spec.fields
            .iter()
            .map(|f| f.name.as_str())
            .filter(|name| *name != field),
    );
    let mut own = package.parquet(&spec.name)?.read(&spec.name, &columns)?;
    let id_values = typed(package, &own, field, Column::as_str)?.clone();
    own.columns.remove(0);
    let mut index: HashMap<Box<str>, u32> = HashMap::new();
    let mut ids: Vec<Box<str>> = Vec::new();
    let mut keep: Vec<u32> = Vec::new();
    for row in 0..id_values.len() {
        if let Some(id) = id_values.get(row)
            && !index.contains_key(id)
        {
            index.insert(id.into(), ids.len() as u32);
            ids.push(id.into());
            keep.push(row as u32);
        }
    }
    if keep.len() != id_values.len() {
        own = select_rows(&own, &keep);
    }
    let children = child
        .map(|child| children(package, spec, field, child, &index, ids.len()))
        .transpose()?;
    Ok(TargetSide {
        index,
        ids,
        hashes: Vec::new(),
        body: Body::Rows(Rows { own, children }),
    })
}

fn children(
    package: &Package,
    parent: &RelationSpec,
    field: &str,
    child: &RelationSpec,
    index: &HashMap<Box<str>, u32>,
    contents: usize,
) -> Result<Children, PackageError> {
    let link = child
        .foreign_keys
        .iter()
        .find(|key| key.relation == parent.name && key.target_fields == [field])
        .and_then(|key| key.fields.first())
        .ok_or_else(|| PackageError::MissingColumn {
            path: package.relation_path(&child.name),
            column: format!("reference to {}.{field}", parent.name),
        })?;
    let order: Vec<&str> = child
        .primary_key
        .iter()
        .map(String::as_str)
        .filter(|name| name != link)
        .collect();
    let content: Vec<&str> = child
        .fields
        .iter()
        .map(|f| f.name.as_str())
        .filter(|name| name != link && !order.contains(name))
        .collect();
    let mut columns = vec![link.as_str()];
    columns.extend(&order);
    columns.extend(&content);
    let mut table = package.parquet(&child.name)?.read(&child.name, &columns)?;
    let links = typed(package, &table, link, Column::as_str)?;
    let parents: Vec<Option<u32>> = links
        .pool()
        .iter()
        .map(|id| index.get(&**id).copied())
        .collect();
    let parent_of = |row: usize| links.code(row).and_then(|code| parents[code as usize]);
    let mut rows: Vec<u32> = (0..table.rows() as u32)
        .filter(|&row| parent_of(row as usize).is_some())
        .collect();
    let keys: Vec<&Column> = table.columns[1..=order.len()]
        .iter()
        .map(|(_, column)| column)
        .collect();
    rows.par_sort_unstable_by(|&x, &y| {
        let (x, y) = (x as usize, y as usize);
        parent_of(x).cmp(&parent_of(y)).then_with(|| {
            keys.iter()
                .map(|column| cmp_rows(column, x, y))
                .find(|ordering| ordering.is_ne())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    let mut ranges = vec![(0_u32, 0_u32); contents];
    let mut start = 0;
    while start < rows.len() {
        let parent = parent_of(rows[start] as usize).unwrap_or(0);
        let mut end = start;
        while end < rows.len() && parent_of(rows[end] as usize) == Some(parent) {
            end += 1;
        }
        ranges[parent as usize] = (start as u32, end as u32);
        start = end;
    }
    // Keep only the content columns; the link and the key have done their work.
    table.columns.drain(..=order.len());
    Ok(Children {
        table,
        order: rows,
        ranges,
    })
}

/// The given rows of a table, in that order.
fn select_rows(table: &Table, rows: &[u32]) -> Table {
    let pick = |column: &Column| -> Column {
        let rows = rows.iter().map(|&row| row as usize);
        match column {
            Column::Str(values) => Column::Str(rows.map(|row| values.get(row)).collect()),
            Column::Int16(values) => Column::Int16(rows.map(|row| values[row]).collect()),
            Column::Int32(values) => Column::Int32(rows.map(|row| values[row]).collect()),
            Column::Double(values) => Column::Double(rows.map(|row| values[row]).collect()),
            Column::Bool(values) => Column::Bool(rows.map(|row| values[row]).collect()),
            Column::Date(values) => Column::Date(rows.map(|row| values[row]).collect()),
        }
    };
    Table {
        name: table.name.clone(),
        columns: table
            .columns
            .iter()
            .map(|(name, column)| (name.clone(), pick(column)))
            .collect(),
    }
}
