//! POST-R131–R133: ordinals of the estimated posts, pinned by the registry.

use std::collections::{BTreeMap, HashSet};
use std::f64::consts::PI;

use jr_formats::geodata::registry::{Post, PostKey, Status};
use jr_model::Coord;

use crate::evaluate::RegistryConstants;

/// One location of a stop to number: its ordering key and position.
#[derive(Clone, Debug)]
pub(crate) struct Location {
    /// `estimated:` + `sha("{stop}|{hypothesis id}")`, the F# location ID; it orders the
    /// locations that take fresh ordinals (R131).
    pub key: String,
    pub coord: Coord,
}

/// `StopRegistry.distanceMetres`.
fn distance(registry: &RegistryConstants, a: Coord, b: Coord) -> f64 {
    let middle = (a.lat + b.lat) / 2.0 * PI / 180.0;
    let north = (a.lat - b.lat) * registry.metres_per_degree;
    let east = (a.lon - b.lon) * registry.metres_per_degree * middle.cos();
    (north * north + east * east).sqrt()
}

/// R132 for one stop: each active registered `est:<k>` post with a reference is reused by at
/// most one location within the match distance, closest pairs first (then ordinal, then
/// location key); the other locations, by key, take ordinals above every registered `est:`
/// ordinal of the stop, retired ones included (verdict Q13: a retired ordinal is never matched
/// or issued again). Returns the ordinal of each location and whether it is fresh.
pub(crate) fn ordinals(
    registry: &RegistryConstants,
    registered: &[&Post],
    locations: &[Location],
) -> Vec<(u32, bool)> {
    let active: Vec<(u32, Coord)> = registered
        .iter()
        .filter(|post| post.status == Status::Active)
        .filter_map(|post| match (&post.post_key, post.reference) {
            (PostKey::Estimated(ordinal), Some(point)) => Some((*ordinal, point)),
            _ => None,
        })
        .collect();
    let mut pairs: Vec<(f64, u32, usize)> = Vec::new();
    for (index, location) in locations.iter().enumerate() {
        for &(ordinal, point) in &active {
            let metres = distance(registry, location.coord, point);
            if metres <= registry.inferred_post_match_m {
                pairs.push((metres, ordinal, index));
            }
        }
    }
    pairs.sort_by(|a, b| {
        a.0.total_cmp(&b.0)
            .then(a.1.cmp(&b.1))
            .then(locations[a.2].key.cmp(&locations[b.2].key))
    });
    let mut assigned: BTreeMap<usize, u32> = BTreeMap::new();
    let mut used = HashSet::new();
    for (_, ordinal, index) in pairs {
        if !assigned.contains_key(&index) && used.insert(ordinal) {
            assigned.insert(index, ordinal);
        }
    }
    let mut next = registered
        .iter()
        .filter_map(|post| match post.post_key {
            PostKey::Estimated(ordinal) => Some(ordinal),
            PostKey::Post(_) => None,
        })
        .max()
        .unwrap_or(0);
    let mut order: Vec<usize> = (0..locations.len()).collect();
    order.sort_by(|&a, &b| locations[a].key.cmp(&locations[b].key));
    let mut result = vec![(0, false); locations.len()];
    for index in order {
        result[index] = match assigned.get(&index) {
            Some(&ordinal) => (ordinal, false),
            None => {
                next += 1;
                (next, true)
            }
        };
    }
    result
}
