//! The batch of edits (OVL-R091, R098, R101, R102, R104, R111, R117, R130), assembled in the
//! order stated in `projection.rs`.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use jr_model::identity::overlay;
use jr_model::timetable::{
    Agency, BindingMethod, BoardingPoint, BoardingPointKind, Call, CoordSource, Entity, Mode,
    Namespace, Place, PlaceKind, Position, Route, SourceKey, TimetableKind, Trip, Tristate, Zone,
};
use jr_model::{
    AgencyId, BoardingPointId, Coord, DateSet, Diagnostic, Diagnostics, EditBuilder, Edits,
    OptTime, PlaceId, RouteId, Rule, ServiceTime, Severity, TripId,
};

use super::additions::Addition;
use super::capability;
use super::selection::Selection;
use super::slices::{Part, TripPlan};
use super::stops::{OutPlace, OutStop, Stops};
use super::summary::Summary;
use super::{keys, transfers};
use crate::PASS;
use crate::authority::{Authority, TargetRoute};
use crate::matching::Matches;
use crate::prepare::{MatchMethod, PlaceRef, Prepared};
use crate::profile::Capability;

/// With `stop_coordinates`, the centroid of every placed source group that was placed from
/// trip context or has an accepted stop (R078) claims its place's position; a base place with
/// exactly one distinct claim takes it (exact, `overlay`: B4 fixed, verdict Q13), several are
/// `stop_coordinate_conflict` and the base position stays; a centroid outside WGS84 is
/// `stop_coordinate_invalid`.
pub const OVL_R091: Rule = Rule {
    id: "OVL-R091",
    summary: "a source centroid places its base place when it is the only claim",
};

/// A base route used by output trips takes, for each display capability its sources grant,
/// the one non-empty value of the source routes bound to it on surviving dates and of the
/// additions on it; several values are `route_display_conflict`. (`route_long_name` is not
/// set: `SetRoutePresentation` has no long name.)
pub const OVL_R098: Rule = Rule {
    id: "OVL-R098",
    summary: "base routes take the source routes' display fields when they agree",
};

/// With the zones hook, a source group's distinct zone IDs are zones of the hook's system at
/// its place, given to every call at the place that has no zones of its own, on every output
/// trip (verdict Q10: place-level, B6 recorded).
pub const OVL_R101: Rule = Rule {
    id: "OVL-R101",
    summary: "a source's stop zones apply to every unzoned call at the place",
};

/// A derived route runs under its source route's agency, added as
/// `identity::overlay::agency` with the source agency's fields.
pub const OVL_R102: Rule = Rule {
    id: "OVL-R102",
    summary: "derived routes run under their source agencies, added once each",
};

/// A derived route of a national line carries that line's key; an added trip carries none of
/// a national trip (no CIS trip number is fabricated).
pub const OVL_R104: Rule = Rule {
    id: "OVL-R104",
    summary: "derived routes carry their national line key; added trips no national trip key",
};

/// Notes, features, claims, restrictions, keys and transfers of a base trip follow its parts
/// (`SplitTrip`) and its calls (`ReplaceTripCalls`); nothing is projected by mapping files.
pub const OVL_R111: Rule = Rule {
    id: "OVL-R111",
    summary: "base semantics follow their trips' parts and calls",
};

/// A source-native place has the group's name and centroid (exact, `overlay`) and no
/// administrative metadata; posts take their place's at publication.
pub const OVL_R117: Rule = Rule {
    id: "OVL-R117",
    summary: "native places carry the group's name and centroid, coordinate source overlay",
};

/// A part whose stated times would make its calls run backwards keeps the base's times (and no
/// further calls) and is diagnosed (`overlay_schedule_not_monotonic`).
///
/// A selection mixes source times at aligned calls with base times at the others; the
/// timetable's invariant forbids time running backwards, which F# did not check.
pub const OVL_R130: Rule = Rule {
    id: "OVL-R130",
    summary: "stated times that would run backwards are not taken",
};

pub const RULES: &[Rule] = &[
    OVL_R091, OVL_R098, OVL_R101, OVL_R102, OVL_R104, OVL_R111, OVL_R117, OVL_R130,
];

/// One output trip: a part of a base trip, an unchanged base trip, or an addition.
#[derive(Clone, Debug)]
pub struct Output {
    pub trip: TripId,
    /// Its public ID.
    pub id: String,
    pub dates: DateSet,
    pub route: RouteId,
    /// The addition it is.
    pub addition: Option<usize>,
    /// Its part's selection.
    pub selection: Option<Selection>,
    /// Its calls' sequences.
    pub sequences: Vec<i32>,
    /// Places its calls are at.
    pub places: Vec<PlaceId>,
    pub points: Vec<Option<BoardingPointId>>,
}

