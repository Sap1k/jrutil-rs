//! Trips and their published calls (JDF-R433–R446, R448, R449, R451, R473).

use std::collections::{BTreeSet, HashMap};

use jr_formats::jdf::TimeCell;
use jr_model::identity::{self, PublicId};
use jr_model::timetable::{Boarding, Call, Trip, Tristate, Zone};
use jr_model::{
    Diagnostic, Diagnostics, OptTime, RouteId, Rule, ServiceTime, Severity, Timetable, TripId,
    ZoneSetId,
};

use super::Policy;
use super::places::Points;
use crate::merge::{Merged, Version};

/// Trip IDs are built from the version's key parts (licence, published start, detour flag,
/// validity hash, side-by-side rank: jdf-merge R254) and the trip number.
pub const R433: Rule = Rule {
    id: "JDF-R433",
    summary: "Trip IDs come from the version's key parts and the trip number",
};

/// JDF convention: odd trips visit the route stops in ascending order (direction 1), even trips
/// in descending order (direction 0).
pub const R434: Rule = Rule {
    id: "JDF-R434",
    summary: "Trip number parity gives the call order and the direction",
};

/// JDF has no headsign: it is the name of the stop of the last published call (the proposed
/// verdict; F# took the last row, served or not, B17).
pub const R435: Rule = Rule {
    id: "JDF-R435",
    summary: "The headsign is the name of the last published call's stop",
};

/// The trip short name is `<licence> <trip number>` (verdict Q7: kept).
pub const R436: Rule = Rule {
    id: "JDF-R436",
    summary: "The trip short name is the licence and the trip number",
};

/// JDF is a closed world for trip accessibility: `@` or `{` makes a trip accessible, their
/// absence inaccessible.
pub const R437: Rule = Rule {
    id: "JDF-R437",
    summary: "Trips are wheelchair accessible exactly when coded @ or {",
};

/// Bicycles: `O` allows them, its absence does not.
pub const R438: Rule = Rule {
    id: "JDF-R438",
    summary: "Trips allow bicycles exactly when coded O",
};

/// A trip's calls are ordered by route stop (R434), equal route stops keeping file order (F#'s
/// sort was unstable, B19); a repeated route stop is diagnosed.
pub const R440: Rule = Rule {
    id: "JDF-R440",
    summary: "Calls are ordered by route stop, stably; repeated route stops are diagnosed",
};

/// A call that passes (`|`) or bypasses (`<`), or has no time, is not published (the source
/// gives it no time). A passing arrival beside a departure time (F# crashed, B13) is diagnosed
/// and the call published with its one time.
pub const R441: Rule = Rule {
    id: "JDF-R441",
    summary: "Passed, bypassed and untimed calls are not published",
};

/// Times are clock readings: in call order, arrival then departure, a reading earlier than the
/// previous one crosses midnight (+24 h) when the gap so implied is at most the policy's; a
/// larger one is a data error that drops the trip (B18). A missing side takes the other's time.
pub const R442: Rule = Rule {
    id: "JDF-R442",
    summary: "Clock times cross midnight on a short backwards step; a long one drops the trip",
};

/// The published sequence is the 0-based index in the ordered calls, unpublished ones counted
/// (verdict Q8), so sequences have gaps.
pub const R443: Rule = Rule {
    id: "JDF-R443",
    summary: "The call sequence is the index among all ordered calls",
};

/// Pickup and drop-off from the codes of the call, its route stop (`Zaslinky`: "the default for
/// all trips of the line", JDF 1.11), its stop and its trip (B1, B2, B23 fixed,
/// `docs/roadmap.md`). A trip's `T` or `!` reaches every call unless some of its calls carry
/// that code themselves, which then are the ones it means ("obsluhu označených zastávek").
/// Arrangement (`T`, `!`) over request (`x`) over regular. `(` and `)` exclude each other: the
/// most specific level carrying either decides; `$` holds from any level.
pub const R444: Rule = Rule {
    id: "JDF-R444",
    summary: "Pickup and drop-off follow the codes of the call, route stop, stop and trip",
};

/// Every JDF time is a timepoint; the 1.11 time windows are not carried (verdict Q10).
pub const R445: Rule = Rule {
    id: "JDF-R445",
    summary: "Calls are timepoints; time windows are not carried",
};

/// The call's tariff kilometre is kept as the source distance.
pub const R446: Rule = Rule {
    id: "JDF-R446",
    summary: "The timetable kilometre is the call's source distance",
};

/// A route stop's zone text is split at `,`, trimmed, emptied and repeated tokens dropped,
/// order kept; no IDS system is inferred.
pub const R448: Rule = Rule {
    id: "JDF-R448",
    summary: "Zone texts become ordered, deduplicated zone codes without a system",
};

