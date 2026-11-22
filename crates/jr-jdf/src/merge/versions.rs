//! Line versions and their dates (JDF-R243–R249, R254).

use std::collections::{BTreeMap, HashMap};

use jr_model::{Date, DateSet, Diagnostic, Diagnostics, Horizon, Rule, Severity};
use rayon::prelude::*;
use sha2::{Digest, Sha256};

use super::{CodeSets, Version};
use crate::batch::day_number;
use crate::calendar::{CalendarNote, TripCalendarInput, running_days_in};
use crate::holidays::Holidays;
use crate::policy::NoteKind;

/// A version's day classes: bits 0–6 Monday–Sunday on ordinary days, bit 7 public holidays,
/// from its trips' day codes (`X`, `+`, a day number). A trip without day codes, or with a
/// runs-also or runs-only note, may run on any day. Two versions are side by side only when
/// their masks are provably disjoint.
pub const R243: Rule = Rule {
    id: "JDF-R243",
    summary: "A version's day-class mask is the union of its trips' day classes",
};

/// CIS publishes weekday and weekend timetables of one line side by side: versions of one
/// licence with the same validity, detour flag and batch date and disjoint masks form a unit,
/// first fit in batch order; only a unit's first version takes part in the per-date decision
/// and every member keeps the unit's dates.
pub const R244: Rule = Rule {
    id: "JDF-R244",
    summary: "Side-by-side versions with disjoint day classes share their dates",
};

/// A detour outranks other versions, except an open-ended detour against a version published
/// later with the same end: such a detour lasts only until the next version starts.
pub const R245: Rule = Rule {
    id: "JDF-R245",
    summary: "A detour has priority unless it ends with a later-starting version",
};

/// Priority: a detour with priority beats a version without it; between two of the same class
/// the newer batch wins (absent dates are oldest). Within one export the batch dates are equal.
pub const R246: Rule = Rule {
    id: "JDF-R246",
    summary: "Detour priority, then the newer batch",
};

/// For every interval between the units' validity boundaries the winner among the units valid
/// then is a left fold in unit order: priority, then the later start, the earlier end, then the
/// earlier version in batch-name order (and position).
pub const R247: Rule = Rule {
    id: "JDF-R247",
    summary: "Each date goes to one version of a licence by priority, start, end, batch order",
};

/// A version is never copied or cut: it keeps the dates it won, and each trip runs on its
/// calendar within each won interval (the interval being the validity, so undated notes behave
/// as on an F# cut version). A version that won nothing is dropped. Both are diagnosed.
pub const R248: Rule = Rule {
    id: "JDF-R248",
    summary: "Versions keep the dates they won; trips run on their calendar within them",
};

/// The GVD change is a hard cutover: a won interval ending before the reference date or the
/// year's start, or starting after its end, is dropped; the rest are clamped to the year. Dates
/// before the reference date inside a kept interval stay. A version left without dates is
/// dropped (expired or next year), a clamped one diagnosed.
pub const R249: Rule = Rule {
    id: "JDF-R249",
    summary: "Won intervals are bounded to the timetable year from the reference date",
};

/// The parts of a version's trip identity: licence, published start and detour flag; among
/// surviving versions of one start and flag, every published validity but the earliest-ending
/// adds eight hex characters of SHA-256 of `<yymmdd from>|<yymmdd to>` (the export date is no
/// longer hashed, B8), and side-by-side versions of identical validity after the first add
/// their rank in batch order (`part`, from 2).
pub const R254: Rule = Rule {
    id: "JDF-R254",
    summary: "Versions get key parts: start, detour, a validity hash and a side-by-side rank",
};

/// The published end as a comparable date: open is the latest.
fn end_of(version: &Version) -> Date {
    version.route.timetable_valid_to.unwrap_or(Date::MAX)
}

