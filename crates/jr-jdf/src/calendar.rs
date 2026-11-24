//! A trip's running days (JDF-R255–R264, R267).
//!
//! F# decided each day of the version's validity by a first-match ladder (R256–R264 in that
//! order). Here the same ladder is set algebra over the horizon's [`DateSet`]s: every step's
//! days are a set, and "the first step that matches decides" becomes differences and unions.

use jr_formats::jdf::NoteType;
use jr_model::{Date, DateSet, Diagnostic, Diagnostics, Horizon, Rule, Severity};

use crate::batch::{self, AttributeCodes, Route, ServiceNote, Trip};
use crate::holidays::Holidays;
use crate::policy::{NoteKind, Policy};

/// A Caskody row without a type does not affect dates. A typed row covers `date_from ..=
/// date_to`; a missing `date_from` is the validity start the calendar is given, a missing
/// `date_to` is `date_from`. So an undated row covers only the first validity day (ported as
/// written, B9; diagnosed). A row ending before it starts covers nothing (F# crashed).
pub const R255: Rule = Rule {
    id: "JDF-R255",
    summary: "A typed Caskody row covers date_from..=date_to, defaulting to the validity start",
};

/// Caskody type 3 "jede jen": the trip runs on the covered days and on no other, whatever its
/// day attributes say.
pub const R256: Rule = Rule {
    id: "JDF-R256",
    summary: "Type 3: a trip with runs-only rows runs exactly on the days they cover",
};

/// Caskody type 4 "nejede": not on the covered days (it beats type 2 on the same day).
pub const R257: Rule = Rule {
    id: "JDF-R257",
    summary: "Type 4: the trip does not run on the covered days",
};

/// Caskody type 2 "jede také": also on the covered days, unless a type-4 row covers them.
pub const R258: Rule = Rule {
    id: "JDF-R258",
    summary: "Type 2: the trip also runs on the covered days",
};

/// Caskody types 5–8, odd and even weeks: a covered day in a week of the other ISO parity does
/// not run; otherwise the ladder goes on. Types 5/6 (normally undated) are treated like 7/8,
/// ported as written (so with R255 an undated row restricts only the first validity day).
pub const R259: Rule = Rule {
    id: "JDF-R259",
    summary: "Types 5-8: a covered day in a week of the wrong ISO parity does not run",
};

/// Caskody type 1 "jede": a trip with such rows runs (by its day attributes) only inside them.
pub const R260: Rule = Rule {
    id: "JDF-R260",
    summary: "Type 1: a trip with runs-in rows does not run outside them",
};

/// Day attribute `+`: Sundays and public holidays.
pub const R261: Rule = Rule {
    id: "JDF-R261",
    summary: "Attribute + runs on Sundays and public holidays",
};

/// Day attribute `X`: working days, i.e. not Saturday, Sunday or a public holiday.
pub const R262: Rule = Rule {
    id: "JDF-R262",
    summary: "Attribute X runs on working days",
};

/// An all-digit day attribute runs on that day of the week, holidays included (a Monday-only
/// trip runs on Easter Monday). A number that is no day matches no day (diagnosed by R025).
pub const R263: Rule = Rule {
    id: "JDF-R263",
    summary: "A day-number attribute runs on that weekday, holidays included",
};

/// A trip without any day attribute (`X`, `+`, a digit) runs every day; with one, it runs only
/// on the days its attributes give.
pub const R264: Rule = Rule {
    id: "JDF-R264",
    summary: "A trip without day attributes runs every day",
};

/// A trip with no running day in the horizon cannot be published. It is reported; what happens
/// to it is the caller's decision.
pub const R267: Rule = Rule {
    id: "JDF-R267",
    summary: "A trip without running days is reported as never running",
};

/// Exactly what the calendar reads of one trip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TripCalendarInput<'a> {
    /// The trip's line version, as diagnostics name it.
    pub subject: &'a str,
    /// The trip number.
    pub trip: u64,
    /// The fixed codes of the trip's attribute slots (`Spoje.txt` fields 3–12).
    pub codes: Vec<&'a str>,
    /// The trip's `Caskody.txt` rows.
    pub notes: Vec<CalendarNote<'a>>,
    /// The version's validity start, after cutting and clamping: undated notes default to it
    /// (R255).
    pub valid_from: Date,
    /// `None`: open, to the end of the horizon (R030).
    pub valid_to: Option<Date>,
}

/// What the calendar reads of a `Caskody.txt` row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalendarNote<'a> {
    pub note_type: Option<&'a NoteType>,
    pub date_from: Option<Date>,
    pub date_to: Option<Date>,
}

impl<'a> From<&'a ServiceNote> for CalendarNote<'a> {
    fn from(note: &'a ServiceNote) -> Self {
        CalendarNote {
            note_type: note.note_type.as_ref(),
            date_from: note.date_from,
            date_to: note.date_to,
        }
    }
}

impl<'a> TripCalendarInput<'a> {
    /// The input of a batch trip over its route's published validity; `notes` are the trip's
    /// rows (same route key and trip number).
    pub fn for_trip(
        subject: &'a str,
        route: &Route,
        trip: &'a Trip,
        codes: &AttributeCodes<'a>,
        notes: impl IntoIterator<Item = &'a ServiceNote>,
    ) -> Self {
        TripCalendarInput {
            subject,
            trip: trip.id,
            codes: codes.resolve(&trip.attributes).collect(),
            notes: notes.into_iter().map(CalendarNote::from).collect(),
            valid_from: route.timetable_valid_from,
            valid_to: route.timetable_valid_to,
        }
    }
}

const KINDS: usize = 6;