/// What the batch builder hands to keys, transfers and reports.
#[derive(Debug)]
pub struct Built {
    pub edits: Edits,
    pub summary: Summary,
    pub outputs: Vec<Output>,
    /// Per base trip index: its outputs.
    pub of_base: HashMap<u32, Vec<usize>>,
    /// Per addition: its output.
    pub of_addition: Vec<usize>,
    /// Post handles by post index (used posts only).
    pub post_handles: Vec<Option<BoardingPointId>>,
    /// Used posts.
    pub used_posts: Vec<bool>,
    pub derived_handles: Vec<Option<RouteId>>,
}

struct Handles {
    native: HashMap<(u32, u32), PlaceId>,
    posts: Vec<Option<BoardingPointId>>,
    derived: Vec<Option<RouteId>>,
}

impl Handles {
    fn place(&self, place: OutPlace) -> PlaceId {
        match place {
            OutPlace::Base(place) => place,
            OutPlace::Native { source, place } => self.native[&(source, place)],
        }
    }

    fn point(&self, stop: OutStop) -> Option<BoardingPointId> {
        match stop {
            OutStop::Post(post) => self.posts[post as usize],
            OutStop::Place(_) => None,
        }
    }

    fn route(&self, target: TargetRoute) -> RouteId {
        match target {
            TargetRoute::Base(route) => route,
            TargetRoute::Derived(index) => {
                self.derived[index as usize].expect("a used derived route")
            }
        }
    }
}

fn time(seconds: Option<i32>) -> OptTime {
    seconds.map_or(OptTime::NONE, |s| {
        OptTime::some(ServiceTime::from_seconds(s))
    })
}

fn monotonic(calls: &[Call]) -> bool {
    let mut latest: Option<ServiceTime> = None;
    for call in calls {
        for value in [call.arrival.get(), call.departure.get()]
            .into_iter()
            .flatten()
        {
            if latest.is_some_and(|l| value < l) {
                return false;
            }
            latest = Some(latest.map_or(value, |l| l.max(value)));
        }
    }
    true
}

