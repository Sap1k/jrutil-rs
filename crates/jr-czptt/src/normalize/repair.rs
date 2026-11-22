//! Time repair and the whole-path checks (R037–R039).

use jr_formats::czptt::Location;
use jr_model::{Diagnostic, Diagnostics, Rule, Severity};

use crate::path::{Call, Rejection, RejectionReason};
use crate::policy::Policy;
use crate::timing::{interpolate, leaves, reached};

/// SŽ flags locations whose planned times are known to be inconsistent
/// (`CZInconsistentTime=1`, location level only; the root-level flag is ignored). A flagged
/// time outside its timed neighbours is replaced by one interpolated between them.
pub(crate) const R037: Rule = Rule {
    id: "CZPTT-R037",
    summary: "A flagged inconsistent time is bounded by its timed neighbours",
};

/// A path no passenger can board is not a passenger service.
pub(crate) const R038: Rule = Rule {
    id: "CZPTT-R038",
    summary: "A path without a passenger call is rejected",
};

/// Times that go backwards cannot be published; nothing says which one is wrong.
pub(crate) const R039: Rule = Rule {
    id: "CZPTT-R039",
    summary: "A path whose times go backwards is rejected",
};

fn format_time(value: Option<i32>) -> String {
    value.map_or_else(|| "-".to_owned(), |seconds| seconds.to_string())
}

/// R037. Earlier neighbours are read after their own repair; later ones as the source has them.
pub(crate) fn repair_inconsistent_times(
    pa: &str,
    locations: &[Location],
    calls: &mut [Call],
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) {
    let parameters = &policy.parameters;
    let flagged: Vec<bool> = locations
        .iter()
        .map(|location| {
            location.parameters.iter().any(|parameter| {
                parameter.name == parameters.inconsistent_time
                    && parameter.value == parameters.inconsistent_time_value
            })
        })
        .collect();
    for position in 0..calls.len() {
        if !flagged[position] {
            continue;
        }
        let left = calls[..position]
            .iter()
            .rev()
            .find_map(|call| leaves(call).map(|time| (call.index, time)));
        let right = calls[position + 1..]
            .iter()
            .zip(&flagged[position + 1..])
            .filter(|(_, flagged)| !**flagged)
            .find_map(|(call, _)| reached(call).map(|time| (call.index, time)));
        let (Some(left), Some(right)) = (left, right) else {
            continue;
        };
        let call = &calls[position];
        let Some(inferred) = interpolate(left, right, call.index) else {
            continue;
        };
        let arrival = reached(call)
            .filter(|time| (left.1..=right.1).contains(time))
            .unwrap_or(inferred);
        let departure = leaves(call)
            .filter(|time| (arrival..=right.1).contains(time))
            .unwrap_or(arrival);
        if call.arrival == Some(arrival) && call.departure == Some(departure) {
            continue;
        }
        diagnostics.push(
            Diagnostic::new(
                "czptt_inconsistent_time_repaired",
                Severity::Info,
                pa,
                format!(
                    "sequence {}: arrival {}, departure {} became arrival {arrival}, departure {departure}",
                    call.sequence(),
                    format_time(call.arrival),
                    format_time(call.departure)
                ),
            )
            .rule(R037),
        );
        let call = &mut calls[position];
        call.arrival = Some(arrival);
        call.departure = Some(departure);
        call.repaired = true;
    }
}

/// R039 over every call in source order; the first violation rejects the path.
pub(crate) fn chronology(
    pa: &str,
    calls: &[Call],
    diagnostics: &mut Diagnostics,
) -> Option<Rejection> {
    let mut previous: Option<i32> = None;
    let mut rejection = None;
    for call in calls {
        if let (Some(arrival), Some(departure)) = (call.arrival, call.departure)
            && departure < arrival
        {
            rejection = Some(Rejection {
                reason: RejectionReason::DeparturePrecedesArrival,
                sequence: Some(call.sequence()),
                previous_seconds: Some(arrival),
                current_seconds: Some(departure),
            });
            break;
        }
        if let (Some(prior), Some(current)) = (previous, reached(call))
            && current < prior
        {
            rejection = Some(Rejection {
                reason: RejectionReason::EventPrecedesPreviousEvent,
                sequence: Some(call.sequence()),
                previous_seconds: Some(prior),
                current_seconds: Some(current),
            });
            break;
        }
        previous = leaves(call).or(previous);
    }
    let found = rejection.as_ref()?;
    let what = match found.reason {
        RejectionReason::DeparturePrecedesArrival => "departure precedes arrival",
        _ => "event precedes the previous event",
    };
    diagnostics.push(
        Diagnostic::new(
            "czptt_path_chronology",
            Severity::Warning,
            pa,
            format!(
                "sequence {}: {what} ({} s after {} s); the path is rejected",
                found.sequence.unwrap_or_default(),
                format_time(found.current_seconds),
                format_time(found.previous_seconds)
            ),
        )
        .rule(R039),
    );
    rejection
}
