//! Route shapes (`docs/rules/serving.md`, section 8; architect's verdict 8).
//!
//! Trips without a shape are reduced to distinct match-point patterns ([`Plan`]), which an
//! external map matcher (the companion `obehy-motis-shapes`, run by the command line) routes
//! over OSM. [`shapes`] maps its answer back: an accepted pattern becomes one `motis` shape and
//! each of its trips gets that shape with a metre offset on every match-point call.
//!
//! Shapes are best effort. Nothing here fails: a pattern the companion could not route, whose
//! output is malformed or does not line up with its match points stays unshaped, and the
//! summary says why. Only published trips take part; a trip that already has a shape keeps it.

use std::collections::{BTreeMap, HashMap};

use jr_model::identity;
use jr_model::timetable::{Shape, ShapeGeneration, TripShape};
use jr_model::{
    Coord, Diagnostic, Diagnostics, Edit, EditBuilder, Edits, PublicId, Rule, Severity, ShapeId,
    Timetable,
};

mod output;
mod plan;

pub use plan::Plan;

use output::{Offset, Routed};

/// The pass name recorded in the timetable's history.
pub const PASS: &str = "route_shapes";

/// The companion's routing class of a route mode (its osr profile): bus and trolleybus are
/// `bus`; tram, metro and rail are `rail`; water is `ferry`. Other modes (cable, funicular)
/// have no profile; their trips are counted under `not_routed.unsupported_mode` and stay
/// unshaped.
pub const SERVING_R030: Rule = Rule {
    id: "SERVING-R030",
    summary: "shape class: bus/trolleybus bus, tram/metro/rail rail, water ferry, else unsupported",
};

/// A trip's match points are its calls in running order, railway points included: the
/// boarding point's position if exact or placed by the post estimator, else the place's if
/// exact, else the call is skipped (a skipped passenger call is counted as
/// `estimated_calls_skipped`). Coordinates are rounded to seven decimals; a call without
/// passenger service is a waypoint. Trips with the same class and match points share a
/// pattern, keyed by the first 20 hex digits of the SHA-256 of the class and the points
/// (section 6.3).
///
/// An estimated position along the route (route time, route end) is a guess; matching a shape
/// through it would bend the shape to the guess. An estimated post is not: the estimator only
/// chooses among real candidates, and the place's own position, a centre between its posts,
/// can lie off the road and pull the shape into a loop through side streets. The pattern key
/// is content, so the companion's cache survives runs.
pub const SERVING_R031: Rule = Rule {
    id: "SERVING-R031",
    summary: "match points: exact or estimator-placed boarding point, else exact place, else skipped; 7 decimals",
};

/// A pattern is accepted when the companion wrote a status row and at least one shape point
/// for it, whatever the status (a partly beelined shape is still taken), and its offsets line
/// up: exactly one per match point, `match_seq` 0..n each once, each pointing at a shape
/// point. Otherwise it stays unshaped with reason `companion_failed`, `malformed`, `missing`
/// (no status row), the companion's status, or `misaligned`. Patterns with identical geometry
/// share one shape, `identity::generated_shape(feed, "motis", class, points)`.
///
/// The companion is best effort and must never fail a build; output that does not line up
/// with the pattern would put offsets on the wrong calls (verdict 8).
pub const SERVING_R032: Rule = Rule {
    id: "SERVING-R032",
    summary: "a pattern with a status and shape points is accepted unless its offsets misalign",
};

/// A shaped trip's offset on a call is the `dist_m` of the pattern's offset whose `match_seq`
/// is the call's index among the trip's match points; a call that is no match point has none.
///
/// Offsets are metres from the shape's first point (serving contract 5.2).
pub const SERVING_R033: Rule = Rule {
    id: "SERVING-R033",
    summary: "trip shape offsets by match_seq on match-point calls, none on the others",
};

pub const RULES: &[Rule] = &[SERVING_R030, SERVING_R031, SERVING_R032, SERVING_R033];

