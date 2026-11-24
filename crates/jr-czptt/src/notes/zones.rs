//! IDS fare zone memberships of calls (R085, R086, R119).

use std::collections::BTreeMap;
use std::ops::RangeInclusive;

use jr_formats::kadr::{Catalog, IdsSystem};
use jr_model::timetable::Zone;
use jr_model::{DateSet, Diagnostic, Diagnostics, Horizon, Rule, Severity};

use super::resolve::{End, resolve};
use crate::assemble::valid_at;
use crate::path::Path;
use crate::policy::Policy;

/// `CZIPTS` names the IDS a stretch of the path belongs to; the catalogue note of an IDS entry
/// names its fare band ("PID pásmo P"). Only entries naming a band give zones. A membership
/// naming a calendar holds on that calendar's days only (R119).
pub(crate) const R085: Rule = Rule {
    id: "CZPTT-R085",
    summary: "A CZIPTS entry whose catalogue note names a fare band puts its passenger calls in that zone",
};

/// A `CZIPTS` value's sixth field names a `CZCalendarIPTS` value (`id|from|to|bitmap|`, the
/// bitmap counting days from `from`): the membership holds on the bitmap's days only. A
/// calendar that cannot be read or found is diagnosed and the membership holds every day.
///
/// SŽ limits an IDS membership to the days the IDS contract covers (B17 fixed,
/// `docs/roadmap.md`); 90 change messages of GVD 2026 do.
pub(crate) const R119: Rule = Rule {
    id: "CZPTT-R119",
    summary: "A CZIPTS membership with a CZCalendarIPTS holds on that calendar's days only",
};

/// One IDS membership of a path (R085, R119).
pub(crate) struct Membership<'a> {
    /// The `CZIPTS` value as written: what names a variant of the path's trips (R120).
    pub value: &'a str,
    /// The catalogue code: the order of a call's zones.
    code: &'a str,
    zone: Zone,
    /// The source indexes of the calls it covers.
    calls: RangeInclusive<u32>,
    /// The days it holds on; `None`: every day.
    pub days: Option<DateSet>,
}

/// R119: the `CZCalendarIPTS` days named `id`.
fn calendar_days(
    path: &Path,
    id: &str,
    horizon: Horizon,
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> Option<DateSet> {
    let separator = policy.notes.field_separator;
    let found = path.parameters.iter().find(|parameter| {
        parameter.name == policy.zones.calendar_parameter
            && parameter.value.split(separator).next() == Some(id)
    });
    let report = |diagnostics: &mut Diagnostics, message: String| {
        diagnostics.push(
            Diagnostic::new(
                "czptt_ids_calendar_unread",
                Severity::Warning,
                &*path.pa,
                message,
            )
            .rule(R119),
        );
    };
    let Some(found) = found else {
        report(
            diagnostics,
            format!(
                "no {} {id}: the membership holds every day",
                policy.zones.calendar_parameter
            ),
        );
        return None;
    };
    let fields: Vec<&str> = found.value.split(separator).collect();
    let date = |text: &str| jiff::civil::Date::strptime("%Y%m%d", text.trim()).ok();
    let (Some(from), Some(bitmap)) = (fields.get(1).and_then(|f| date(f)), fields.get(3)) else {
        report(
            diagnostics,
            format!(
                "{:?} cannot be read: the membership holds every day",
                found.value
            ),
        );
        return None;
    };
    let to = fields.get(2).and_then(|f| date(f));
    let mut days = DateSet::EMPTY;
    let mut day = from;
    for bit in bitmap.trim().chars() {
        if to.is_some_and(|to| day > to) {
            break;
        }
        if bit == '1'
            && let Some(index) = horizon.index_of(day)
        {
            days.insert(index);
        }
        let Ok(next) = day.tomorrow() else { break };
        day = next;
    }
    Some(days)
}

/// KADR abbreviations are `<system>_<area>` (`PID_PrahaP`).
pub(crate) const R086: Rule = Rule {
    id: "CZPTT-R086",
    summary: "The IDS system is the catalogue abbreviation up to its first separator",
};

/// `… pásmo <band>`: the text after the first whole-word, case-insensitive `word` followed by
/// whitespace, trimmed; `None` when there is no such text.
fn fare_band<'a>(note: &'a str, word: &str) -> Option<&'a str> {
    let lowered_word: Vec<char> = word.chars().flat_map(char::to_lowercase).collect();
    let chars: Vec<(usize, char)> = note.char_indices().collect();
    for start in 0..chars.len() {
        if start > 0 && chars[start - 1].1.is_alphanumeric() {
            continue;
        }
        let mut position = start;
        let matched = lowered_word.iter().all(|expected| {
            let found = chars
                .get(position)
                .is_some_and(|(_, c)| c.to_lowercase().eq(std::iter::once(*expected)));
            position += 1;
            found
        });
        if !matched || !chars.get(position).is_some_and(|(_, c)| c.is_whitespace()) {
            continue;
        }
        let band = note[chars[position].0..].trim();
        if !band.is_empty() {
            return Some(band);
        }
    }
    None
}