/// A published call carries the zones of its route stop.
pub const R449: Rule = Rule {
    id: "JDF-R449",
    summary: "Calls carry their route stop's zones",
};

/// A route stop number naming several stops in one version cannot place zones: its calls get
/// none, diagnosed.
pub const R451: Rule = Rule {
    id: "JDF-R451",
    summary: "A route stop naming several stops gives no zones",
};

/// What F# crashed on becomes a diagnostic that drops the fact: a trip with fewer than two
/// published calls is dropped (the model needs two).
pub const R473: Rule = Rule {
    id: "JDF-R473",
    summary: "Trips with fewer than two published calls are dropped",
};

/// Where a call boards.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Point {
    Unspecified,
    /// An `Oznacniky` post number.
    Authored(u64),
    /// A `Zasspoje` stanoviště text, trimmed.
    Textual(Box<str>),
}

#[derive(Clone, Debug)]
pub(crate) struct PlannedCall {
    pub stop: u32,
    pub point: Point,
    /// The textual number a call gives beside its authored post (R419).
    pub authored_number: Option<Box<str>>,
    pub arrival: ServiceTime,
    pub departure: ServiceTime,
    pub sequence: i32,
    pub pickup: Boarding,
    pub dropoff: Boarding,
    pub zones: Option<ZoneSetId>,
    pub kilometre: Option<f64>,
}

#[derive(Clone, Debug)]
pub(crate) struct PlannedTrip {
    /// Index into the version's trips.
    pub trip: usize,
    pub calls: Vec<PlannedCall>,
    /// Route stop of every ordered call, published or not, with its sequence and whether it is
    /// published (claims and restrictions on unpublished calls are diagnosed).
    pub ordered: Vec<(u64, i32, bool)>,
}

/// Per version, its publishable trips.
pub(crate) struct Plans {
    pub trips: Vec<Vec<PlannedTrip>>,
}

/// A trip in the timetable.
#[derive(Clone, Debug)]
pub(crate) struct PublishedTrip {
    pub trip: usize,
    pub handle: TripId,
    pub id: PublicId,
    /// (route stop, sequence, published) of every ordered call.
    pub ordered: Vec<(u64, i32, bool)>,
}

/// Per version, its published trips.
pub(crate) struct Published {
    pub versions: Vec<Vec<PublishedTrip>>,
}

fn has(codes: &[Box<str>], list: &[Box<str>]) -> bool {
    codes.iter().any(|code| list.iter().any(|l| l == code))
}

/// R442: a clock reading in minutes, with the running day offset.
struct Clock {
    last: Option<i32>,
    offset: i32,
    max_gap: i32,
}

impl Clock {
    fn read(&mut self, minutes: i32) -> Option<ServiceTime> {
        if let Some(last) = self.last
            && minutes < last
        {
            if minutes + 1440 - last > self.max_gap {
                return None;
            }
            self.offset += ServiceTime::DAY;
        }
        self.last = Some(minutes);
        Some(ServiceTime::from_seconds(minutes * 60 + self.offset))
    }
}

/// R448, R451: the zones of each route stop number of a version.
fn route_stop_zones(
    version: &Version,
    timetable: &mut Timetable,
    diagnostics: &mut Diagnostics,
) -> HashMap<u64, Option<ZoneSetId>> {
    let mut stops: HashMap<u64, BTreeSet<u32>> = HashMap::new();
    for row in &version.route_stops {
        stops.entry(row.route_stop_id).or_default().insert(row.stop);
    }
    let mut zones = HashMap::new();
    for row in &version.route_stops {
        if zones.contains_key(&row.route_stop_id) {
            continue;
        }
        if stops[&row.route_stop_id].len() > 1 {
            diagnostics.push(
                Diagnostic::new(
                    "jdf_route_stop_ambiguous",
                    Severity::Warning,
                    format!("{}:route-stop:{}", version.subject(), row.route_stop_id),
                    "the route stop names several stops: no zones applied",
                )
                .rule(R451),
            );
            zones.insert(row.route_stop_id, None);
            continue;
        }
        let mut codes: Vec<Zone> = Vec::new();
        for token in row.zone.as_deref().unwrap_or("").split(',').map(str::trim) {
            if !token.is_empty() && !codes.iter().any(|z| &*z.code == token) {
                codes.push(Zone {
                    code: token.into(),
                    system: None,
                });
            }
        }
        zones.insert(row.route_stop_id, timetable.zone_sets.intern(&codes));
    }
    zones
}

