//! Columns into a contract relation: contract column order and types, required columns
//! without nulls, rows sorted by primary key, and no two rows with one key.

use std::cmp::Ordering;

use jiff::civil::Date;
use rayon::prelude::*;

use super::PublishError;
use crate::contract::{RelationSpec, contract};
use crate::table::{Column, StrColumn, Table};

/// Columns by name, in any order.
pub type Columns = Vec<(&'static str, Column)>;

pub fn text<'s>(values: impl IntoIterator<Item = Option<&'s str>>) -> Column {
    Column::Str(values.into_iter().collect::<StrColumn>())
}

pub fn int16(values: impl IntoIterator<Item = Option<i16>>) -> Column {
    Column::Int16(values.into_iter().collect())
}

pub fn int32(values: impl IntoIterator<Item = Option<i32>>) -> Column {
    Column::Int32(values.into_iter().collect())
}

pub fn double(values: impl IntoIterator<Item = Option<f64>>) -> Column {
    Column::Double(values.into_iter().collect())
}

pub fn boolean(values: impl IntoIterator<Item = Option<bool>>) -> Column {
    Column::Bool(values.into_iter().collect())
}

pub fn date(values: impl IntoIterator<Item = Option<Date>>) -> Column {
    Column::Date(values.into_iter().collect())
}

/// Builds one column.
pub type ColumnBuilder<'a> = Box<dyn Fn() -> Column + Send + Sync + 'a>;

/// Builds the columns in parallel, one task per column.
pub fn parallel(builders: Vec<(&'static str, ColumnBuilder)>) -> Columns {
    builders
        .into_par_iter()
        .map(|(name, build)| (name, build()))
        .collect()
}

pub fn spec(name: &str) -> Result<&'static RelationSpec, PublishError> {
    contract()
        .relation(name)
        .ok_or_else(|| PublishError::Contract(format!("no relation {name}")))
}

/// A relation with no rows.
pub fn empty(name: &str) -> Result<Table, PublishError> {
    let spec = spec(name)?;
    let columns = spec
        .fields
        .iter()
        .map(|field| {
            let column = match field.ty {
                crate::FieldType::String => Column::Str(StrColumn::default()),
                crate::FieldType::Int16 => Column::Int16(Vec::new()),
                crate::FieldType::Int32 => Column::Int32(Vec::new()),
                crate::FieldType::Double => Column::Double(Vec::new()),
                crate::FieldType::Bool => Column::Bool(Vec::new()),
                crate::FieldType::Date32 => Column::Date(Vec::new()),
            };
            (field.name.clone(), column)
        })
        .collect();
    Ok(Table {
        name: name.to_owned(),
        columns,
    })
}

/// The relation `name` from its columns: contract order, checked, sorted by primary key.
pub fn finish(name: &str, mut columns: Columns) -> Result<Table, PublishError> {
    let spec = spec(name)?;
    let mismatch = |what: String| Err(PublishError::Contract(format!("{name}: {what}")));
    let mut ordered = Vec::with_capacity(spec.fields.len());
    for field in &spec.fields {
        let Some(position) = columns.iter().position(|(n, _)| *n == field.name) else {
            return mismatch(format!("column {} is not built", field.name));
        };
        let (_, column) = columns.swap_remove(position);
        if column.field_type() != field.ty {
            return mismatch(format!("column {} is {}", field.name, column.field_type()));
        }
        if !field.nullable
            && let Some(row) = (0..column.len()).find(|&row| column.is_null(row))
        {
            return Err(PublishError::NullInRequired {
                relation: name.to_owned(),
                field: field.name.clone(),
                row,
            });
        }
        ordered.push((field.name.clone(), column));
    }
    if let Some((extra, _)) = columns.first() {
        return mismatch(format!("column {extra} is not in the contract"));
    }
    let rows = ordered.first().map_or(0, |(_, column)| column.len());
    if ordered.iter().any(|(_, column)| column.len() != rows) {
        return mismatch("columns differ in length".to_owned());
    }
    let table = Table {
        name: name.to_owned(),
        columns: ordered,
    };
    sort_unique(table, spec)
}

