//! The merge of one route direction's stop patterns into ordered slots (SERVING-R028).
//!
//! Works on opaque place values and relative times only; the caller has already put the
//! patterns in merge order.

/// One distinct stop pattern of a route direction, in merge order.
#[derive(Debug)]
pub(super) struct Pattern<P> {
    pub places: Vec<P>,
    /// The fraction (0..1) of the representative trip's duration at each position, `None`
    /// where unknown (SERVING-R029). Only orders stops inserted into the same gap.
    pub times: Vec<Option<f64>>,
}

/// A slot being built: its place and the running mean of the times aligned to it.
#[derive(Debug)]
struct Slot<P> {
    place: P,
    sum: f64,
    count: u32,
}

impl<P> Slot<P> {
    fn add(&mut self, time: Option<f64>) {
        if let Some(time) = time {
            self.sum += time;
            self.count += 1;
        }
    }

    fn time(&self) -> Option<f64> {
        (self.count > 0).then(|| self.sum / f64::from(self.count))
    }
}

/// The merged result: the place of every slot in order and, per pattern (in the order given),
/// the slot index of each of its positions.
#[derive(Debug, PartialEq)]
pub(super) struct Merged<P> {
    pub places: Vec<P>,
    pub assigned: Vec<Vec<usize>>,
}

/// Merges `patterns`, which must already be in merge order (weight descending, length
/// descending, key ascending).
pub(super) fn merge<P: Copy + Eq>(patterns: &[Pattern<P>]) -> Merged<P> {
    // Slots live in an arena; `order` is the current merged list as arena indices.
    let mut arena: Vec<Slot<P>> = Vec::new();
    let mut order: Vec<usize> = Vec::new();
    let mut assigned = Vec::with_capacity(patterns.len());
    for pattern in patterns {
        let pairs = align(&arena, &order, &pattern.places);
        let mut slots = vec![0; pattern.places.len()];
        let mut next = Vec::with_capacity(order.len() + pattern.places.len());
        let (mut merged_start, mut pattern_start) = (0, 0);
        let fill_gap = |arena: &mut Vec<Slot<P>>,
                        next: &mut Vec<usize>,
                        slots: &mut [usize],
                        (merged_start, merged_end): (usize, usize),
                        (pattern_start, pattern_end): (usize, usize)| {
            let added: Vec<usize> = (pattern_start..pattern_end)
                .map(|position| {
                    let mut slot = Slot {
                        place: pattern.places[position],
                        sum: 0.0,
                        count: 0,
                    };
                    slot.add(pattern.times[position]);
                    arena.push(slot);
                    slots[position] = arena.len() - 1;
                    arena.len() - 1
                })
                .collect();
            interleave(arena, &order[merged_start..merged_end], &added, next);
        };
        for &(merged_index, position) in &pairs {
            fill_gap(
                &mut arena,
                &mut next,
                &mut slots,
                (merged_start, merged_index),
                (pattern_start, position),
            );
            let slot = order[merged_index];
            arena[slot].add(pattern.times[position]);
            slots[position] = slot;
            next.push(slot);
            merged_start = merged_index + 1;
            pattern_start = position + 1;
        }
        fill_gap(
            &mut arena,
            &mut next,
            &mut slots,
            (merged_start, order.len()),
            (pattern_start, pattern.places.len()),
        );
        order = next;
        assigned.push(slots);
    }
    let mut position_of = vec![0; arena.len()];
    for (position, &slot) in order.iter().enumerate() {
        position_of[slot] = position;
    }
    Merged {
        places: order.iter().map(|&slot| arena[slot].place).collect(),
        assigned: assigned
            .into_iter()
            .map(|slots| slots.into_iter().map(|slot| position_of[slot]).collect())
            .collect(),
    }
}

/// Longest common subsequence of the merged list and the pattern, as aligned
/// `(merged index, pattern position)` pairs in order. On a tie the merged list advances, so a
/// pattern's stop aligns with the earliest matching slot.
fn align<P: Copy + Eq>(arena: &[Slot<P>], order: &[usize], pattern: &[P]) -> Vec<(usize, usize)> {
    let (n, m) = (order.len(), pattern.len());
    let width = m + 1;
    let mut table = vec![0u32; (n + 1) * width];
    let at = |i: usize, j: usize| i * width + j;
    let same = |i: usize, j: usize| arena[order[i]].place == pattern[j];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[at(i, j)] = if same(i, j) {
                table[at(i + 1, j + 1)] + 1
            } else {
                table[at(i + 1, j)].max(table[at(i, j + 1)])
            };
        }
    }
    let mut pairs = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if same(i, j) && table[at(i, j)] == table[at(i + 1, j + 1)] + 1 {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else if table[at(i + 1, j)] >= table[at(i, j + 1)] {
            i += 1;
        } else {
            j += 1;
        }
    }
    pairs
}

/// Interleaves the existing slots of a gap with the new ones, keeping each side's order. A new
/// slot goes first only when both times are known and the new one is strictly earlier.
fn interleave<P>(arena: &[Slot<P>], existing: &[usize], added: &[usize], target: &mut Vec<usize>) {
    let (mut i, mut j) = (0, 0);
    while i < existing.len() || j < added.len() {
        let take_added = if j == added.len() {
            false
        } else if i == existing.len() {
            true
        } else {
            match (arena[existing[i]].time(), arena[added[j]].time()) {
                (Some(left), Some(right)) => right < left,
                _ => false,
            }
        };
        if take_added {
            target.push(added[j]);
            j += 1;
        } else {
            target.push(existing[i]);
            i += 1;
        }
    }
}