/// R434, R440–R446, R449, R473 for every trip of every version.
pub(crate) fn plan(
    merged: &Merged,
    policy: &Policy,
    timetable: &mut Timetable,
    diagnostics: &mut Diagnostics,
) -> Plans {
    let codes = &policy.codes;
    let mut trips = Vec::with_capacity(merged.versions.len());
    for version in &merged.versions {
        let subject = version.subject();
        let zones = route_stop_zones(version, timetable, diagnostics);
        let route_stop_codes: HashMap<u64, &[Box<str>]> = version
            .route_stops
            .iter()
            .map(|rs| (rs.route_stop_id, merged.code_sets.get(rs.codes)))
            .collect();
        let mut planned = Vec::with_capacity(version.trips.len());
        'trips: for (index, trip) in version.trips.iter().enumerate() {
            let trip_subject = || format!("{subject}/{}", trip.id);
            let reverse = trip.id % 2 == 0;
            let mut order: Vec<usize> = (0..trip.calls.len()).collect();
            if reverse {
                order.sort_by_key(|&i| std::cmp::Reverse(trip.calls[i].route_stop_id));
            } else {
                order.sort_by_key(|&i| trip.calls[i].route_stop_id);
            }
            if order
                .windows(2)
                .any(|w| trip.calls[w[0]].route_stop_id == trip.calls[w[1]].route_stop_id)
            {
                diagnostics.push(
                    Diagnostic::new(
                        "jdf_route_stop_repeated",
                        Severity::Warning,
                        trip_subject(),
                        "the trip calls at one route stop more than once: file order kept",
                    )
                    .rule(R440),
                );
            }
            let mut clock = Clock {
                last: None,
                offset: 0,
                max_gap: policy.times.max_midnight_gap_minutes,
            };
            // R444: the trip's arrangement codes its calls do not narrow down.
            let trip_codes = merged.code_sets.get(trip.codes);
            let trip_wide: Vec<&Box<str>> = codes
                .phone_agency
                .iter()
                .filter(|code| trip_codes.contains(code))
                .filter(|code| {
                    !trip
                        .calls
                        .iter()
                        .any(|c| merged.code_sets.get(c.codes).contains(code))
                })
                .collect();
            let mut calls = Vec::new();
            let mut ordered = Vec::with_capacity(order.len());
            for (sequence, &at) in order.iter().enumerate() {
                let call = &trip.calls[at];
                let sequence = sequence as i32;
                let minutes = |cell: Option<TimeCell>| match cell {
                    Some(TimeCell::At { minutes }) => Some(i32::from(minutes)),
                    _ => None,
                };
                let passes = |cell: Option<TimeCell>| {
                    matches!(cell, Some(TimeCell::Passes | TimeCell::OtherRoute))
                };
                let (arrival, departure) = (minutes(call.arrival), minutes(call.departure));
                let served = !passes(call.departure) && (arrival.is_some() || departure.is_some());
                if passes(call.arrival) && !passes(call.departure) && departure.is_some() {
                    diagnostics.push(
                        Diagnostic::new(
                            "jdf_inconsistent_pass",
                            Severity::Warning,
                            trip_subject(),
                            format!(
                                "route stop {} passes on arrival but has a departure: published \
                                 with its departure",
                                call.route_stop_id
                            ),
                        )
                        .rule(R441),
                    );
                }
                ordered.push((call.route_stop_id, sequence, served));
                if !served {
                    continue;
                }
                let mut read = |value: Option<i32>| value.map(|m| clock.read(m));
                let (arrival, departure) = match (read(arrival), read(departure)) {
                    (Some(None), _) | (_, Some(None)) => {
                        diagnostics.push(
                            Diagnostic::new(
                                "jdf_time_backwards",
                                Severity::Warning,
                                trip_subject(),
                                format!(
                                    "time goes backwards at route stop {} by more than a midnight \
                                     allows: trip dropped",
                                    call.route_stop_id
                                ),
                            )
                            .rule(R442),
                        );
                        continue 'trips;
                    }
                    (Some(Some(a)), Some(Some(d))) => (a, d),
                    (Some(Some(a)), None) => (a, a),
                    (None, Some(Some(d))) => (d, d),
                    (None, None) => continue,
                };
                let stop = &merged.stops[call.stop as usize];
                // R444, most specific level first.
                let levels: [&[Box<str>]; 3] = [
                    merged.code_sets.get(call.codes),
                    route_stop_codes
                        .get(&call.route_stop_id)
                        .copied()
                        .unwrap_or_default(),
                    merged.code_sets.get(stop.codes),
                ];
                let flag = |list: &[Box<str>]| levels.iter().any(|level| has(level, list));
                let service = if flag(&codes.phone_agency) || !trip_wide.is_empty() {
                    Boarding::PhoneAgency
                } else if flag(&codes.ask_driver) {
                    Boarding::AskDriver
                } else {
                    Boarding::Regular
                };
                let exclusive = levels
                    .iter()
                    .find(|level| has(level, &codes.no_pickup) || has(level, &codes.no_dropoff))
                    .copied()
                    .unwrap_or_default();
                let border = flag(&codes.both_directions_none);
                let no_pickup = border || has(exclusive, &codes.no_pickup);
                let no_dropoff = border || has(exclusive, &codes.no_dropoff);
                let number = call
                    .stop_post_number
                    .map(|sym| merged.texts.get(sym).trim())
                    .filter(|text| !text.is_empty());
                let point = match call.stop_post_id {
                    Some(post) if stop.posts.iter().any(|p| p.post_id == post) => {
                        Point::Authored(post)
                    }
                    Some(post) => {
                        diagnostics.push(
                            Diagnostic::new(
                                "jdf_unknown_post",
                                Severity::Warning,
                                trip_subject(),
                                format!(
                                    "post {post} of stop {} is not in Oznacniky: boards at the \
                                     stop",
                                    stop.number
                                ),
                            )
                            .rule(super::places::R421),
                        );
                        Point::Unspecified
                    }
                    None => number.map_or(Point::Unspecified, |n| Point::Textual(n.into())),
                };
                calls.push(PlannedCall {
                    stop: call.stop,
                    authored_number: match point {
                        Point::Authored(_) => number.map(Box::from),
                        _ => None,
                    },
                    point,
                    arrival,
                    departure,
                    sequence,
                    pickup: if no_pickup { Boarding::None } else { service },
                    dropoff: if no_dropoff { Boarding::None } else { service },
                    zones: zones.get(&call.route_stop_id).copied().flatten(),
                    kilometre: call.km.map(|km| km.value()),
                });
            }
            if calls.len() < 2 {
                diagnostics.push(
                    Diagnostic::new(
                        "jdf_short_trip",
                        Severity::Warning,
                        trip_subject(),
                        format!("{} published calls: trip dropped", calls.len()),
                    )
                    .rule(R473),
                );
                continue;
            }
            planned.push(PlannedTrip {
                trip: index,
                calls,
                ordered,
            });
        }
        trips.push(planned);
    }
    Plans { trips }
}

