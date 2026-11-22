//! What becomes of each base trip (OVL-R086–R088, R093, R094, R105).

use jr_model::identity::overlay;
use jr_model::{DateSet, Diagnostics, PublicId, Rule};

use super::claims::Resolved;
use super::headsign::Headsigns;
use super::selection::Selection;
use crate::authority::{Authority, TargetRoute};
use crate::prepare::Prepared;

/// A base trip keeps the days it runs except those of its line that a source owns (R061–R063)
/// without a resolved selection or a quarantined conflict (B23 fixed) on them; a trip left
/// without days is retired (an empty service).
///
/// Inside its window, a line a source owns runs only what the source runs.
pub const OVL_R086: Rule = Rule {
    id: "OVL-R086",
    summary: "national trips retire on owned line-dates the sources do not claim",
};

/// A trip whose kept days are not all stated alike is split: its days grouped by the selection
/// resolved on them (none for the base's own), each group a part under
/// `identity::overlay::slice` of the base trip and the group's days, the original retired. A
/// trip stated alike on every kept day keeps its ID. Everything stated for the trip follows its
/// parts (`SplitTrip`).
pub const OVL_R087: Rule = Rule {
    id: "OVL-R087",
    summary: "a trip stated differently on some days is split into date slices",
};

/// A part whose selection comes from a projected source trip of another mode than the base
/// route's moves to the projection's target route (the derived route of R059).
///
/// JDF publishes trolleybus lines as bus routes; the source keeps its mode.
pub const OVL_R088: Rule = Rule {
    id: "OVL-R088",
    summary: "a part stated by a source trip of another mode moves to its derived route",
};

/// A part with a selection takes, per base call, the selection's boarding point and times
/// and, for a projected source trip, the source call's stop headsign (R089); kilometres,
/// boarding rules and zones stay the base call's (B9).
pub const OVL_R093: Rule = Rule {
    id: "OVL-R093",
    summary: "a part's calls take the selection's boarding points, times and stop headsigns",
};

/// The source's further calls are inserted after the base call they follow, as copies of it
/// with the source's place, boarding point and times; later calls move back by the calls
/// inserted before them, and what was stated for a call follows it (`ReplaceTripCalls`).
pub const OVL_R094: Rule = Rule {
    id: "OVL-R094",
    summary: "further source calls are inserted and later calls resequenced",
};

/// A call's zones are part of the call, so they follow it into every part and to its new
/// sequence; an inserted call shares the zones of the base call it follows.
///
/// "Repeated calls of a stop place share its zones."
pub const OVL_R105: Rule = Rule {
    id: "OVL-R105",
    summary: "call zones follow their calls into slices and new sequences",
};

pub const RULES: &[Rule] = &[OVL_R086, OVL_R087, OVL_R088, OVL_R093, OVL_R094, OVL_R105];

/// One output part of a base trip.
#[derive(Clone, Debug)]
pub struct Part {
    pub dates: DateSet,
    /// `None`: the base trip's own ID (the trip is not split).
    pub id: Option<PublicId>,
    pub selection: Option<Selection>,
    /// R088.
    pub route: Option<TargetRoute>,
    /// R089: the headsign to set.
    pub headsign: Option<String>,
    /// R089: per base call and per further call, the stop headsign to set.
    pub stop_headsigns: Vec<Option<String>>,
    pub repeat_headsigns: Vec<Option<String>>,
}

/// What becomes of one base trip.
#[derive(Clone, Debug)]
pub struct TripPlan {
    /// Index into the base view's trips.
    pub base: u32,
    /// R087: its parts; empty when retired.
    pub parts: Vec<Part>,
}

impl TripPlan {
    pub fn split(&self) -> bool {
        self.parts.iter().any(|part| part.id.is_some())
    }
}