fn slot(kind: NoteKind) -> usize {
    match kind {
        NoteKind::RunsIn => 0,
        NoteKind::RunsAlso => 1,
        NoteKind::RunsOnly => 2,
        NoteKind::DoesNotRun => 3,
        NoteKind::OddWeeks => 4,
        NoteKind::EvenWeeks => 5,
    }
}

/// The days the trip runs inside `horizon` (R255–R264). An empty set is reported (R267).
pub fn running_days(
    input: &TripCalendarInput<'_>,
    horizon: Horizon,
    holidays: &Holidays,
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> DateSet {
    let days = ladder(
        input,
        input.valid_from,
        input.valid_to,
        horizon,
        holidays,
        policy,
        diagnostics,
    );
    if days.is_empty() {
        diagnostics.push(never_runs(input));
    }
    days
}

/// The running days of a version that won several intervals (JDF-R248): the ladder over each
/// interval with the interval as the validity (so undated notes default to the interval's start,
/// as on an F# cut version), united. An empty result is reported once (R267).
pub fn running_days_in(
    input: &TripCalendarInput<'_>,
    intervals: &[(Date, Date)],
    horizon: Horizon,
    holidays: &Holidays,
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> DateSet {
    let mut days = DateSet::EMPTY;
    for &(from, to) in intervals {
        days = days
            | ladder(
                input,
                from,
                Some(to),
                horizon,
                holidays,
                policy,
                diagnostics,
            );
    }
    if days.is_empty() {
        diagnostics.push(never_runs(input));
    }
    days
}

fn never_runs(input: &TripCalendarInput<'_>) -> Diagnostic {
    Diagnostic::new(
        "jdf_trip_never_runs",
        Severity::Warning,
        format!("{}/{}", input.subject, input.trip),
        "the trip has no running day in the horizon",
    )
    .rule(R267)
}

/// R261–R264: the days a list of fixed codes gives by its day attributes (`X`, `+`, day
/// numbers); `None` when it has none. Other codes are not day attributes and are skipped.
pub(crate) fn day_attribute_days(
    codes: &[&str],
    holidays: &Holidays,
    policy: &Policy,
) -> Option<DateSet> {
    let calendar = &policy.calendar;
    let mut day_attribute = false;
    let mut days = DateSet::EMPTY;
    for &code in codes {
        if code == &*calendar.working_days {
            day_attribute = true;
            let working = (0..5).fold(DateSet::EMPTY, |set, day| set | holidays.weekday(day));
            days = days | (working - holidays.holidays());
        } else if code == &*calendar.sundays_and_holidays {
            day_attribute = true;
            days = days | holidays.weekday(6) | holidays.holidays();
        } else if batch::is_digits(code) {
            day_attribute = true;
            if let Some(day) = batch::day_number(code, policy) {
                days = days | holidays.weekday(day);
            }
        }
    }
    day_attribute.then_some(days)
}

/// R255–R264 over `valid_from ..= valid_to` (open: to the horizon's end).
fn ladder(
    input: &TripCalendarInput<'_>,
    valid_from: Date,
    valid_to: Option<Date>,
    horizon: Horizon,
    holidays: &Holidays,
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> DateSet {
    debug_assert_eq!(holidays.horizon(), horizon);
    let subject = || format!("{}/{}", input.subject, input.trip);
    let full = DateSet::full(horizon.days());
    let validity = DateSet::range(horizon, valid_from, valid_to.unwrap_or(horizon.end()));

    // R255: the days each kind of row covers, and which kinds the trip has at all.
    let mut cover = [DateSet::EMPTY; KINDS];
    let mut has = [false; KINDS];
    for note in &input.notes {
        let Some(code) = note.note_type else { continue };
        // An unlisted type is diagnosed per batch (R026) and matches nothing.
        let Some(&kind) = policy.calendar.note_types.get(code.code()) else {
            continue;
        };
        let from = note.date_from.unwrap_or(valid_from);
        let to = note.date_to.unwrap_or(from);
        if note.date_from.is_none() {
            diagnostics.push(
                Diagnostic::new(
                    "jdf_note_undated",
                    Severity::Info,
                    subject(),
                    format!(
                        "Caskody type {} without a start date covers {from}..={to}",
                        code.code()
                    ),
                )
                .rule(R255),
            );
        }
        has[slot(kind)] = true;
        if to < from {
            diagnostics.push(
                Diagnostic::new(
                    "jdf_note_reversed",
                    Severity::Warning,
                    subject(),
                    format!(
                        "Caskody type {} ends ({to}) before it starts ({from}): covers no day",
                        code.code()
                    ),
                )
                .rule(R255),
            );
            continue;
        }
        cover[slot(kind)] = cover[slot(kind)] | DateSet::range(horizon, from, to);
    }

    let by_attributes = day_attribute_days(&input.codes, holidays, policy).unwrap_or(full);

    let covered = |kind| cover[slot(kind)];
    let days = if has[slot(NoteKind::RunsOnly)] {
        // R256.
        covered(NoteKind::RunsOnly)
    } else {
        let not_running = covered(NoteKind::DoesNotRun);
        // R259.
        let wrong_week = (covered(NoteKind::OddWeeks) & holidays.even_iso_weeks())
            | (covered(NoteKind::EvenWeeks) & holidays.odd_iso_weeks());
        // R260.
        let outside_runs_in = if has[slot(NoteKind::RunsIn)] {
            full - covered(NoteKind::RunsIn)
        } else {
            DateSet::EMPTY
        };
        // R257 before R258; R258 before R259, R260 and the day attributes.
        (covered(NoteKind::RunsAlso) - not_running)
            | (by_attributes - not_running - wrong_week - outside_runs_in)
    };
    days & validity
}
