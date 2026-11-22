//! 64-bit hashes of typed cells and key tuples. A value hashes the same whichever relation
//! (or GTFS text) it comes from, so keys compare across files without building strings.

use std::hash::{DefaultHasher, Hasher};

use crate::table::{Column, Table};

const TAG_NULL: u64 = 0x6e75_6c6c;
const TAG_INT: u64 = 0x1f83_d9ab_fb41_bd6b;
const TAG_DOUBLE: u64 = 0x5be0_cd19_137e_2179;
const TAG_BOOL: u64 = 0x9b05_688c_2b3e_6c1f;
const TAG_DATE: u64 = 0x510e_527f_ade6_82d1;

/// murmur3's 64-bit finaliser.
fn mix(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^ (x >> 33)
}

pub fn text(bytes: &[u8]) -> u64 {
    // SipHash with fixed keys: deterministic, run once per distinct string.
    let mut hasher = DefaultHasher::new();
    hasher.write(bytes);
    hasher.finish()
}

pub fn int(value: i64) -> u64 {
    mix(TAG_INT ^ value as u64)
}

pub fn null() -> u64 {
    mix(TAG_NULL)
}

pub fn date(year: i16, month: i8, day: i8) -> u64 {
    let packed = (i64::from(year) << 16) | (i64::from(month) << 8) | i64::from(day);
    mix(TAG_DATE ^ packed as u64)
}

/// Hash of a tuple of value hashes, order-sensitive.
pub fn combine(values: impl IntoIterator<Item = u64>) -> u64 {
    values.into_iter().fold(SEED, step)
}

const SEED: u64 = 0x243f_6a88_85a3_08d3;

fn step(hash: u64, value: u64) -> u64 {
    mix(hash.rotate_left(23) ^ value)
}

/// One column's value hashes, with string hashes computed once per pool entry.
pub struct ValueHashes<'a> {
    column: &'a Column,
    pool: Vec<u64>,
}

impl<'a> ValueHashes<'a> {
    pub fn new(column: &'a Column) -> ValueHashes<'a> {
        let pool = match column {
            Column::Str(values) => values
                .pool()
                .iter()
                .map(|value| text(value.as_bytes()))
                .collect(),
            _ => Vec::new(),
        };
        ValueHashes { column, pool }
    }

    /// `None` for null.
    pub fn get(&self, row: usize) -> Option<u64> {
        match self.column {
            Column::Str(values) => values.code(row).map(|code| self.pool[code as usize]),
            Column::Int16(values) => values[row].map(|value| int(value.into())),
            Column::Int32(values) => values[row].map(|value| int(value.into())),
            Column::Double(values) => values[row].map(|value| mix(TAG_DOUBLE ^ value.to_bits())),
            Column::Bool(values) => values[row].map(|value| mix(TAG_BOOL ^ u64::from(value))),
            Column::Date(values) => {
                values[row].map(|value| date(value.year(), value.month(), value.day()))
            }
        }
    }

    /// Like `get`, with null as a value of its own.
    pub fn get_or_null(&self, row: usize) -> u64 {
        self.get(row).unwrap_or_else(null)
    }
}

/// Key tuples of a table: hashing, exact comparison and display.
pub struct Keys<'a> {
    columns: Vec<ValueHashes<'a>>,
}

impl<'a> Keys<'a> {
    /// `None` when the table lacks one of the fields.
    pub fn new(table: &'a Table, fields: &[String]) -> Option<Keys<'a>> {
        let columns = fields
            .iter()
            .map(|field| table.column(field).map(ValueHashes::new))
            .collect::<Option<Vec<_>>>()?;
        Some(Keys { columns })
    }

    /// The key's hash; `None` when any part is null.
    pub fn hash(&self, row: usize) -> Option<u64> {
        let mut hash = SEED;
        for column in &self.columns {
            hash = step(hash, column.get(row)?);
        }
        Some(hash)
    }

    pub fn same(&self, left: usize, right: usize) -> bool {
        self.columns.iter().all(|column| match column.column {
            Column::Str(values) => values.code(left) == values.code(right),
            Column::Int16(values) => values[left] == values[right],
            Column::Int32(values) => values[left] == values[right],
            Column::Double(values) => {
                values[left].map(f64::to_bits) == values[right].map(f64::to_bits)
            }
            Column::Bool(values) => values[left] == values[right],
            Column::Date(values) => values[left] == values[right],
        })
    }

    pub fn display(&self, row: usize) -> String {
        let parts: Vec<String> = self
            .columns
            .iter()
            .map(|column| column.column.display(row))
            .collect();
        parts.join(", ")
    }
}
