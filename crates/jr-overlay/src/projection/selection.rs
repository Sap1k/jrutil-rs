//! What a binding states for its base trip (OVL-R080), and when two such statements agree
//! (R081).

use rayon::prelude::*;

use jr_model::Rule;

use super::capability;
use super::stops::{OutStop, Stops};
use crate::matching::Matches;
use crate::prepare::Prepared;
use crate::profile::Capability;

/// Per base call of the bound base trip: the output stop of its aligned source call
/// (`call_boarding_points`), its arrival (of the run's first source call) and departure (of the
/// run's last) (`schedules`); a call without an aligned source call keeps the base's values.
/// The source's further calls are kept, after the base call they follow, only when the binding
/// may state both where and when the trip calls, every base call is aligned, the source has
/// more calls, the first base call aligns with the first source call, the alignment increases
/// and every source call is placed.
///
/// "The source's further calls are kept only where it may state both where and when the trip
/// calls."
pub const OVL_R080: Rule = Rule {
    id: "OVL-R080",
    summary: "a binding's selection: stops and times of aligned calls, further calls if complete",
};

/// Two selections are compatible when at every base call their stops and times are equal or one
/// is absent, and their further calls are equal; compatible selections merge in (source, source
/// trip) order, the first present value at each call winning. Times compare as seconds (F#
/// compared their text).
pub const OVL_R081: Rule = Rule {
    id: "OVL-R081",
    summary: "selections agree when no call differs; compatible ones merge in source order",
};

pub const RULES: &[Rule] = &[OVL_R080, OVL_R081];

/// A further source call kept after base call `target` (R080).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Repeat {
    pub target: u32,
    pub stop: OutStop,
    pub arrival: Option<i32>,
    pub departure: Option<i32>,
}

/// What a selection states (the F# fact key's content).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Facts {
    pub stops: Box<[Option<OutStop>]>,
    pub arrivals: Box<[Option<i32>]>,
    pub departures: Box<[Option<i32>]>,
    pub repeats: Box<[Repeat]>,
}

/// A binding's selection, or a merge of compatible ones (R080, R081).
#[derive(Clone, Debug, PartialEq)]
pub struct Selection {
    pub facts: Facts,
    /// The representative: source and trip index.
    pub source: u32,
    pub trip: u32,
    /// The sources that state it, sorted.
    pub sources: Vec<u32>,
    /// The representative's source ordinal per base call.
    pub ordinals: Box<[Option<u32>]>,
    /// The representative's source ordinal per further call.
    pub repeat_ordinals: Box<[u32]>,
}

impl Selection {
    /// R081.
    pub fn compatible(&self, other: &Selection) -> bool {
        fn agree<T: PartialEq>(a: &[Option<T>], b: &[Option<T>]) -> bool {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(x, y)| x.is_none() || y.is_none() || x == y)
        }
        agree(&self.facts.stops, &other.facts.stops)
            && agree(&self.facts.arrivals, &other.facts.arrivals)
            && agree(&self.facts.departures, &other.facts.departures)
            && self.facts.repeats == other.facts.repeats
    }
}

/// The (source, source trip ID) order of R081.
pub fn order_key<'p>(prepared: &'p Prepared<'_>, selection: &Selection) -> (u32, &'p str) {
    let source = &prepared.sources[selection.source as usize];
    let trip = &source.trips[selection.trip as usize];
    (
        selection.source,
        &source.feed.trips[trip.feed_index as usize].trip_id,
    )
}