#[allow(clippy::too_many_arguments)]
pub fn build(
    prepared: &Prepared<'_>,
    authority: &Authority,
    matches: &Matches,
    stops: &Stops,
    selections: &[Selection],
    plans: &[TripPlan],
    additions: &[Addition],
    diagnostics: &mut Diagnostics,
) -> Built {
    let base = &prepared.base;
    let t = base.timetable;
    let mut b = EditBuilder::new(t, PASS);
    let mut summary = Summary::default();

    // What output calls use.
    let mut used_stops: BTreeSet<OutStop> = BTreeSet::new();
    let mut used_derived: BTreeSet<u32> = BTreeSet::new();
    for plan in plans {
        for part in &plan.parts {
            if let Some(selection) = &part.selection {
                used_stops.extend(selection.facts.stops.iter().flatten().copied());
                used_stops.extend(selection.facts.repeats.iter().map(|r| r.stop));
            }
            if let Some(TargetRoute::Derived(index)) = part.route {
                used_derived.insert(index);
            }
        }
    }
    for addition in additions {
        if let TargetRoute::Derived(index) = addition.target {
            used_derived.insert(index);
        }
        let source = &prepared.sources[addition.source as usize];
        for call in source.trip_calls(&source.trips[addition.trip as usize]) {
            used_stops.extend(stops.out(addition.source, call.stop));
        }
    }
    let mut used_posts = vec![false; stops.posts.len()];
    let mut used_native: BTreeSet<(u32, u32)> = BTreeSet::new();
    for stop in &used_stops {
        if let OutStop::Post(post) = stop {
            used_posts[*post as usize] = true;
        }
        if let OutPlace::Native { source, place } = stops.place_of(*stop) {
            used_native.insert((source, place));
        }
    }

    // R102, R059: agencies and derived routes.
    let mut agencies: BTreeMap<(u32, Box<str>), AgencyId> = BTreeMap::new();
    let mut derived = vec![None; authority.derived_routes.len()];
    for &index in &used_derived {
        let route = &authority.derived_routes[index as usize];
        let source = &prepared.sources[route.source as usize];
        let feed_route =
            &source.feed.routes[source.routes[route.route as usize].feed_index as usize];
        let agency_key: Box<str> = feed_route.agency_id.clone().unwrap_or_default();
        let agency = match agencies.get(&(route.source, agency_key.clone())) {
            Some(&agency) => agency,
            None => {
                let row = source
                    .feed
                    .agencies
                    .iter()
                    .find(|a| a.agency_id.as_deref().unwrap_or("") == &*agency_key)
                    .or(source.feed.agencies.first());
                let added = b.add_agency(
                    OVL_R102,
                    Agency {
                        id: overlay::agency(source.source_id(), &agency_key),
                        name: row.and_then(|a| a.agency_name.clone()).unwrap_or_default(),
                        url: row.and_then(|a| a.agency_url.clone()),
                        timezone: row
                            .and_then(|a| a.agency_timezone.clone())
                            .unwrap_or_else(|| "Europe/Prague".into()),
                        language: row.and_then(|a| a.agency_lang.clone()),
                        phone: row.and_then(|a| a.agency_phone.clone()),
                        fare_url: row.and_then(|a| a.agency_fare_url.clone()),
                        email: row.and_then(|a| a.agency_email.clone()),
                        registration: None,
                    },
                );
                summary.agencies_added += 1;
                agencies.insert((route.source, agency_key), added);
                added
            }
        };
        let mode = source.routes[route.route as usize]
            .mode
            .unwrap_or(Mode::Bus);
        let handle = b.add_route(
            crate::authority::OVL_R059,
            Route {
                id: route.id.clone(),
                agency,
                mode,
                gtfs_route_type: feed_route.route_type.unwrap_or(3),
                short_name: feed_route.route_short_name.clone(),
                long_name: feed_route.route_long_name.clone(),
                description: feed_route.route_desc.clone(),
                url: feed_route.route_url.clone(),
                color: feed_route.route_color.clone(),
                text_color: feed_route.route_text_color.clone(),
                sort_order: feed_route.route_sort_order,
                timetable_kind: TimetableKind::Regular,
            },
        );
        summary.routes_added += 1;
        derived[index as usize] = Some(handle);
    }

    // R047, R117: native places; R079, R100: posts.
    let mut native = HashMap::new();
    for &(source_index, place_index) in &used_native {
        let source = &prepared.sources[source_index as usize];
        let place = &source.native_places[place_index as usize];
        let group = &source.groups[place.group as usize];
        let handle = b.add_place(
            crate::prepare::OVL_R047,
            Place {
                id: super::stops::place_public_id(
                    prepared,
                    OutPlace::Native {
                        source: source_index,
                        place: place_index,
                    },
                ),
                kind: PlaceKind::StopPlace,
                name: group.name.clone(),
                source_code: None,
                description: None,
                municipality: None,
                district_name: None,
                district_code: None,
                nearby_place: None,
                country: None,
                position: group
                    .position
                    .filter(|c| c.is_valid())
                    .map(|c| Position::exact(c, CoordSource::Overlay)),
                url: None,
                timezone: None,
                wheelchair_boarding: Tristate::Unknown,
            },
        );
        summary.places_added += 1;
        native.insert((source_index, place_index), handle);
    }
    let mut handles = Handles {
        native,
        posts: vec![None; stops.posts.len()],
        derived,
    };
    for (index, post) in stops.posts.iter().enumerate() {
        if !used_posts[index] {
            continue;
        }
        let (source_index, stop) = post.stop;
        let source = &prepared.sources[source_index as usize];
        let row = &source.feed.stops[stop as usize];
        let position = (capability::enabled(prepared, source_index, Capability::BoardingPoints))
            .then_some(())
            .and(row.stop_lat.zip(row.stop_lon))
            .map(|(lat, lon)| Coord::new(lon, lat))
            .filter(|c| c.is_valid())
            .map(|c| Position::exact(c, CoordSource::Overlay));
        let handle = b.add_boarding_point(
            super::stops::OVL_R079,
            BoardingPoint {
                id: post.id.clone(),
                place: handles.place(post.place),
                kind: BoardingPointKind::Post,
                public_code: row.platform_code.clone().filter(|p| !p.trim().is_empty()),
                source_code: source.post_keys[stop as usize].clone(),
                source_name: None,
                description: None,
                position,
                wheelchair_boarding: match row.wheelchair_boarding {
                    Some(1) => Tristate::Yes,
                    Some(2) => Tristate::No,
                    _ => Tristate::Unknown,
                },
            },
        );
        summary.posts_added += 1;
        handles.posts[index] = Some(handle);
    }

    // R101: zones per place.
    let mut zones: HashMap<PlaceId, Box<[Zone]>> = HashMap::new();
    for (source_index, source) in prepared.sources.iter().enumerate() {
        let Some(hook) = &source.profile.hooks.zones else {
            continue;
        };
        if !capability::enabled(prepared, source_index as u32, Capability::StopZones) {
            continue;
        }
        for group in &source.groups {
            if group.zones.is_empty() {
                continue;
            }
            let Some(matched) = group.matched else {
                continue;
            };
            let place = match matched.target {
                PlaceRef::Base(place) => Some(place),
                PlaceRef::Native(index) => {
                    handles.native.get(&(source_index as u32, index)).copied()
                }
            };
            if let Some(place) = place {
                zones.insert(
                    place,
                    group
                        .zones
                        .iter()
                        .map(|code| Zone {
                            code: code.clone(),
                            system: Some(hook.system.clone()),
                        })
                        .collect(),
                );
            }
        }
    }
    let zone_calls =
        |b: &mut EditBuilder<'_>, trip: TripId, calls: &[Call], summary: &mut Summary| {
            for call in calls {
                if call.passenger
                    && call.zones.is_none()
                    && let Some(set) = zones.get(&call.place)
                {
                    b.push(
                        OVL_R101,
                        jr_model::Edit::SetCallZones {
                            trip,
                            sequence: call.sequence,
                            zones: set.clone(),
                        },
                    );
                    summary.calls_zoned += 1;
                }
            }
        };

    // Base trips, in handle order.
    let plan_of: HashMap<u32, &TripPlan> = plans.iter().map(|p| (p.base, p)).collect();
    let mut outputs: Vec<Output> = Vec::new();
    let mut of_base: HashMap<u32, Vec<usize>> = HashMap::new();
    let empty = b.service(DateSet::EMPTY);
    for (index, base_trip) in base.trips.iter().enumerate() {
        let index = index as u32;
        let trip = &t.trips[base_trip.trip];
        let Some(plan) = plan_of.get(&index) else {
            zone_calls(&mut b, base_trip.trip, &trip.calls, &mut summary);
            outputs.push(output(
                trip.id.to_string(),
                base_trip.trip,
                base_trip.dates,
                trip.route,
                None,
                None,
                &trip.calls,
            ));
            of_base.entry(index).or_default().push(outputs.len() - 1);
            continue;
        };
        if plan.parts.is_empty() {
            b.push(
                super::slices::OVL_R086,
                jr_model::Edit::SetTripService {
                    trip: base_trip.trip,
                    service: empty,
                },
            );
            summary.trips_retired += 1;
            continue;
        }
        if plan.split() {
            summary.trips_sliced += 1;
        }
        for part in &plan.parts {
            let service = b.service(part.dates);
            let handle = match &part.id {
                Some(id) => {
                    summary.slices += 1;
                    b.split_trip(super::slices::OVL_R087, base_trip.trip, id.clone(), service)
                }
                None => {
                    if part.dates != base_trip.dates {
                        b.push(
                            super::slices::OVL_R086,
                            jr_model::Edit::SetTripService {
                                trip: base_trip.trip,
                                service,
                            },
                        );
                        summary.trips_shortened += 1;
                    }
                    base_trip.trip
                }
            };
            let mut calls = trip.calls.clone();
            if let Some(selection) = &part.selection {
                summary.trips_bound += 1;
                let (new_calls, kept) = part_calls(&mut b, &handles, stops, trip, part, selection);
                if monotonic(&new_calls) {
                    calls = new_calls;
                    replace_calls(
                        &mut b,
                        super::slices::OVL_R093,
                        handle,
                        &trip.calls,
                        &calls,
                        kept,
                    );
                } else {
                    diagnostics.push(
                        Diagnostic::new(
                            "overlay_schedule_not_monotonic",
                            Severity::Warning,
                            part.id.as_ref().unwrap_or(&trip.id).to_string(),
                            "Stated times would run backwards; the base times are kept",
                        )
                        .rule(OVL_R130),
                    );
                    let mut timeless = part.clone();
                    let mut stripped = selection.clone();
                    stripped.facts.arrivals = vec![None; stripped.facts.arrivals.len()].into();
                    stripped.facts.departures = vec![None; stripped.facts.departures.len()].into();
                    stripped.facts.repeats = Box::new([]);
                    stripped.repeat_ordinals = Box::new([]);
                    timeless.repeat_headsigns.clear();
                    let (new_calls, kept) =
                        part_calls(&mut b, &handles, stops, trip, &timeless, &stripped);
                    calls = new_calls;
                    replace_calls(&mut b, OVL_R130, handle, &trip.calls, &calls, kept);
                }
            }
            let mut route = trip.route;
            if let Some(target) = part.route {
                route = handles.route(target);
                b.push(
                    super::slices::OVL_R088,
                    jr_model::Edit::SetTripRoute {
                        trip: handle,
                        route,
                    },
                );
            }
            if let Some(headsign) = &part.headsign {
                let sym = b.string(headsign);
                b.push(
                    super::headsign::OVL_R089,
                    jr_model::Edit::SetTripHeadsign {
                        trip: handle,
                        headsign: Some(sym),
                    },
                );
            }
            zone_calls(&mut b, handle, &calls, &mut summary);
            outputs.push(output(
                part.id.as_ref().unwrap_or(&trip.id).to_string(),
                handle,
                part.dates,
                route,
                None,
                part.selection.clone(),
                &calls,
            ));
            of_base.entry(index).or_default().push(outputs.len() - 1);
        }
        if plan.split() {
            b.push(
                super::slices::OVL_R087,
                jr_model::Edit::SetTripService {
                    trip: base_trip.trip,
                    service: empty,
                },
            );
        }
    }

    // R090, R095: additions in ID order.
    let mut of_addition = Vec::with_capacity(additions.len());
    for (index, addition) in additions.iter().enumerate() {
        let source = &prepared.sources[addition.source as usize];
        let source_trip = &source.trips[addition.trip as usize];
        let feed_trip = &source.feed.trips[source_trip.feed_index as usize];
        let source_calls = source.trip_calls(source_trip);
        let ordered = source_calls
            .windows(2)
            .all(|w| matches!((w[0].sequence, w[1].sequence), (Some(a), Some(b)) if a < b));
        let mut calls = Vec::with_capacity(source_calls.len());
        for (ordinal, call) in source_calls.iter().enumerate() {
            let stop = stops
                .out(addition.source, call.stop)
                .expect("projected calls are placed");
            let row = source.feed.stop_times.get(call.row as usize);
            let boarding = |code: Option<i32>| match code {
                Some(1) => jr_model::timetable::Boarding::None,
                Some(2) => jr_model::timetable::Boarding::PhoneAgency,
                Some(3) => jr_model::timetable::Boarding::AskDriver,
                _ => jr_model::timetable::Boarding::Regular,
            };
            let stop_headsign = addition.stop_headsigns[ordinal]
                .as_deref()
                .map(|h| b.string(h));
            calls.push(Call {
                place: handles.place(stops.place_of(stop)),
                boarding_point: handles.point(stop),
                track: None,
                route_stop: None,
                arrival: time(call.arrival.filter(|&a| a >= 0)),
                departure: time(call.departure.filter(|&d| d >= 0)),
                sequence: if ordered {
                    call.sequence.unwrap_or(0)
                } else {
                    ordinal as i32 + 1
                },
                stop_headsign,
                line: None,
                zones: None,
                kilometre: None,
                pickup: boarding(row.pickup_type),
                dropoff: boarding(row.drop_off_type),
                passenger: true,
                timepoint: row.timepoint != Some(0),
            });
        }
        let route = handles.route(addition.target);
        let service = b.service(addition.dates);
        let headsign = addition.headsign.as_deref().map(|h| b.string(h));
        let short_name = feed_trip.trip_short_name.as_deref().map(|s| b.string(s));
        let block = feed_trip
            .block_id
            .as_deref()
            .map(|block| b.string(&format!("{}:{block}", source.source_id())));
        let tristate = |value: Option<i32>| match value {
            Some(1) => Tristate::Yes,
            Some(2) => Tristate::No,
            _ => Tristate::Unknown,
        };
        let handle = b.add_trip(
            super::additions::OVL_R090,
            Trip {
                id: addition.id.clone(),
                route,
                service,
                direction: feed_trip.direction_id.and_then(|d| u8::try_from(d).ok()),
                headsign,
                short_name,
                block,
                run: None,
                wheelchair_accessible: tristate(feed_trip.wheelchair_accessible),
                bikes_allowed: tristate(feed_trip.bikes_allowed),
                shape: None,
                calls: calls.clone(),
            },
        );
        summary.trips_added += 1;
        zone_calls(&mut b, handle, &calls, &mut summary);
        let mut out = output(
            addition.id.to_string(),
            handle,
            addition.dates,
            route,
            Some(index),
            None,
            &calls,
        );
        out.addition = Some(index);
        outputs.push(out);
        of_addition.push(outputs.len() - 1);
    }

    // R046: corrected names; R091: coordinates.
    let used_places: BTreeSet<PlaceId> = outputs
        .iter()
        .flat_map(|o| o.places.iter().copied())
        .collect();
    places(
        prepared,
        stops,
        &used_places,
        &mut b,
        diagnostics,
        &mut summary,
    );

    // R104: line keys of derived routes.
    for (index, route) in authority.derived_routes.iter().enumerate() {
        let (Some(handle), crate::authority::AuthorityLine::National(line)) =
            (handles.derived[index], &route.line)
        else {
            continue;
        };
        let mut dates = DateSet::EMPTY;
        for o in outputs.iter().filter(|o| o.route == handle) {
            dates = dates | o.dates;
        }
        let (Some(first), Some(last)) = (dates.first(), dates.last()) else {
            continue;
        };
        // A national line key is six digits, zero-padded; a claim that is no such number gives
        // none (F# `isCisLine`).
        if line.is_empty() || line.len() > 6 || !line.bytes().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let line: Box<str> = format!("{line:0>6}").into();
        b.push(
            OVL_R104,
            jr_model::Edit::AddKey(SourceKey {
                namespace: Namespace(std::borrow::Cow::Owned(
                    base.profile.line_namespace.to_string(),
                )),
                identifier: line,
                entity: Entity::Route(handle),
                valid_from: t.horizon.date_at(first),
                valid_to: t.horizon.date_at(last),
                binding: BindingMethod::Identity,
            }),
        );
    }

    // R098.
    presentation(
        prepared,
        matches,
        additions,
        &outputs,
        &of_base,
        &of_addition,
        &mut b,
        diagnostics,
    );

    // R107–R109 and R112–R116.
    let context = Context {
        prepared,
        selections,
        authority,
        matches,
        stops,
        outputs: &outputs,
        of_base: &of_base,
        of_addition: &of_addition,
        additions,
        handles: &handles.posts,
        used_posts: &used_posts,
        derived: &handles.derived,
    };
    transfers::build(&context, &mut b, diagnostics, &mut summary);
    keys::build(&context, &mut b, &mut summary);

    Built {
        edits: b.finish(),
        summary,
        outputs,
        of_base,
        of_addition,
        post_handles: handles.posts,
        used_posts,
        derived_handles: handles.derived,
    }
}

