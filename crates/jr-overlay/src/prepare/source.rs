//! A source read against its profile and the base horizon (OVL-R013, R028, R030): eligible
//! routes and trips with their calendars and calls, and the hooks' results.

use std::collections::HashMap;
use std::ops::Range;

use jr_formats::geodata::stop_overrides;
use jr_formats::gtfs::{Feed, IdRef};
use jr_model::timetable::Mode;
use jr_model::{
    Date, DateSet, Diagnostic, Diagnostics, PlaceId, PublicId, RouteId, Rule, Severity,
};

use super::base::BaseView;
use super::calendar::{self, DAY, OVL_R056};
use super::columns::{StopColumns, TripColumns};
use super::hooks::{self, CrosswalkRow};
use crate::profile::{RouteTier, SourceProfile};

/// A source route enters unless its `route_type` is one the profile excludes or its mode is one
/// the profile excludes; trips of a route that does not enter do not enter either, nor do trips
/// whose route the feed lacks (diagnosed). Every other trip is eligible whatever its agency or
/// direction (R028).
///
/// Heavy rail stays national: "No regional static rail data is used" (BASE_PLAN 12). As
/// profile data, a foreign rail overlay can let rail in.
pub const OVL_R013: Rule = Rule {
    id: "OVL-R013",
    summary: "routes of excluded route types or modes (heavy rail) never enter",
};

/// The eligible trips are those of entering routes (R013), in feed order.
pub const OVL_R028: Rule = Rule {
    id: "OVL-R028",
    summary: "every trip of an entering route is eligible",
};

/// Only stops that an eligible trip running on some day of the horizon calls are grouped and
/// matched. A call naming a stop the feed lacks is dropped (diagnosed).
pub const OVL_R030: Rule = Rule {
    id: "OVL-R030",
    summary: "only stops called by an eligible trip running in the horizon are matched",
};

pub const RULES: &[Rule] = &[OVL_R013, OVL_R028, OVL_R030];

/// Where a source call or stop group is placed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PlaceRef {
    /// A base stop place.
    Base(PlaceId),
    /// A source-native place: an index into [`PreparedSource::native_places`] (R047).
    Native(u32),
}

/// How a stop group was placed (R040–R047); the names are the review reports'.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MatchMethod {
    ReviewedOverride,
    NameGeoUnique,
    CoordinateIdentityUnique,
    TripContextUnique,
    /// Placed from context at an approximate base place (R045).
    TripContextGtfsAuthoritative,
    SourceNative,
}

impl MatchMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReviewedOverride => "reviewed_override",
            Self::NameGeoUnique => "name_geo_unique",
            Self::CoordinateIdentityUnique => "coordinate_identity_unique",
            Self::TripContextUnique => "trip_context_unique",
            Self::TripContextGtfsAuthoritative => "trip_context_gtfs_authoritative",
            Self::SourceNative => "source_native",
        }
    }

    /// R040–R043: the tiers that place a group before trip context.
    pub const fn is_initial(self) -> bool {
        matches!(
            self,
            Self::ReviewedOverride | Self::NameGeoUnique | Self::CoordinateIdentityUnique
        )
    }
}

/// A placed stop group.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StopMatch {
    pub target: PlaceRef,
    pub method: MatchMethod,
    /// The smallest distance between the group's and the place's points, where measured.
    pub distance: Option<f64>,
    /// R046: the source names and places this approximate base place.
    pub corrected: bool,
}

/// A source stop group (R039).
#[derive(Clone, Debug)]
pub struct StopGroup {
    /// The group's ID as the profile and the reviewed overrides see it: a parent station's stop
    /// ID, or `group:<key>[:<name words>]` for a flat group.
    pub local_id: Box<str>,
    /// Grouped by a parent station.
    pub parent: bool,
    /// Member stops (indexes into the feed's stops), in feed order.
    pub members: Vec<u32>,
    /// The representative's `stop_name`: the parent station's, else the first member's.
    pub name: Box<str>,
    /// The mean of the members' coordinates.
    pub position: Option<Coord>,
    /// The members' distinct coordinates, else the mean.
    pub points: Vec<Coord>,
    /// R101 (zones hook): the members' distinct zone IDs, sorted.
    pub zones: Vec<Box<str>>,
    /// Where it is placed, if it is.
    pub matched: Option<StopMatch>,
}

