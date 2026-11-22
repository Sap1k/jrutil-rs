//! Notes, their links and features, trip flags, request stops (R070, R072–R079) and zones
//! (R085, R086) of accepted paths.

mod resolve;
mod zones;

use std::collections::{BTreeMap, BTreeSet};

use jr_formats::kadr::Catalog;
use jr_model::identity::czptt;
use jr_model::timetable::{Assignment, AssignmentKind, Feature, Note, NoteKind, Scope, Tristate};
use jr_model::{Date, Diagnostic, Diagnostics, Horizon, Rule, Severity, Timetable, TripId};

use crate::path::{Part, Path};
use crate::policy::Policy;
use resolve::{End, resolve};

pub(crate) use resolve::R071;
pub(crate) use zones::{R085, R086, zones};

/// GTFS trip flags are conservative: a claim counts only if a central note makes it for the
/// whole part on every running day; contradictory bicycle claims stay unknown.
pub(crate) const R070: Rule = Rule {
    id: "CZPTT-R070",
    summary: "Wheelchair and bicycle flags come from central notes covering the whole part on all days",
};

/// `CZCalendarPTTNote` rows are bitmaps from a start date, chained to a next calendar.
pub(crate) const R072: Rule = Rule {
    id: "CZPTT-R072",
    summary: "A note calendar is the union of its rows and of the calendar it chains to",
};

/// `CZCentralPTTNote`: `code|from|occ|to|occ|?|calendar`, labelled from the catalogue.
pub(crate) const R073: Rule = Rule {
    id: "CZPTT-R073",
    summary: "A central note covers its resolved interval and is labelled by the catalogue",
};

/// `CZNonCentralPTTNote`: `from|occ|to|occ|label|…|calendar`.
pub(crate) const R074: Rule = Rule {
    id: "CZPTT-R074",
    summary: "A local note covers its resolved interval and carries its own label",
};

/// `CZCalendarPTTNote` values are kept as notes valid between their own dates.
pub(crate) const R075: Rule = Rule {
    id: "CZPTT-R075",
    summary: "A calendar note is valid from its start to its end date",
};

/// Notes belong to the trips they describe; a rejected path has none (Q13). A note links to
/// the calls of each part inside its interval, else to every part.
pub(crate) const R076: Rule = Rule {
    id: "CZPTT-R076",
    summary: "Notes of accepted paths link to the parts and calls inside their interval",
};

/// Central note codes map to rider features; a replacement bus is not the train they describe.
pub(crate) const R077: Rule = Rule {
    id: "CZPTT-R077",
    summary: "A mapped central note gives its features to the rail parts it covers",
};

/// Activity `0030` marks a request stop; only a passenger call can be one (fixes B34).
pub(crate) const R078: Rule = Rule {
    id: "CZPTT-R078",
    summary: "A passenger call with the request-stop activity gets the on_request feature",
};

/// A part claimed to carry and to refuse bicycles on all its days.
pub(crate) const R079: Rule = Rule {
    id: "CZPTT-R079",
    summary: "Contradictory whole-part bicycle claims are diagnosed",
};

struct Spec {
    kind: NoteKind,
    id_kind: Box<str>,
    /// 1-based among the notes of this kind.
    index: usize,
    raw: Box<str>,
    code: Option<Box<str>>,
    label: Option<Box<str>>,
    note_type: Option<Box<str>>,
    /// `(first, last)` source indices, when both resolved and in order.
    range: Option<(u32, u32)>,
    covers_all_dates: bool,
    valid: (Option<Date>, Option<Date>),
}

/// The notes of one accepted path.
pub(crate) struct PathNotes {
    specs: Vec<Spec>,
}

type Calendars = BTreeMap<Box<str>, Vec<(Vec<Date>, Option<Box<str>>)>>;

fn date(text: &str, policy: &Policy) -> Option<Date> {
    Date::strptime(&*policy.notes.date_format, text.trim()).ok()
}