/// What transfers, keys and reports read.
pub struct Context<'c, 'p> {
    pub prepared: &'c Prepared<'p>,
    pub selections: &'c [Selection],
    pub authority: &'c Authority,
    pub matches: &'c Matches,
    pub stops: &'c Stops,
    pub outputs: &'c [Output],
    pub of_base: &'c HashMap<u32, Vec<usize>>,
    pub of_addition: &'c [usize],
    pub additions: &'c [Addition],
    pub handles: &'c [Option<BoardingPointId>],
    pub used_posts: &'c [bool],
    pub derived: &'c [Option<RouteId>],
}

impl Context<'_, '_> {
    /// The outputs of a binding's base trip whose selection agrees with the binding's, with the
    /// dates they share (F#'s `slicesForBinding`).
    pub fn slices_for(&self, binding: usize, selection: &Selection) -> Vec<(usize, DateSet)> {
        let b = &self.matches.bindings[binding];
        let Some(outputs) = self.of_base.get(&b.candidate.base_trip) else {
            return Vec::new();
        };
        outputs
            .iter()
            .filter_map(|&o| {
                let output = &self.outputs[o];
                let selected = output.selection.as_ref()?;
                if !selected.compatible(selection) {
                    return None;
                }
                let dates = output.dates & b.dates;
                (!dates.is_empty()).then_some((o, dates))
            })
            .collect()
    }
}

