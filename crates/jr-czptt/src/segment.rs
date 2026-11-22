//! Cutting an accepted path into trip parts (R041–R047, R051), timing their edges (R048–R050)
//! and giving them their railway points (R117).

mod edges;
mod railway;

use jr_model::{Diagnostic, Diagnostics, Rule, Severity};

use crate::path::{Call, Part, Rejection, RejectionReason, SegmentKey};
use crate::policy::Policy;

pub(crate) use edges::{R048, R049, R050};
pub(crate) use railway::R117;

/// Parts are cut from the calls a passenger can use (Q15: there is no operational-point mode;
/// railway points are attached afterwards by R117).
pub(crate) const R041: Rule = Rule {
    id: "CZPTT-R041",
    summary: "Trip parts are cut from the path's passenger calls",
};

/// SŽ writes one location per track/subsidiary when a train changes tracks inside a station;
/// to a passenger it is one stop. A whole run of such rows becomes one call.
pub(crate) const R042: Rule = Rule {
    id: "CZPTT-R042",
    summary: "Consecutive subsidiary rows of one point with equal keys collapse into one call",
};

/// A trip needs two calls.
pub(crate) const R043: Rule = Rule {
    id: "CZPTT-R043",
    summary: "A path with fewer than two passenger calls is rejected",
};

/// A change of line, operator, category or replacement bus splits the path into parts that
/// share the boundary call. A change at the final call has nothing left to describe (Q8).
pub(crate) const R044: Rule = Rule {
    id: "CZPTT-R044",
    summary: "A path splits where its segment key changes; a change at the final call is ignored",
};

/// A replacement-bus part is a bus, whatever the train's category says.
pub(crate) const R045: Rule = Rule {
    id: "CZPTT-R045",
    summary: "A replacement-bus part has the replacement-bus route type",
};

/// Rail and replacement bus must not form one vehicle block.
pub(crate) const R046: Rule = Rule {
    id: "CZPTT-R046",
    summary: "The mode block counts switches between rail and replacement bus",
};

/// A replacement bus runs between public stops; one that serves fewer than two is not a trip.
/// Its number stays unused so the other parts keep theirs.
pub(crate) const R047: Rule = Rule {
    id: "CZPTT-R047",
    summary: "A replacement-bus part keeps its passenger calls and is dropped below two",
};

/// An operator change written at a non-passenger point becomes visible only at the next
/// passenger call; the move is reported (replaces the first-change-only diagnostic, B14).
pub(crate) const R051: Rule = Rule {
    id: "CZPTT-R051",
    summary: "An operator change at a non-passenger point inside the passenger window is diagnosed",
};

type RawKey = (Option<Box<str>>, Box<str>, i32, Box<str>, bool);

fn raw_key(call: &Call) -> RawKey {
    (
        call.projected.line.clone(),
        call.responsible_ru.clone(),
        call.projected.route_type,
        call.projected.category.clone(),
        call.replacement_bus,
    )
}

