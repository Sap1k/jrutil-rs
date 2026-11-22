//! The string interner for the hot tables (trips and calls).
//!
//! Small tables (places, routes, agencies) hold their strings directly; a trip's headsign or a
//! call's line code is a [`Sym`], so a million trips share a few thousand strings.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::handle::{Handle, Sym};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(from = "Vec<Box<str>>", into = "Vec<Box<str>>")]
pub struct Strings {
    values: Vec<Box<str>>,
    /// Looked up, never iterated.
    index: HashMap<Box<str>, Sym>,
}

impl Strings {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern(&mut self, value: &str) -> Sym {
        if let Some(&sym) = self.index.get(value) {
            return sym;
        }
        let sym = Sym::from_index(self.values.len());
        self.values.push(value.into());
        self.index.insert(value.into(), sym);
        sym
    }

    /// `None` for an empty or absent value, so "no headsign" has one representation.
    pub fn intern_opt(&mut self, value: Option<&str>) -> Option<Sym> {
        value
            .filter(|text| !text.is_empty())
            .map(|text| self.intern(text))
    }

    pub fn find(&self, value: &str) -> Option<Sym> {
        self.index.get(value).copied()
    }

    pub fn get(&self, sym: Sym) -> &str {
        &self.values[sym.index()]
    }

    pub fn get_opt(&self, sym: Option<Sym>) -> Option<&str> {
        sym.map(|sym| self.get(sym))
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

impl From<Vec<Box<str>>> for Strings {
    fn from(values: Vec<Box<str>>) -> Self {
        let index = values
            .iter()
            .enumerate()
            .map(|(position, value)| (value.clone(), Sym::from_index(position)))
            .collect();
        Self { values, index }
    }
}

impl From<Strings> for Vec<Box<str>> {
    fn from(strings: Strings) -> Self {
        strings.values
    }
}

impl PartialEq for Strings {
    fn eq(&self, other: &Self) -> bool {
        self.values == other.values
    }
}