fn output(
    id: String,
    trip: TripId,
    dates: DateSet,
    route: RouteId,
    addition: Option<usize>,
    selection: Option<Selection>,
    calls: &[Call],
) -> Output {
    Output {
        trip,
        id,
        dates,
        route,
        addition,
        selection,
        sequences: calls.iter().map(|c| c.sequence).collect(),
        places: calls
            .iter()
            .filter(|c| c.passenger)
            .map(|c| c.place)
            .collect(),
        points: calls.iter().map(|c| c.boarding_point).collect(),
    }
}

/// The edits that turn `old` calls into `new`: nothing when equal, boarding points alone when
/// only they differ, else the whole call list.
fn replace_calls(
    b: &mut EditBuilder<'_>,
    rule: Rule,
    trip: TripId,
    old: &[Call],
    new: &[Call],
    kept: Vec<(i32, i32)>,
) {
    if old == new {
        return;
    }
    let only_points = old.len() == new.len()
        && old.iter().zip(new).all(|(a, b)| {
            Call {
                boarding_point: b.boarding_point,
                ..*a
            } == *b
        });
    if only_points {
        for (index, (a, c)) in old.iter().zip(new).enumerate() {
            if a.boarding_point != c.boarding_point {
                b.push(
                    rule,
                    jr_model::Edit::SetCallBoardingPoint {
                        trip,
                        call: index as u32,
                        boarding_point: c.boarding_point,
                    },
                );
            }
        }
    } else {
        b.push(
            rule,
            jr_model::Edit::ReplaceTripCalls {
                trip,
                calls: new.to_vec(),
                kept,
            },
        );
    }
}

