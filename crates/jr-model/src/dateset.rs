//! [`DateSet`]: the only calendar type.
//!
//! A set of day offsets from the start of the timetable's [`crate::Horizon`]. Sources' weekday
//! flags, bitmaps, exception lists and validity ranges all become one of these when they are
//! read; a weekday mask with exceptions exists again only when a package is published. Set
//! algebra on dates (cancellations, overlapping line versions, overlays that own some days) is
//! then plain bit operations, and equal calendars are equal values.

use std::collections::HashMap;
use std::fmt;
use std::ops::{BitAnd, BitOr, Sub};

use serde::{Deserialize, Serialize};

use crate::handle::{Handle, ServiceId};
use crate::time::{Date, Horizon};

const WORDS: usize = 6;

/// The longest horizon a [`DateSet`] can describe: a 53-week timetable year plus a margin.
pub const CAPACITY: usize = WORDS * 64;

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DateSet {
    words: [u64; WORDS],
}

impl DateSet {
    pub const EMPTY: Self = Self { words: [0; WORDS] };

    /// Every day of a horizon of `days` days.
    pub fn full(days: usize) -> Self {
        assert!(days <= CAPACITY);
        let mut set = Self::EMPTY;
        for (word, bits) in set.words.iter_mut().enumerate() {
            let low = word * 64;
            *bits = match days.saturating_sub(low) {
                0 => 0,
                n if n >= 64 => u64::MAX,
                n => (1u64 << n) - 1,
            };
        }
        set
    }

    pub fn from_indices(indices: impl IntoIterator<Item = usize>) -> Self {
        let mut set = Self::EMPTY;
        for index in indices {
            set.insert(index);
        }
        set
    }

    /// The dates of `dates` that lie inside `horizon`; the others are counted, not kept.
    pub fn from_dates(horizon: Horizon, dates: impl IntoIterator<Item = Date>) -> (Self, usize) {
        let mut set = Self::EMPTY;
        let mut outside = 0;
        for date in dates {
            match horizon.index_of(date) {
                Some(index) => set.insert(index),
                None => outside += 1,
            }
        }
        (set, outside)
    }

    /// The days `from..=to` (clamped to the horizon).
    pub fn range(horizon: Horizon, from: Date, to: Date) -> Self {
        let first = if from <= horizon.start() {
            Some(0)
        } else {
            horizon.index_of(from)
        };
        let last = if to >= horizon.end() {
            Some(horizon.days() - 1)
        } else {
            horizon.index_of(to)
        };
        match (first, last) {
            (Some(first), Some(last)) if first <= last => Self::from_indices(first..=last),
            _ => Self::EMPTY,
        }
    }

    pub fn insert(&mut self, index: usize) {
        assert!(index < CAPACITY, "day offset {index} outside a date set");
        self.words[index / 64] |= 1 << (index % 64);
    }

    pub fn remove(&mut self, index: usize) {
        if index < CAPACITY {
            self.words[index / 64] &= !(1 << (index % 64));
        }
    }

    pub fn contains(&self, index: usize) -> bool {
        index < CAPACITY && self.words[index / 64] & (1 << (index % 64)) != 0
    }

    pub fn is_empty(&self) -> bool {
        self.words == [0; WORDS]
    }

    pub fn count(&self) -> usize {
        self.words
            .iter()
            .map(|word| word.count_ones() as usize)
            .sum()
    }

    pub fn first(&self) -> Option<usize> {
        self.iter().next()
    }

    pub fn last(&self) -> Option<usize> {
        self.words
            .iter()
            .enumerate()
            .rev()
            .find(|(_, word)| **word != 0)
            .map(|(position, word)| position * 64 + 63 - word.leading_zeros() as usize)
    }

    /// Day offsets in ascending order.
    pub fn iter(&self) -> impl Iterator<Item = usize> + use<> {
        let words = self.words;
        (0..WORDS).flat_map(move |position| {
            let mut word = words[position];
            std::iter::from_fn(move || {
                (word != 0).then(|| {
                    let bit = word.trailing_zeros() as usize;
                    word &= word - 1;
                    position * 64 + bit
                })
            })
        })
    }

    pub fn dates(&self, horizon: Horizon) -> impl Iterator<Item = Date> + use<> {
        self.iter().map(move |index| horizon.date_at(index))
    }

    pub fn intersects(&self, other: &Self) -> bool {
        self.words.iter().zip(&other.words).any(|(a, b)| a & b != 0)
    }

    pub fn is_subset(&self, other: &Self) -> bool {
        self.words
            .iter()
            .zip(&other.words)
            .all(|(a, b)| a & !b == 0)
    }

    /// The set moved by `days` (positive is later) within a horizon of `horizon_days`, and how
    /// many of its days fell off either end.
    pub fn shifted(&self, days: i32, horizon_days: usize) -> (Self, usize) {
        let mut moved = Self::EMPTY;
        let mut lost = 0;
        for index in self.iter() {
            let target = index as i64 + i64::from(days);
            if (0..horizon_days as i64).contains(&target) {
                moved.insert(target as usize);
            } else {
                lost += 1;
            }
        }
        (moved, lost)
    }