fn calendars(path: &Path, policy: &Policy) -> Calendars {
    let mut calendars: Calendars = BTreeMap::new();
    for parameter in path
        .parameters
        .iter()
        .filter(|p| p.name == policy.notes.calendar)
    {
        let fields: Vec<&str> = parameter
            .value
            .split(policy.notes.field_separator)
            .collect();
        if fields.len() < 4 {
            continue;
        }
        let Some(start) = date(fields[1], policy) else {
            continue;
        };
        let dates = fields[3]
            .chars()
            .zip(start.series(jiff::Span::new().days(1)))
            .filter(|(bit, _)| *bit == policy.calendar.running_day)
            .map(|(_, day)| day)
            .collect();
        let next = fields
            .get(4)
            .map(|next| next.trim())
            .filter(|next| !next.is_empty())
            .map(Box::from);
        calendars
            .entry(fields[0].into())
            .or_default()
            .push((dates, next));
    }
    calendars
}

/// R072: `None` for an unknown id, a cycle or more than one next calendar.
fn calendar_dates(calendars: &Calendars, id: &str) -> Option<BTreeSet<Date>> {
    let mut visited = BTreeSet::new();
    let mut dates = BTreeSet::new();
    let mut current = id;
    loop {
        if !visited.insert(current) {
            return None;
        }
        let rows = calendars.get(current)?;
        dates.extend(rows.iter().flat_map(|(own, _)| own.iter().copied()));
        let next: BTreeSet<&str> = rows
            .iter()
            .filter_map(|(_, next)| next.as_deref())
            .collect();
        match next.len() {
            0 => return Some(dates),
            1 => current = next.into_iter().next()?,
            _ => return None,
        }
    }
}

fn non_blank(value: Option<&&str>) -> Option<Box<str>> {
    value
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(Box::from)
}