/// R093, R094: the calls of a part with a selection, and the old-to-new sequence map.
fn part_calls(
    b: &mut EditBuilder<'_>,
    handles: &Handles,
    stops: &Stops,
    trip: &Trip,
    part: &Part,
    selection: &Selection,
) -> (Vec<Call>, Vec<(i32, i32)>) {
    let mut calls = Vec::with_capacity(trip.calls.len() + selection.facts.repeats.len());
    let mut kept = Vec::with_capacity(trip.calls.len());
    let mut inserted = 0;
    let mut ordinal = 0usize;
    for call in &trip.calls {
        if !call.passenger {
            let mut moved = *call;
            moved.sequence += inserted;
            kept.push((call.sequence, moved.sequence));
            calls.push(moved);
            continue;
        }
        let mut out = *call;
        out.sequence += inserted;
        if let Some(stop) = selection.facts.stops.get(ordinal).copied().flatten() {
            out.boarding_point = handles.point(stop);
        }
        if let Some(Some(arrival)) = selection.facts.arrivals.get(ordinal) {
            out.arrival = time(Some(*arrival));
        }
        if let Some(Some(departure)) = selection.facts.departures.get(ordinal) {
            out.departure = time(Some(*departure));
        }
        if let Some(Some(headsign)) = part.stop_headsigns.get(ordinal) {
            out.stop_headsign = Some(b.string(headsign));
        }
        kept.push((call.sequence, out.sequence));
        calls.push(out);
        for (index, repeat) in selection.facts.repeats.iter().enumerate() {
            if repeat.target as usize != ordinal {
                continue;
            }
            inserted += 1;
            let mut further = *call;
            further.sequence = call.sequence + inserted;
            further.place = handles.place(stops.place_of(repeat.stop));
            further.boarding_point = handles.point(repeat.stop);
            further.arrival = time(repeat.arrival);
            further.departure = time(repeat.departure);
            further.route_stop = None;
            if let Some(Some(headsign)) = part.repeat_headsigns.get(index) {
                further.stop_headsign = Some(b.string(headsign));
            }
            calls.push(further);
        }
        ordinal += 1;
    }
    (calls, kept)
}

