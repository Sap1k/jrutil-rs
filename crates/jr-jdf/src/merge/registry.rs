//! The stop registry as the merge uses it (JDF-R207–R210).

use std::collections::BTreeSet;

use jr_formats::geodata::registry;
use jr_model::{Coord, Diagnostic, Diagnostics, Rule, Severity};

use super::Policy;

/// The registry's numbers are the reviewed, published stop numbers: a row whose number is not
/// in `1..provisional_base` cannot be one. Such a row is dropped with an error diagnostic (F#
/// refused the whole registry); the reader has already checked each row's own cells.
pub const R207: Rule = Rule {
    id: "JDF-R207",
    summary: "Registry numbers lie below the provisional base; other rows are dropped",
};

/// A `merged_into:` note marks a duplicate number: its rows count for the surviving number.
/// The target must be a registered number that is not itself merged (chains are not followed);
/// otherwise the row is dropped with an error diagnostic.
pub const R208: Rule = Rule {
    id: "JDF-R208",
    summary: "Merged registry numbers resolve to the surviving number; chains are refused",
};

/// A retired row still matches, so a stop that comes back keeps its number (verdict Q4: not
/// treated specially).
pub const R209: Rule = Rule {
    id: "JDF-R209",
    summary: "Retired registry rows match like active ones",
};

/// Registered identities are known before any batch, so they resolve the same way whatever the
/// batch order: every row is an alias of its (resolved) number before the first batch, with its
/// reference as a precise position; the first reference of a number is the number's reference.
pub const R210: Rule = Rule {
    id: "JDF-R210",
    summary: "Registry rows are aliases of their numbers before the first batch",
};

/// One usable registry row.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Row {
    /// The resolved number (R208).
    pub number: u64,
    pub town: Box<str>,
    pub district: Option<Box<str>>,
    pub nearby_place: Option<Box<str>>,
    pub okres: Option<Box<str>>,
    pub country: Option<Box<str>>,
    pub reference: Option<Coord>,
}

/// The registry rows the merge matches against, in file order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Registry {
    pub(crate) rows: Vec<Row>,
}

impl Registry {
    pub fn empty() -> Registry {
        Registry::default()
    }

    /// R207–R209 over the rows of `stops.csv`.
    pub fn new(
        stops: &[registry::Stop],
        policy: &Policy,
        diagnostics: &mut Diagnostics,
    ) -> Registry {
        let base = policy.stops.provisional_base;
        let in_range = |id: i64| u64::try_from(id).is_ok_and(|id| id >= 1 && id < base);
        let numbers: BTreeSet<i64> = stops.iter().map(|s| s.id).collect();
        let merged: BTreeSet<i64> = stops
            .iter()
            .filter(|s| s.merged_into.is_some())
            .map(|s| s.id)
            .collect();
        let mut rows = Vec::with_capacity(stops.len());
        for stop in stops {
            let refuse = |rule, message: String| {
                Diagnostic::new(
                    "jdf_registry_row_invalid",
                    Severity::Error,
                    format!("stops.csv:{}", stop.line),
                    message,
                )
                .rule(rule)
            };
            if !in_range(stop.id) {
                diagnostics.push(refuse(
                    R207,
                    format!("id {} is not a registered number (1..{base})", stop.id),
                ));
                continue;
            }
            let number = match stop.merged_into {
                None => stop.id,
                Some(target) if !numbers.contains(&target) => {
                    diagnostics.push(refuse(
                        R208,
                        format!("merged_into:{target} is not a registered number"),
                    ));
                    continue;
                }
                Some(target) if merged.contains(&target) => {
                    diagnostics.push(refuse(
                        R208,
                        format!("merged_into:{target} is itself merged: chains are not followed"),
                    ));
                    continue;
                }
                Some(target) => target,
            };
            rows.push(Row {
                number: number as u64,
                town: stop.town.clone(),
                district: stop.district.clone(),
                nearby_place: stop.nearby_place.clone(),
                okres: stop.okres.clone(),
                country: stop.country.clone(),
                reference: stop.reference,
            });
        }
        Registry { rows }
    }
}