use jr_model::Coord;

impl StopGroup {
    /// The group's key in the stop registry (`overlay_places.csv`, section 4.2), and its ID in
    /// the review reports: `<source>:<parent>` or `group:<source>:<local ID>`.
    pub fn registry_key(&self, source_id: &str) -> String {
        if self.parent {
            format!("{source_id}:{}", self.local_id)
        } else {
            format!("group:{source_id}:{}", self.local_id)
        }
    }
}

/// An eligible source route.
#[derive(Clone, Debug)]
pub struct SourceRoute {
    /// Index into the feed's routes.
    pub feed_index: u32,
    /// R038.
    pub mode: Option<Mode>,
    /// R049, R050: the primary candidate base routes and the tier that found them.
    pub primary: Vec<RouteId>,
    pub primary_tier: RouteTier,
    /// R051: the relaxed structural candidates (primary ones included).
    pub structural: Vec<RouteId>,
}

/// An eligible source trip.
#[derive(Clone, Debug)]
pub struct SourceTrip {
    /// Index into the feed's trips.
    pub feed_index: u32,
    /// Index into [`PreparedSource::routes`].
    pub route: u32,
    /// Its days in the base horizon, after the fold (R029, R056).
    pub dates: DateSet,
    /// R056: moved to the next day.
    pub folded: bool,
    /// R015.
    pub revision: Option<Date>,
    /// R018: the national line it asserts.
    pub line_claim: Option<Box<str>>,
    /// R049: the base routes of its asserted line compatible with its mode, sorted by ID.
    pub direct_routes: Vec<RouteId>,
    /// R019: index into [`PreparedSource::crosswalk`].
    pub operational: Option<u32>,
    /// Its calls: a range of [`PreparedSource::calls`].
    pub calls: Range<u32>,
}

/// A call of an eligible trip, in `stop_sequence` order.
#[derive(Clone, Copy, Debug)]
pub struct SourceCall {
    /// Index into the feed's stops.
    pub stop: u32,
    /// Index into the feed's `stop_times` rows (for headsign, pickup and the other columns).
    pub row: u32,
    pub sequence: Option<i32>,
    /// Seconds from the trip's (folded) service day's midnight.
    pub arrival: Option<i32>,
    pub departure: Option<i32>,
    /// R054: where its stop's group is placed (the final placement, native places included).
    pub place: Option<PlaceRef>,
}

/// A stop place only the source has (R047).
#[derive(Clone, Debug)]
pub struct NativePlace {
    /// Index into [`PreparedSource::groups`].
    pub group: u32,
    /// `identity::overlay::stop_place` of the group's registry key.
    pub id: PublicId,
    /// The registry's pinned place ID, where the registry pins the group.
    pub pinned: Option<Box<str>>,
}

impl NativePlace {
    /// The ID the place is published under: the pinned one, else the built one.
    pub fn published_id(&self) -> &str {
        self.pinned.as_deref().unwrap_or(self.id.as_str())
    }
}

/// One accepted trip-context placement (R044, R045), as `stop_context_inference` reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct ContextInference {
    /// Index into [`PreparedSource::groups`].
    pub group: u32,
    /// Index into [`PreparedSource::trips`].
    pub trip: u32,
    /// 1-based call ordinal of the group in the trip.
    pub ordinal: u32,
    pub target: PlaceId,
    pub method: MatchMethod,
}

