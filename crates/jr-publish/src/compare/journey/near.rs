//! Steps 3 and 4 of the trip pairing, over the trips content did not pair: near pairs (same
//! route, dates, call count and first call) and pairs by dates (the same without equal dates,
//! by the most dates in common).

use super::{Calls, DEPARTURE, Input, LOCATION_ID, ROUTE_ID, SERVICE_ID, column};
use crate::compare::cells::Pair;
use crate::validate::keys;

/// The columns a near key reads.
struct Key<'p, 'a> {
    route: &'p Pair<'a>,
    service: &'p Pair<'a>,
    location: &'p Pair<'a>,
    departure: &'p Pair<'a>,
}

impl<'p, 'a> Key<'p, 'a> {
    fn new(input: &'p Input<'a>) -> Option<Key<'p, 'a>> {
        Some(Key {
            route: column(input.values, ROUTE_ID)?,
            service: column(input.values, SERVICE_ID)?,
            location: column(input.calls, LOCATION_ID)?,
            departure: column(input.calls, DEPARTURE)?,
        })
    }
}

/// The trips left by content (indexes into `sides`) and their calls.
pub(super) struct Left<'s> {
    pub sides: &'s [Vec<u32>; 2],
    pub calls: &'s [Calls; 2],
    pub trips: &'s [Vec<u32>; 2],
}

/// Step 3: same route, service dates, call count and first call (location and scheduled
/// departure), and neither trip has another candidate under that key.
pub(super) fn near_pairs(input: &Input, left: &Left) -> Vec<(u32, u32)> {
    let Some(key) = Key::new(input) else {
        return Vec::new();
    };
    best_partners(&key, left, true, |ra, rb| u64::from(key.service.eq(ra, rb)))
}

/// Step 4: same route, call count and first call, at least one service date in common, and
/// each trip is the other's strictly best partner by dates in common (a tie on either side
/// pairs neither).
pub(super) fn date_pairs(input: &Input, left: &Left) -> Vec<(u32, u32)> {
    let Some(key) = Key::new(input) else {
        return Vec::new();
    };
    best_partners(&key, left, false, |ra, rb| {
        match (key.service.days(0, ra), key.service.days(1, rb)) {
            (Some(da), Some(db)) => common(da, db),
            _ => 0,
        }
    })
}

/// Days both sorted lists hold.
fn common(a: &[i32], b: &[i32]) -> u64 {
    let (mut i, mut j, mut count) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                count += 1;
                i += 1;
                j += 1;
            }
        }
    }
    count
}

/// Pairs trips with the same key (route, call count, first call and, with `with_service`,
/// the service) whose `score` (of A's and B's trip rows) is positive and strictly the
/// highest among each one's candidates.
fn best_partners(
    key: &Key,
    left: &Left,
    with_service: bool,
    score: impl Fn(usize, usize) -> u64,
) -> Vec<(u32, u32)> {
    let Left {
        sides,
        calls,
        trips,
    } = *left;
    let keyed = [0, 1].map(|side| {
        let own = [key.route.hasher(side), key.service.hasher(side)];
        let first = [key.location.hasher(side), key.departure.hasher(side)];
        let mut hashes: Vec<(u64, u32)> = trips[side]
            .iter()
            .map(|&trip| {
                let row = sides[side][trip as usize] as usize;
                let trip_calls = calls[side].of(trip as usize);
                let first_call = trip_calls.first().map_or_else(keys::null, |&call| {
                    keys::combine(first.iter().map(|hasher| hasher.get(call as usize)))
                });
                let service = if with_service {
                    own[1].get(row)
                } else {
                    keys::null()
                };
                let parts = [
                    own[0].get(row),
                    service,
                    keys::int(trip_calls.len() as i64),
                    first_call,
                ];
                (keys::combine(parts), trip)
            })
            .collect();
        hashes.sort_unstable();
        hashes
    });
    let row = |side: usize, trip: u32| sides[side][trip as usize] as usize;
    // The key without the service, which `score` judges.
    let same_key = |ta: u32, tb: u32| {
        let (ca, cb) = (calls[0].of(ta as usize), calls[1].of(tb as usize));
        key.route.eq(row(0, ta), row(1, tb))
            && ca.len() == cb.len()
            && match (ca.first(), cb.first()) {
                (None, None) => true,
                (Some(&p), Some(&q)) => {
                    let (p, q) = (p as usize, q as usize);
                    key.location.eq(p, q) && key.departure.eq(p, q)
                }
                _ => false,
            }
    };
    let (score, same_key, row) = (&score, &same_key, &row);
    let (a, b) = (&keyed[0], &keyed[1]);
    let mut pairs = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        let (x, y) = (a[i].0, b[j].0);
        if x != y {
            if x < y {
                i += 1;
            } else {
                j += 1;
            }
            continue;
        }
        let end_a = i + a[i..].partition_point(|entry| entry.0 == x);
        let end_b = j + b[j..].partition_point(|entry| entry.0 == x);
        let (group_a, group_b) = (&a[i..end_a], &b[j..end_b]);
        // Scores of every A x B candidate in the group, row-major.
        let scores: Vec<u64> = group_a
            .iter()
            .flat_map(|&(_, ta)| {
                group_b.iter().map(move |&(_, tb)| {
                    if same_key(ta, tb) {
                        score(row(0, ta), row(1, tb))
                    } else {
                        0
                    }
                })
            })
            .collect();
        let width = group_b.len();
        let at = |p: usize, q: usize| scores[p * width + q];
        // The index of the strictly highest positive score, if there is one.
        let best = |values: &mut dyn Iterator<Item = u64>| {
            let (mut best, mut top, mut tied) = (None, 0, false);
            for (index, value) in values.enumerate() {
                if value > top {
                    (best, top, tied) = (Some(index), value, false);
                } else if value == top && value > 0 {
                    tied = true;
                }
            }
            best.filter(|_| !tied)
        };
        for (p, &(_, ta)) in group_a.iter().enumerate() {
            let Some(q) = best(&mut (0..width).map(|q| at(p, q))) else {
                continue;
            };
            if best(&mut (0..group_a.len()).map(|other| at(other, q))) == Some(p) {
                pairs.push((ta, group_b[q].1));
            }
        }
        (i, j) = (end_a, end_b);
    }
    pairs
}