/// R243.
fn mask(version: &Version, code_sets: &CodeSets, jdf: &crate::Policy) -> u8 {
    let calendar = &jdf.calendar;
    let dated: std::collections::HashSet<u64> = version
        .notes
        .iter()
        .filter(|note| {
            note.note_type.as_ref().is_some_and(|t| {
                matches!(
                    calendar.note_types.get(t.code()),
                    Some(NoteKind::RunsAlso | NoteKind::RunsOnly)
                )
            })
        })
        .map(|note| note.trip_id)
        .collect();
    version.trips.iter().fold(0u8, |mask, trip| {
        let mut days = 0u8;
        let mut any = false;
        for code in code_sets.get(trip.codes) {
            let bits = if **code == *calendar.working_days {
                0b0001_1111
            } else if **code == *calendar.sundays_and_holidays {
                0b1100_0000
            } else if let Some(day) = day_number(code, jdf) {
                (1u8 << day) | 0b1000_0000
            } else {
                continue;
            };
            any = true;
            days |= bits;
        }
        let trip_mask = if dated.contains(&trip.id) || !any {
            0xFF
        } else {
            days
        };
        mask | trip_mask
    })
}

/// R245.
fn detour_priority(r: &Version, o: &Version) -> bool {
    r.route.detour
        && !(end_of(r) == end_of(o) && r.route.timetable_valid_from < o.route.timetable_valid_from)
}

/// R246.
fn has_priority(r: &Version, o: &Version) -> bool {
    let (rd, od) = (detour_priority(r, o), detour_priority(o, r));
    (rd && (!od || o.created < r.created)) || (!od && o.created < r.created)
}

/// R247; `ri`, `oi` are the versions' places in batch order.
fn beats(r: &Version, ri: usize, o: &Version, oi: usize) -> bool {
    if has_priority(r, o) {
        true
    } else if has_priority(o, r) {
        false
    } else if r.route.timetable_valid_from != o.route.timetable_valid_from {
        r.route.timetable_valid_from > o.route.timetable_valid_from
    } else if end_of(r) != end_of(o) {
        end_of(r) < end_of(o)
    } else {
        ri < oi
    }
}

/// R244–R247 for the versions of one licence (indexes in batch order); returns each version's
/// won intervals.
fn won_intervals(
    versions: &[Version],
    licence: &[usize],
    code_sets: &CodeSets,
    jdf: &crate::Policy,
) -> HashMap<usize, Vec<(Date, Date)>> {
    // R244: units.
    let mut units: Vec<(Vec<usize>, u8)> = Vec::new();
    for &index in licence {
        let version = &versions[index];
        let mask = mask(version, code_sets, jdf);
        let partner = units.iter().position(|(members, unit_mask)| {
            let head = &versions[members[0]];
            head.route.timetable_valid_from == version.route.timetable_valid_from
                && end_of(head) == end_of(version)
                && head.route.detour == version.route.detour
                && head.created == version.created
                && unit_mask & mask == 0
        });
        match partner {
            Some(at) => {
                units[at].0.push(index);
                units[at].1 |= mask;
            }
            None => units.push((vec![index], mask)),
        }
    }
    let head = |unit: usize| units[unit].0[0];

    // R247: boundaries of the heads' validity.
    let mut boundaries: Vec<Date> = Vec::new();
    for unit in 0..units.len() {
        let version = &versions[head(unit)];
        boundaries.push(version.route.timetable_valid_from);
        if let Ok(after) = end_of(version).tomorrow() {
            boundaries.push(after);
        }
    }
    boundaries.sort();
    boundaries.dedup();
    let mut won: Vec<Vec<(Date, Date)>> = vec![Vec::new(); units.len()];
    for pair in boundaries.windows(2).map(|w| (w[0], w[1])).chain(
        // The last boundary opens an interval only if some head is valid on it (an open end).
        boundaries.last().map(|&last| (last, Date::MAX)),
    ) {
        let (first, next) = pair;
        let last = if next == Date::MAX {
            Date::MAX
        } else {
            next.yesterday().unwrap_or(first)
        };
        let mut winner: Option<usize> = None;
        for unit in 0..units.len() {
            let version = &versions[head(unit)];
            if !(version.route.timetable_valid_from <= first && end_of(version) >= first) {
                continue;
            }
            winner = match winner {
                Some(best) if !beats(version, head(unit), &versions[head(best)], head(best)) => {
                    Some(best)
                }
                _ => Some(unit),
            };
        }
        if let Some(unit) = winner {
            let ranges = &mut won[unit];
            match ranges.last_mut() {
                Some(previous) if previous.1.tomorrow().ok() == Some(first) => previous.1 = last,
                _ => ranges.push((first, last)),
            }
        }
    }
    let mut result = HashMap::new();
    for (unit, (members, _)) in units.iter().enumerate() {
        for &member in members {
            result.insert(member, won[unit].clone());
        }
    }
    result
}

