//! Route estimates for passenger places without a real position (CZPTT-R099–R101).

use std::cmp::{Ordering, Reverse};

use jr_geo::plane;
use jr_model::Coord;
use jr_model::geo::distance_m;

use super::Policy;
use super::network::{Network, PathCall};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EstimateMethod {
    RouteTime,
    RouteEnd,
}

impl EstimateMethod {
    /// The F# `coordinate_match_method`.
    pub fn name(self) -> &'static str {
        match self {
            EstimateMethod::RouteTime => "route_time",
            EstimateMethod::RouteEnd => "route_end_north",
        }
    }
}

pub(super) struct Estimate {
    pub coord: Coord,
    pub method: EstimateMethod,
}

/// One estimate from one occurrence, with its ranking key (CZPTT-R100).
struct Candidate<'t> {
    coord: Coord,
    method: EstimateMethod,
    tier: u8,
    span: usize,
    distance: f64,
    service_days: usize,
    key: &'t str,
    sequence: i32,
}

impl Candidate<'_> {
    fn order(&self, other: &Self) -> Ordering {
        (self.tier, self.span)
            .cmp(&(other.tier, other.span))
            .then(self.distance.total_cmp(&other.distance))
            .then_with(|| {
                (Reverse(self.service_days), self.key, self.sequence).cmp(&(
                    Reverse(other.service_days),
                    other.key,
                    other.sequence,
                ))
            })
    }
}

/// Per point, its estimate. `real` holds the real positions only, so an estimate never anchors
/// another (CZPTT-R101).
pub(super) fn estimates(
    network: &Network<'_>,
    real: &[Option<Coord>],
    policy: &Policy,
) -> Vec<Option<Estimate>> {
    network
        .points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            if !point.passenger || real[index].is_some() {
                return None;
            }
            point
                .occurrences
                .iter()
                .flat_map(|&(path, position)| {
                    let path = &network.paths[path];
                    occurrence_candidates(&path.calls, position, path.key, real, policy)
                })
                .min_by(Candidate::order)
                .map(|best| Estimate {
                    coord: best.coord,
                    method: best.method,
                })
        })
        .collect()
}

/// The candidates of one occurrence: passenger anchors (tiers 0 and 2), then any (1 and 3).
fn occurrence_candidates<'t>(
    calls: &[PathCall],
    position: usize,
    key: &'t str,
    real: &[Option<Coord>],
    policy: &Policy,
) -> Vec<Candidate<'t>> {
    let current = calls[position];
    let Some(time) = current.reaches() else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for (passenger_only, two_sided, one_sided) in [(true, 0, 2), (false, 1, 3)] {
        let anchor = |position: usize| -> Option<plane::Anchor> {
            let call = calls[position];
            if passenger_only && !call.passenger {
                return None;
            }
            let coord = call.point.and_then(|point| real[point])?;
            Some(plane::Anchor {
                index: position,
                coord,
                time: f64::from(call.reaches()?),
            })
        };
        let previous = (0..position).rev().find_map(anchor);
        let next = (position + 1..calls.len()).find_map(anchor);
        let candidate = |coord, method, tier, span, distance| Candidate {
            coord,
            method,
            tier,
            span,
            distance,
            service_days: current.service_days,
            key,
            sequence: current.sequence,
        };
        match plane::along_route(previous, next, position, f64::from(time)) {
            Some(plane::AlongRoute::ByTime { left, right, coord }) => found.push(candidate(
                coord,
                EstimateMethod::RouteTime,
                two_sided,
                right.index - left.index,
                distance_m(left.coord, right.coord),
            )),
            Some(plane::AlongRoute::Beside {
                anchor,
                calls_apart,
            }) => {
                let rules = &policy.estimate;
                let coord = plane::north_of(
                    anchor.coord,
                    rules.route_end_offset_m_per_call * calls_apart as f64,
                    rules.metres_per_degree_latitude,
                );
                found.push(candidate(
                    coord,
                    EstimateMethod::RouteEnd,
                    one_sided,
                    calls_apart,
                    0.0,
                ));
            }
            None => {}
        }
    }
    found
}
