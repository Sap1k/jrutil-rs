//! Trip-set authority (OVL-R057–R064): on which national lines and dates a source runs the
//! complete trip set, and the source trips projected there. Lines and blocks are decided across
//! all sources together, as one line can be asserted by more than one source.

mod silence;

use std::collections::BTreeMap;

use jr_model::identity::overlay;
use jr_model::{DateSet, Diagnostic, Diagnostics, PublicId, RouteId, Rule, Severity};

use crate::prepare::{Prepared, PreparedSource, SourceTrip};

/// A source trip's authority identity: the line it asserts (R018) with that line's direct base
/// routes; else, when its route's primary candidates are structural and exactly one of them
/// (regular routes preferred, R058) has a national line, that line and route; else, in a
/// source-native mode, its own source route standing for a line; else none. (A single
/// structural route without a national line gives none, as in the reference.)
///
/// "Decide on which CIS lines and dates the source owns the trip set."
pub const OVL_R057: Rule = Rule {
    id: "OVL-R057",
    summary: "a trip's authority line: its asserted line, a unique structural line, or its route",
};

/// Of a line's routes, the regular ones are preferred over detour (výluka) routes, unless there
/// are only detour routes (when the base profile says so).
///
/// A CIS line can have a regular and a detour base route; source trips belong to the regular
/// one, and trip matching still considers both.
pub const OVL_R058: Rule = Rule {
    id: "OVL-R058",
    summary: "regular routes of a line are preferred over its detour routes",
};

/// The route an authority trip runs on: its one direct route when of the same mode; a derived
/// route of the source (`identity::overlay::route`) when that one route is of a compatible but
/// different mode, or else when the trip's mode is source-native; otherwise none, and the
/// line's dates are blocked (`trip_set_authority_withheld`).
///
/// JDF publishes trolleybus lines as bus routes; the source mode is kept on a derived route
/// that keeps the national line.
pub const OVL_R059: Rule = Rule {
    id: "OVL-R059",
    summary: "an authority trip's target route: its same-mode base route, or a derived route",
};

/// Every eligible trip in an authority mode running in the horizon is a candidate when it has an
/// identity, calls, a target route and every call placed. Otherwise its line's running days
/// are blocked: no target route, an unplaced call, no calls; without identity, every line other
/// trips of its source route assert. Each failure is `trip_set_authority_withheld`.
///
/// Partial evidence must not silence national trips.
pub const OVL_R060: Rule = Rule {
    id: "OVL-R060",
    summary: "complete source trips are authority candidates; incomplete ones block their line",
};

/// A line's authority dates are the union of its candidates' running days.
pub const OVL_R061: Rule = Rule {
    id: "OVL-R061",
    summary: "a line's authority dates are its candidates' running days",
};

/// For each integrated system of a source's profile: the national lines of base routes filed
/// under it, in one of the source's authority modes, not excluded, running inside the feed
/// window (R020). When more of them have no authority than the profile's maximum, nothing
/// changes (`trip_set_authority_silence_withheld`); otherwise every one gets authority on every
/// day of the horizon inside the feed window, those without any so far diagnosed
/// (`trip_set_authority_silent`). A line the source runs on some days thus owns the whole window.
///
/// Inside its feed window an integrated system's line runs only what its source runs: tram 16
/// cancelled for trackworks is still in CIS and simply absent from PID.
pub const OVL_R062: Rule = Rule {
    id: "OVL-R062",
    summary: "an integrated system's lines run only what the source runs, within a maximum",
};

/// Blocked line-dates are removed from the authority dates after R062.
pub const OVL_R063: Rule = Rule {
    id: "OVL-R063",
    summary: "blocked line-dates win over authority",
};

/// Each candidate is projected on its running days that its line owns; a candidate without such
/// days is not projected.
pub const OVL_R064: Rule = Rule {
    id: "OVL-R064",
    summary: "candidates are projected on their line's authority dates",
};

pub const RULES: &[Rule] = &[
    OVL_R057, OVL_R058, OVL_R059, OVL_R060, OVL_R061, OVL_R062, OVL_R063, OVL_R064,
];

/// What a source owns the trip set of (R057).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AuthorityLine {
    /// A national line (the base profile's line key).
    National(Box<str>),
    /// A source route standing for a line: source index, index into its `routes`.
    SourceRoute { source: u32, route: u32 },
}

/// Where an authority trip runs (R059).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TargetRoute {
    Base(RouteId),
    /// An index into [`Authority::derived_routes`].
    Derived(u32),
}

/// A route a source adds (R059): its trips keep the source mode or run on their own.
#[derive(Clone, Debug, PartialEq)]
pub struct DerivedRoute {
    pub source: u32,
    /// Index into the source's `routes`.
    pub route: u32,
    pub line: AuthorityLine,
    /// `identity::overlay::route` of the source route and the line (a source-route line hashes
    /// as `source:<source>:<route>`, the F# pseudo-line, so the ID is unchanged).
    pub id: PublicId,
}