/// One source, prepared.
#[derive(Debug)]
pub struct PreparedSource<'a> {
    pub profile: &'a SourceProfile,
    pub feed: Feed,
    pub overrides: Vec<stop_overrides::Row>,
    pub routes: Vec<SourceRoute>,
    /// By feed route index, the index into `routes`.
    pub route_of_feed: Vec<Option<u32>>,
    pub trips: Vec<SourceTrip>,
    /// By feed trip index, the index into `trips`.
    pub trip_of_feed: Vec<Option<u32>>,
    pub calls: Vec<SourceCall>,
    pub groups: Vec<StopGroup>,
    /// By feed stop index, its group (scheduled stops only).
    pub group_of_stop: Vec<Option<u32>>,
    /// R017, by feed stop index.
    pub post_keys: Vec<Option<Box<str>>>,
    pub inferences: Vec<ContextInference>,
    pub native_places: Vec<NativePlace>,
    /// R020.
    pub window: Option<(Date, Date)>,
    /// R019.
    pub crosswalk: Vec<CrosswalkRow>,
    /// R056: how many trips were folded and how many of their days left the horizon.
    pub folded_trips: usize,
    pub folded_days_lost: usize,
}

impl PreparedSource<'_> {
    pub fn source_id(&self) -> &str {
        &self.profile.source_id
    }

    pub fn trip_calls(&self, trip: &SourceTrip) -> &[SourceCall] {
        &self.calls[trip.calls.start as usize..trip.calls.end as usize]
    }
}

/// Reads one source: routes, trips, calendars, calls and the hooks (R013–R019, R028–R030,
/// R056). Stop groups and matching follow in their own modules.
pub fn read<'a>(
    profile: &'a SourceProfile,
    feed: Feed,
    overrides: Vec<stop_overrides::Row>,
    base: &BaseView<'_>,
    diagnostics: &mut Diagnostics,
) -> PreparedSource<'a> {
    let horizon = base.timetable.horizon;
    let source = &*profile.source_id;
    let trip_columns = TripColumns::new(profile);
    let stop_columns = StopColumns::new(profile);

    // R013.
    let mut routes = Vec::new();
    let mut route_of_feed = vec![None; feed.routes.len()];
    let mut route_by_id: HashMap<&str, Option<u32>> = HashMap::new();
    for (index, route) in feed.routes.iter().enumerate() {
        let mode = base.profile.mode_of_route_type(route.route_type);
        let excluded = route
            .route_type
            .is_some_and(|code| profile.routes.excluded_route_types.contains(&code))
            || mode.is_some_and(|mode| profile.routes.excluded_modes.contains(&mode));
        let slot = (!excluded).then(|| {
            routes.push(SourceRoute {
                feed_index: index as u32,
                mode,
                primary: Vec::new(),
                primary_tier: RouteTier::StructuralTripEvidence,
                structural: Vec::new(),
            });
            (routes.len() - 1) as u32
        });
        route_of_feed[index] = slot;
        route_by_id.entry(&route.route_id).or_insert(slot);
    }

    // R028, R029, R015, R018, R019.
    let services = calendar::services(&feed, horizon);
    let claims = hooks::line_claims(profile, &feed, &trip_columns, diagnostics);
    let crosswalk = hooks::crosswalk(profile, &feed, diagnostics);
    let mut operational = vec![None; feed.trips.len()];
    for (index, row) in crosswalk.iter().enumerate() {
        operational[row.trip as usize] = Some(index as u32);
    }
    let mut trips = Vec::new();
    let mut trip_of_feed = vec![None; feed.trips.len()];
    let mut calls = Vec::new();
    let mut missing_routes = 0usize;
    let mut unknown_stops = 0usize;
    for (index, trip) in feed.trips.iter().enumerate() {
        let route = match route_by_id.get(&*trip.route_id) {
            Some(Some(route)) => *route,
            Some(None) => continue,
            None => {
                missing_routes += 1;
                continue;
            }
        };
        let start = calls.len() as u32;
        for row in feed.stop_times.trip_rows(index) {
            let time = feed.stop_times.get(row);
            match time.stop {
                IdRef::Index(stop) => calls.push(SourceCall {
                    stop,
                    row: row as u32,
                    sequence: time.stop_sequence,
                    arrival: time.arrival_time,
                    departure: time.departure_time,
                    place: None,
                }),
                IdRef::Unknown(_) => unknown_stops += 1,
            }
        }
        trip_of_feed[index] = Some(trips.len() as u32);
        trips.push(SourceTrip {
            feed_index: index as u32,
            route,
            dates: services.get(&*trip.service_id).copied().unwrap_or_default(),
            folded: false,
            revision: hooks::revision(profile, &trip_columns, trip),
            line_claim: claims[index].clone(),
            direct_routes: Vec::new(),
            operational: operational[index],
            calls: start..calls.len() as u32,
        });
    }
    if missing_routes > 0 {
        diagnostics.push(
            Diagnostic::new(
                "overlay_trip_route_missing",
                Severity::Warning,
                source,
                format!("{missing_routes} trips name a route routes.txt lacks; they do not enter"),
            )
            .rule(OVL_R028),
        );
    }
    if unknown_stops > 0 {
        diagnostics.push(
            Diagnostic::new(
                "overlay_call_stop_missing",
                Severity::Warning,
                source,
                format!(
                    "{unknown_stops} calls of eligible trips name a stop stops.txt lacks; dropped"
                ),
            )
            .rule(OVL_R030),
        );
    }

    let post_keys = feed
        .stops
        .iter()
        .map(|stop| hooks::post_key(profile, &stop_columns, stop).map(Box::from))
        .collect();
    let window = calendar::feed_window(&feed);
    let mut source = PreparedSource {
        profile,
        feed,
        overrides,
        routes,
        route_of_feed,
        trips,
        trip_of_feed,
        calls,
        groups: Vec::new(),
        group_of_stop: Vec::new(),
        post_keys,
        inferences: Vec::new(),
        native_places: Vec::new(),
        window,
        crosswalk,
        folded_trips: 0,
        folded_days_lost: 0,
    };
    source.group_of_stop = vec![None; source.feed.stops.len()];
    source
}

