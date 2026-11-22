//! Merging timetables and cancellations into each path's running days (R013–R016, R065, R081,
//! R116).

use std::collections::BTreeMap;

use jr_formats::czptt::{Calendar, TimetableMessage};
use jr_model::{Date, DateSet, Diagnostic, Diagnostics, Horizon, Rule, Severity};

use crate::order::{Ordered, Payload};
use crate::policy::Policy;

/// A cancellation's bitmap marks the days that stop running; days it leaves at `0` are
/// unchanged. A path left without days is cancelled.
pub(crate) const R013: Rule = Rule {
    id: "CZPTT-R013",
    summary: "A cancellation removes its marked days from the path's calendar",
};

/// SŽ publishes cancellations for paths the snapshot does not contain (they were never in this
/// timetable year, or were published before it).
pub(crate) const R014: Rule = Rule {
    id: "CZPTT-R014",
    summary: "A cancellation of an unknown path is diagnosed once per path",
};

/// A calendar whose bitmap does not fit its validity cannot be read reliably; a cancellation
/// reaching past the validity of its path still says which of the path's days stop.
pub(crate) const R015: Rule = Rule {
    id: "CZPTT-R015",
    summary: "A malformed calendar drops its message; a cancellation past the validity applies its overlap",
};

/// Cancelled paths are reported, not silently forgotten.
pub(crate) const R016: Rule = Rule {
    id: "CZPTT-R016",
    summary: "Paths left without running days are listed as cancelled",
};

/// `BitmapDays[i]` is the day `StartDateTime + i`; the calendar is that of day offset 0 (Q2).
pub(crate) const R065: Rule = Rule {
    id: "CZPTT-R065",
    summary: "A path runs on StartDateTime + i for every running bit i",
};

/// The snapshot's version is the newest message it contains.
pub(crate) const R081: Rule = Rule {
    id: "CZPTT-R081",
    summary: "The source version is the newest creation time among surviving messages",
};

/// Calendars may reach outside the timetable year (a path published for the previous year's
/// last days); those days are outside what the timetable describes.
pub(crate) const R116: Rule = Rule {
    id: "CZPTT-R116",
    summary: "Running days outside the widened GVD horizon are dropped",
};

/// A surviving path's message and running days.
pub(crate) struct Merged {
    pub pa: Box<str>,
    pub message: TimetableMessage,
    pub dates: DateSet,
}

pub(crate) struct MergeOutput {
    /// Paths with at least one running day, in PA order.
    pub surviving: Vec<Merged>,
    pub cancelled: Vec<Box<str>>,
    pub source_version: Option<Box<str>>,
}

struct Entry {
    message: TimetableMessage,
    dates: DateSet,
    start: Date,
    end: Date,
}

struct Days {
    start: Date,
    end: Date,
    running: Vec<Date>,
}

enum Fault {
    EndBeforeStart,
    Length { bitmap: usize, days: i64 },
    Character(char),
}

/// The running days of a calendar over `[start, end ?? start]` (R013, R065).
fn days(calendar: &Calendar, policy: &Policy) -> Result<Days, Fault> {
    let start = calendar.start.local.date();
    let end = calendar.end.map_or(start, |end| end.local.date());
    let span = start
        .until(end)
        .map_err(|_| Fault::EndBeforeStart)?
        .get_days();
    if span < 0 {
        return Err(Fault::EndBeforeStart);
    }
    let days = i64::from(span) + 1;
    let bitmap = calendar.bitmap.chars().count();
    if bitmap as i64 != days {
        return Err(Fault::Length { bitmap, days });
    }
    let mut running = Vec::new();
    for (bit, date) in calendar
        .bitmap
        .chars()
        .zip(start.series(jiff::Span::new().days(1)))
    {
        if bit == policy.calendar.running_day {
            running.push(date);
        } else if bit != policy.calendar.not_running_day {
            return Err(Fault::Character(bit));
        }
    }
    Ok(Days {
        start,
        end,
        running,
    })
}