impl PathNotes {
    pub(crate) fn parse(
        path: &Path,
        horizon: Horizon,
        catalog: &Catalog,
        policy: &Policy,
        diagnostics: &mut Diagnostics,
    ) -> PathNotes {
        let pa = &*path.pa;
        let notes = &policy.notes;
        let calendars = calendars(path, policy);
        // The running days as the source dates them (before R066).
        let running: Vec<Date> = path
            .dates
            .dates(horizon)
            .filter_map(|day| {
                day.checked_add(jiff::Span::new().days(i64::from(path.day_shift)))
                    .ok()
            })
            .collect();
        let mut specs = Vec::new();
        let mut counts = [0usize; 3];
        for parameter in &path.parameters {
            let (kind, slot, id_kind, rule) = if parameter.name == notes.central {
                (NoteKind::CzpttCentral, 0, &notes.central_kind, R073)
            } else if parameter.name == notes.local {
                (NoteKind::CzpttLocal, 1, &notes.local_kind, R074)
            } else if parameter.name == notes.calendar {
                (NoteKind::CzpttCalendar, 2, &notes.calendar_kind, R075)
            } else {
                continue;
            };
            counts[slot] += 1;
            let raw = &*parameter.value;
            let fields: Vec<&str> = raw.split(notes.field_separator).collect();
            let mut spec = Spec {
                kind,
                id_kind: id_kind.clone(),
                index: counts[slot],
                raw: raw.into(),
                code: None,
                label: None,
                note_type: None,
                range: None,
                covers_all_dates: true,
                valid: (None, None),
            };
            let report = |diagnostics: &mut Diagnostics, code: &'static str, what: &str| {
                diagnostics.push(
                    Diagnostic::new(code, Severity::Warning, pa, format!("{what} {raw:?}"))
                        .rule(rule),
                );
            };
            if kind == NoteKind::CzpttCalendar {
                spec.code = fields.first().map(|code| Box::from(*code));
                spec.note_type = spec.code.clone();
                spec.valid = (
                    fields.get(1).and_then(|text| date(text, policy)),
                    fields.get(2).and_then(|text| date(text, policy)),
                );
                if fields.len() < 4 || spec.valid.0.is_none() || spec.valid.1.is_none() {
                    report(
                        diagnostics,
                        "czptt_calendar_note_malformed",
                        "calendar note",
                    );
                }
                specs.push(spec);
                continue;
            }
            let (endpoints, calendar) = if kind == NoteKind::CzpttCentral {
                let code = fields.first().copied().unwrap_or("");
                spec.code = Some(code.into());
                spec.note_type = Some(code.into());
                spec.label = catalog
                    .central_notes
                    .iter()
                    .find(|note| &*note.code == code)
                    .map(|note| note.name.clone().unwrap_or_else(|| note.code.clone()));
                ((fields.len() >= 5).then_some(1), non_blank(fields.get(6)))
            } else {
                spec.label = non_blank(fields.get(4));
                ((fields.len() >= 4).then_some(0), non_blank(fields.get(9)))
            };
            if let Some(at) = endpoints {
                let first = resolve(&path.calls, fields[at], fields[at + 1], End::Start);
                let last = resolve(&path.calls, fields[at + 2], fields[at + 3], End::End(first));
                spec.range = first.zip(last).filter(|(first, last)| first <= last);
            }
            if spec.range.is_none() {
                report(
                    diagnostics,
                    "czptt_note_unresolved_interval",
                    "note interval of",
                );
            }
            if let Some(id) = calendar {
                match calendar_dates(&calendars, &id) {
                    Some(dates) => {
                        spec.covers_all_dates = running.iter().all(|day| dates.contains(day));
                        spec.valid = (dates.first().copied(), dates.last().copied());
                    }
                    None => {
                        spec.covers_all_dates = false;
                        diagnostics.push(
                            Diagnostic::new(
                                "czptt_note_unresolved_calendar",
                                Severity::Warning,
                                pa,
                                format!("calendar {id:?} of {raw:?} does not resolve"),
                            )
                            .rule(R072),
                        );
                    }
                }
            }
            specs.push(spec);
        }
        PathNotes { specs }
    }

    /// R070, R079: the wheelchair and bicycle flags of a part.
    pub(crate) fn flags(
        &self,
        pa: &str,
        part: &Part,
        policy: &Policy,
        diagnostics: &mut Diagnostics,
    ) -> (Tristate, Tristate) {
        if part.key.replacement_bus {
            return (Tristate::Unknown, Tristate::Unknown);
        }
        let (Some(low), Some(high)) = (
            part.passenger_calls().map(|c| c.index).min(),
            part.passenger_calls().map(|c| c.index).max(),
        ) else {
            return (Tristate::Unknown, Tristate::Unknown);
        };
        let complete = |codes: &[Box<str>]| {
            self.specs.iter().any(|spec| {
                spec.kind == NoteKind::CzpttCentral
                    && spec.covers_all_dates
                    && spec.code.as_ref().is_some_and(|code| codes.contains(code))
                    && spec
                        .range
                        .is_some_and(|(first, last)| first <= low && last >= high)
            })
        };
        let features = &policy.features;
        let wheelchair = if complete(&features.wheelchair_codes) {
            Tristate::Yes
        } else {
            Tristate::Unknown
        };
        let bikes = match (
            complete(&features.bicycle_codes),
            complete(&features.bicycle_prohibited_codes),
        ) {
            (true, false) => Tristate::Yes,
            (false, true) => Tristate::No,
            (true, true) => {
                diagnostics.push(
                    Diagnostic::new(
                        "czptt_conflicting_bicycle_notes",
                        Severity::Warning,
                        pa,
                        format!(
                            "part {} is said to carry and to refuse bicycles",
                            part.number
                        ),
                    )
                    .rule(R079),
                );
                Tristate::Unknown
            }
            (false, false) => Tristate::Unknown,
        };
        (wheelchair, bikes)
    }

    /// R076, R077: the notes, their links and features.
    pub(crate) fn publish(
        &self,
        path: &Path,
        trips: &[(TripId, &Part)],
        timetable: &mut Timetable,
        policy: &Policy,
    ) {
        for spec in &self.specs {
            let id = czptt::note(&path.pa, &spec.id_kind, spec.index);
            let source_object: Box<str> = id.as_str().into();
            let note = timetable.notes.push(Note {
                id,
                kind: spec.kind,
                label: spec.label.clone(),
                text: Some(spec.raw.clone()),
                valid_from: spec.valid.0,
                valid_to: spec.valid.1,
                note_type: spec.note_type.clone(),
                source_object: source_object.clone(),
            });
            let link = |scope| Assignment {
                scope,
                kind: AssignmentKind::Note,
                source_code: None,
                note: Some(note),
                source_object: None,
            };
            let ranged: Vec<Assignment> = spec
                .range
                .map(|(first, last)| {
                    trips
                        .iter()
                        .filter_map(|(trip, part)| {
                            let inside = part
                                .calls
                                .iter()
                                .filter(|call| (first..=last).contains(&call.index))
                                .map(|call| call.sequence() as i32);
                            let (from, to) = inside.fold(None, |range, sequence| match range {
                                None => Some((sequence, sequence)),
                                Some((from, to)) => Some((from.min(sequence), to.max(sequence))),
                            })?;
                            Some(link(Scope::CallRange {
                                trip: *trip,
                                from,
                                to,
                            }))
                        })
                        .collect()
                })
                .unwrap_or_default();
            if ranged.is_empty() {
                let all = trips.iter().map(|(trip, _)| link(Scope::Trip(*trip)));
                timetable.assignments.extend(all);
            } else {
                timetable.assignments.extend(ranged);
            }

            let kinds = spec
                .code
                .as_deref()
                .filter(|_| spec.kind == NoteKind::CzpttCentral)
                .map_or(&[][..], |code| policy.central_features(code));
            let Some((first, last)) = spec.range.filter(|_| !kinds.is_empty()) else {
                continue;
            };
            for (trip, part) in trips.iter().filter(|(_, part)| !part.key.replacement_bus) {
                let covered: Vec<i32> = part
                    .passenger_calls()
                    .filter(|call| (first..=last).contains(&call.index))
                    .map(|call| call.sequence() as i32)
                    .collect();
                if covered.is_empty() {
                    continue;
                }
                let low = part.passenger_calls().map(|c| c.index).min();
                let high = part.passenger_calls().map(|c| c.index).max();
                let whole =
                    low.is_some_and(|low| first <= low) && high.is_some_and(|high| last >= high);
                for kind in kinds {
                    let scopes: Vec<Scope> = if whole {
                        vec![Scope::Trip(*trip)]
                    } else {
                        covered
                            .iter()
                            .map(|&sequence| Scope::Call {
                                trip: *trip,
                                sequence,
                            })
                            .collect()
                    };
                    for scope in scopes {
                        timetable.assignments.push(Assignment {
                            scope,
                            kind: AssignmentKind::Feature(*kind),
                            source_code: spec.code.clone(),
                            note: Some(note),
                            source_object: Some(source_object.clone()),
                        });
                    }
                }
            }
        }
    }
}

/// R078.
pub(crate) fn request_stops(
    path: &Path,
    trips: &[(TripId, &Part)],
    timetable: &mut Timetable,
    policy: &Policy,
) {
    let code = &policy.activities.request_stop;
    for (trip, part) in trips {
        for call in part.passenger_calls() {
            if !call.activities.contains(code) {
                continue;
            }
            timetable.assignments.push(Assignment {
                scope: Scope::Call {
                    trip: *trip,
                    sequence: call.sequence() as i32,
                },
                kind: AssignmentKind::Feature(Feature::OnRequest),
                source_code: Some(code.clone()),
                note: None,
                source_object: Some(
                    format!("{}:sequence:{}:activity:{code}", path.pa, call.sequence())
                        .into_boxed_str(),
                ),
            });
        }
    }
}
