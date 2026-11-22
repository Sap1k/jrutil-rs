//! The scan (serving 8.1) and the companion's input (8.2).

use std::collections::BTreeMap;

use jr_model::timetable::{Call, Mode, Position, Precision};
use jr_model::{Coord, Timetable, TripId};
use sha2::{Digest, Sha256};

use super::{NotRouted, Summary};

/// The patterns to route and the trips that run them (serving 8.1).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    /// By pattern ID (byte order).
    pub(super) patterns: Vec<Pattern>,
    /// Published unshaped trips of a supported mode, in handle order.
    pub(super) trips: Vec<PlannedTrip>,
    published_trips: usize,
    already_shaped: usize,
    unsupported_mode: BTreeMap<String, usize>,
    estimated_calls_skipped: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Pattern {
    /// The companion's key: 20 hex digits (serving 6.3), not a public ID.
    pub id: String,
    pub class: &'static str,
    pub points: Vec<MatchPoint>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct MatchPoint {
    /// Rounded to seven decimals.
    pub coord: Coord,
    pub waypoint: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PlannedTrip {
    pub trip: TripId,
    /// Index into `Plan::patterns`.
    pub pattern: usize,
    /// The positions in the trip's call list of its match points, in order.
    pub calls: Vec<u32>,
}

impl Plan {
    pub fn of(timetable: &Timetable) -> Plan {
        let mut plan = Plan::default();
        let mut patterns: BTreeMap<String, Pattern> = BTreeMap::new();
        let mut trips = Vec::new();
        for (handle, trip) in timetable.trips.iter() {
            if timetable.services.get(trip.service).is_empty() {
                continue;
            }
            plan.published_trips += 1;
            if trip.shape.is_some() {
                plan.already_shaped += 1;
                continue;
            }
            let mode = timetable.routes[trip.route].mode;
            let Some(class) = shape_class(mode) else {
                *plan.unsupported_mode.entry(mode_name(mode)).or_default() += 1;
                continue;
            };
            let mut points = Vec::new();
            let mut calls = Vec::new();
            for (position, call) in trip.calls.iter().enumerate() {
                match match_point(timetable, call) {
                    Some(coord) => {
                        points.push(MatchPoint {
                            coord: Coord::new(round7(coord.lon), round7(coord.lat)),
                            waypoint: !call.passenger,
                        });
                        calls.push(u32::try_from(position).expect("fewer than 2^32 calls"));
                    }
                    None if call.passenger => plan.estimated_calls_skipped += 1,
                    None => {}
                }
            }
            let id = pattern_id(class, &points);
            patterns.entry(id.clone()).or_insert_with(|| Pattern {
                id: id.clone(),
                class,
                points,
            });
            trips.push((handle, id, calls));
        }
        plan.patterns = patterns.into_values().collect();
        plan.trips = trips
            .into_iter()
            .map(|(trip, id, calls)| PlannedTrip {
                trip,
                pattern: plan.position(&id).expect("every trip's pattern is planned"),
                calls,
            })
            .collect();
        plan
    }

    /// `patterns.csv` exactly as the companion reads it (serving 8.2): LF, header
    /// `pattern_id,class,seq,lat,lon,waypoint`, patterns by ID, `seq` from 0, coordinates with
    /// seven decimals. A pattern without match points has no rows. Empty plan: header only.
    pub fn patterns_csv(&self) -> String {
        let mut csv = String::from("pattern_id,class,seq,lat,lon,waypoint\n");
        for pattern in &self.patterns {
            for (seq, point) in pattern.points.iter().enumerate() {
                csv.push_str(&format!(
                    "{},{},{seq},{:.7},{:.7},{}\n",
                    pattern.id,
                    pattern.class,
                    point.coord.lat,
                    point.coord.lon,
                    u8::from(point.waypoint)
                ));
            }
        }
        csv
    }

    /// No pattern: the companion is not run.
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    pub(super) fn position(&self, id: &str) -> Option<usize> {
        self.patterns
            .binary_search_by(|pattern| pattern.id.as_str().cmp(id))
            .ok()
    }

    /// The scan's counts; the mapping fills in the rest.
    pub(super) fn summary(&self) -> Summary {
        Summary {
            trips: self.published_trips,
            already_shaped: self.already_shaped,
            routed_trips: self.trips.len(),
            patterns: self.patterns.len(),
            not_routed: NotRouted {
                unsupported_mode: self.unsupported_mode.clone(),
            },
            estimated_calls_skipped: self.estimated_calls_skipped,
            ..Summary::default()
        }
    }

    /// Per pattern, the smallest public ID of its trips: what its diagnostics are about.
    pub(super) fn subjects<'a>(&self, timetable: &'a Timetable) -> Vec<&'a str> {
        let mut subjects: Vec<Option<&str>> = vec![None; self.patterns.len()];
        for planned in &self.trips {
            let id = timetable.trips[planned.trip].id.as_str();
            let subject = &mut subjects[planned.pattern];
            if subject.is_none_or(|current| id < current) {
                *subject = Some(id);
            }
        }
        subjects
            .into_iter()
            .map(|subject| subject.expect("every pattern has a trip"))
            .collect()
    }
}

/// SERVING-R030: the one mode → class table of this phase.
fn shape_class(mode: Mode) -> Option<&'static str> {
    match mode {
        Mode::Bus | Mode::Trolleybus => Some("bus"),
        Mode::Tram | Mode::Metro | Mode::Rail => Some("rail"),
        Mode::Water => Some("ferry"),
        Mode::Cable | Mode::Funicular => None,
    }
}

/// The mode's serialised name, as the contract publishes it.
fn mode_name(mode: Mode) -> String {
    serde_json::to_value(mode)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{mode:?}"))
}

/// SERVING-R031: the exact boarding point, else the exact place.
fn match_point(timetable: &Timetable, call: &Call) -> Option<Coord> {
    let exact = |position: Option<Position>| {
        position
            .filter(|position| position.precision == Precision::Exact)
            .map(|position| position.coord)
    };
    call.boarding_point
        .and_then(|point| exact(timetable.boarding_points[point].position))
        .or_else(|| exact(timetable.places[call.place].position))
}

/// Seven decimals, rounded as they are written, so the pattern key, `patterns.csv` and the
/// point agree.
fn round7(value: f64) -> f64 {
    format!("{value:.7}")
        .parse()
        .expect("a formatted finite number parses")
}

/// Serving 6.3: the first 20 hex digits of the SHA-256 of the class and, per point,
/// `\n<lat>,<lon>,<1|0>`.
fn pattern_id(class: &str, points: &[MatchPoint]) -> String {
    let mut key = String::from(class);
    for point in points {
        key.push_str(&format!(
            "\n{:.7},{:.7},{}",
            point.coord.lat,
            point.coord.lon,
            u8::from(point.waypoint)
        ));
    }
    Sha256::digest(key.as_bytes())[..10]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