fn fault_diagnostic(fault: Fault, subject: &str, source_path: &str) -> Diagnostic {
    let problem = match fault {
        Fault::EndBeforeStart => "the validity ends before it starts".to_owned(),
        Fault::Length { bitmap, days } => {
            format!("{bitmap} bitmap days for a validity of {days} days")
        }
        Fault::Character(bit) => format!("bitmap character {bit:?}"),
    };
    Diagnostic::new(
        "czptt_calendar_malformed",
        Severity::Warning,
        subject,
        format!("{source_path}: {problem}; the message is dropped"),
    )
    .rule(R015)
}

pub(crate) fn merge(
    ordered: Vec<Ordered>,
    horizon: Horizon,
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> MergeOutput {
    let mut paths: BTreeMap<Box<str>, Entry> = BTreeMap::new();
    let mut unknown: BTreeMap<Box<str>, usize> = BTreeMap::new();
    for message in ordered {
        match message.payload {
            Payload::Timetable(timetable) => {
                let days = match days(&timetable.calendar, policy) {
                    Ok(days) => days,
                    Err(fault) => {
                        diagnostics.push(fault_diagnostic(
                            fault,
                            &message.pa,
                            &message.source_path,
                        ));
                        continue;
                    }
                };
                let (dates, outside) = DateSet::from_dates(horizon, days.running);
                if outside > 0 {
                    diagnostics.push(
                        Diagnostic::new(
                            "czptt_dates_outside_horizon",
                            Severity::Warning,
                            &*message.pa,
                            format!(
                                "{outside} running days outside {}..={} are dropped",
                                horizon.start(),
                                horizon.end()
                            ),
                        )
                        .rule(R116),
                    );
                }
                // A timetable is applied once per PA: duplicates and conflicts were settled in
                // application order (R006, R012).
                paths.insert(
                    message.pa,
                    Entry {
                        message: timetable,
                        dates,
                        start: days.start,
                        end: days.end,
                    },
                );
            }
            Payload::Cancellation(cancellation) => {
                let Some(entry) = paths.get_mut(&message.pa) else {
                    *unknown.entry(message.pa).or_default() += 1;
                    continue;
                };
                let days = match days(&cancellation.calendar, policy) {
                    Ok(days) => days,
                    Err(fault) => {
                        diagnostics.push(fault_diagnostic(
                            fault,
                            &message.pa,
                            &message.source_path,
                        ));
                        continue;
                    }
                };
                if days.start < entry.start || days.end > entry.end {
                    diagnostics.push(
                        Diagnostic::new(
                            "czptt_cancellation_outside_validity",
                            Severity::Warning,
                            &*message.pa,
                            format!(
                                "{}: cancellation {}..={} reaches outside the validity {}..={}; \
                                 its overlap is applied",
                                message.source_path, days.start, days.end, entry.start, entry.end
                            ),
                        )
                        .rule(R015),
                    );
                }
                // The path's days lie inside its validity, so removing the whole cancellation
                // removes exactly its overlap.
                let (cancelled, _) = DateSet::from_dates(horizon, days.running);
                entry.dates = entry.dates - cancelled;
            }
        }
    }
    for (pa, count) in unknown {
        diagnostics.push(
            Diagnostic::new(
                "czptt_unknown_cancellation_target",
                Severity::Warning,
                &*pa,
                format!("{count} cancellation messages target a path that is not known"),
            )
            .rule(R014),
        );
    }

    let mut surviving = Vec::new();
    let mut cancelled = Vec::new();
    let mut newest = None;
    for (pa, entry) in paths {
        if entry.dates.is_empty() {
            diagnostics.push(
                Diagnostic::new(
                    "czptt_path_cancelled",
                    Severity::Info,
                    &*pa,
                    "no running day is left",
                )
                .rule(R016),
            );
            cancelled.push(pa);
            continue;
        }
        let creation = entry.message.creation.local;
        newest = newest.max(Some(creation));
        surviving.push(Merged {
            pa,
            message: entry.message,
            dates: entry.dates,
        });
    }
    let source_version = newest.map(|creation| {
        format!(
            "{}{}",
            policy.source_version.prefix,
            creation.strftime(&*policy.source_version.format)
        )
        .into_boxed_str()
    });
    MergeOutput {
        surviving,
        cancelled,
        source_version,
    }
}