/// Why a trip did not become a candidate (R060).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Withheld {
    NoCompatibleRoute,
    UnplacedCall,
    NoCallPattern,
    NoIdentity,
}

impl Withheld {
    pub const fn message(self) -> &'static str {
        match self {
            Self::NoCompatibleRoute => "The asserted CIS line has no compatible base route",
            Self::UnplacedCall => "At least one source call has no resolved stop place",
            Self::NoCallPattern => "The active source trip has no call pattern",
            Self::NoIdentity => "The active source trip has no unique companion CIS line assertion",
        }
    }
}

/// A source trip projected under authority (R064).
#[derive(Clone, Debug, PartialEq)]
pub struct Projection {
    pub source: u32,
    /// Index into the source's `trips`.
    pub trip: u32,
    pub line: AuthorityLine,
    pub target: TargetRoute,
    /// Its running days its line owns.
    pub dates: DateSet,
}

/// The outcome of trip-set authority.
#[derive(Debug, Default)]
pub struct Authority {
    /// R061–R063: per line, the days the sources own its trip set.
    pub lines: BTreeMap<AuthorityLine, DateSet>,
    /// R060: per line, the days blocked by incomplete evidence.
    pub blocked: BTreeMap<AuthorityLine, DateSet>,
    /// R060: (source, trip, why) of every withheld trip, in trip order.
    pub withheld: Vec<(u32, u32, Withheld)>,
    /// R059: the derived routes, in order of first use.
    pub derived_routes: Vec<DerivedRoute>,
    /// R064, in source and trip order.
    pub projections: Vec<Projection>,
}

impl Authority {
    /// The authority dates of a base route's line (R086 reads this).
    pub fn dates_of_line(&self, line: &str) -> DateSet {
        self.lines
            .get(&AuthorityLine::National(line.into()))
            .copied()
            .unwrap_or_default()
    }
}

/// The text a line stands as in a derived route's ID.
fn line_text(source: &PreparedSource<'_>, line: &AuthorityLine) -> String {
    match line {
        AuthorityLine::National(line) => line.to_string(),
        AuthorityLine::SourceRoute { route, .. } => {
            let feed_route =
                &source.feed.routes[source.routes[*route as usize].feed_index as usize];
            format!("source:{}:{}", source.source_id(), feed_route.route_id)
        }
    }
}

/// R058.
fn prefer_regular(prepared: &Prepared<'_>, routes: &[RouteId]) -> Vec<RouteId> {
    if !prepared.base.profile.prefer_regular_routes {
        return routes.to_vec();
    }
    let regular: Vec<RouteId> = routes
        .iter()
        .copied()
        .filter(|&route| !prepared.base.route(route).detour)
        .collect();
    if regular.is_empty() {
        routes.to_vec()
    } else {
        regular
    }
}

/// R057.
fn identity(
    prepared: &Prepared<'_>,
    index: u32,
    trip: &SourceTrip,
) -> Option<(AuthorityLine, Vec<RouteId>)> {
    let source = &prepared.sources[index as usize];
    if let Some(line) = &trip.line_claim {
        return Some((
            AuthorityLine::National(line.clone()),
            prefer_regular(prepared, &trip.direct_routes),
        ));
    }
    let route = &source.routes[trip.route as usize];
    if route.primary_tier == crate::profile::RouteTier::StructuralTripEvidence {
        let routes = prefer_regular(prepared, &route.primary);
        if let [only] = routes.as_slice() {
            return prepared
                .base
                .route(*only)
                .line
                .clone()
                .map(|line| (AuthorityLine::National(line), routes));
        }
    }
    native_mode(source, trip).then(|| {
        (
            AuthorityLine::SourceRoute {
                source: index,
                route: trip.route,
            },
            Vec::new(),
        )
    })
}

fn native_mode(source: &PreparedSource<'_>, trip: &SourceTrip) -> bool {
    source.routes[trip.route as usize]
        .mode
        .is_some_and(|mode| source.profile.authority.source_native_modes.contains(&mode))
}

fn authority_mode(source: &PreparedSource<'_>, trip: &SourceTrip) -> bool {
    source.routes[trip.route as usize]
        .mode
        .is_some_and(|mode| source.profile.authority.modes.contains(&mode))
}