/// R243–R249, the trips' running days (R255–R267) and R254 over every version (in batch order).
pub(crate) fn resolve(
    mut versions: Vec<Version>,
    horizon: Horizon,
    reference: Date,
    holidays: &Holidays,
    code_sets: &CodeSets,
    jdf: &crate::Policy,
    diagnostics: &mut Diagnostics,
) -> Survivors {
    let mut licences: BTreeMap<Box<str>, Vec<usize>> = BTreeMap::new();
    for (index, version) in versions.iter().enumerate() {
        licences
            .entry(version.route.id.clone())
            .or_default()
            .push(index);
    }
    let mut won: HashMap<usize, Vec<(Date, Date)>> = HashMap::new();
    for indexes in licences.values() {
        won.extend(won_intervals(&versions, indexes, code_sets, jdf));
    }

    let (start, end) = (horizon.start(), horizon.end());
    let mut keep = vec![false; versions.len()];
    for indexes in licences.values() {
        for &index in indexes {
            let version = &mut versions[index];
            let subject = version.subject();
            let intervals = won.remove(&index).unwrap_or_default();
            let published = (version.route.timetable_valid_from, end_of(version));
            let info = |code, message: String, rule| {
                Diagnostic::new(code, Severity::Info, subject.clone(), message).rule(rule)
            };
            if intervals.is_empty() {
                diagnostics.push(info(
                    "jdf_version_superseded",
                    "superseded on every date: dropped".to_owned(),
                    R248,
                ));
                continue;
            }
            if intervals != [published] {
                diagnostics.push(info(
                    "jdf_version_cut",
                    format!("keeps {}", ranges(&intervals)),
                    R248,
                ));
            }
            // R249.
            let mut bounded = Vec::with_capacity(intervals.len());
            let mut next_year = false;
            for &(from, to) in &intervals {
                if to < reference || to < start {
                    continue;
                }
                if from > end {
                    next_year = true;
                    continue;
                }
                bounded.push((from.max(start), to.min(end)));
            }
            if bounded.is_empty() {
                let (code, why) = if next_year {
                    (
                        "jdf_version_next_gvd",
                        "valid only after the timetable year",
                    )
                } else {
                    ("jdf_version_expired", "expired before the reference date")
                };
                diagnostics.push(info(code, format!("{why}: dropped"), R249));
                continue;
            }
            if bounded != intervals {
                diagnostics.push(info(
                    "jdf_version_clamped",
                    format!("bounded to {}", ranges(&bounded)),
                    R249,
                ));
            }
            version.dates = bounded.iter().fold(DateSet::EMPTY, |set, &(from, to)| {
                set | DateSet::range(horizon, from, to)
            });
            version.won = bounded;
            keep[index] = true;
        }
    }
    // Survivors in (licence, batch order).
    let order: Vec<usize> = licences
        .values()
        .flatten()
        .copied()
        .filter(|&index| keep[index])
        .collect();
    let mut versions_map = vec![None; versions.len()];
    let mut slots: Vec<Option<Version>> = versions.into_iter().map(Some).collect();
    let mut survivors: Vec<Version> = Vec::with_capacity(order.len());
    for &index in &order {
        if let Some(version) = slots[index].take() {
            versions_map[index] = Some(survivors.len());
            survivors.push(version);
        }
    }

    // R248 with R255–R267, per version in parallel, results and diagnostics in order.
    let reports: Vec<(Diagnostics, Vec<Option<u32>>)> = survivors
        .par_iter_mut()
        .map(|version| calendars(version, horizon, holidays, code_sets, jdf))
        .collect();
    let mut trips_map = Vec::with_capacity(reports.len());
    for (report, kept) in reports {
        diagnostics.append(report);
        trips_map.push(kept);
    }

    keys(&mut survivors);
    Survivors {
        versions: survivors,
        versions_map,
        trips_map,
    }
}