/// R046, R091.
fn places(
    prepared: &Prepared<'_>,
    stops: &Stops,
    used: &BTreeSet<PlaceId>,
    b: &mut EditBuilder<'_>,
    diagnostics: &mut Diagnostics,
    summary: &mut Summary,
) {
    let mut claims: BTreeMap<PlaceId, Vec<Coord>> = BTreeMap::new();
    for (index, source) in prepared.sources.iter().enumerate() {
        for group in &source.groups {
            let Some(matched) = group.matched else {
                continue;
            };
            let PlaceRef::Base(place) = matched.target else {
                continue;
            };
            if matched.corrected {
                b.push(
                    crate::prepare::OVL_R046,
                    jr_model::Edit::SetPlaceName {
                        place,
                        name: group.name.clone(),
                    },
                );
                summary.places_renamed += 1;
            }
            if !capability::enabled(prepared, index as u32, Capability::StopCoordinates) {
                continue;
            }
            let inferred = matches!(
                matched.method,
                MatchMethod::TripContextUnique | MatchMethod::TripContextGtfsAuthoritative
            );
            let accepted = group
                .members
                .iter()
                .any(|&m| stops.accepted[index][m as usize]);
            if !(inferred || accepted) {
                continue;
            }
            let Some(centroid) = group.position else {
                continue;
            };
            if !centroid.is_valid() {
                diagnostics.push(
                    Diagnostic::new(
                        "stop_coordinate_invalid",
                        Severity::Warning,
                        group.registry_key(source.source_id()),
                        format!(
                            "Refusing invalid GTFS centroid {},{}",
                            centroid.lat, centroid.lon
                        ),
                    )
                    .rule(OVL_R091),
                );
                continue;
            }
            claims.entry(place).or_default().push(centroid);
        }
    }
    for (place, mut centroids) in claims {
        if !used.contains(&place) {
            continue;
        }
        centroids.dedup_by(|a, b| a == b);
        let mut distinct: Vec<Coord> = Vec::new();
        for c in centroids {
            if !distinct.contains(&c) {
                distinct.push(c);
            }
        }
        if let [only] = distinct.as_slice() {
            b.push(
                OVL_R091,
                jr_model::Edit::SetPlacePosition {
                    place,
                    position: Position::exact(*only, CoordSource::Overlay),
                },
            );
            summary.places_moved += 1;
        } else {
            diagnostics.push(
                Diagnostic::new(
                    "stop_coordinate_conflict",
                    Severity::Warning,
                    prepared.base.timetable.places[place].id.to_string(),
                    "Multiple authoritative coordinates",
                )
                .rule(OVL_R091),
            );
        }
    }
}