/// The companion's output files, as text (serving 8.3).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompanionOutput {
    pub pattern_status: String,
    pub shapes: String,
    pub offsets: String,
    /// `manifest.json`, when the companion wrote one.
    pub manifest: Option<serde_json::Value>,
}

/// The `route_shapes` section of diagnostics.json (serving 8.5).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct Summary {
    /// Published trips.
    pub trips: usize,
    pub already_shaped: usize,
    /// Trips with a pattern, shaped or not.
    pub routed_trips: usize,
    pub patterns: usize,
    pub not_routed: NotRouted,
    pub estimated_calls_skipped: usize,
    pub shaped_patterns: usize,
    pub shapes: usize,
    pub shaped_trips: usize,
    pub unshaped_patterns: BTreeMap<String, usize>,
    /// The companion's `manifest.json`, verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub companion: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct NotRouted {
    /// Trips by route mode (its serialised name).
    pub unsupported_mode: BTreeMap<String, usize>,
}

/// An accepted pattern: the companion's points and, per match point, its offset in metres.
struct Accepted {
    points: Vec<Coord>,
    offsets: Vec<f64>,
}

/// Maps the companion's answer onto the timetable. `Err(reason)` means the companion could not
/// be run or failed: every pattern is then unshaped with reason `companion_failed`. With an
/// empty plan the companion is not run and `output` is not looked at.
///
/// Pure: returns one [`Edit::AddShape`] per new shape, in shape-ID order, then one
/// [`Edit::SetTripShape`] per shaped trip, in trip order.
pub fn shapes(
    timetable: &Timetable,
    plan: &Plan,
    output: Result<&CompanionOutput, &str>,
    diagnostics: &mut Diagnostics,
) -> (Edits, Summary) {
    let mut summary = plan.summary();
    let mut edits = EditBuilder::new(timetable, PASS);
    if plan.is_empty() {
        return (edits.finish(), summary);
    }

    let count = plan.patterns.len();
    let outcomes: Vec<Result<Accepted, String>> = match output {
        Err(reason) => {
            diagnostics.push(
                Diagnostic::new(
                    "route_shapes_companion_failed",
                    Severity::Warning,
                    PASS,
                    format!("{count} patterns left unshaped: {reason}"),
                )
                .rule(SERVING_R032),
            );
            (0..count).map(|_| Err("companion_failed".into())).collect()
        }
        Ok(output) => {
            summary.companion.clone_from(&output.manifest);
            match output::read(plan, output) {
                Err(problem) => {
                    diagnostics.push(
                        Diagnostic::new(
                            "route_shapes_malformed",
                            Severity::Warning,
                            PASS,
                            format!("{count} patterns left unshaped: {problem}"),
                        )
                        .rule(SERVING_R032),
                    );
                    (0..count).map(|_| Err("malformed".into())).collect()
                }
                Ok(routed) => {
                    if routed.unknown_rows > 0 {
                        diagnostics.push(
                            Diagnostic::new(
                                "route_shapes_malformed",
                                Severity::Warning,
                                PASS,
                                format!(
                                    "{} companion rows name no planned pattern; ignored",
                                    routed.unknown_rows
                                ),
                            )
                            .rule(SERVING_R032),
                        );
                    }
                    let subjects = plan.subjects(timetable);
                    (0..count)
                        .map(|pattern| {
                            decide(plan, &routed, pattern, subjects[pattern], diagnostics)
                        })
                        .collect()
                }
            }
        }
    };

    // Shapes by ID: patterns with identical geometry share one (SERVING-R032).
    let mut shape_of_pattern: Vec<Option<PublicId>> = vec![None; count];
    let mut new_shapes: BTreeMap<PublicId, &[Coord]> = BTreeMap::new();
    for (pattern, outcome) in outcomes.iter().enumerate() {
        match outcome {
            Ok(accepted) => {
                let id = identity::generated_shape(
                    timetable.feed,
                    "motis",
                    plan.patterns[pattern].class,
                    &accepted.points,
                );
                new_shapes.entry(id.clone()).or_insert(&accepted.points);
                shape_of_pattern[pattern] = Some(id);
                summary.shaped_patterns += 1;
            }
            Err(reason) => *summary.unshaped_patterns.entry(reason.clone()).or_default() += 1,
        }
    }
    summary.shapes = new_shapes.len();
    // A shape this timetable already holds (same ID, so same geometry) is reused.
    let existing: HashMap<&str, ShapeId> = timetable
        .shapes
        .iter()
        .map(|(handle, shape)| (shape.id.as_str(), handle))
        .collect();
    let mut handles: BTreeMap<PublicId, ShapeId> = BTreeMap::new();
    for (id, points) in new_shapes {
        let handle = match existing.get(id.as_str()) {
            Some(&handle) => handle,
            None => edits.add_shape(
                SERVING_R032,
                Shape {
                    id: id.clone(),
                    generation: ShapeGeneration::Motis,
                    points: points.to_vec(),
                },
            ),
        };
        handles.insert(id, handle);
    }

    for planned in &plan.trips {
        let (Some(id), Ok(accepted)) = (
            &shape_of_pattern[planned.pattern],
            &outcomes[planned.pattern],
        ) else {
            continue;
        };
        let mut offsets = vec![None; timetable.trips[planned.trip].calls.len()];
        for (&call, &metres) in planned.calls.iter().zip(&accepted.offsets) {
            offsets[call as usize] = Some(metres);
        }
        edits.push(
            SERVING_R033,
            Edit::SetTripShape {
                trip: planned.trip,
                shape: Some(TripShape {
                    shape: handles[id],
                    offsets: offsets.into_boxed_slice(),
                }),
            },
        );
        summary.shaped_trips += 1;
    }
    (edits.finish(), summary)
}