/// The surviving versions and where each version and trip of the input went.
pub(crate) struct Survivors {
    pub versions: Vec<Version>,
    /// Input version index to survivor index.
    pub versions_map: Vec<Option<usize>>,
    /// Per survivor, input trip index to surviving trip index.
    pub trips_map: Vec<Vec<Option<u32>>>,
}

fn ranges(intervals: &[(Date, Date)]) -> String {
    intervals
        .iter()
        .map(|(from, to)| format!("{from}..={to}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Each trip's running days within the version's won intervals; trips that never run are
/// dropped (R267).
fn calendars(
    version: &mut Version,
    horizon: Horizon,
    holidays: &Holidays,
    code_sets: &CodeSets,
    jdf: &crate::Policy,
) -> (Diagnostics, Vec<Option<u32>>) {
    let mut diagnostics = Diagnostics::new();
    let subject = version.subject();
    let mut notes: HashMap<u64, Vec<CalendarNote<'_>>> = HashMap::new();
    for note in &version.notes {
        notes.entry(note.trip_id).or_default().push(note.into());
    }
    let days: Vec<DateSet> = version
        .trips
        .iter()
        .map(|trip| {
            let input = TripCalendarInput {
                subject: &subject,
                trip: trip.id,
                codes: code_sets.get(trip.codes).iter().map(|c| &**c).collect(),
                notes: notes.get(&trip.id).cloned().unwrap_or_default(),
                valid_from: version.route.timetable_valid_from,
                valid_to: version.route.timetable_valid_to,
            };
            running_days_in(
                &input,
                &version.won,
                horizon,
                holidays,
                jdf,
                &mut diagnostics,
            )
        })
        .collect();
    drop(notes);
    for (trip, days) in version.trips.iter_mut().zip(days) {
        trip.days = days;
    }
    let mut kept = Vec::with_capacity(version.trips.len());
    let mut next = 0u32;
    for trip in &version.trips {
        if trip.days.is_empty() {
            kept.push(None);
        } else {
            kept.push(Some(next));
            next += 1;
        }
    }
    version.trips.retain(|trip| !trip.days.is_empty());
    (diagnostics, kept)
}

/// R254 over the survivors (in licence, batch order).
fn keys(versions: &mut [Version]) {
    let text = |date: Date| date.strftime("%y%m%d").to_string();
    let mut groups: BTreeMap<(Box<str>, Date, bool), Vec<usize>> = BTreeMap::new();
    for (index, version) in versions.iter().enumerate() {
        groups
            .entry((
                version.route.id.clone(),
                version.route.timetable_valid_from,
                version.route.detour,
            ))
            .or_default()
            .push(index);
    }
    for members in groups.values() {
        // Published validities of the group, earliest end first (open last).
        let mut ends: Vec<Date> = members.iter().map(|&i| end_of(&versions[i])).collect();
        ends.sort();
        ends.dedup();
        for (rank, &end) in ends.iter().enumerate() {
            let same: Vec<usize> = members
                .iter()
                .copied()
                .filter(|&i| end_of(&versions[i]) == end)
                .collect();
            let hash = (rank > 0).then(|| {
                let version = &versions[same[0]];
                let to = version
                    .route
                    .timetable_valid_to
                    .map(text)
                    .unwrap_or_default();
                let payload = format!("{}|{}", text(version.route.timetable_valid_from), to);
                let digest = Sha256::digest(payload.as_bytes());
                digest[..4]
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
                    .into_boxed_str()
            });
            for (part, &index) in same.iter().enumerate() {
                versions[index].key.hash = hash.clone();
                versions[index].key.part = (part > 0).then_some(part as u32 + 1);
            }
        }
    }
}
