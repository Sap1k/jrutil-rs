//! Small base timetables and source feeds for the preparation tests.
#![allow(dead_code)]

use jr_formats::geodata::registry::OverlayPlace;
use jr_formats::geodata::{stop_overrides, synonyms};
use jr_formats::gtfs::{self, Feed, Options};
use jr_model::identity::jdf;
use jr_model::timetable::*;
use jr_model::{
    Coord, Date, DateSet, Diagnostics, Feed as FeedKind, Horizon, OptTime, PlaceId, RouteId,
    ServiceTime, Timetable,
};
use jr_overlay::prepare::{self, Prepared};
use jr_overlay::profile::{BaseProfile, SourceProfile};
use jr_overlay::{OverlayInput, SourceInput};

pub fn horizon() -> Horizon {
    Horizon::gvd(2026).unwrap()
}

/// A day of GVD 2026 by its offset; day 0 is Sunday 2025-12-14.
pub fn day(offset: usize) -> Date {
    horizon().date_at(offset)
}

pub fn days(offsets: impl IntoIterator<Item = usize>) -> DateSet {
    DateSet::from_indices(offsets)
}

/// A base timetable under construction.
pub struct Base {
    pub t: Timetable,
    agency: jr_model::AgencyId,
    trips: u64,
}

impl Base {
    pub fn new() -> Base {
        let mut t = Timetable::new(FeedKind::Jdf, horizon());
        let agency = t.agencies.push(Agency {
            id: jdf::agency("00000001", 1),
            name: "Dopravce".into(),
            url: None,
            timezone: "Europe/Prague".into(),
            language: None,
            phone: None,
            fare_url: None,
            email: None,
            registration: None,
        });
        Base {
            t,
            agency,
            trips: 0,
        }
    }

    /// A stop place `jdf:stop:<number>` at `(lat, lon)`.
    pub fn place(&mut self, number: u64, name: &str, at: Option<(f64, f64)>) -> PlaceId {
        self.place_with(number, name, at, Precision::Exact)
    }

    pub fn estimated_place(&mut self, number: u64, name: &str, at: (f64, f64)) -> PlaceId {
        self.place_with(number, name, Some(at), Precision::Estimated)
    }

    fn place_with(
        &mut self,
        number: u64,
        name: &str,
        at: Option<(f64, f64)>,
        precision: Precision,
    ) -> PlaceId {
        self.t.places.push(Place {
            id: jdf::stop(number),
            kind: PlaceKind::StopPlace,
            name: name.into(),
            source_code: None,
            description: None,
            municipality: None,
            district_name: None,
            district_code: None,
            nearby_place: None,
            country: Some("CZ".into()),
            position: at.map(|(lat, lon)| Position {
                coord: Coord::new(lon, lat),
                precision,
                source: Some(CoordSource::Catalogue),
            }),
            url: None,
            timezone: None,
            wheelchair_boarding: Tristate::Unknown,
        })
    }

    /// A route of national line `line` (a `cis:line` key) with a public number.
    pub fn route(&mut self, line: &str, number: &str, mode: Mode) -> RouteId {
        self.route_with(line, number, mode, false)
    }

    pub fn route_with(&mut self, line: &str, number: &str, mode: Mode, detour: bool) -> RouteId {
        let route = self.t.routes.push(Route {
            id: jdf::route(line, detour, None),
            agency: self.agency,
            mode,
            gtfs_route_type: 3,
            short_name: Some(number.into()),
            long_name: None,
            description: None,
            url: None,
            color: None,
            text_color: None,
            sort_order: None,
            timetable_kind: if detour {
                TimetableKind::Detour
            } else {
                TimetableKind::Regular
            },
        });
        self.t.keys.push(SourceKey {
            namespace: Namespace::CIS_LINE,
            identifier: line.into(),
            entity: Entity::Route(route),
            valid_from: horizon().start(),
            valid_to: horizon().end(),
            binding: BindingMethod::Identity,
        });
        route
    }

    /// A trip calling at `(place, "HH:MM")`, the time being both arrival and departure.
    pub fn trip(
        &mut self,
        route: RouteId,
        dates: DateSet,
        calls: &[(PlaceId, &str)],
    ) -> jr_model::TripId {
        self.trips += 1;
        let service = self.t.services.intern(dates);
        let calls = calls
            .iter()
            .enumerate()
            .map(|(index, &(place, time))| {
                let time = OptTime::some(ServiceTime::from_seconds(seconds(time)));
                Call {
                    place,
                    boarding_point: None,
                    track: None,
                    route_stop: None,
                    arrival: time,
                    departure: time,
                    sequence: index as i32 + 1,
                    stop_headsign: None,
                    line: None,
                    zones: None,
                    kilometre: None,
                    pickup: Boarding::Regular,
                    dropoff: Boarding::Regular,
                    passenger: true,
                    timepoint: true,
                }
            })
            .collect();
        self.t.trips.push(Trip {
            id: jdf::trip("100001", day(0), false, None, None, self.trips),
            route,
            service,
            direction: None,
            headsign: None,
            short_name: None,
            block: None,
            run: None,
            wheelchair_accessible: Tristate::Unknown,
            bikes_allowed: Tristate::Unknown,
            shape: None,
            calls,
        })
    }
}

