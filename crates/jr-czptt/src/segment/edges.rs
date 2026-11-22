//! Timing the first and last call of every part (R048–R050).

use jr_model::{Diagnostic, Diagnostics, Rule, Severity};

use crate::path::{Call, Part};
use crate::timing::{interpolate, is_timed, leaves, mirrored, reached};

/// Where adjacent parts meet at one point and only one side is timed, the source timed the
/// point once; the other side takes that time.
pub(crate) const R048: Rule = Rule {
    id: "CZPTT-R048",
    summary: "An untimed part edge copies the timed edge of its neighbour at the same point",
};

/// A part edge the source left untimed is placed linearly by source index between the
/// nearest timed calls of the whole path.
pub(crate) const R049: Rule = Rule {
    id: "CZPTT-R049",
    summary: "An untimed part edge is interpolated between the path's timed calls",
};

/// A trip cannot start or end without a time.
pub(crate) const R050: Rule = Rule {
    id: "CZPTT-R050",
    summary: "A part whose edge stays untimed is dropped",
};

fn copy_timing(source: &Call, target: &mut Call) {
    (target.arrival, target.departure) = mirrored(source.arrival, source.departure);
    target.inferred = true;
}

pub(crate) fn reconcile(
    pa: &str,
    mut parts: Vec<Part>,
    path: &[Call],
    diagnostics: &mut Diagnostics,
) -> Vec<Part> {
    for position in 1..parts.len() {
        let (before, after) = parts.split_at_mut(position);
        let (Some(left), Some(right)) = (
            before[position - 1].calls.last_mut(),
            after[0].calls.first_mut(),
        ) else {
            continue;
        };
        if (&left.country, &left.code) != (&right.country, &right.code) {
            continue;
        }
        let copied = if is_timed(left) && !is_timed(right) {
            copy_timing(left, right);
            right.sequence()
        } else if is_timed(right) && !is_timed(left) {
            copy_timing(right, left);
            left.sequence()
        } else {
            continue;
        };
        diagnostics.push(
            Diagnostic::new(
                "czptt_edge_time_copied",
                Severity::Info,
                pa,
                format!("sequence {copied} takes the time of its neighbouring part's edge"),
            )
            .rule(R048),
        );
    }

    parts.retain_mut(|part| {
        let last = part.calls.len() - 1;
        for position in [0, last] {
            let call = &mut part.calls[position];
            if is_timed(call) {
                continue;
            }
            let index = call.index as usize;
            let left = path[..index]
                .iter()
                .rev()
                .find_map(|other| leaves(other).map(|time| (other.index, time)));
            let right = path[index + 1..]
                .iter()
                .find_map(|other| reached(other).map(|time| (other.index, time)));
            let (Some(left), Some(right)) = (left, right) else {
                continue;
            };
            let Some(time) = interpolate(left, right, call.index) else {
                continue;
            };
            call.arrival = Some(time);
            call.departure = Some(time);
            call.inferred = true;
            diagnostics.push(
                Diagnostic::new(
                    "czptt_edge_time_interpolated",
                    Severity::Info,
                    pa,
                    format!(
                        "part {}: sequence {} is placed at {time} s",
                        part.number,
                        call.sequence()
                    ),
                )
                .rule(R049),
            );
        }
        let untimed: Vec<String> = [0, last]
            .into_iter()
            .filter(|&position| !is_timed(&part.calls[position]))
            .map(|position| part.calls[position].sequence().to_string())
            .collect();
        if untimed.is_empty() {
            return true;
        }
        diagnostics.push(
            Diagnostic::new(
                "czptt_part_untimed_edge",
                Severity::Warning,
                pa,
                format!(
                    "part {} is dropped: no time can be inferred at sequence {}",
                    part.number,
                    untimed.join(",")
                ),
            )
            .rule(R050),
        );
        false
    });
    parts
}
