//! Dense typed handles and the tables they index.
//!
//! A handle is a position in one table of one [`crate::Timetable`]. Rows are never removed, so a
//! handle stays valid for the life of the timetable; a trip that no longer runs has an empty
//! service, and unreferenced rows are left out when publishing. `Option<Handle>` is four bytes.

use std::fmt;
use std::marker::PhantomData;
use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

/// A typed position in a [`Table`].
pub trait Handle: Copy + Ord + fmt::Debug {
    fn from_index(index: usize) -> Self;
    fn index(self) -> usize;
}

macro_rules! handle {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(NonZeroU32);

        impl Handle for $name {
            fn from_index(index: usize) -> Self {
                let raw = u32::try_from(index + 1).expect("table larger than u32");
                Self(NonZeroU32::new(raw).expect("index + 1 is never zero"))
            }

            fn index(self) -> usize {
                self.0.get() as usize - 1
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), self.index())
            }
        }
    };
}

handle!(AgencyId);
handle!(
    /// A stop place, a station or a railway operational point.
    PlaceId
);
handle!(
    /// A post, platform or track of one place.
    BoardingPointId
);
handle!(RouteId);
handle!(TripId);
handle!(NoteId);
handle!(ShapeId);
handle!(
    /// One slot of a route direction's ordered stop list.
    RouteStopId
);
handle!(
    /// An interned [`crate::DateSet`]: the days something runs.
    ServiceId
);
handle!(
    /// An interned ordered list of zones.
    ZoneSetId
);
handle!(
    /// An interned string ([`crate::Strings`]).
    Sym
);

/// Rows addressed by a typed handle, in insertion order.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Table<I, T> {
    rows: Vec<T>,
    #[serde(skip)]
    handle: PhantomData<I>,
}

impl<I, T> Default for Table<I, T> {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            handle: PhantomData,
        }
    }
}

impl<I: Handle, T> Table<I, T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, row: T) -> I {
        self.rows.push(row);
        I::from_index(self.rows.len() - 1)
    }

    /// The handle the next [`Table::push`] returns.
    pub fn next_handle(&self) -> I {
        I::from_index(self.rows.len())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn contains(&self, handle: I) -> bool {
        handle.index() < self.rows.len()
    }

    pub fn get(&self, handle: I) -> Option<&T> {
        self.rows.get(handle.index())
    }

    pub fn get_mut(&mut self, handle: I) -> Option<&mut T> {
        self.rows.get_mut(handle.index())
    }

    pub fn handles(&self) -> impl ExactSizeIterator<Item = I> + use<I, T> {
        (0..self.rows.len()).map(I::from_index)
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = (I, &T)> {
        self.rows
            .iter()
            .enumerate()
            .map(|(index, row)| (I::from_index(index), row))
    }

    pub fn iter_mut(&mut self) -> impl ExactSizeIterator<Item = (I, &mut T)> {
        self.rows
            .iter_mut()
            .enumerate()
            .map(|(index, row)| (I::from_index(index), row))
    }

    pub fn rows(&self) -> &[T] {
        &self.rows
    }

    pub fn rows_mut(&mut self) -> &mut [T] {
        &mut self.rows
    }
}

impl<I: Handle, T> std::ops::Index<I> for Table<I, T> {
    type Output = T;

    fn index(&self, handle: I) -> &T {
        &self.rows[handle.index()]
    }
}

impl<I: Handle, T> std::ops::IndexMut<I> for Table<I, T> {
    fn index_mut(&mut self, handle: I) -> &mut T {
        &mut self.rows[handle.index()]
    }
}

impl<I, T: fmt::Debug> fmt::Debug for Table<I, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(&self.rows).finish()
    }
}

impl<I: Handle, T> FromIterator<T> for Table<I, T> {
    fn from_iter<It: IntoIterator<Item = T>>(iter: It) -> Self {
        Self {
            rows: iter.into_iter().collect(),
            handle: PhantomData,
        }
    }
}
