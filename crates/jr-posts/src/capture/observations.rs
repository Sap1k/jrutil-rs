//! POST-R030–R033: observations admitted near their stop, grouped into route points, and the
//! stops' anchors (R045).

use std::collections::{BTreeMap, HashMap};
use std::f64::consts::PI;
use std::ops::Range;

use jr_formats::post_evidence::ids::stable_id;
use jr_formats::post_evidence::{Observations, RoutePoints};
use jr_jdf::assemble::PostFacts;
use jr_jdf::merge::PostCandidate;
use jr_model::Coord;

use crate::evaluate::Constants;

/// The admitted evidence of every stop.
#[derive(Debug, Default)]
pub(crate) struct Admitted {
    pub observations: Observations,
    pub route_points: RoutePoints,
    /// Stop number → its route-point rows.
    pub points: BTreeMap<u64, Range<usize>>,
    /// Stop number → its anchor coordinate (R045): the medoid of its route points, else its
    /// precise position.
    pub anchors: HashMap<u64, Coord>,
    /// Stops with a precise position whose candidates were all beyond the radius.
    pub stops_without_points: usize,
}

/// The squared local distance of the capture (F# `distanceSquared`).
fn distance_squared(constants: &Constants, a: Coord, b: Coord) -> f64 {
    let metric = &constants.metric;
    let latitude = (a.lat + b.lat) * PI / 360.0;
    let dx = (a.lon - b.lon) * metric.metres_per_degree_longitude * latitude.cos();
    let dy = (a.lat - b.lat) * metric.metres_per_degree_latitude;
    dx * dx + dy * dy
}

fn fact<'a>(candidate: &'a PostCandidate, key: &str) -> &'a str {
    candidate
        .facts
        .iter()
        .find(|(name, _)| &**name == key)
        .map_or("", |(_, value)| value)
}

/// The F# `raw_tags`: the audit tags as `key=value` joined `;`, a `;` in a value written `,`.
fn raw_tags(candidate: &PostCandidate) -> String {
    let pairs: Vec<String> = candidate
        .facts
        .iter()
        .filter_map(|(name, value)| {
            name.strip_prefix("tag:")
                .map(|key| format!("{key}={}", value.replace(';', ",")))
        })
        .collect();
    pairs.join(";")
}

/// R032: mode tokens split on `; , space`, trimmed, upper-cased, distinct, sorted, joined `;`.
fn modes<'a>(values: impl Iterator<Item = &'a str>) -> String {
    let mut tokens: Vec<String> = values
        .flat_map(|value| value.split([';', ',', ' ']))
        .map(|token| token.trim().to_uppercase())
        .filter(|token| !token.is_empty())
        .collect();
    tokens.sort();
    tokens.dedup();
    tokens.join(";")
}

/// R031: `route-point:` + `sha("{stop}|{lat}|{lon}")` with the coordinate as the source gives
/// it, in the shortest text that reads back as the same `f64`; equal coordinates, equal ID.
/// F# hashed the text of its own EPSG:3035 round trip of the coordinate, so the IDs differ
/// from F#'s (verdict Q1: the pack is format-compatible, not identical).
pub(crate) fn route_point_id(stop: u64, coord: Coord) -> String {
    stable_id(
        "route-point:",
        &[
            &stop.to_string(),
            &coord.lat.to_string(),
            &coord.lon.to_string(),
        ],
    )
}

/// R045 with POST-R191: the anchor of a stop's route points `rows`: the medoid (least sum of
/// squared distances); on a tie the point nearest the stop's precise position; then the
/// smaller latitude, then the smaller longitude.
pub(crate) fn anchor(
    constants: &Constants,
    points: &RoutePoints,
    rows: std::ops::Range<usize>,
    precise: Option<Coord>,
) -> Coord {
    let coord = |row: usize| Coord::new(points.longitude[row], points.latitude[row]);
    let key = |row: usize| {
        let sum: f64 = rows
            .clone()
            .map(|other| distance_squared(constants, coord(row), coord(other)))
            .sum();
        let near = precise.map_or(0.0, |stop| distance_squared(constants, coord(row), stop));
        (sum, near, points.latitude[row], points.longitude[row])
    };
    let best = rows
        .clone()
        .min_by(|&a, &b| {
            let (ka, kb) = (key(a), key(b));
            ka.0.total_cmp(&kb.0)
                .then(ka.1.total_cmp(&kb.1))
                .then(ka.2.total_cmp(&kb.2))
                .then(ka.3.total_cmp(&kb.3))
        })
        .unwrap_or(rows.start);
    coord(best)
}

