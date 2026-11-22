//! Rules and diagnostics.
//!
//! A [`Rule`] is the declaration of one named special case (AGENTS.md rule 4). A [`Diagnostic`]
//! is what a compiler or pass reports when something in the data needed a decision: a repaired
//! time, a dropped train, an ambiguous match left unresolved.

use std::borrow::Cow;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One named special case. Declared as a constant next to the code that implements it; every
/// crate lists its rules in a `RULES` slice so the ledger test can check them against
/// `docs/rules/`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Rule {
    /// `AREA-Rnnn`, for example `CZPTT-R007`.
    pub id: &'static str,
    pub summary: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// A rule fired as designed (a repair, a carried-forward value).
    Info,
    /// Source data was dropped, quarantined or left unresolved.
    Warning,
    /// An entity could not be compiled at all.
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Stable snake_case code; diagnostics are counted and sampled by it.
    pub code: Cow<'static, str>,
    /// What it is about: a source object ID or a public ID.
    pub subject: String,
    pub message: String,
    pub severity: Severity,
    /// The rule that fired, if one did.
    pub rule: Option<Cow<'static, str>>,
}

impl Diagnostic {
    pub fn new(
        code: &'static str,
        severity: Severity,
        subject: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: Cow::Borrowed(code),
            subject: subject.into(),
            message: message.into(),
            severity,
            rule: None,
        }
    }

    pub fn rule(mut self, rule: Rule) -> Self {
        self.rule = Some(Cow::Borrowed(rule.id));
        self
    }
}

/// Diagnostics in the order they were reported. Workers report into their own sink and the
/// sinks are merged in input order, so the order never depends on scheduling; published output
/// is sorted anyway ([`Diagnostics::into_sorted`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.items.push(diagnostic);
    }

    pub fn append(&mut self, mut other: Self) {
        self.items.append(&mut other.items);
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &Diagnostic> {
        self.items.iter()
    }

    pub fn counts_by_code(&self) -> BTreeMap<&str, usize> {
        let mut counts = BTreeMap::new();
        for item in &self.items {
            *counts.entry(item.code.as_ref()).or_default() += 1;
        }
        counts
    }

    pub fn into_sorted(mut self) -> Vec<Diagnostic> {
        self.items.sort();
        self.items
    }
}

impl Extend<Diagnostic> for Diagnostics {
    fn extend<It: IntoIterator<Item = Diagnostic>>(&mut self, iter: It) {
        self.items.extend(iter);
    }
}
