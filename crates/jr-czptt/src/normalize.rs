//! Normalizing a path's locations into calls (R027–R036), repairing and checking their times
//! (R037–R039, R066) and projecting line boundaries onto passenger calls (R040).

mod project;
mod repair;

use std::collections::BTreeSet;

use jr_formats::czptt::{Location, Parameter, TimetableMessage, Timing};
use jr_formats::kadr::{Catalog, CodeLabel};
use jr_model::{DateSet, Diagnostic, Diagnostics, Horizon, Rule, ServiceTime, Severity};

use crate::path::{Call, Rejection, RejectionReason, ServiceKey};
use crate::policy::Policy;

pub(crate) use project::{Adjustment, R040, report_adjustments};
pub(crate) use repair::{R037, R038, R039};

/// `Timing/Time` is local wall time with a redundant offset (Q4); `Offset` counts days after
/// the calendar day. Only the arrival and departure qualifiers carry the timetable.
pub(crate) const R027: Rule = Rule {
    id: "CZPTT-R027",
    summary: "A call's time is the first ALA/ALD timing's local time plus whole days of Offset",
};

/// SŽ gives a pass or an end point one timing only; a call is reached and left at that time.
pub(crate) const R028: Rule = Rule {
    id: "CZPTT-R028",
    summary: "A call with one time gets it on both sides",
};

/// Activity `0001` is the only mark of passenger service; `CZ13` ("unpublished stop") together
/// with it occurs (3 calls in the golden subset) and is kept as a passenger call (Q7).
pub(crate) const R029: Rule = Rule {
    id: "CZPTT-R029",
    summary: "A call is a passenger call iff it has the passenger activity",
};

/// The activity list grows with the standard; an unknown code must not stop the build.
pub(crate) const R030: Rule = Rule {
    id: "CZPTT-R030",
    summary: "An activity code outside the known list is diagnosed and otherwise ignored",
};

/// A parameter may repeat; the one value a rule needs is the last one.
pub(crate) const R031: Rule = Rule {
    id: "CZPTT-R031",
    summary: "Where one parameter value is needed, the last value of that name wins",
};

/// Location-level line codes describe the onward section and a missing one clears the line;
/// a message without any falls back to the root-level code.
pub(crate) const R032: Rule = Rule {
    id: "CZPTT-R032",
    summary: "The active line is the location's own line code if any location has one, else the root's",
};

/// `ResponsibleRU` is only written where the operator changes; the train's company is the
/// initial one.
pub(crate) const R033: Rule = Rule {
    id: "CZPTT-R033",
    summary: "The operator starts as the TR company and is carried forward",
};

/// The category is written where it changes, as a KADR code; the catalogue gives its label.
pub(crate) const R034: Rule = Rule {
    id: "CZPTT-R034",
    summary: "The category is the catalogue label of the commercial, else traffic type, carried forward",
};

/// The passenger-facing service class of a category decides its GTFS route type.
pub(crate) const R035: Rule = Rule {
    id: "CZPTT-R035",
    summary: "The route type follows the category's service class",
};

/// `CZAlternativeTransport=1` marks a location served by a rail replacement bus.
pub(crate) const R036: Rule = Rule {
    id: "CZPTT-R036",
    summary: "A location is served by replacement bus iff its last CZAlternativeTransport is 1",
};

/// The calendar is that of day offset 0 (Q2); a run starting the day before has negative
/// times, which GTFS cannot hold. The run moves as a whole: times forward, dates back.
pub(crate) const R066: Rule = Rule {
    id: "CZPTT-R066",
    summary: "A path with a negative time moves forward by whole days and its dates back as many",
};