/// R030: the stops eligible trips running in the horizon call, by feed stop index.
pub fn scheduled_stops(source: &PreparedSource<'_>) -> Vec<bool> {
    let mut scheduled = vec![false; source.feed.stops.len()];
    for trip in source.trips.iter().filter(|trip| !trip.dates.is_empty()) {
        for call in source.trip_calls(trip) {
            scheduled[call.stop as usize] = true;
        }
    }
    scheduled
}

/// R056, applied after stop matching as in the reference (the scheduled stops are those of the
/// unfolded days).
pub fn fold_after_midnight(
    source: &mut PreparedSource<'_>,
    horizon_days: usize,
    diagnostics: &mut Diagnostics,
) {
    let mut folded = 0;
    let mut lost = 0;
    for trip in &mut source.trips {
        let range = trip.calls.start as usize..trip.calls.end as usize;
        let first = source.calls[range.clone()]
            .first()
            .and_then(|call| call.departure);
        if first.is_none_or(|departure| departure < DAY) {
            continue;
        }
        let (dates, dropped) = trip.dates.shifted(1, horizon_days);
        trip.dates = dates;
        trip.folded = true;
        lost += dropped;
        folded += 1;
        for call in &mut source.calls[range] {
            call.arrival = call.arrival.map(|time| time - DAY);
            call.departure = call.departure.map(|time| time - DAY);
        }
    }
    source.folded_trips = folded;
    source.folded_days_lost = lost;
    if folded > 0 {
        diagnostics.push(
            Diagnostic::new(
                "overlay_after_midnight_folded",
                Severity::Info,
                source.profile.source_id.to_string(),
                format!("{folded} trips departing after midnight moved to the next day; {lost} of their days left the horizon"),
            )
            .rule(OVL_R056),
        );
    }
}