/// R433–R438: the trips of every version with a route.
pub(crate) fn build(
    merged: &Merged,
    plans: &Plans,
    routes: &[Option<RouteId>],
    points: &Points,
    policy: &Policy,
    timetable: &mut Timetable,
) -> Published {
    let codes = &policy.codes;
    let mut versions = Vec::with_capacity(merged.versions.len());
    for (v, version) in merged.versions.iter().enumerate() {
        let mut published = Vec::new();
        let Some(route) = routes[v] else {
            versions.push(published);
            continue;
        };
        let key = &version.key;
        for planned in &plans.trips[v] {
            let trip = &version.trips[planned.trip];
            let trip_codes: &[Box<str>] = merged.code_sets.get(trip.codes);
            let id = identity::jdf::trip(
                &key.licence,
                key.valid_from,
                key.detour,
                key.hash.as_deref(),
                key.part,
                trip.id,
            );
            let calls: Vec<Call> = planned
                .calls
                .iter()
                .map(|call| Call {
                    place: points.places[&call.stop],
                    boarding_point: Some(points.points[&(call.stop, call.point.clone())]),
                    track: None,
                    route_stop: None,
                    arrival: OptTime::some(call.arrival),
                    departure: OptTime::some(call.departure),
                    sequence: call.sequence,
                    stop_headsign: None,
                    line: None,
                    zones: call.zones,
                    kilometre: call.kilometre,
                    pickup: call.pickup,
                    dropoff: call.dropoff,
                    passenger: true,
                    timepoint: true,
                })
                .collect();
            let last_place = calls.last().map(|c| c.place);
            let headsign = last_place
                .and_then(|place| timetable.places.get(place))
                .map(|place| place.name.clone());
            let headsign = timetable.strings.intern_opt(headsign.as_deref());
            let short_name = timetable
                .strings
                .intern_opt(Some(&format!("{} {}", version.route.id, trip.id)));
            let service = timetable.services.intern(trip.days);
            let tristate = |yes: bool| if yes { Tristate::Yes } else { Tristate::No };
            let handle = timetable.trips.push(Trip {
                id: id.clone(),
                route,
                service,
                direction: Some(if trip.id % 2 == 0 { 0 } else { 1 }),
                headsign,
                short_name,
                block: None,
                run: None,
                wheelchair_accessible: tristate(has(trip_codes, &codes.wheelchair_trip)),
                bikes_allowed: tristate(has(trip_codes, &codes.bikes_trip)),
                shape: None,
                calls,
            });
            published.push(PublishedTrip {
                trip: planned.trip,
                handle,
                id,
                ordered: planned.ordered.clone(),
            });
        }
        versions.push(published);
    }
    Published { versions }
}