impl Base {
    /// Publishes places: one trip of a route `999` calls them (in view of the overlay, R025).
    pub fn serve(&mut self, places: &[PlaceId]) {
        let route = self.route("999999", "999", Mode::Bus);
        let calls: Vec<(PlaceId, &str)> = places.iter().map(|&place| (place, "05:00")).collect();
        self.trip(route, days(0..364), &calls);
    }
}

/// `HH:MM` as seconds.
pub fn seconds(time: &str) -> i32 {
    let (hours, minutes) = time.split_once(':').unwrap();
    hours.parse::<i32>().unwrap() * 3600 + minutes.parse::<i32>().unwrap() * 60
}

/// A test source profile: the PID profile's shape with a test ID and the given changes
/// (`key = value` lines appended under their tables by the caller's TOML).
pub fn profile(text: &str) -> SourceProfile {
    SourceProfile::parse(text).unwrap_or_else(|error| panic!("{error}"))
}

/// A generic profile without hooks; `extra` is appended verbatim.
pub fn generic_profile(extra: &str) -> SourceProfile {
    profile(&format!(
        r#"
source_id = "test-gtfs"
key_namespace = "test"
maximum_snapshot_skew_days = 7
[routes]
excluded_route_types = [2]
excluded_modes = ["rail"]
tiers = ["companion_assertion", "structural_trip_evidence"]
[stops]
maximum_distance_metres = 300.0
split_flat_groups_by_name = true
coordinate_identity = {{ maximum_metres = 25.0, minimum_margin_metres = 10.0 }}
context = {{ enabled = true, minimum_mapped_calls = 1, maximum_unresolved_groups_per_trip = 1 }}
[trips]
time_resolution_seconds = 60
exact_pattern_proximity = true
keep_source_calls = false
tiers = ["full_signature", "pattern_endpoints", "pattern_first", "pattern_nearest", "pattern_edit_nearest"]
pattern_edit = {{ enabled = true, maximum_edits = 3, minimum_agreement = 0.9, require_same_endpoints = true }}
[authority]
modes = ["bus", "tram"]
source_native_modes = ["tram"]
[capabilities]
schedules = {{ minimum_tier = "pattern_nearest" }}
{extra}
"#
    ))
}

/// A feed from member texts, read as the profile asks.
pub fn feed(profile: &SourceProfile, files: &[(&str, &str)]) -> Feed {
    let request = profile.reader();
    let columns: Vec<(&str, Vec<&str>)> = request
        .extra_columns
        .iter()
        .map(|(file, columns)| (&**file, columns.iter().map(|c| &**c).collect()))
        .collect();
    let extra_columns: Vec<(&str, &[&str])> =
        columns.iter().map(|(f, c)| (*f, c.as_slice())).collect();
    let tables: Vec<&str> = request.tables.iter().map(|t| &**t).collect();
    let raw: Vec<&str> = request.raw.iter().map(|t| &**t).collect();
    let options = Options {
        extra_columns: &extra_columns,
        tables: &tables,
        raw: &raw,
        shapes: false,
    };
    gtfs::read_files(
        files.iter().map(|(name, text)| (*name, text.as_bytes())),
        &options,
    )
    .unwrap()
}

pub const AGENCY: &str =
    "agency_id,agency_name,agency_url,agency_timezone\na,A,https://a.cz,Europe/Prague\n";

/// A service running on every day of the horizon.
pub const CALENDAR: &str = "service_id,monday,tuesday,wednesday,thursday,friday,saturday,sunday,start_date,end_date\ns,1,1,1,1,1,1,1,20251214,20261212\n";

/// The shared synonym table.
pub fn synonym_lines() -> Vec<synonyms::Line> {
    synonyms::parse(include_str!("../../../../data/names/stop_synonyms.txt"))
}

/// Runs preparation on one source (every input of `OverlayInput`, spelled out).
#[allow(clippy::too_many_arguments)]
pub fn run<'a>(
    base: &'a Timetable,
    base_profile: &'a BaseProfile,
    profile: &'a SourceProfile,
    feed: Feed,
    overrides: Vec<stop_overrides::Row>,
    places: &'a [OverlayPlace],
    synonyms: &'a [synonyms::Line],
    diagnostics: &mut Diagnostics,
) -> Prepared<'a> {
    prepare::prepare(
        OverlayInput {
            base,
            base_profile,
            sources: vec![SourceInput {
                profile,
                feed,
                overrides,
            }],
            places,
            synonyms,
        },
        diagnostics,
    )
    .unwrap()
}

/// The report row values of a group by its namespaced ID: (target, method, status).
pub fn group_row(prepared: &Prepared<'_>, group: &str) -> (String, String, String) {
    let report = &prepared.reports()[0];
    let row = report
        .rows
        .iter()
        .find(|row| row[0] == group)
        .unwrap_or_else(|| panic!("no group {group} in {:#?}", report.rows));
    (row[3].clone(), row[6].clone(), row[7].clone())
}