/// R086–R089 for every base trip that a source owns or claims, in base trip order.
pub fn plan(
    prepared: &Prepared<'_>,
    authority: &Authority,
    resolved: &Resolved,
    headsigns: &mut Headsigns,
    diagnostics: &mut Diagnostics,
) -> Vec<TripPlan> {
    let base = &prepared.base;
    let projected: std::collections::HashMap<(u32, u32), usize> = authority
        .projections
        .iter()
        .enumerate()
        .map(|(index, p)| ((p.source, p.trip), index))
        .collect();
    let mut plans = Vec::new();
    for (index, trip) in base.trips.iter().enumerate() {
        let index = index as u32;
        let route = base.route(trip.route);
        let owned = route
            .line
            .as_deref()
            .map_or(DateSet::EMPTY, |line| authority.dates_of_line(line));
        let claims = resolved.trips.get(&index);
        if owned.is_empty() && claims.is_none_or(|c| c.resolved.is_empty()) {
            continue;
        }
        let empty = Default::default();
        let claims = claims.unwrap_or(&empty);
        let mut retained = DateSet::EMPTY;
        for date in trip.dates.iter() {
            if !owned.contains(date)
                || claims.resolved.contains_key(&(date as u32))
                || claims.conflicts.contains(date)
            {
                retained.insert(date);
            }
        }
        let mut groups: Vec<(DateSet, Option<&Selection>)> = Vec::new();
        for date in retained.iter() {
            let selection = claims.resolved.get(&(date as u32));
            match groups
                .iter_mut()
                .find(|(_, s)| s.map(|s| &s.facts) == selection.map(|s| &s.facts))
            {
                Some((dates, _)) => dates.insert(date),
                None => groups.push((DateSet::from_indices([date]), selection)),
            }
        }
        let split = groups.len() > 1;
        let base_id = &base.timetable.trips[trip.trip].id;
        let mut parts: Vec<Part> = groups
            .into_iter()
            .map(|(dates, selection)| {
                let mut part = Part {
                    dates,
                    id: split.then(|| overlay::slice(base_id, &dates)),
                    selection: selection.cloned(),
                    route: None,
                    headsign: None,
                    stop_headsigns: Vec::new(),
                    repeat_headsigns: Vec::new(),
                };
                if let Some(selection) = selection
                    && let Some(&p) = projected.get(&(selection.source, selection.trip))
                {
                    let projection = &authority.projections[p];
                    let source = &prepared.sources[selection.source as usize];
                    let source_trip = &source.trips[selection.trip as usize];
                    if source.routes[source_trip.route as usize].mode != Some(route.mode) {
                        part.route = Some(projection.target);
                    }
                    if headsigns.applies(prepared, selection.source, selection.trip) {
                        let raw = source.feed.trips[source_trip.feed_index as usize]
                            .trip_headsign
                            .as_deref()
                            .unwrap_or("");
                        part.headsign = Some(headsigns.full(
                            prepared,
                            selection.source,
                            selection.trip,
                            None,
                            raw,
                            diagnostics,
                        ));
                        let calls = source.trip_calls(source_trip);
                        let stop_headsign =
                            |ordinal: u32,
                             headsigns: &mut Headsigns,
                             diagnostics: &mut Diagnostics| {
                                let call = &calls[ordinal as usize];
                                let raw = source
                                    .feed
                                    .stop_times
                                    .get(call.row as usize)
                                    .stop_headsign?;
                                Some(headsigns.full(
                                    prepared,
                                    selection.source,
                                    selection.trip,
                                    call.sequence,
                                    raw,
                                    diagnostics,
                                ))
                            };
                        part.stop_headsigns = selection
                            .ordinals
                            .iter()
                            .map(|o| o.and_then(|o| stop_headsign(o, headsigns, diagnostics)))
                            .collect();
                        part.repeat_headsigns = selection
                            .repeat_ordinals
                            .iter()
                            .map(|&o| stop_headsign(o, headsigns, diagnostics))
                            .collect();
                    }
                }
                part
            })
            .collect();
        parts.sort_by(|a, b| a.id.cmp(&b.id));
        plans.push(TripPlan { base: index, parts });
    }
    plans
}