/// R057–R064.
pub fn authority(prepared: &Prepared<'_>, diagnostics: &mut Diagnostics) -> Authority {
    let mut result = Authority::default();
    let mut derived_index: BTreeMap<String, u32> = BTreeMap::new();
    struct Candidate {
        source: u32,
        trip: u32,
        line: AuthorityLine,
        target: TargetRoute,
        active: DateSet,
    }
    let mut candidates: Vec<Candidate> = Vec::new();
    for (index, source) in prepared.sources.iter().enumerate() {
        let index = index as u32;
        let identities: Vec<Option<(AuthorityLine, Vec<RouteId>)>> = source
            .trips
            .iter()
            .map(|trip| identity(prepared, index, trip))
            .collect();
        // R060: the lines each source route's trips assert.
        let mut asserted: Vec<Vec<AuthorityLine>> = vec![Vec::new(); source.routes.len()];
        for (trip, found) in source.trips.iter().zip(&identities) {
            if let Some((line, _)) = found {
                let lines = &mut asserted[trip.route as usize];
                if !lines.contains(line) {
                    lines.push(line.clone());
                }
            }
        }
        for (trip_index, (trip, found)) in source.trips.iter().zip(identities).enumerate() {
            if !authority_mode(source, trip) || trip.dates.is_empty() {
                continue;
            }
            let active = trip.dates;
            let calls = source.trip_calls(trip);
            let block = |result: &mut Authority, line: &AuthorityLine| {
                *result.blocked.entry(line.clone()).or_default() =
                    result.blocked.get(line).copied().unwrap_or_default() | active;
            };
            let withheld = match found {
                Some((line, direct)) if !calls.is_empty() => {
                    let source_mode = source.routes[trip.route as usize].mode;
                    let target = match direct.as_slice() {
                        [only] if Some(prepared.base.route(*only).mode) == source_mode => {
                            Some(TargetRoute::Base(*only))
                        }
                        [only]
                            if prepared
                                .base
                                .profile
                                .compatible(Some(prepared.base.route(*only).mode), source_mode) =>
                        {
                            Some(derive(
                                &mut result,
                                &mut derived_index,
                                source,
                                index,
                                trip,
                                &line,
                            ))
                        }
                        _ if native_mode(source, trip) => Some(derive(
                            &mut result,
                            &mut derived_index,
                            source,
                            index,
                            trip,
                            &line,
                        )),
                        _ => None,
                    };
                    match target {
                        None => {
                            block(&mut result, &line);
                            Some(Withheld::NoCompatibleRoute)
                        }
                        Some(target) if calls.iter().all(|call| call.place.is_some()) => {
                            candidates.push(Candidate {
                                source: index,
                                trip: trip_index as u32,
                                line,
                                target,
                                active,
                            });
                            None
                        }
                        Some(_) => {
                            block(&mut result, &line);
                            Some(Withheld::UnplacedCall)
                        }
                    }
                }
                Some((line, _)) => {
                    block(&mut result, &line);
                    Some(Withheld::NoCallPattern)
                }
                None => {
                    for line in &asserted[trip.route as usize] {
                        block(&mut result, line);
                    }
                    Some(Withheld::NoIdentity)
                }
            };
            if let Some(reason) = withheld {
                diagnostics.push(
                    Diagnostic::new(
                        "trip_set_authority_withheld",
                        Severity::Warning,
                        trip_subject(source, trip),
                        reason.message(),
                    )
                    .rule(OVL_R060),
                );
                result.withheld.push((index, trip_index as u32, reason));
            }
        }
    }
    // R061.
    for candidate in &candidates {
        let dates = result.lines.entry(candidate.line.clone()).or_default();
        *dates = *dates | candidate.active;
    }
    // R062.
    silence::extend(prepared, &mut result.lines, diagnostics);
    // R063.
    for (line, blocked) in &result.blocked {
        if let Some(dates) = result.lines.get_mut(line) {
            *dates = *dates - *blocked;
        }
    }
    // R064.
    for candidate in candidates {
        let owned = result
            .lines
            .get(&candidate.line)
            .copied()
            .unwrap_or_default();
        let dates = candidate.active & owned;
        if !dates.is_empty() {
            result.projections.push(Projection {
                source: candidate.source,
                trip: candidate.trip,
                line: candidate.line,
                target: candidate.target,
                dates,
            });
        }
    }
    result
}

/// R059: the derived route of a source route and line, added once.
fn derive(
    result: &mut Authority,
    index: &mut BTreeMap<String, u32>,
    source: &PreparedSource<'_>,
    source_index: u32,
    trip: &SourceTrip,
    line: &AuthorityLine,
) -> TargetRoute {
    let feed_route = &source.feed.routes[source.routes[trip.route as usize].feed_index as usize];
    let id = overlay::route(
        source.source_id(),
        &feed_route.route_id,
        &line_text(source, line),
    );
    let next = result.derived_routes.len() as u32;
    let slot = *index.entry(id.to_string()).or_insert_with(|| {
        result.derived_routes.push(DerivedRoute {
            source: source_index,
            route: trip.route,
            line: line.clone(),
            id,
        });
        next
    });
    TargetRoute::Derived(slot)
}

/// A source trip as reports and diagnostics name it: `<source>:<trip_id>`.
pub fn trip_subject(source: &PreparedSource<'_>, trip: &SourceTrip) -> String {
    format!(
        "{}:{}",
        source.source_id(),
        source.feed.trips[trip.feed_index as usize].trip_id
    )
}
