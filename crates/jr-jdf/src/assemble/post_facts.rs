//! What the boarding-post estimator reads of the road compiler's facts and the timetable
//! does not carry (`docs/rules/posts.md`, sections 2.2, 3.3 and 3.4): the observed positions
//! that may be posts of each stop, the stop's number and precise position, for each published
//! trip its line version and trip number facts, and the authored post key of each authored or
//! textual boarding point. Everything is a handle or a source fact; nothing is to be read out
//! of an ID.

use std::collections::HashMap;

use jr_model::timetable::Mode;
use jr_model::{BoardingPointId, Coord, PlaceId, TripId};

use super::calls::{Point, Published};
use super::places::Points;
use crate::merge::{Merged, PostCandidate};

/// See the module.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PostFacts {
    /// One per merged stop that became a place, in stop-number order.
    pub stops: Vec<StopFacts>,
    /// One per published trip, in the merge's version and trip order.
    pub trips: Vec<TripFacts>,
    /// The authored post key (POST-R043) of every authored or textual boarding point:
    /// `id:<post number>` or `num:<trimmed text>`, in boarding-point order.
    pub authored_keys: Vec<(BoardingPointId, Box<str>)>,
}

/// A stop as the estimator sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct StopFacts {
    pub place: PlaceId,
    /// The merged stop number (`jdf:stop:<number>`).
    pub number: u64,
    /// The stop's position when the merge has a precise one (catalogue, gap-fill, OSM).
    pub precise: Option<Coord>,
    /// Observed positions that may be its posts (JDF-R236), by observation.
    pub candidates: Vec<PostCandidate>,
}

/// A published trip as the estimator sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TripFacts {
    pub trip: TripId,
    /// The line's licence number (`Linky.id`).
    pub line: Box<str>,
    /// The version's ordinal among the merged versions of its licence, from 1, in merge order:
    /// the merged JDF's route distinction.
    pub distinction: i32,
    /// The JDF transport mode letter after the mode rules (`A`, `E`, `T`, …).
    pub mode: Box<str>,
    /// The JDF trip number (its parity gives the direction, JDF-R440).
    pub number: u64,
}

/// Fills [`PostFacts`] from the merge and what the assembly published.
pub(super) fn build(
    merged: &Merged,
    modes: &[Box<str>],
    points: &Points,
    published: &Published,
) -> PostFacts {
    let mut stops: Vec<StopFacts> = merged
        .stops
        .iter()
        .enumerate()
        .filter_map(|(index, stop)| {
            let place = *points.places.get(&(index as u32))?;
            Some(StopFacts {
                place,
                number: stop.number,
                precise: stop
                    .location
                    .as_ref()
                    .filter(|location| location.origin.is_precise())
                    .map(|location| location.coord),
                candidates: stop.post_candidates.clone(),
            })
        })
        .collect();
    stops.sort_by_key(|stop| stop.number);

    let mut ordinals: HashMap<&str, i32> = HashMap::new();
    let mut trips = Vec::new();
    for (index, version) in merged.versions.iter().enumerate() {
        let ordinal = ordinals.entry(&version.route.id).or_insert(0);
        *ordinal += 1;
        let distinction = *ordinal;
        for trip in published.versions.get(index).into_iter().flatten() {
            trips.push(TripFacts {
                trip: trip.handle,
                line: version.route.id.clone(),
                distinction,
                mode: modes.get(index).cloned().unwrap_or_else(|| "A".into()),
                number: version.trips[trip.trip].id,
            });
        }
    }

    let mut authored_keys: Vec<(BoardingPointId, Box<str>)> = points
        .points
        .iter()
        .filter_map(|((_, point), &handle)| match point {
            Point::Unspecified => None,
            Point::Authored(number) => Some((handle, format!("id:{number}").into())),
            Point::Textual(text) => Some((handle, format!("num:{text}").into())),
        })
        .collect();
    authored_keys.sort_by_key(|(handle, _)| *handle);
    PostFacts {
        stops,
        trips,
        authored_keys,
    }
}

/// The JDF transport mode letter of a line's mode (`A` bus, `E` tram, `T` trolleybus, …).
pub(super) fn mode_letter(mode: Mode) -> &'static str {
    match mode {
        Mode::Bus => "A",
        Mode::Tram => "E",
        Mode::Trolleybus => "T",
        Mode::Metro => "M",
        Mode::Rail => "V",
        Mode::Water => "P",
        Mode::Cable | Mode::Funicular => "L",
    }
}
