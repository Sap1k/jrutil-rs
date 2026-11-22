//! POST-R040–R044: the contexts of a trip's calls. Capture and publication share this
//! enumeration, so a call always finds the context it was captured under.

use std::collections::HashMap;

use jr_formats::post_evidence::BlockRole;
use jr_formats::post_evidence::ids::{context_id, same_stop_block_id, sha256_hex};
use jr_jdf::assemble::{PostFacts, TripFacts};
use jr_model::{BoardingPointId, PlaceId, Timetable};

/// Lookups from the timetable's handles to the facts the rules read.
#[derive(Debug)]
pub(crate) struct FactIndex<'a> {
    pub facts: &'a PostFacts,
    /// Place → index into `facts.stops`.
    pub stops: HashMap<PlaceId, usize>,
    /// Authored or textual boarding point → its authored key (R043).
    pub authored: HashMap<BoardingPointId, &'a str>,
}

impl<'a> FactIndex<'a> {
    pub fn new(facts: &'a PostFacts) -> Self {
        FactIndex {
            facts,
            stops: facts
                .stops
                .iter()
                .enumerate()
                .map(|(index, stop)| (stop.place, index))
                .collect(),
            authored: facts
                .authored_keys
                .iter()
                .map(|(point, key)| (*point, &**key))
                .collect(),
        }
    }
}

/// One call of a trip with its context identity.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CallContext {
    /// Index of the call in the trip's call list.
    pub call: usize,
    pub stop: u64,
    pub position: i32,
    pub role: BlockRole,
    /// The block of repeated calls at the stop: calls `start..finish` of the pattern.
    pub start: usize,
    pub finish: usize,
    pub block_id: Option<String>,
    pub authored: Option<Box<str>>,
    pub context_id: String,
}

/// A trip's pattern: its usable calls' stops and their contexts (R040–R042).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TripPattern<'a> {
    pub facts: &'a TripFacts,
    /// 1 when the trip number is even (reverse), else 0 (R040).
    pub direction: i32,
    /// R041: `sha(stop numbers joined ",")`.
    pub pattern_hash: String,
    /// The stop number of each call.
    pub stops: Vec<u64>,
    pub calls: Vec<CallContext>,
}

impl TripPattern<'_> {
    /// R041: the identity under which identical trip patterns share their contexts.
    pub fn identity(&self) -> String {
        let authored: Vec<&str> = self
            .calls
            .iter()
            .map(|call| call.authored.as_deref().unwrap_or(""))
            .collect();
        format!(
            "{}|{}|{}|{}|{}",
            self.facts.line,
            self.facts.distinction,
            self.direction,
            self.pattern_hash,
            authored.join(";")
        )
    }
}

/// R040–R044 for one trip. `None` when the trip's mode has no contexts or a call's place is
/// not a JDF stop. The timetable's calls are the trip's usable calls in running order (the
/// assembly drops passed and timeless calls and orders by route stop and parity).
pub(crate) fn trip_pattern<'a>(
    timetable: &Timetable,
    index: &FactIndex<'_>,
    trip: &'a TripFacts,
    modes: &[Box<str>],
) -> Option<TripPattern<'a>> {
    if !modes.contains(&trip.mode) {
        return None;
    }
    let calls = &timetable.trips.get(trip.trip)?.calls;
    let stops: Vec<u64> = calls
        .iter()
        .map(|call| {
            index
                .stops
                .get(&call.place)
                .map(|&stop| index.facts.stops[stop].number)
        })
        .collect::<Option<_>>()?;
    let text: Vec<String> = stops.iter().map(u64::to_string).collect();
    let pattern_hash = sha256_hex(text.join(",").as_bytes());
    let direction = i32::from(trip.number.is_multiple_of(2));
    let mut contexts = Vec::with_capacity(calls.len());
    let mut start = 0;
    while start < stops.len() {
        let stop = stops[start];
        let mut finish = start + 1;
        while finish < stops.len() && stops[finish] == stop {
            finish += 1;
        }
        let block_id = (finish - start > 1).then(|| {
            same_stop_block_id(
                &trip.line,
                trip.distinction,
                &pattern_hash,
                start as i32,
                finish as i32,
            )
        });
        for (call, timetable_call) in calls.iter().enumerate().take(finish).skip(start) {
            let role = if finish - start == 1 {
                BlockRole::Through
            } else if call == start {
                BlockRole::Incoming
            } else if call == finish - 1 {
                BlockRole::Outgoing
            } else {
                BlockRole::Interior
            };
            let authored: Option<Box<str>> = timetable_call
                .boarding_point
                .and_then(|point| index.authored.get(&point))
                .map(|&key| key.into());
            let context_id = context_id(
                stop,
                &trip.mode,
                &trip.line,
                trip.distinction,
                direction,
                &pattern_hash,
                call as i32,
                role.as_str(),
                authored.as_deref(),
            );
            contexts.push(CallContext {
                call,
                stop,
                position: call as i32,
                role,
                start,
                finish,
                block_id: block_id.clone(),
                authored,
                context_id,
            });
        }
        start = finish;
    }
    Some(TripPattern {
        facts: trip,
        direction,
        pattern_hash,
        stops,
        calls: contexts,
    })
}