pub(crate) struct Normalized {
    pub calls: Vec<Call>,
    pub dates: DateSet,
    /// Days R066 moved the dates back.
    pub day_shift: u16,
    pub rejection: Option<Rejection>,
    /// Line boundary moves (R040), reported only for accepted paths.
    pub adjustments: Vec<Adjustment>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn normalize(
    pa: &str,
    message: &TimetableMessage,
    tr_company: Option<&str>,
    dates: DateSet,
    horizon: Horizon,
    catalog: &Catalog,
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> Normalized {
    let mut calls = calls(pa, message, tr_company, catalog, policy, diagnostics);
    repair::repair_inconsistent_times(pa, &message.locations, &mut calls, policy, diagnostics);
    let (dates, day_shift) = shift_negative_times(pa, &mut calls, dates, horizon, diagnostics);
    let rejection = if !calls.iter().any(|call| call.passenger) {
        diagnostics.push(
            Diagnostic::new(
                "czptt_path_without_passenger_call",
                Severity::Warning,
                pa,
                "no call has the passenger activity; the path is rejected",
            )
            .rule(R038),
        );
        Some(Rejection {
            reason: RejectionReason::NoPassengerCall,
            sequence: None,
            previous_seconds: None,
            current_seconds: None,
        })
    } else {
        repair::chronology(pa, &calls, diagnostics)
    };
    let adjustments = project::project(&mut calls);
    Normalized {
        calls,
        dates,
        day_shift,
        rejection,
        adjustments,
    }
}

/// The values of parameter `name`, in order (R031).
fn values<'a>(parameters: &'a [Parameter], name: &'a str) -> impl Iterator<Item = &'a str> {
    parameters
        .iter()
        .filter(move |parameter| &*parameter.name == name)
        .map(|parameter| &*parameter.value)
}

/// The last value of parameter `name` (R031).
fn last<'a>(parameters: &'a [Parameter], name: &'a str) -> Option<&'a str> {
    values(parameters, name).last()
}

fn clean_line(value: Option<&str>) -> Option<Box<str>> {
    value
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(Box::from)
}

/// The catalogue label of a category code, `None` when the catalogue lacks the code (R034).
fn category_label<'a>(code: &'a str, entries: &'a [CodeLabel]) -> Option<&'a str> {
    let entry = entries.iter().find(|entry| &*entry.code == code)?;
    Some(
        entry
            .abbreviation
            .as_deref()
            .filter(|label| !label.trim().is_empty())
            .unwrap_or(code),
    )
}

fn calls(
    pa: &str,
    message: &TimetableMessage,
    tr_company: Option<&str>,
    catalog: &Catalog,
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> Vec<Call> {
    let parameters = &policy.parameters;
    let line_name = &*parameters.line;
    let location_lines = message
        .locations
        .iter()
        .any(|location| values(&location.parameters, line_name).next().is_some());
    let root_line = clean_line(last(&message.parameters, line_name));
    let mut ru: Option<Box<str>> = tr_company.map(Box::from);
    let mut category: Box<str> = policy.category.default_label.clone();
    let mut unknown_categories = BTreeSet::new();
    let mut unknown_qualifiers = BTreeSet::new();
    let mut missing_ru = false;
    let mut calls = Vec::with_capacity(message.locations.len());
    for (index, location) in message.locations.iter().enumerate() {
        let index = index as u32;
        let (arrival, departure) = times(
            pa,
            index,
            location,
            policy,
            &mut unknown_qualifiers,
            diagnostics,
        );
        let line = if location_lines {
            clean_line(last(&location.parameters, line_name))
        } else {
            root_line.clone()
        };
        if let Some(responsible) = &location.responsible_ru {
            ru = Some(responsible.clone());
        }
        if ru.is_none() {
            missing_ru = true;
        }
        let coded = location
            .commercial_traffic_type
            .as_deref()
            .map(|code| {
                (
                    code,
                    &catalog.commercial_train_types,
                    "commercial train type",
                )
            })
            .or_else(|| {
                location
                    .traffic_type
                    .as_deref()
                    .map(|code| (code, &catalog.train_types, "train type"))
            });
        if let Some((code, entries, kind)) = coded {
            category = match category_label(code, entries) {
                Some(label) => label.into(),
                None => {
                    unknown_categories.insert((kind, Box::<str>::from(code)));
                    code.into()
                }
            };
        }
        let activities = &policy.activities;
        let passenger = location.activities.contains(&activities.passenger);
        for code in &location.activities {
            if !activities.known.contains(code) {
                diagnostics.push(
                    Diagnostic::new(
                        "czptt_unknown_activity",
                        Severity::Warning,
                        pa,
                        format!("sequence {}: activity {code:?} is not known", index + 1),
                    )
                    .rule(R030),
                );
            }
        }
        if passenger && location.activities.contains(&activities.unpublished_stop) {
            diagnostics.push(
                Diagnostic::new(
                    "czptt_unpublished_stop",
                    Severity::Info,
                    pa,
                    format!(
                        "sequence {}: activity {} with {} stays a passenger call",
                        index + 1,
                        activities.unpublished_stop,
                        activities.passenger
                    ),
                )
                .rule(R029),
            );
        }
        let route_type = policy.route_type(&category);
        calls.push(Call {
            index,
            country: location.country.clone(),
            code: location.primary_code.clone(),
            source_name: location.primary_name.clone(),
            subsidiary: location.subsidiary.clone(),
            arrival,
            departure,
            repaired: false,
            inferred: false,
            passenger,
            activities: location.activities.clone(),
            responsible_ru: ru.clone().unwrap_or_default(),
            category: category.clone(),
            route_type,
            projected: ServiceKey {
                line: line.clone(),
                route_type,
                category: category.clone(),
            },
            line,
            replacement_bus: last(&location.parameters, &parameters.replacement_bus)
                == Some(&*parameters.replacement_bus_value),
            train_number: location.operational_train_number.clone(),
        });
    }
    if missing_ru {
        diagnostics.push(
            Diagnostic::new(
                "czptt_responsible_ru_missing",
                Severity::Warning,
                pa,
                "no TR company and no ResponsibleRU before some calls; their operator is empty",
            )
            .rule(R033),
        );
    }
    for (kind, code) in unknown_categories {
        diagnostics.push(
            Diagnostic::new(
                "czptt_unknown_category",
                Severity::Warning,
                pa,
                format!("{kind} {code:?} is not in the catalogue; the code is the label"),
            )
            .rule(R034),
        );
    }
    for qualifier in unknown_qualifiers {
        diagnostics.push(
            Diagnostic::new(
                "czptt_unknown_timing_qualifier",
                Severity::Warning,
                pa,
                format!("timing qualifier {qualifier:?} is ignored"),
            )
            .rule(R027),
        );
    }
    calls
}

/// Arrival and departure of a location (R027, R028).
fn times(
    pa: &str,
    index: u32,
    location: &Location,
    policy: &Policy,
    unknown_qualifiers: &mut BTreeSet<Box<str>>,
    diagnostics: &mut Diagnostics,
) -> (Option<i32>, Option<i32>) {
    let timing = &policy.timing;
    for entry in &location.timings {
        if entry.qualifier != timing.arrival_qualifier
            && entry.qualifier != timing.departure_qualifier
        {
            unknown_qualifiers.insert(entry.qualifier.clone());
        }
    }
    let mut read = |qualifier: &str| {
        let entry = location
            .timings
            .iter()
            .find(|t| &*t.qualifier == qualifier)?;
        let value = seconds(entry);
        if value.is_none() {
            diagnostics.push(
                Diagnostic::new(
                    "czptt_time_out_of_range",
                    Severity::Warning,
                    pa,
                    format!(
                        "sequence {}: {qualifier} {} with day offset {} is ignored",
                        index + 1,
                        entry.time,
                        entry.day_offset
                    ),
                )
                .rule(R027),
            );
        }
        value
    };
    let arrival = read(&timing.arrival_qualifier);
    let departure = read(&timing.departure_qualifier);
    crate::timing::mirrored(arrival, departure)
}

fn seconds(timing: &Timing) -> Option<i32> {
    let time = timing.time;
    let value = i64::from(time.hour()) * 3600
        + i64::from(time.minute()) * 60
        + i64::from(time.second())
        + i64::from(timing.day_offset) * i64::from(ServiceTime::DAY);
    i32::try_from(value).ok()
}

/// R066.
fn shift_negative_times(
    pa: &str,
    calls: &mut [Call],
    dates: DateSet,
    horizon: Horizon,
    diagnostics: &mut Diagnostics,
) -> (DateSet, u16) {
    let earliest = calls
        .iter()
        .flat_map(|call| [call.arrival, call.departure])
        .flatten()
        .min();
    let Some(earliest) = earliest.filter(|seconds| *seconds < 0) else {
        return (dates, 0);
    };
    let day = i64::from(ServiceTime::DAY);
    let days = (-i64::from(earliest) + day - 1) / day;
    let shift = (days * day) as i32;
    for call in calls.iter_mut() {
        call.arrival = call.arrival.map(|seconds| seconds + shift);
        call.departure = call.departure.map(|seconds| seconds + shift);
    }
    let (moved, lost) = dates.shifted(-(days as i32), horizon.days());
    let (severity, lost_text) = if lost > 0 {
        (
            Severity::Warning,
            format!("; {lost} days fall before the horizon and are dropped"),
        )
    } else {
        (Severity::Info, String::new())
    };
    diagnostics.push(
        Diagnostic::new(
            "czptt_negative_times_shifted",
            severity,
            pa,
            format!(
                "earliest time {earliest} s: times move {days} days forward and dates {days} days back{lost_text}"
            ),
        )
        .rule(R066),
    );
    (moved, days as u16)
}