/// The parts of an accepted path, or why it has none.
pub(crate) fn segment(
    pa: &str,
    calls: &[Call],
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> Result<Vec<Part>, Rejection> {
    let minimum = policy.parts.minimum_calls;
    let selected = collapse(pa, calls.iter().filter(|call| call.passenger), diagnostics);
    if selected.len() < minimum {
        diagnostics.push(
            Diagnostic::new(
                "czptt_path_too_few_passenger_calls",
                Severity::Warning,
                pa,
                format!(
                    "{} passenger calls, fewer than {minimum}; the path is rejected",
                    selected.len()
                ),
            )
            .rule(R043),
        );
        return Err(Rejection {
            reason: RejectionReason::TooFewPassengerCalls,
            sequence: None,
            previous_seconds: None,
            current_seconds: None,
        });
    }

    // R044: (first, last) positions in `selected`, inclusive, and the key.
    let mut segments: Vec<(usize, usize, RawKey)> = Vec::new();
    let mut start = 0;
    let mut key = raw_key(&selected[0]);
    let last = selected.len() - 1;
    for (position, call) in selected.iter().enumerate().skip(1) {
        let current = raw_key(call);
        if current != key && position < last {
            segments.push((start, position, key));
            start = position;
            key = current;
        }
    }
    if raw_key(&selected[last]) != key {
        diagnostics.push(
            Diagnostic::new(
                "czptt_terminal_change_ignored",
                Severity::Info,
                pa,
                format!(
                    "the change at the final passenger call (sequence {}) is ignored",
                    selected[last].sequence()
                ),
            )
            .rule(R044),
        );
    }
    segments.push((start, last, key));

    let mut parts = Vec::with_capacity(segments.len());
    let mut mode_block = 1u16;
    let mut previous_mode = None;
    for (number, (first, last, key)) in segments.into_iter().enumerate() {
        let (line, responsible_ru, rail_route_type, category, replacement_bus) = key;
        // Counted over every segment, dropped ones included, so blocks do not move.
        if previous_mode.is_some_and(|previous| previous != replacement_bus) {
            mode_block += 1;
        }
        previous_mode = Some(replacement_bus);
        let number = number as u16 + 1;
        let mut part_calls = selected[first..=last].to_vec();
        if replacement_bus {
            part_calls.retain(|call| call.passenger);
            if part_calls.len() < minimum {
                diagnostics.push(
                    Diagnostic::new(
                        "czptt_replacement_bus_part_dropped",
                        Severity::Warning,
                        pa,
                        format!(
                            "part {number}: {} passenger calls, fewer than {minimum}",
                            part_calls.len()
                        ),
                    )
                    .rule(R047),
                );
                continue;
            }
        }
        parts.push(Part {
            number,
            key: SegmentKey {
                line,
                responsible_ru,
                route_type: if replacement_bus {
                    policy.route_types.replacement_bus
                } else {
                    rail_route_type
                },
                category,
                replacement_bus,
            },
            mode_block,
            calls: part_calls,
        });
    }

    let mut parts = edges::reconcile(pa, parts, calls, diagnostics);
    railway::assign(pa, &mut parts, calls, diagnostics);
    operator_boundaries(pa, calls, diagnostics);
    Ok(parts)
}

/// R042 over the selected calls. A run is extended while each row follows the last one merged.
fn collapse<'a>(
    pa: &str,
    selected: impl Iterator<Item = &'a Call>,
    diagnostics: &mut Diagnostics,
) -> Vec<Call> {
    let mut kept: Vec<Call> = Vec::new();
    let mut last_merged = 0u32;
    for call in selected {
        if let Some(prior) = kept.last_mut()
            && last_merged + 1 == call.index
            && prior.subsidiary.is_some()
            && call.subsidiary.is_some()
            && prior.country == call.country
            && prior.code == call.code
            && raw_key(prior) == raw_key(call)
        {
            prior.passenger |= call.passenger;
            prior.arrival = prior.arrival.or(call.arrival);
            prior.departure = call.departure.or(prior.departure);
            diagnostics.push(
                Diagnostic::new(
                    "czptt_subsidiary_rows_collapsed",
                    Severity::Info,
                    pa,
                    format!(
                        "sequence {} is merged into sequence {}",
                        call.sequence(),
                        prior.sequence()
                    ),
                )
                .rule(R042),
            );
            last_merged = call.index;
            continue;
        }
        last_merged = call.index;
        kept.push(call.clone());
    }
    kept
}

/// R051.
fn operator_boundaries(pa: &str, calls: &[Call], diagnostics: &mut Diagnostics) {
    let first = calls.iter().position(|call| call.passenger);
    let last = calls.iter().rposition(|call| call.passenger);
    let (Some(first), Some(last)) = (first, last) else {
        return;
    };
    for index in first + 1..last {
        let call = &calls[index];
        if !call.passenger && call.responsible_ru != calls[index - 1].responsible_ru {
            diagnostics.push(
                Diagnostic::new(
                    "czptt_operator_boundary_moved",
                    Severity::Info,
                    pa,
                    format!(
                        "the operator change at sequence {} moves to the next passenger call",
                        call.sequence()
                    ),
                )
                .rule(R051),
            );
        }
    }
}