/// One primary-key column, comparable by row.
enum KeyColumn<'a> {
    /// Rank of each pool value in byte order; null ranks first.
    Str(&'a StrColumn, Vec<u32>),
    Int16(&'a [Option<i16>]),
    Int32(&'a [Option<i32>]),
    Double(&'a [Option<f64>]),
    Bool(&'a [Option<bool>]),
    Date(&'a [Option<Date>]),
}

impl KeyColumn<'_> {
    fn new(column: &Column) -> KeyColumn<'_> {
        match column {
            Column::Str(values) => {
                let pool = values.pool();
                let mut order: Vec<u32> = (0..pool.len() as u32).collect();
                order.par_sort_unstable_by(|&a, &b| pool[a as usize].cmp(&pool[b as usize]));
                let mut ranks = vec![0; pool.len()];
                for (rank, code) in order.into_iter().enumerate() {
                    ranks[code as usize] = rank as u32 + 1;
                }
                KeyColumn::Str(values, ranks)
            }
            Column::Int16(values) => KeyColumn::Int16(values),
            Column::Int32(values) => KeyColumn::Int32(values),
            Column::Double(values) => KeyColumn::Double(values),
            Column::Bool(values) => KeyColumn::Bool(values),
            Column::Date(values) => KeyColumn::Date(values),
        }
    }

    fn cmp(&self, a: usize, b: usize) -> Ordering {
        match self {
            KeyColumn::Str(values, ranks) => {
                let rank = |row| values.code(row).map_or(0, |code| ranks[code as usize]);
                rank(a).cmp(&rank(b))
            }
            KeyColumn::Int16(values) => values[a].cmp(&values[b]),
            KeyColumn::Int32(values) => values[a].cmp(&values[b]),
            KeyColumn::Double(values) => match (values[a], values[b]) {
                (Some(x), Some(y)) => x.total_cmp(&y),
                (x, y) => x.is_some().cmp(&y.is_some()),
            },
            KeyColumn::Bool(values) => values[a].cmp(&values[b]),
            KeyColumn::Date(values) => values[a].cmp(&values[b]),
        }
    }
}

fn sort_unique(table: Table, spec: &RelationSpec) -> Result<Table, PublishError> {
    let key_columns: Vec<&Column> = spec
        .primary_key
        .iter()
        .filter_map(|name| table.column(name))
        .collect();
    let keys: Vec<KeyColumn> = key_columns.iter().map(|c| KeyColumn::new(c)).collect();
    let cmp = |a: usize, b: usize| {
        keys.iter()
            .map(|key| key.cmp(a, b))
            .find(|ordering| ordering.is_ne())
            .unwrap_or(Ordering::Equal)
    };
    let rows = table.rows();
    let sorted = (1..rows).all(|row| cmp(row - 1, row).is_le());
    let order: Option<Vec<usize>> = (!sorted).then(|| {
        let mut order: Vec<usize> = (0..rows).collect();
        order.par_sort_by(|&a, &b| cmp(a, b));
        order
    });
    let at = |position: usize| order.as_ref().map_or(position, |order| order[position]);
    if let Some(position) = (1..rows).find(|&p| cmp(at(p - 1), at(p)).is_eq()) {
        let row = at(position);
        let key = key_columns
            .iter()
            .map(|column| column.display(row))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(PublishError::DuplicateKey {
            relation: table.name.clone(),
            key,
        });
    }
    drop(keys);
    let Some(order) = order else {
        return Ok(table);
    };
    let columns = table
        .columns
        .into_par_iter()
        .map(|(name, column)| (name, permute(&column, &order)))
        .collect();
    Ok(Table {
        name: table.name,
        columns,
    })
}

fn permute(column: &Column, order: &[usize]) -> Column {
    fn pick<T: Copy>(values: &[T], order: &[usize]) -> Vec<T> {
        order.iter().map(|&row| values[row]).collect()
    }
    match column {
        Column::Str(values) => text(order.iter().map(|&row| values.get(row))),
        Column::Int16(values) => Column::Int16(pick(values, order)),
        Column::Int32(values) => Column::Int32(pick(values, order)),
        Column::Double(values) => Column::Double(pick(values, order)),
        Column::Bool(values) => Column::Bool(pick(values, order)),
        Column::Date(values) => Column::Date(pick(values, order)),
    }
}
