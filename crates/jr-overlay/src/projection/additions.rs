//! Source trips added as their own trips (OVL-R090, R095).

use std::collections::HashMap;

use jr_model::identity::overlay;
use jr_model::timetable::Mode;
use jr_model::{DateSet, Diagnostics, PlaceId, PublicId, Rule};

use super::claims::Resolved;
use super::headsign::Headsigns;
use crate::authority::{Authority, AuthorityLine, TargetRoute};
use crate::prepare::{PlaceRef, Prepared};

/// A projected source trip is added on its projected days on which no base trip represents it
/// (R085). Equivalent additions (same mode, public line number, days and calls at the same
/// places and times) collapse into one, represented by the first by (source, trip ID); every
/// member maps to it. An addition is `identity::overlay::added_trip` of its representative and
/// days, on the projection's target route, with the source trip's fields and full headsign
/// (R092). (Calls at source-native places compare by their source stop, as in the reference.)
pub const OVL_R090: Rule = Rule {
    id: "OVL-R090",
    summary: "projected source trips not represented by a base trip are added; equivalents collapse",
};

/// An added trip's calls are its representative's source calls with their (folded) times, the
/// output boarding point of each stop (R079), the source's stop sequences, boarding rules and
/// timepoints and full stop headsigns; it has no kilometres.
///
/// "Times come from the projected calls, not the raw row."
pub const OVL_R095: Rule = Rule {
    id: "OVL-R095",
    summary: "added trips carry their source calls with output boarding points",
};

pub const RULES: &[Rule] = &[OVL_R090, OVL_R095];

/// One added trip.
#[derive(Clone, Debug)]
pub struct Addition {
    pub id: PublicId,
    /// The representative: source and trip index.
    pub source: u32,
    pub trip: u32,
    /// Every member (source, trip), sorted by (source, trip ID).
    pub members: Vec<(u32, u32)>,
    pub line: AuthorityLine,
    pub target: TargetRoute,
    pub dates: DateSet,
    pub headsign: Option<String>,
    /// Per call, its stop headsign.
    pub stop_headsigns: Vec<Option<String>>,
}

/// A call as R090 compares it: its base place, or its (source, stop), and its times.
type CallKey = (
    Option<PlaceId>,
    Option<(u32, u32)>,
    Option<i32>,
    Option<i32>,
);

/// The equivalence of R090.
#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    mode: Option<Mode>,
    number: Box<str>,
    dates: DateSet,
    calls: Vec<CallKey>,
}

/// R090, additions in ID order.
pub fn plan(
    prepared: &Prepared<'_>,
    authority: &Authority,
    resolved: &Resolved,
    headsigns: &mut Headsigns,
    diagnostics: &mut Diagnostics,
) -> Vec<Addition> {
    let mut groups: Vec<(Key, Vec<usize>, DateSet)> = Vec::new();
    let mut index: HashMap<Key, usize> = HashMap::new();
    for (p, projection) in authority.projections.iter().enumerate() {
        let dates = projection.dates
            - resolved.represented[projection.source as usize][projection.trip as usize];
        if dates.is_empty() {
            continue;
        }
        let source = &prepared.sources[projection.source as usize];
        let trip = &source.trips[projection.trip as usize];
        let route = &source.routes[trip.route as usize];
        let key = Key {
            mode: route.mode,
            number: source.feed.routes[route.feed_index as usize]
                .route_short_name
                .clone()
                .unwrap_or_default(),
            dates,
            calls: source
                .trip_calls(trip)
                .iter()
                .map(|call| match call.place {
                    Some(PlaceRef::Base(place)) => {
                        (Some(place), None, call.arrival, call.departure)
                    }
                    _ => (
                        None,
                        Some((projection.source, call.stop)),
                        call.arrival,
                        call.departure,
                    ),
                })
                .collect(),
        };
        match index.get(&key) {
            Some(&at) => groups[at].1.push(p),
            None => {
                index.insert(key.clone(), groups.len());
                groups.push((key, vec![p], dates));
            }
        }
    }
    let trip_id = |source: u32, trip: u32| -> &str {
        let s = &prepared.sources[source as usize];
        &s.feed.trips[s.trips[trip as usize].feed_index as usize].trip_id
    };
    let mut additions: Vec<Addition> = groups
        .into_iter()
        .map(|(_, mut members, dates)| {
            members.sort_by(|&a, &b| {
                let (pa, pb) = (&authority.projections[a], &authority.projections[b]);
                (pa.source, trip_id(pa.source, pa.trip))
                    .cmp(&(pb.source, trip_id(pb.source, pb.trip)))
            });
            let first = &authority.projections[members[0]];
            let source = &prepared.sources[first.source as usize];
            let trip = &source.trips[first.trip as usize];
            let raw = source.feed.trips[trip.feed_index as usize]
                .trip_headsign
                .as_deref();
            let applies = headsigns.applies(prepared, first.source, first.trip);
            let headsign = if applies {
                Some(headsigns.full(
                    prepared,
                    first.source,
                    first.trip,
                    None,
                    raw.unwrap_or(""),
                    diagnostics,
                ))
            } else {
                raw.map(str::to_owned)
            };
            let stop_headsigns = source
                .trip_calls(trip)
                .iter()
                .map(|call| {
                    let raw = source
                        .feed
                        .stop_times
                        .get(call.row as usize)
                        .stop_headsign?;
                    Some(if applies {
                        headsigns.full(
                            prepared,
                            first.source,
                            first.trip,
                            call.sequence,
                            raw,
                            diagnostics,
                        )
                    } else {
                        raw.to_owned()
                    })
                })
                .collect();
            Addition {
                id: overlay::added_trip(
                    source.source_id(),
                    trip_id(first.source, first.trip),
                    &dates,
                ),
                source: first.source,
                trip: first.trip,
                members: members
                    .iter()
                    .map(|&m| {
                        (
                            authority.projections[m].source,
                            authority.projections[m].trip,
                        )
                    })
                    .collect(),
                line: first.line.clone(),
                target: first.target,
                dates,
                headsign,
                stop_headsigns,
            }
        })
        .collect();
    additions.sort_by(|a, b| a.id.cmp(&b.id));
    additions
}
