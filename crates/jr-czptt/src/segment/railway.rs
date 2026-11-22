//! Railway points: the path's non-passenger calls, attached to the part that runs through them
//! (R117, serving contract §3.3).

use jr_model::{Diagnostic, Diagnostics, Rule, Severity};

use crate::path::{Call, Part};

/// Trips carry the points the train passes (Q15) so that positions and running times between
/// stops survive. A point belongs to the rail part whose passenger calls enclose it; points
/// before the first or after the last passenger call belong to the first or last part. A
/// replacement bus does not run along the railway, so its part gets none, and a point between
/// two parts that do not meet belongs to neither.
pub(crate) const R117: Rule = Rule {
    id: "CZPTT-R117",
    summary: "A non-passenger call joins the rail part that encloses it, or the opening or closing part",
};

pub(crate) fn assign(pa: &str, parts: &mut [Part], path: &[Call], diagnostics: &mut Diagnostics) {
    let bounds: Vec<(u32, u32)> = parts
        .iter()
        .map(|part| {
            let mut indices = part.calls.iter().map(|call| call.index);
            let first = indices.next().unwrap_or_default();
            indices.fold((first, first), |(low, high), index| {
                (low.min(index), high.max(index))
            })
        })
        .collect();
    let (Some(path_low), Some(path_high)) = (
        bounds.iter().map(|bound| bound.0).min(),
        bounds.iter().map(|bound| bound.1).max(),
    ) else {
        return;
    };
    let mut points: Vec<Vec<Call>> = vec![Vec::new(); parts.len()];
    let mut unassigned = Vec::new();
    for call in path.iter().filter(|call| !call.passenger) {
        let index = call.index;
        let owner = parts.iter().zip(&bounds).position(|(part, &(low, high))| {
            // R047: a replacement-bus part takes none.
            !part.key.replacement_bus
                && ((low < index && index < high)
                    || (low == path_low && index < low)
                    || (high == path_high && index > high))
        });
        match owner {
            Some(owner) => points[owner].push(call.clone()),
            None => unassigned.push(call.sequence().to_string()),
        }
    }
    for (part, points) in parts.iter_mut().zip(points) {
        if points.is_empty() {
            continue;
        }
        part.calls.extend(points);
        part.calls.sort_by_key(|call| call.index);
    }
    if !unassigned.is_empty() {
        diagnostics.push(
            Diagnostic::new(
                "czptt_railway_points_unassigned",
                Severity::Info,
                pa,
                format!(
                    "railway points at sequence {} belong to no rail part",
                    unassigned.join(",")
                ),
            )
            .rule(R117),
        );
    }
}
