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
        let anchor = |index: usize| -> Option<plane::Anchor> {
            let (stop, time) = calls[index];
            Some(plane::Anchor {
                index,
                coord: *precise.get(&stop)?,
                time: time?,
            })
        };
        for (index, &(stop, time)) in calls.iter().enumerate() {
            let Some(time) = time else { continue };
            if precise.contains_key(&stop) {
                continue;
            }
            let before = (0..index).rev().find_map(anchor);
            let after = (index + 1..calls.len()).find_map(anchor);
            // R076, R077.
            let value = plane::along_route(before, after, index, time).map(|along| match along {
                plane::AlongRoute::ByTime { coord, .. } => (coord, Origin::RouteTime),
                plane::AlongRoute::Beside {
                    anchor,
                    calls_apart,
                } => (
                    plane::north_of(
                        anchor.coord,
                        estimate.route_end_offset_m_per_call * calls_apart as f64,
                        estimate.metres_per_degree_latitude,
                    ),
                    Origin::RouteEnd,
                ),
            });
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