/// R030–R033 and the anchors.
pub(crate) fn admit(facts: &PostFacts, constants: &Constants) -> Admitted {
    let capture = &constants.capture;
    let radius = capture.max_distance_from_stop_m * capture.max_distance_from_stop_m;
    let mut admitted = Admitted::default();
    for stop in &facts.stops {
        let Some(precise) = stop.precise else {
            continue;
        };
        admitted.anchors.insert(stop.number, precise);
        // R030: within the radius of the precise position; R031: grouped by coordinate.
        let mut groups: BTreeMap<String, Vec<&PostCandidate>> = BTreeMap::new();
        for candidate in &stop.candidates {
            if distance_squared(constants, candidate.coord, precise) <= radius {
                groups
                    .entry(route_point_id(stop.number, candidate.coord))
                    .or_default()
                    .push(candidate);
            }
        }
        if groups.is_empty() {
            if !stop.candidates.is_empty() {
                admitted.stops_without_points += 1;
            }
            continue;
        }
        let first = admitted.route_points.len();
        let mut observations: Vec<(&str, u32, &PostCandidate)> = Vec::new();
        for (id, members) in &groups {
            let row = admitted.route_points.len() as u32;
            let points = &mut admitted.route_points;
            let lifecycles: Vec<String> = members
                .iter()
                .map(|candidate| fact(candidate, "lifecycle").trim().to_lowercase())
                .collect();
            let representative = members
                .iter()
                .map(|candidate| &*candidate.observation)
                .min()
                .unwrap_or("");
            points.stop.push(stop.number);
            points.route_point_id.push(id.as_str().into());
            points
                .representative_observation_id
                .push(representative.into());
            points.latitude.push(members[0].coord.lat);
            points.longitude.push(members[0].coord.lon);
            points.has_current_lifecycle.push(
                lifecycles
                    .iter()
                    .any(|value| capture.current_lifecycles.iter().any(|c| **c == **value)),
            );
            points.has_obsolete_lifecycle.push(
                lifecycles
                    .iter()
                    .any(|value| capture.obsolete_lifecycles.iter().any(|c| **c == **value)),
            );
            // R032: every observation weighs 1 (the F# support weight was always 1).
            points.source_support_weight.push(members.len() as f64);
            points
                .explicit_modes
                .push(modes(members.iter().map(|c| fact(c, "explicit_modes"))).into());
            points
                .denied_modes
                .push(modes(members.iter().map(|c| fact(c, "denied_modes"))).into());
            for candidate in members {
                observations.push((&candidate.observation, row, candidate));
            }
        }
        observations.sort_by(|a, b| a.0.cmp(b.0));
        for (id, row, candidate) in observations {
            let table = &mut admitted.observations;
            table.route_point.push(row);
            table.observation_id.push(id.into());
            table
                .source_kind
                .push(fact(candidate, "source_kind").into());
            table.source_object_id.push(Some(id.into()));
            table.observed_at.push(None);
            table.latitude.push(candidate.coord.lat);
            table.longitude.push(candidate.coord.lon);
            table.support_weight.push(1.0);
            table.raw_tags.push(raw_tags(candidate).into());
            table
                .explicit_modes
                .push(fact(candidate, "explicit_modes").into());
            table
                .denied_modes
                .push(fact(candidate, "denied_modes").into());
            table.lifecycle.push(fact(candidate, "lifecycle").into());
        }
        let range = first..admitted.route_points.len();
        let anchor = anchor(
            constants,
            &admitted.route_points,
            range.clone(),
            Some(precise),
        );
        admitted.anchors.insert(stop.number, anchor);
        admitted.points.insert(stop.number, range);
    }
    admitted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points(coords: &[(f64, f64)]) -> RoutePoints {
        let mut points = RoutePoints::default();
        for &(lat, lon) in coords {
            points.latitude.push(lat);
            points.longitude.push(lon);
        }
        points
    }

    #[test]
    fn post_r191_an_anchor_tie_goes_to_the_point_nearest_the_stop_then_south_then_west() {
        let constants = Constants::builtin();
        // Two points always tie as the medoid.
        let pair = points(&[(50.0001, 14.0), (50.0, 14.0)]);
        let north = Coord::new(14.0, 50.0001);
        let south = Coord::new(14.0, 50.0);
        assert_eq!(
            anchor(&constants, &pair, 0..2, Some(Coord::new(14.0, 50.00009))),
            north
        );
        assert_eq!(
            anchor(&constants, &pair, 0..2, Some(Coord::new(14.0, 49.9))),
            south
        );
        assert_eq!(
            anchor(&constants, &pair, 0..2, None),
            south,
            "the smaller latitude"
        );
        let row = points(&[(50.0, 14.0001), (50.0, 14.0)]);
        assert_eq!(anchor(&constants, &row, 0..2, None), Coord::new(14.0, 50.0));
        // A clear medoid wins whatever the stop's position.
        let three = points(&[(50.0, 14.0), (50.0001, 14.0), (50.0002, 14.0)]);
        assert_eq!(
            anchor(&constants, &three, 0..3, Some(Coord::new(14.0, 50.0))),
            Coord::new(14.0, 50.0001)
        );
    }
}