/// R098.
#[allow(clippy::too_many_arguments)]
fn presentation(
    prepared: &Prepared<'_>,
    matches: &Matches,
    additions: &[Addition],
    outputs: &[Output],
    of_base: &HashMap<u32, Vec<usize>>,
    of_addition: &[usize],
    b: &mut EditBuilder<'_>,
    diagnostics: &mut Diagnostics,
) {
    let base = &prepared.base;
    let used: BTreeSet<RouteId> = outputs.iter().map(|o| o.route).collect();
    let mut sources: BTreeMap<RouteId, Vec<(u32, u32)>> = BTreeMap::new();
    for binding in &matches.bindings {
        let still = of_base
            .get(&binding.candidate.base_trip)
            .is_some_and(|parts| {
                parts.iter().any(|&o| {
                    outputs[o].selection.is_some() && outputs[o].dates.intersects(&binding.dates)
                })
            });
        if still {
            let route = base.trips[binding.candidate.base_trip as usize].route;
            let source = &prepared.sources[binding.source as usize];
            sources
                .entry(route)
                .or_default()
                .push((binding.source, source.trips[binding.trip as usize].route));
        }
    }
    for (index, addition) in additions.iter().enumerate() {
        let route = outputs[of_addition[index]].route;
        let source = &prepared.sources[addition.source as usize];
        sources
            .entry(route)
            .or_default()
            .push((addition.source, source.trips[addition.trip as usize].route));
    }
    for (route, list) in sources {
        if !used.contains(&route) || route.index_is_added(base.timetable) {
            continue;
        }
        let values = |capability: Capability,
                      field: fn(&jr_formats::gtfs::Route) -> Option<&str>|
         -> Vec<Box<str>> {
            let mut values: Vec<Box<str>> = Vec::new();
            for &(source, source_route) in &list {
                if !capability::enabled(prepared, source, capability) {
                    continue;
                }
                let s = &prepared.sources[source as usize];
                let row = &s.feed.routes[s.routes[source_route as usize].feed_index as usize];
                if let Some(value) = field(row).filter(|v| !v.trim().is_empty())
                    && !values.iter().any(|v| **v == *value)
                {
                    values.push(value.into());
                }
            }
            values
        };
        let id = base.timetable.routes[route].id.to_string();
        let mut pick = |column: &str, values: Vec<Box<str>>| match values.as_slice() {
            [one] => Some(one.clone()),
            [] => None,
            _ => {
                diagnostics.push(
                    Diagnostic::new(
                        "route_display_conflict",
                        Severity::Warning,
                        id.clone(),
                        format!("Conflicting {column} values"),
                    )
                    .rule(OVL_R098),
                );
                None
            }
        };
        let short_name = pick(
            "route_short_name",
            values(Capability::RouteShortName, |r| {
                r.route_short_name.as_deref()
            }),
        );
        let _long_name = pick(
            "route_long_name",
            values(Capability::RouteLongName, |r| r.route_long_name.as_deref()),
        );
        let color = pick(
            "route_color",
            values(Capability::RouteColor, |r| r.route_color.as_deref()),
        );
        let text_color = pick(
            "route_text_color",
            values(Capability::RouteTextColor, |r| {
                r.route_text_color.as_deref()
            }),
        );
        if short_name.is_some() || color.is_some() || text_color.is_some() {
            b.push(
                OVL_R098,
                jr_model::Edit::SetRoutePresentation {
                    route,
                    short_name,
                    color,
                    text_color,
                },
            );
        }
    }
}

trait AddedRoute {
    fn index_is_added(self, timetable: &jr_model::Timetable) -> bool;
}

impl AddedRoute for RouteId {
    fn index_is_added(self, timetable: &jr_model::Timetable) -> bool {
        use jr_model::handle::Handle;
        self.index() >= timetable.routes.len()
    }
}
