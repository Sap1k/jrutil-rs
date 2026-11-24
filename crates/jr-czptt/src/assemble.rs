//! `Paths` to the `Timetable`: places and boarding points (R052–R055), agencies and routes
//! (R056–R061, R080), trips and calls (R057, R062–R069), keys (R118), and the notes,
//! zones and transfers of each accepted path.

mod direction;

pub(crate) use direction::R121;
pub(crate) use routes::R122;
mod points;
mod routes;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use jr_formats::kadr::Catalog;
use jr_model::identity::czptt;
use jr_model::timetable::{
    BindingMethod, Boarding, BoardingPointKind, Call as ModelCall, Entity, Namespace, Run,
    SourceKey, Trip, Zone,
};
use jr_model::{
    AgencyId, BoardingPointId, Date, Diagnostic, Diagnostics, Feed, OptTime, PlaceId, PublicId,
    RouteId, Rule, ServiceTime, Severity, Timetable, TripId,
};
use serde::Serialize;

use crate::path::{Call, Part, Path, RejectionReason};
use crate::policy::Policy;
use crate::{Paths, PointNames, ids_days, notes, transfers};

pub(crate) use points::{R052, R053, R054, R055};
pub(crate) use routes::{R056, R058, R059, R060, R061, R080, valid_at};

/// The operational train number names the train; the TR core is a fallback that need not be a
/// number (B23).
pub(crate) const R057: Rule = Rule {
    id: "CZPTT-R057",
    summary: "A part's train number is its first operational number, else the TR core (diagnosed)",
};

/// Riders know a train by category and number ("Os 3456").
pub(crate) const R062: Rule = Rule {
    id: "CZPTT-R062",
    summary: "A trip's short name is its category and train number",
};

/// Every part shows where the train ends, not where the part ends.
pub(crate) const R063: Rule = Rule {
    id: "CZPTT-R063",
    summary: "Every part's headsign is the path's last passenger call",
};

/// Rail and replacement bus are different vehicles.
pub(crate) const R064: Rule = Rule {
    id: "CZPTT-R064",
    summary: "A path's parts form one block, one per mode block when it alternates",
};

/// Embark-only and disembark-only forbid one side; a request stop is on request.
pub(crate) const R067: Rule = Rule {
    id: "CZPTT-R067",
    summary: "Pickup and drop-off follow the activities; railway points have neither",
};

/// An inferred or missing time is not exact.
pub(crate) const R068: Rule = Rule {
    id: "CZPTT-R068",
    summary: "A passenger call is a timepoint when timed by the source; a railway point when timed",
};

/// The parts of one path share the source's numbering.
pub(crate) const R069: Rule = Rule {
    id: "CZPTT-R069",
    summary: "A call's sequence is its source sequence",
};

/// Realtime and other feeds find trains and points by national identifiers.
pub(crate) const R118: Rule = Rule {
    id: "CZPTT-R118",
    summary: "Trips carry PA, TR and numeric train number keys; Czech five-digit points sr70 keys",
};

/// What a build produced, for the build report.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Summary {
    /// Paths with at least one trip.
    pub paths_accepted: usize,
    /// Rejected paths by reason.
    pub paths_rejected: BTreeMap<&'static str, usize>,
    /// Accepted paths all of whose parts were dropped (R050).
    pub paths_without_parts: usize,
    pub paths_cancelled: usize,
    pub trips: usize,
    pub places: usize,
    pub notes: usize,
}

fn reason_name(reason: RejectionReason) -> &'static str {
    match reason {
        RejectionReason::NoPassengerCall => "no_passenger_call",
        RejectionReason::DeparturePrecedesArrival => "departure_precedes_arrival",
        RejectionReason::EventPrecedesPreviousEvent => "event_precedes_previous_event",
        RejectionReason::TooFewPassengerCalls => "too_few_passenger_calls",
    }
}

struct Builder<'a> {
    timetable: Timetable,
    catalog: &'a Catalog,
    point_names: &'a PointNames,
    policy: &'a Policy,
    places: HashMap<(Box<str>, Box<str>), (PlaceId, PublicId)>,
    /// The first source name of each place, for R053.
    source_names: HashMap<PlaceId, Option<Box<str>>>,
    name_conflicts: BTreeSet<PlaceId>,
    points: HashMap<PublicId, BoardingPointId>,
    bus_collisions: BTreeSet<BoardingPointId>,
    agencies: BTreeMap<Box<str>, AgencyId>,
    routes: HashMap<PublicId, RouteId>,
    /// Points with a passenger call of a part (R052).
    stop_places: BTreeSet<(Box<str>, Box<str>)>,
    /// R120.
    ids_days: ids_days::IdsDays,
}