/// R081: compatible selections merged.
pub fn merge(prepared: &Prepared<'_>, claims: &[&Selection]) -> Selection {
    let mut ordered: Vec<&Selection> = claims.to_vec();
    ordered.sort_by(|a, b| order_key(prepared, a).cmp(&order_key(prepared, b)));
    let first = ordered[0];
    let pick = |get: &dyn Fn(&Selection, usize) -> Option<i32>, index: usize| {
        ordered.iter().find_map(|selection| get(selection, index))
    };
    let length = first.facts.stops.len();
    let stops = (0..length)
        .map(|i| ordered.iter().find_map(|s| s.facts.stops[i]))
        .collect();
    let arrivals = (0..length)
        .map(|i| pick(&|s, i| s.facts.arrivals[i], i))
        .collect();
    let departures = (0..length)
        .map(|i| pick(&|s, i| s.facts.departures[i], i))
        .collect();
    let mut sources: Vec<u32> = ordered
        .iter()
        .flat_map(|s| s.sources.iter().copied())
        .collect();
    sources.sort_unstable();
    sources.dedup();
    Selection {
        facts: Facts {
            stops,
            arrivals,
            departures,
            repeats: first.facts.repeats.clone(),
        },
        source: first.source,
        trip: first.trip,
        sources,
        ordinals: first.ordinals.clone(),
        repeat_ordinals: first.repeat_ordinals.clone(),
    }
}

/// R080 for every binding, in binding order.
pub fn selections(prepared: &Prepared<'_>, matches: &Matches, stops: &Stops) -> Vec<Selection> {
    matches
        .bindings
        .par_iter()
        .map(|binding| {
            let schedules = capability::eligible(prepared, binding, Capability::Schedules);
            let points = capability::eligible(prepared, binding, Capability::CallBoardingPoints);
            let source = &prepared.sources[binding.source as usize];
            let calls = source.trip_calls(&source.trips[binding.trip as usize]);
            let alignment = &binding.candidate.alignment;
            let runs = &binding.candidate.run_lengths;
            let primaries: Vec<u32> = alignment.iter().flatten().copied().collect();
            let keep = schedules
                && points
                && primaries.len() == alignment.len()
                && primaries.len() < calls.len()
                && primaries.first() == Some(&0)
                && primaries.windows(2).all(|w| w[0] < w[1])
                && calls.iter().all(|call| call.place.is_some());
            let out_of = |ordinal: u32| stops.out(binding.source, calls[ordinal as usize].stop);
            let stops_at = alignment
                .iter()
                .map(|aligned| aligned.filter(|_| points).and_then(out_of))
                .collect();
            let arrivals = alignment
                .iter()
                .map(|aligned| {
                    aligned
                        .filter(|_| schedules)
                        .and_then(|o| calls[o as usize].arrival)
                })
                .collect();
            let departures = alignment
                .iter()
                .enumerate()
                .map(|(target, aligned)| {
                    let ordinal = aligned.filter(|_| schedules)?;
                    let length = if runs.is_empty() {
                        1
                    } else {
                        runs[target].max(1)
                    };
                    let departing = if keep { ordinal } else { ordinal + length - 1 };
                    calls[departing as usize].departure
                })
                .collect();
            let mut repeats = Vec::new();
            let mut repeat_ordinals = Vec::new();
            if keep {
                for (target, &first) in primaries.iter().enumerate() {
                    let next = primaries
                        .get(target + 1)
                        .copied()
                        .unwrap_or(calls.len() as u32);
                    for ordinal in first + 1..next {
                        let call = &calls[ordinal as usize];
                        let stop = out_of(ordinal).or_else(|| {
                            super::stops::mapped(prepared, binding.source, call.stop)
                                .map(OutStop::Place)
                        });
                        if let Some(stop) = stop {
                            repeats.push(Repeat {
                                target: target as u32,
                                stop,
                                arrival: call.arrival,
                                departure: call.departure,
                            });
                            repeat_ordinals.push(ordinal);
                        }
                    }
                }
            }
            Selection {
                facts: Facts {
                    stops: stops_at,
                    arrivals,
                    departures,
                    repeats: repeats.into(),
                },
                source: binding.source,
                trip: binding.trip,
                sources: vec![binding.source],
                ordinals: alignment.clone(),
                repeat_ordinals: repeat_ordinals.into(),
            }
        })
        .collect()
}
