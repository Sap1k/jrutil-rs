//! Stop positions (JDF-R075–R078): from the surviving matches, and estimated along trips for the
//! rest.

use std::collections::BTreeMap;

use jr_geo::plane;
use jr_model::Coord;

use super::policy::Policy;
use crate::fixed::{Location, Origin};

/// The two estimate methods and their `PolzastZdroj` texts.
pub(crate) const ROUTE_TIME: &str = "estimated:route-time";
pub(crate) const ROUTE_END: &str = "estimated:route-end-north";

/// R076–R078: estimates for the stops of timed calls without a precise position. `trips` are
/// the calls in travel order as (stop ID, minutes if timed); `precise` the matched positions.
pub(crate) fn estimates(
    trips: &[Vec<(u64, Option<f64>)>],
    precise: &BTreeMap<u64, Coord>,
    policy: &Policy,
) -> BTreeMap<u64, Location> {
    let estimate = &policy.file.estimate;
    let mut found: BTreeMap<u64, Vec<(Coord, Origin)>> = BTreeMap::new();
    for calls in trips {
        let anchor = |index: usize| -> Option<(Coord, f64)> {
            let (stop, time) = calls[index];
            Some((*precise.get(&stop)?, time?))
        };
        for (index, &(stop, time)) in calls.iter().enumerate() {
            let Some(time) = time else { continue };
            if precise.contains_key(&stop) {
                continue;
            }
            let before = (0..index).rev().find(|&i| anchor(i).is_some());
            let after = (index + 1..calls.len()).find(|&i| anchor(i).is_some());
            let north = |anchor_index: usize, calls_apart: usize| {
                let (coord, _) = anchor(anchor_index).expect("an anchor");
                plane::north_of(
                    coord,
                    estimate.route_end_offset_m_per_call * calls_apart as f64,
                    estimate.metres_per_degree_latitude,
                )
            };
            let value = match (before, after) {
                (Some(left), Some(right)) => {
                    let ((left_coord, left_time), (right_coord, right_time)) = (
                        anchor(left).expect("anchor"),
                        anchor(right).expect("anchor"),
                    );
                    // R076: inverted anchors give nothing (B18 kept).
                    (right_time > left_time).then(|| {
                        let ratio = ((time - left_time) / (right_time - left_time)).clamp(0.0, 1.0);
                        (
                            plane::interpolate(left_coord, right_coord, ratio),
                            Origin::RouteTime,
                        )
                    })
                }
                (None, Some(right)) => Some((north(right, right - index), Origin::RouteEnd)),
                (Some(left), None) => Some((north(left, index - left), Origin::RouteEnd)),
                (None, None) => None,
            };
            if let Some(value) = value {
                found.entry(stop).or_default().push(value);
            }
        }
    }
    found
        .into_iter()
        .map(|(stop, values)| {
            // R078: route-time estimates if any, else the route-end ones; their centroid.
            let timed = values
                .iter()
                .any(|(_, origin)| *origin == Origin::RouteTime);
            let origin = if timed {
                Origin::RouteTime
            } else {
                Origin::RouteEnd
            };
            let points: Vec<Coord> = values
                .iter()
                .filter(|(_, o)| *o == origin)
                .map(|(coord, _)| *coord)
                .collect();
            let coord = plane::centroid(&points).expect("at least one estimate");
            let source = if timed { ROUTE_TIME } else { ROUTE_END };
            (
                stop,
                Location {
                    coord,
                    origin,
                    source: source.into(),
                },
            )
        })
        .collect()
}
