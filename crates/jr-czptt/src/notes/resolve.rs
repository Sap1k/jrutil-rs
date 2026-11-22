//! The one interval resolver (R071, Q3), used by notes and IDS zones.

use jr_model::{Diagnostic, Diagnostics, Rule, Severity};

use crate::path::Call;

/// Notes and IDS entries name their first and last point as a location code (country-prefixed
/// in every real message) and an occurrence for points the path visits twice. A blank
/// occurrence is the first match, for a start and for an end alike (a station written as two
/// consecutive rows ends a note at its first row); only when an end's first match lies before
/// the start is it the first match at or after the start. No real message gives an
/// occurrence, so one that appears is resolved 1-based and diagnosed.
pub(crate) const R071: Rule = Rule {
    id: "CZPTT-R071",
    summary: "An interval endpoint is the first matching call (for an end, not before the start), or the n-th",
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum End {
    Start,
    /// The end of an interval whose start resolved to the given source index, if it did.
    End(Option<u32>),
}

/// The source index of the call an endpoint names, or `None`.
pub(crate) fn resolve(
    pa: &str,
    calls: &[Call],
    code: &str,
    occurrence: &str,
    end: End,
    diagnostics: &mut Diagnostics,
) -> Option<u32> {
    let code = code.trim();
    let (country, code) = match code.get(..2) {
        Some(prefix) if code.len() > 2 && prefix.chars().all(|c| c.is_ascii_alphabetic()) => {
            (Some(prefix), &code[2..])
        }
        _ => (None, code),
    };
    let matches = calls.iter().filter(|call| {
        &*call.code == code
            && country.is_none_or(|country| call.country.eq_ignore_ascii_case(country))
    });
    let occurrence = occurrence.trim();
    if occurrence.is_empty() {
        return match end {
            End::Start => matches.map(|call| call.index).next(),
            End::End(start) => {
                let indices: Vec<u32> = matches.map(|call| call.index).collect();
                match (indices.first(), start) {
                    (Some(&first), Some(start)) if first < start => {
                        indices.into_iter().find(|index| *index >= start)
                    }
                    (first, _) => first.copied(),
                }
            }
        };
    }
    let nth: usize = occurrence
        .bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| occurrence.parse().ok())
        .flatten()?;
    diagnostics.push(
        Diagnostic::new(
            "czptt_explicit_occurrence",
            Severity::Info,
            pa,
            format!("endpoint {code} names occurrence {occurrence}, read 1-based"),
        )
        .rule(R071),
    );
    nth.checked_sub(1)
        .and_then(|position| matches.map(|call| call.index).nth(position))
}
