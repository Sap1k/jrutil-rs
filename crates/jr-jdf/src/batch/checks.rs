//! The code checks of a batch (R025, R026, R283).

use std::collections::{BTreeMap, BTreeSet};

use jr_formats::jdf::{NoteType, RouteType, TransportMode};
use jr_model::{Diagnostic, Diagnostics, Severity};

use super::{Batch, R025, R026, R283};
use crate::policy::Policy;

/// Whether an all-digit code names a day of the week (R025, R263).
pub(crate) fn day_number(code: &str, policy: &Policy) -> Option<usize> {
    let number: u32 = code.parse().ok()?;
    policy
        .calendar
        .days_of_week
        .iter()
        .position(|&day| day == number)
}

pub(crate) fn is_digits(code: &str) -> bool {
    !code.is_empty() && code.bytes().all(|b| b.is_ascii_digit())
}

pub(super) fn check_fixed_codes(batch: &Batch, policy: &Policy, diagnostics: &mut Diagnostics) {
    let warn = |code, message: String| {
        Diagnostic::new(code, Severity::Warning, &*batch.name, message).rule(R025)
    };
    let mut seen = BTreeMap::new();
    let mut unknown = BTreeMap::<&str, usize>::new();
    let mut bad_days = BTreeMap::<&str, usize>::new();
    for (position, attribute) in batch.attribute_refs.iter().enumerate() {
        let code = &*attribute.code;
        let first = *seen.entry(attribute.id).or_insert(position);
        if first != position {
            let first = &*batch.attribute_refs[first].code;
            diagnostics.push(warn(
                "jdf_fixed_code_duplicate",
                format!(
                    "Pevnykod {} is written twice ({first:?}, {code:?}): the first is used",
                    attribute.id
                ),
            ));
        }
        if is_digits(code) {
            if day_number(code, policy).is_none() {
                *bad_days.entry(code).or_default() += 1;
            }
        } else if !policy
            .fixed_codes
            .known
            .iter()
            .any(|known| &**known == code)
        {
            *unknown.entry(code).or_default() += 1;
        }
    }
    for (code, count) in unknown {
        diagnostics.push(warn(
            "jdf_fixed_code_unknown",
            format!("Pevnykod code {code:?} ({count} rows) is not a JDF fixed code"),
        ));
    }
    for (code, count) in bad_days {
        diagnostics.push(warn(
            "jdf_day_code_invalid",
            format!("Pevnykod code {code:?} ({count} rows) is no day of the week: matches no day"),
        ));
    }
}

pub(super) fn check_codes(batch: &Batch, diagnostics: &mut Diagnostics) {
    let mut unknown = BTreeMap::<(&str, &str), usize>::new();
    for route in &batch.routes {
        if let Some(RouteType::Other(code)) = &route.route_type {
            *unknown.entry(("route type", code)).or_default() += 1;
        }
        if let Some(TransportMode::Other(code)) = &route.transport_mode {
            *unknown.entry(("transport mode", code)).or_default() += 1;
        }
    }
    for note in &batch.service_notes {
        if let Some(NoteType::Other(code)) = &note.note_type {
            *unknown.entry(("Caskody type", code)).or_default() += 1;
        }
    }
    for ((field, code), count) in unknown {
        diagnostics.push(
            Diagnostic::new(
                "jdf_code_unknown",
                Severity::Warning,
                &*batch.name,
                format!("{field} {code:?} ({count} rows) is not a JDF code: matches nothing"),
            )
            .rule(R026),
        );
    }
}

pub(super) fn check_attribute_numbers(batch: &Batch, diagnostics: &mut Diagnostics) {
    let known: BTreeSet<u32> = batch.attribute_refs.iter().map(|a| a.id).collect();
    let mut dangling = BTreeMap::<u32, usize>::new();
    let slots = batch
        .stops
        .iter()
        .flat_map(|s| s.attributes.iter())
        .chain(batch.route_stops.iter().flat_map(|s| s.attributes.iter()))
        .chain(batch.trips.iter().flat_map(|t| t.attributes.iter()))
        .chain(batch.trip_stops.iter().flat_map(|c| c.attributes.iter()))
        .chain(
            batch
                .agency_alternations
                .iter()
                .flat_map(|a| a.attributes.iter()),
        );
    for &id in slots.flatten() {
        if !known.contains(&id) {
            *dangling.entry(id).or_default() += 1;
        }
    }
    for (id, count) in dangling {
        diagnostics.push(
            Diagnostic::new(
                "jdf_attribute_dangling",
                Severity::Warning,
                &*batch.name,
                format!("attribute number {id} ({count} slots) has no Pevnykod row: ignored"),
            )
            .rule(R283),
        );
    }
}