    /// The raw bits, low day first: the canonical bytes a content-derived service ID hashes.
    pub fn to_bytes(&self) -> [u8; WORDS * 8] {
        let mut bytes = [0; WORDS * 8];
        for (chunk, word) in bytes.as_chunks_mut::<8>().0.iter_mut().zip(&self.words) {
            chunk.copy_from_slice(&word.to_le_bytes());
        }
        bytes
    }
}

impl BitOr for DateSet {
    type Output = Self;

    fn bitor(mut self, other: Self) -> Self {
        for (a, b) in self.words.iter_mut().zip(other.words) {
            *a |= b;
        }
        self
    }
}

impl BitAnd for DateSet {
    type Output = Self;

    fn bitand(mut self, other: Self) -> Self {
        for (a, b) in self.words.iter_mut().zip(other.words) {
            *a &= b;
        }
        self
    }
}

/// Set difference: the days of `self` that are not in `other`.
impl Sub for DateSet {
    type Output = Self;

    fn sub(mut self, other: Self) -> Self {
        for (a, b) in self.words.iter_mut().zip(other.words) {
            *a &= !b;
        }
        self
    }
}

impl fmt::Debug for DateSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.first(), self.last()) {
            (Some(first), Some(last)) => {
                write!(f, "DateSet({} days, {first}..={last})", self.count())
            }
            _ => f.write_str("DateSet(empty)"),
        }
    }
}

/// The interned date sets of a timetable. Trips with the same running days share a
/// [`ServiceId`]; the published `service_id` is derived from the set itself.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(from = "Vec<DateSet>", into = "Vec<DateSet>")]
pub struct Services {
    sets: Vec<DateSet>,
    /// Looked up, never iterated.
    index: HashMap<DateSet, ServiceId>,
}

impl Services {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern(&mut self, set: DateSet) -> ServiceId {
        if let Some(&id) = self.index.get(&set) {
            return id;
        }
        let id = ServiceId::from_index(self.sets.len());
        self.sets.push(set);
        self.index.insert(set, id);
        id
    }

    pub fn find(&self, set: &DateSet) -> Option<ServiceId> {
        self.index.get(set).copied()
    }

    pub fn get(&self, id: ServiceId) -> DateSet {
        self.sets[id.index()]
    }

    pub fn len(&self) -> usize {
        self.sets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sets.is_empty()
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = (ServiceId, DateSet)> {
        self.sets
            .iter()
            .enumerate()
            .map(|(index, set)| (ServiceId::from_index(index), *set))
    }
}

impl From<Vec<DateSet>> for Services {
    fn from(sets: Vec<DateSet>) -> Self {
        let index = sets
            .iter()
            .enumerate()
            .map(|(position, set)| (*set, ServiceId::from_index(position)))
            .collect();
        Self { sets, index }
    }
}

impl From<Services> for Vec<DateSet> {
    fn from(services: Services) -> Self {
        services.sets
    }
}

impl PartialEq for Services {
    fn eq(&self, other: &Self) -> bool {
        self.sets == other.sets
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn full_covers_exactly_the_horizon() {
        for days in [0, 1, 63, 64, 65, 364, 371, CAPACITY] {
            let set = DateSet::full(days);
            assert_eq!(set.count(), days);
            assert_eq!(set.last(), days.checked_sub(1));
            assert!(!set.contains(days));
        }
    }

    #[test]
    fn algebra_and_iteration() {
        let a = DateSet::from_indices([0, 63, 64, 200, 383]);
        let b = DateSet::from_indices([63, 200, 201]);
        assert_eq!((a & b).iter().collect::<Vec<_>>(), [63, 200]);
        assert_eq!((a - b).iter().collect::<Vec<_>>(), [0, 64, 383]);
        assert_eq!((a | b).count(), 6);
        assert!(a.intersects(&b));
        assert!((a & b).is_subset(&a));
        assert_eq!((a.first(), a.last()), (Some(0), Some(383)));
    }

    #[test]
    fn shifting_drops_days_that_leave_the_horizon() {
        let set = DateSet::from_indices([0, 1, 363]);
        let (later, lost) = set.shifted(1, 364);
        assert_eq!((later.iter().collect::<Vec<_>>(), lost), (vec![1, 2], 1));
        let (earlier, lost) = set.shifted(-1, 364);
        assert_eq!(
            (earlier.iter().collect::<Vec<_>>(), lost),
            (vec![0, 362], 1)
        );
    }

    #[test]
    fn ranges_clamp_to_the_horizon() {
        let gvd = Horizon::gvd(2026).unwrap();
        let open = DateSet::range(gvd, date(2020, 1, 1), date(3000, 1, 1));
        assert_eq!(open, DateSet::full(364));
        let week = DateSet::range(gvd, date(2025, 12, 14), date(2025, 12, 20));
        assert_eq!(week.count(), 7);
        assert!(DateSet::range(gvd, date(2027, 1, 1), date(2027, 2, 1)).is_empty());
        let (set, outside) = DateSet::from_dates(gvd, [date(2025, 12, 15), date(2025, 1, 1)]);
        assert_eq!((set.iter().collect::<Vec<_>>(), outside), (vec![1], 1));
    }

    #[test]
    fn equal_sets_intern_to_one_service() {
        let mut services = Services::new();
        let a = services.intern(DateSet::from_indices([1, 2]));
        let b = services.intern(DateSet::from_indices([2, 1]));
        let c = services.intern(DateSet::EMPTY);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(services.len(), 2);
    }
}
