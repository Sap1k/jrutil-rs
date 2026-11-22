//! A relation in memory: columnar, independent of arrow at the API surface.

use std::collections::HashMap;

use jiff::civil::Date;

use crate::contract::FieldType;

/// One relation (or a projection of its columns), in file column order.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub name: String,
    pub columns: Vec<(String, Column)>,
}

impl Table {
    /// Row count (0 for a table without columns).
    pub fn rows(&self) -> usize {
        self.columns.first().map_or(0, |(_, column)| column.len())
    }

    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns
            .iter()
            .find(|(column, _)| column == name)
            .map(|(_, column)| column)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Column {
    Str(StrColumn),
    Int16(Vec<Option<i16>>),
    Int32(Vec<Option<i32>>),
    Double(Vec<Option<f64>>),
    Bool(Vec<Option<bool>>),
    Date(Vec<Option<Date>>),
}

impl Column {
    pub fn len(&self) -> usize {
        match self {
            Column::Str(values) => values.len(),
            Column::Int16(values) => values.len(),
            Column::Int32(values) => values.len(),
            Column::Double(values) => values.len(),
            Column::Bool(values) => values.len(),
            Column::Date(values) => values.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn field_type(&self) -> FieldType {
        match self {
            Column::Str(_) => FieldType::String,
            Column::Int16(_) => FieldType::Int16,
            Column::Int32(_) => FieldType::Int32,
            Column::Double(_) => FieldType::Double,
            Column::Bool(_) => FieldType::Bool,
            Column::Date(_) => FieldType::Date32,
        }
    }

    pub fn as_str(&self) -> Option<&StrColumn> {
        match self {
            Column::Str(values) => Some(values),
            _ => None,
        }
    }

    pub fn as_int16(&self) -> Option<&[Option<i16>]> {
        match self {
            Column::Int16(values) => Some(values),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<&[Option<bool>]> {
        match self {
            Column::Bool(values) => Some(values),
            _ => None,
        }
    }

    pub fn as_date(&self) -> Option<&[Option<Date>]> {
        match self {
            Column::Date(values) => Some(values),
            _ => None,
        }
    }

    pub fn is_null(&self, row: usize) -> bool {
        match self {
            Column::Str(values) => values.code(row).is_none(),
            Column::Int16(values) => values[row].is_none(),
            Column::Int32(values) => values[row].is_none(),
            Column::Double(values) => values[row].is_none(),
            Column::Bool(values) => values[row].is_none(),
            Column::Date(values) => values[row].is_none(),
        }
    }

    /// The cell as text for messages: dates ISO, doubles round-trip, null as `null`.
    pub fn display(&self, row: usize) -> String {
        fn text<T: ToString>(value: Option<T>) -> String {
            value.map_or_else(|| "null".to_owned(), |value| value.to_string())
        }
        match self {
            Column::Str(values) => text(values.get(row)),
            Column::Int16(values) => text(values[row]),
            Column::Int32(values) => text(values[row]),
            Column::Double(values) => text(values[row]),
            Column::Bool(values) => text(values[row]),
            Column::Date(values) => text(values[row]),
        }
    }
}

/// Dictionary-coded strings: one code per row into a pool of distinct values.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StrColumn {
    codes: Vec<u32>,
    pool: Vec<Box<str>>,
}

/// Code of a null cell.
const NULL: u32 = u32::MAX;

impl StrColumn {
    pub fn builder() -> StrColumnBuilder {
        StrColumnBuilder::default()
    }

    pub fn get(&self, row: usize) -> Option<&str> {
        self.code(row).map(|code| &*self.pool[code as usize])
    }

    /// The pool index of a row's value, `None` for null.
    pub fn code(&self, row: usize) -> Option<u32> {
        let code = self.codes[row];
        (code != NULL).then_some(code)
    }

    /// Distinct values; codes index into it.
    pub fn pool(&self) -> &[Box<str>] {
        &self.pool
    }

    pub fn len(&self) -> usize {
        self.codes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.codes.is_empty()
    }
}

impl<'a> FromIterator<Option<&'a str>> for StrColumn {
    fn from_iter<I: IntoIterator<Item = Option<&'a str>>>(values: I) -> Self {
        let mut builder = StrColumn::builder();
        for value in values {
            builder.push(value);
        }
        builder.finish()
    }
}

/// Interns values as they are pushed, in first-seen order.
#[derive(Debug, Default)]
pub struct StrColumnBuilder {
    codes: Vec<u32>,
    pool: Vec<Box<str>>,
    index: HashMap<Box<str>, u32>,
}

impl StrColumnBuilder {
    pub fn reserve(&mut self, rows: usize) {
        self.codes.reserve(rows);
    }

    pub fn push(&mut self, value: Option<&str>) {
        let code = match value {
            None => NULL,
            Some(value) => self.intern(value),
        };
        self.codes.push(code);
    }

    fn intern(&mut self, value: &str) -> u32 {
        // Ids repeat in runs (the calls of one trip); skip the map for a repeat.
        if let Some(&last) = self.codes.last()
            && last != NULL
            && *self.pool[last as usize] == *value
        {
            return last;
        }
        if let Some(&code) = self.index.get(value) {
            return code;
        }
        let code = u32::try_from(self.pool.len())
            .ok()
            .filter(|&code| code != NULL)
            .unwrap_or_else(|| panic!("string pool exceeds {NULL} distinct values"));
        self.pool.push(value.into());
        self.index.insert(value.into(), code);
        code
    }

    pub fn finish(self) -> StrColumn {
        StrColumn {
            codes: self.codes,
            pool: self.pool,
        }
    }
}