pub(crate) fn assemble(
    paths: &Paths,
    catalog: &Catalog,
    point_names: &PointNames,
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> (Timetable, Summary, ids_days::IdsDays) {
    let mut timetable = Timetable::new(Feed::Czptt, paths.horizon);
    timetable.source_version = paths.source_version.clone();
    let mut builder = Builder {
        timetable,
        catalog,
        point_names,
        policy,
        places: HashMap::new(),
        source_names: HashMap::new(),
        name_conflicts: BTreeSet::new(),
        points: HashMap::new(),
        bus_collisions: BTreeSet::new(),
        agencies: BTreeMap::new(),
        routes: HashMap::new(),
        stop_places: BTreeSet::new(),
        ids_days: ids_days::IdsDays::default(),
    };
    let mut summary = Summary {
        paths_cancelled: paths.cancelled.len(),
        ..Summary::default()
    };
    let accepted: Vec<&Path> = paths
        .paths
        .iter()
        .filter(|path| {
            if let Some(rejection) = &path.rejection {
                *summary
                    .paths_rejected
                    .entry(reason_name(rejection.reason))
                    .or_default() += 1;
                false
            } else if path.parts.is_empty() {
                summary.paths_without_parts += 1;
                false
            } else {
                true
            }
        })
        .collect();
    summary.paths_accepted = accepted.len();

    // R080: one agency per operator of a part, in code order.
    let operators: BTreeSet<&str> = accepted
        .iter()
        .flat_map(|path| path.parts.iter().map(|part| &*part.key.responsible_ru))
        .collect();
    for code in operators {
        let agency = routes::agency(code, catalog, policy, diagnostics);
        let id = builder.timetable.agencies.push(agency);
        builder.agencies.insert(code.into(), id);
    }
    for path in &accepted {
        for part in &path.parts {
            for call in part.passenger_calls() {
                builder
                    .stop_places
                    .insert((call.country.clone(), call.code.clone()));
            }
        }
    }
    for path in accepted {
        builder.path(path, diagnostics);
    }
    builder.keys();
    direction::assign(&mut builder.timetable, diagnostics);
    for place in &builder.name_conflicts {
        diagnostics.push(
            Diagnostic::new(
                "czptt_place_names_differ",
                Severity::Info,
                builder.timetable.places[*place].id.as_str(),
                "the source names this point differently in different paths; the first is kept",
            )
            .rule(R053),
        );
    }
    summary.trips = builder.timetable.trips.len();
    summary.places = builder.timetable.places.len();
    summary.notes = builder.timetable.notes.len();
    (builder.timetable, summary, builder.ids_days)
}

/// A call's display name (R053, R063): the catalogue's, else the source's own.
fn display_name<'a>(names: &'a PointNames, call: &'a Call) -> Option<&'a str> {
    names
        .get(&(call.country.clone(), call.code.clone()))
        .map(|name| &**name)
        .or(call.source_name.as_deref())
}

impl Builder<'_> {
    fn path(&mut self, path: &Path, diagnostics: &mut Diagnostics) {
        let pa = &*path.pa;
        let policy = self.policy;
        let notes = notes::PathNotes::parse(
            path,
            self.timetable.horizon,
            self.catalog,
            policy,
            diagnostics,
        );
        let memberships = notes::memberships(
            path,
            self.catalog,
            self.timetable.horizon,
            policy,
            diagnostics,
        );
        let zones = notes::zones(path, &memberships, |_| true);
        let service = self.timetable.services.intern(path.dates);
        let headsign =
            path.calls.iter().rfind(|call| call.passenger).map(|call| {
                match display_name(self.point_names, call).filter(|name| !name.trim().is_empty()) {
                    Some(name) => name.to_owned(),
                    None => format!("{} {}", call.country, call.code),
                }
            });
        let headsign = self.timetable.strings.intern_opt(headsign.as_deref());
        let alternates = path
            .parts
            .windows(2)
            .any(|pair| pair[0].key.replacement_bus != pair[1].key.replacement_bus);
        let run_key = self.timetable.strings.intern(pa);
        let mut core_diagnosed = false;
        let mut trips: Vec<(TripId, &Part)> = Vec::with_capacity(path.parts.len());
        for part in &path.parts {
            let number = match part
                .passenger_calls()
                .find_map(|call| call.train_number.as_deref())
            {
                Some(number) => Some(number),
                None => {
                    if !core_diagnosed && path.tr_core.is_some() {
                        core_diagnosed = true;
                        diagnostics.push(
                            Diagnostic::new(
                                "czptt_train_number_from_tr_core",
                                Severity::Warning,
                                pa,
                                format!(
                                    "no operational train number; the TR core {:?} is used",
                                    path.tr_core.as_deref().unwrap_or_default()
                                ),
                            )
                            .rule(R057),
                        );
                    }
                    path.tr_core.as_deref()
                }
            };
            if let Some((first, last)) = self.running_window(&path.dates) {
                routes::check_line_version(
                    pa,
                    part.key.line.as_deref(),
                    first,
                    last,
                    self.catalog,
                    diagnostics,
                );
            }
            let agency = self.agencies[&part.key.responsible_ru];
            let route = routes::route(
                &part.key,
                number,
                path.first_date,
                agency,
                self.catalog,
                policy,
            );
            let route = match self.routes.get(&route.id) {
                Some(&existing) => {
                    routes::report_disagreement(
                        &self.timetable.routes[existing],
                        &route,
                        diagnostics,
                    );
                    existing
                }
                None => {
                    let id = route.id.clone();
                    let handle = self.timetable.routes.push(route);
                    self.routes.insert(id, handle);
                    handle
                }
            };
            let short_name = match number {
                Some(number) => format!("{} {number}", part.key.category),
                None => part.key.category.to_string(),
            };
            let (wheelchair, bikes) = notes.flags(pa, part, policy, diagnostics);
            let block = czptt::block(pa, alternates.then_some(part.mode_block));
            let calls = part
                .calls
                .iter()
                .map(|call| self.call(call, part, &zones, diagnostics))
                .collect();
            let trip = Trip {
                id: czptt::trip(pa, part.number),
                route,
                service,
                direction: None,
                headsign,
                short_name: Some(self.timetable.strings.intern(&short_name)),
                block: Some(self.timetable.strings.intern(block.as_str())),
                run: Some(Run {
                    key: run_key,
                    part: part.number,
                }),
                wheelchair_accessible: wheelchair,
                bikes_allowed: bikes,
                shape: None,
                calls,
            };
            let handle = self.timetable.trips.push(trip);
            self.trip_keys(path, handle, number);
            trips.push((handle, part));
        }
        self.ids_days(path, &memberships, &trips);
        notes.publish(path, &trips, &mut self.timetable, policy);
        notes::request_stops(path, &trips, &mut self.timetable, policy);
        transfers::in_path(pa, &trips, &mut self.timetable, diagnostics);
    }

    /// R119, R120: the days on which some memberships do not hold, per set of those.
    fn ids_days(
        &mut self,
        path: &Path,
        memberships: &[notes::Membership<'_>],
        trips: &[(TripId, &Part)],
    ) {
        if memberships.iter().all(|m| m.days.is_none()) {
            return;
        }
        let mut by_off: BTreeMap<Vec<usize>, jr_model::DateSet> = BTreeMap::new();
        for day in path.dates.iter() {
            let off: Vec<usize> = memberships
                .iter()
                .enumerate()
                .filter(|(_, m)| m.days.is_some_and(|days| !days.contains(day)))
                .map(|(index, _)| index)
                .collect();
            by_off.entry(off).or_default().insert(day);
        }
        let full = by_off.remove(&Vec::new()).unwrap_or_default();
        if by_off.is_empty() {
            return;
        }
        let all = notes::zones(path, memberships, |_| true);
        for &(trip, part) in trips {
            let id = self.timetable.trips[trip].id.clone();
            let variants = by_off
                .iter()
                .map(|(off, days)| {
                    let holding = notes::zones(path, memberships, |index| !off.contains(&index));
                    let zones = part
                        .calls
                        .iter()
                        .filter(|call| all.get(&call.index) != holding.get(&call.index))
                        .map(|call| {
                            let zones: Box<[Zone]> =
                                holding.get(&call.index).cloned().unwrap_or_default().into();
                            (call.sequence() as i32, zones)
                        })
                        .collect();
                    let names: Vec<&str> =
                        off.iter().map(|&index| memberships[index].value).collect();
                    ids_days::Variant {
                        id: czptt::ids_variant(&id, &names),
                        days: *days,
                        zones,
                    }
                })
                .collect();
            self.ids_days.trips.push(ids_days::TripDays {
                trip,
                full,
                variants,
            });
        }
    }

    fn running_window(&self, dates: &jr_model::DateSet) -> Option<(Date, Date)> {
        let horizon = self.timetable.horizon;
        Some((
            horizon.date_at(dates.first()?),
            horizon.date_at(dates.last()?),
        ))
    }

    /// R118 for one trip.
    fn trip_keys(&mut self, path: &Path, trip: TripId, number: Option<&str>) {
        let Some((valid_from, valid_to)) = self.running_window(&path.dates) else {
            return;
        };
        let numeric = number.filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        let keys = [
            (Namespace::CZPTT_PA, Some(&*path.pa)),
            (Namespace::CZPTT_TR, path.tr.as_deref()),
            (Namespace::CZPTT_TRAIN_NUMBER, numeric),
        ];
        for (namespace, identifier) in keys {
            let Some(identifier) = identifier else {
                continue;
            };
            self.timetable.keys.push(SourceKey {
                namespace,
                identifier: identifier.into(),
                entity: Entity::Trip(trip),
                valid_from,
                valid_to,
                binding: BindingMethod::Identity,
            });
        }
    }

    /// R118 for places and tracks, valid over every trip's days.
    fn keys(&mut self) {
        let window = self
            .timetable
            .trips
            .rows()
            .iter()
            .filter_map(|trip| self.running_window(&self.timetable.services.get(trip.service)))
            .fold(
                None,
                |window: Option<(Date, Date)>, (from, to)| match window {
                    None => Some((from, to)),
                    Some((a, b)) => Some((a.min(from), b.max(to))),
                },
            );
        let Some((valid_from, valid_to)) = window else {
            return;
        };
        let rules = &self.policy.keys;
        let mut sr70: HashMap<PlaceId, Box<str>> = HashMap::new();
        let mut keys = Vec::new();
        for (handle, place) in self.timetable.places.iter() {
            let code = place.source_code.as_deref().unwrap_or_default();
            let eligible = place.country.as_deref() == Some(&*rules.sr70_country)
                && code.len() == rules.sr70_code_length
                && code.bytes().all(|b| b.is_ascii_digit());
            if !eligible {
                continue;
            }
            sr70.insert(handle, code.into());
            keys.push(SourceKey {
                namespace: Namespace::SR70,
                identifier: code.into(),
                entity: Entity::Place(handle),
                valid_from,
                valid_to,
                binding: BindingMethod::Identity,
            });
        }
        for (handle, point) in self.timetable.boarding_points.iter() {
            let (Some(code), Some(track)) = (sr70.get(&point.place), point.source_code.as_deref())
            else {
                continue;
            };
            if point.kind != BoardingPointKind::Track {
                continue;
            }
            keys.push(SourceKey {
                namespace: Namespace::SR70_TRACK,
                identifier: format!("{code}:{track}").into_boxed_str(),
                entity: Entity::BoardingPoint(handle),
                valid_from,
                valid_to,
                binding: BindingMethod::Identity,
            });
        }
        self.timetable.keys.extend(keys);
    }

    /// R067–R069 and the boarding point of one call.
    fn call(
        &mut self,
        call: &Call,
        part: &Part,
        zones: &BTreeMap<u32, Vec<Zone>>,
        diagnostics: &mut Diagnostics,
    ) -> ModelCall {
        let (place, station) = self.place(call);
        let track = self.track(place, &station, call, diagnostics);
        let boarding_point = match (call.passenger, part.key.replacement_bus) {
            (false, _) => None,
            (true, true) => Some(self.bus_point(place, &station, diagnostics)),
            (true, false) => Some(track.unwrap_or_else(|| {
                let id = czptt::unspecified_point(&station);
                self.point(Self::new_point(place, id, BoardingPointKind::Unspecified))
            })),
        };
        let activities = &self.policy.activities;
        let has = |code: &str| call.activities.iter().any(|a| &**a == code);
        let (pickup, dropoff) = if call.passenger {
            let open = if has(&activities.request_stop) {
                Boarding::AskDriver
            } else {
                Boarding::Regular
            };
            (
                if has(&activities.disembark_only) {
                    Boarding::None
                } else {
                    open
                },
                if has(&activities.embark_only) {
                    Boarding::None
                } else {
                    open
                },
            )
        } else {
            (Boarding::None, Boarding::None)
        };
        let timed = call.arrival.is_some() || call.departure.is_some();
        let time = |seconds: Option<i32>| OptTime::from(seconds.map(ServiceTime::from_seconds));
        let zone_list = zones
            .get(&call.index)
            .map(Vec::as_slice)
            .unwrap_or_default();
        ModelCall {
            place,
            boarding_point,
            track,
            route_stop: None,
            arrival: time(call.arrival),
            departure: time(call.departure),
            sequence: call.sequence() as i32,
            stop_headsign: None,
            line: self.timetable.strings.intern_opt(call.line.as_deref()),
            zones: self.timetable.zone_sets.intern(zone_list),
            kilometre: None,
            pickup,
            dropoff,
            passenger: call.passenger,
            timepoint: if call.passenger {
                timed && !call.inferred
            } else {
                timed
            },
        }
    }
}