/// R086.
fn system(record: &IdsSystem, policy: &Policy) -> Box<str> {
    let abbreviation = record.abbreviation.as_deref().unwrap_or(&record.code);
    match abbreviation.split_once(policy.zones.system_separator) {
        Some((system, _)) => system.into(),
        None => abbreviation.into(),
    }
}

/// The path's IDS memberships that give zones (R085, R119).
pub(crate) fn memberships<'p>(
    path: &'p Path,
    catalog: &Catalog,
    horizon: Horizon,
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> Vec<Membership<'p>> {
    let pa = &*path.pa;
    let separator = policy.notes.field_separator;
    let mut found = Vec::new();
    for parameter in &path.parameters {
        if parameter.name != policy.zones.parameter {
            continue;
        }
        let value = &*parameter.value;
        let fields: Vec<&str> = value.split(separator).collect();
        if fields.len() < 5 {
            diagnostics.push(
                Diagnostic::new(
                    "czptt_ids_malformed",
                    Severity::Warning,
                    pa,
                    format!(
                        "{} {value:?} has fewer than 5 fields",
                        policy.zones.parameter
                    ),
                )
                .rule(R085),
            );
            continue;
        }
        let first = resolve(&path.calls, fields[1], fields[2], End::Start);
        let last = resolve(&path.calls, fields[3], fields[4], End::End(first));
        let record = valid_at(&catalog.ids, fields[0], path.first_date, |record| {
            (&record.code, record.valid_from, record.valid_to)
        });
        let (Some(first), Some(last), Some(record)) = (first, last, record) else {
            diagnostics.push(
                Diagnostic::new(
                    "czptt_ids_unresolved",
                    Severity::Warning,
                    pa,
                    format!(
                        "{} {value:?}: endpoints or a valid catalogue entry are missing",
                        policy.zones.parameter
                    ),
                )
                .rule(R085),
            );
            continue;
        };
        if first > last {
            diagnostics.push(
                Diagnostic::new(
                    "czptt_ids_unresolved",
                    Severity::Warning,
                    pa,
                    format!(
                        "{} {value:?}: the interval is reversed",
                        policy.zones.parameter
                    ),
                )
                .rule(R085),
            );
            continue;
        }
        let Some(band) = record
            .note
            .as_deref()
            .and_then(|note| fare_band(note, &policy.zones.fare_band_word))
        else {
            continue;
        };
        // R119.
        let days = fields
            .get(5)
            .map(|id| id.trim())
            .filter(|id| !id.is_empty())
            .and_then(|id| calendar_days(path, id, horizon, policy, diagnostics));
        found.push(Membership {
            value,
            code: fields[0],
            zone: Zone {
                code: band.into(),
                system: Some(system(record, policy)),
            },
            calls: first..=last,
            days,
        });
    }
    found
}

/// The zones of each passenger call of the path's parts, by source index, from the memberships
/// `holds` keeps. A call's zones are ordered by the catalogue code of the IDS entry that gives
/// them, not by the order a message happens to list its `CZIPTS` values in: two paths calling
/// at one stop then agree on the list, so the stop's zones can be stated once for the route
/// (serving 7.5).
pub(crate) fn zones(
    path: &Path,
    memberships: &[Membership<'_>],
    holds: impl Fn(usize) -> bool,
) -> BTreeMap<u32, Vec<Zone>> {
    let mut members: BTreeMap<u32, Vec<(&str, Zone)>> = BTreeMap::new();
    for (index, membership) in memberships.iter().enumerate() {
        if !holds(index) {
            continue;
        }
        for part in &path.parts {
            for call in part.passenger_calls() {
                if membership.calls.contains(&call.index) {
                    members
                        .entry(call.index)
                        .or_default()
                        .push((membership.code, membership.zone.clone()));
                }
            }
        }
    }
    members
        .into_iter()
        .map(|(index, mut entries)| {
            entries.sort_by_key(|(code, _)| *code);
            let mut zones: Vec<Zone> = Vec::with_capacity(entries.len());
            for (_, zone) in entries {
                if !zones.contains(&zone) {
                    zones.push(zone);
                }
            }
            (index, zones)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::fare_band;

    #[test]
    fn r085_the_fare_band_follows_the_whole_word() {
        assert_eq!(fare_band("PID pásmo P", "pásmo"), Some("P"));
        assert_eq!(fare_band("IDOL PÁSMO  12 ", "pásmo"), Some("12"));
        assert_eq!(fare_band("předpásmo X", "pásmo"), None);
        assert_eq!(fare_band("pásmo", "pásmo"), None);
    }
}
