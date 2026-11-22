//! Matches contradicted by scheduled travel time (JDF-R069, R070).

use std::collections::{BTreeMap, BTreeSet};

use jr_model::Coord;
use jr_model::geo::distance_m;

use super::policy::Policy;

/// A matched stop as the check sees it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Matched {
    pub coord: Coord,
    pub checked: bool,
}

/// R069, R070: the stop IDs whose match is rejected. `trips` are the timed calls in travel
/// order as (stop ID, minutes); `matches` the matched stops by ID.
pub(crate) fn rejected(
    trips: &[Vec<(u64, f64)>],
    matches: &BTreeMap<u64, Matched>,
    policy: &Policy,
) -> BTreeSet<u64> {
    let plausibility = &policy.file.plausibility;
    let metres_per_minute = plausibility.speed_kmh * 1000.0 / 60.0;
    let mut rejected: BTreeSet<u64> = BTreeSet::new();
    loop {
        let mut conflicts: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
        let mut supports: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
        for trip in trips {
            let calls: Vec<&(u64, f64)> = trip
                .iter()
                .filter(|(stop, _)| matches.contains_key(stop) && !rejected.contains(stop))
                .collect();
            for pair in calls.windows(2) {
                let (&(left, left_time), &(right, right_time)) = (pair[0], pair[1]);
                let elapsed = right_time - left_time;
                if elapsed < 0.0 || left == right {
                    continue;
                }
                let distance = distance_m(matches[&left].coord, matches[&right].coord);
                let target = if distance > plausibility.slack_m + elapsed * metres_per_minute {
                    &mut conflicts
                } else {
                    &mut supports
                };
                target.entry(left).or_default().insert(right);
                target.entry(right).or_default().insert(left);
            }
        }
        if conflicts.is_empty() {
            return rejected;
        }
        // Plausible pairs that are not also conflicts chain into groups.
        let mut parent: BTreeMap<u64, u64> = BTreeMap::new();
        fn find(parent: &mut BTreeMap<u64, u64>, stop: u64) -> u64 {
            let mut root = stop;
            while let Some(&next) = parent.get(&root) {
                if next == root {
                    break;
                }
                root = next;
            }
            let mut at = stop;
            while let Some(&next) = parent.get(&at) {
                if next == root || next == at {
                    break;
                }
                parent.insert(at, root);
                at = next;
            }
            root
        }
        for (&stop, neighbours) in &supports {
            for &neighbour in neighbours {
                if conflicts.get(&stop).is_some_and(|c| c.contains(&neighbour)) {
                    continue;
                }
                let (left, right) = (find(&mut parent, stop), find(&mut parent, neighbour));
                if left != right {
                    parent.insert(left, right);
                }
            }
        }
        let mut sizes: BTreeMap<u64, usize> = BTreeMap::new();
        for &stop in matches.keys().filter(|stop| !rejected.contains(stop)) {
            *sizes.entry(find(&mut parent, stop)).or_default() += 1;
        }
        let mut strength = |stop: u64| {
            let root = find(&mut parent, stop);
            (
                sizes.get(&root).copied().unwrap_or(0),
                -(conflicts.get(&stop).map_or(0, BTreeSet::len) as i64),
                matches[&stop].checked,
            )
        };
        let mut losers = BTreeSet::new();
        for (&stop, neighbours) in &conflicts {
            for &neighbour in neighbours {
                let (left, right) = (strength(stop), strength(neighbour));
                if left > right {
                    losers.insert(neighbour);
                } else if right > left {
                    losers.insert(stop);
                } else {
                    losers.insert(stop);
                    losers.insert(neighbour);
                }
            }
        }
        rejected.extend(losers);
    }
}