/// SERVING-R032 for one pattern.
fn decide(
    plan: &Plan,
    routed: &Routed,
    pattern: usize,
    subject: &str,
    diagnostics: &mut Diagnostics,
) -> Result<Accepted, String> {
    let id = &plan.patterns[pattern].id;
    if let Some(problem) = &routed.malformed[pattern] {
        diagnostics.push(
            Diagnostic::new(
                "route_shapes_malformed",
                Severity::Warning,
                subject,
                format!("pattern {id}: {problem}"),
            )
            .rule(SERVING_R032),
        );
        return Err("malformed".into());
    }
    let Some(status) = &routed.status[pattern] else {
        return Err("missing".into());
    };
    let points = &routed.points[pattern];
    if points.is_empty() {
        return Err(status.clone());
    }
    let match_points = plan.patterns[pattern].points.len();
    match line_up(&routed.offsets[pattern], match_points, points.len()) {
        Ok(offsets) => Ok(Accepted {
            points: points.clone(),
            offsets,
        }),
        Err(problem) => {
            diagnostics.push(
                Diagnostic::new(
                    "route_shapes_misaligned",
                    Severity::Warning,
                    subject,
                    format!("pattern {id}: {problem}; discarded"),
                )
                .rule(SERVING_R032),
            );
            Err("misaligned".into())
        }
    }
}

/// The offset of every match point, by `match_seq`, or why the offsets do not line up.
fn line_up(
    offsets: &[Offset],
    match_points: usize,
    shape_points: usize,
) -> Result<Vec<f64>, String> {
    if offsets.len() != match_points {
        return Err(format!(
            "{} offsets for {match_points} match points",
            offsets.len()
        ));
    }
    let mut by_point = vec![None; match_points];
    for offset in offsets {
        if offset.point_index >= shape_points {
            return Err(format!(
                "offset {} points at shape point {} of {shape_points}",
                offset.match_seq, offset.point_index
            ));
        }
        match by_point.get_mut(offset.match_seq) {
            Some(slot @ None) => *slot = Some(offset.dist_m),
            Some(Some(_)) => return Err(format!("match point {} twice", offset.match_seq)),
            None => return Err(format!("no match point {}", offset.match_seq)),
        }
    }
    // Exactly `match_points` distinct in-range slots were filled.
    Ok(by_point.into_iter().flatten().collect())
}

#[cfg(test)]
mod tests;
