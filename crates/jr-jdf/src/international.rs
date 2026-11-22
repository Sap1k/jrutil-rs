//! The international route filter (JDF-R269–R278, R281).
//!
//! Oběhy keeps regional cross-border service near the border and drops long-distance
//! international lines. The filter judges one line version at a time from a small view the
//! caller builds ([`RouteView`]), so it does not depend on how stops are merged: per call the
//! stop's country and okres as published, the timetable kilometre and whether the call is
//! emitted ([`is_emitted`]). Country codes stay as the feed publishes them; the filter compares
//! them in a canonical form (R270) and records that form.

use std::collections::BTreeSet;

use jr_formats::jdf::{Km, RouteType, TimeCell};
use jr_model::{Diagnostic, Diagnostics, Rule, Severity};

use crate::policy::{FilterPolicy, Policy};

/// `regional-adjacent` classifies and filters; `keep-all` keeps every version and records no
/// decisions (backward compatibility). Oběhy runs `regional-adjacent`.
pub const R269: Rule = Rule {
    id: "JDF-R269",
    summary: "The filter policy is regional-adjacent or keep-all",
};

/// A stop's country is its trimmed, upper-cased country code with the aliases of policy
/// (`AT` → `A`, `DE` → `D`); a stop without a country but with an okres is domestic; otherwise
/// the country is unknown (and stays a candidate for inspection).
pub const R270: Rule = Rule {
    id: "JDF-R270",
    summary: "Stop country: canonical code, domestic when only an okres is given, else unknown",
};

/// Only versions declared international (`M`, `N`, `P`) or with a route stop outside the
/// domestic country (unknown included) are classified; the others are kept whole.
pub const R271: Rule = Rule {
    id: "JDF-R271",
    summary: "Only declared-international versions or ones with a non-domestic stop are classified",
};

/// A trip that never runs contributes nothing to its version's decision and is kept (whoever
/// drops never-running trips does so for that reason).
pub const R272: Rule = Rule {
    id: "JDF-R272",
    summary: "Trips that do not run are not classified and do not count",
};

/// A call counts when its departure is a time, or it has no departure and has an arrival. A
/// passed (`|`) or bypassed (`<`) foreign stop is not service abroad.
pub const R273: Rule = Rule {
    id: "JDF-R273",
    summary: "Only emitted calls count for countries, span and depth",
};

/// Over the trip's emitted calls, first match: no foreign country → domestic if every country
/// is known and the domestic one occurs, else rejected (unknown country); no domestic call →
/// foreign-only; an unknown country → rejected; a non-adjacent country → rejected; a call
/// without kilometre → rejected; span or depth beyond the limit → rejected; else qualifying.
pub const R274: Rule = Rule {
    id: "JDF-R274",
    summary: "Trips are domestic, foreign-only, rejected with a reason or qualifying",
};

/// Span = the trip's kilometre range; depth = the farthest a foreign call lies (in timetable
/// kilometres) from the nearest domestic call.
pub const R275: Rule = Rule {
    id: "JDF-R275",
    summary: "Span is the kilometre range; depth the farthest foreign call from a domestic one",
};

/// A version with a `LinExt` row (part of an integrated system) has the wider envelope. The
/// limits are policy (120/60 km, integrated 200/80 km); strictly greater rejects.
pub const R276: Rule = Rule {
    id: "JDF-R276",
    summary: "Span and depth limits, wider for integrated versions",
};

/// A classified version is kept when it is not potentially international (no foreign country
/// among its running trips' emitted calls and not declared international) or has a qualifying
/// trip. The reason is the first of: domestic, regional adjacent, international metadata
/// without foreign geography, foreign-only trips, the one rejection reason, no qualifying trip.
/// So a declared-international version with only domestic calls is dropped whole (B13, ported).
pub const R277: Rule = Rule {
    id: "JDF-R277",
    summary: "A classified version is kept when not potentially international or a trip qualifies",
};

/// In a kept classified version only running domestic and qualifying trips (and trips that do
/// not run) stay: foreign-only and rejected trips are dropped and diagnosed, including trips
/// through an unknown-country stop on a version kept as domestic (B13, ported).
pub const R278: Rule = Rule {
    id: "JDF-R278",
    summary: "Kept versions keep their domestic, qualifying and non-running trips only",
};

/// Every classified version's decision carries its reason, countries, maxima and counts for the
/// manifest; a dropped version is diagnosed with them.
pub const R281: Rule = Rule {
    id: "JDF-R281",
    summary: "Each classified version's decision is recorded; dropped versions are diagnosed",
};

/// A stop's published country facts (R270).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StopCountry<'a> {
    /// The country code as published.
    pub country: Option<&'a str>,
    /// The okres code as published.
    pub okres: Option<&'a str>,
}

/// One call of a trip, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallView<'a> {
    pub stop: StopCountry<'a>,
    /// The `Zasspoje` timetable kilometre.
    pub km: Option<Km>,
    /// [`is_emitted`] (R273).
    pub emitted: bool,
}

/// One trip of a line version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TripView<'a> {
    pub trip: u64,
    /// Whether the trip has any running day (R272).
    pub runs: bool,
    pub calls: Vec<CallView<'a>>,
}

/// One line version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteView<'a> {
    /// The version, as diagnostics name it.
    pub subject: &'a str,
    pub route_type: Option<&'a RouteType>,
    /// Whether the version has a `LinExt` row (R276).
    pub integrated: bool,
    /// The stops of its `Zaslinky` rows (R271).
    pub route_stops: Vec<StopCountry<'a>>,
    pub trips: Vec<TripView<'a>>,
}

/// Why a version or trip was decided as it was. The recorded texts are policy
/// ([`crate::policy::InternationalPolicy::reason_text`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Reason {
    Domestic,
    RegionalAdjacent,
    InternationalMetadataWithoutForeignGeography,
    ForeignOnlyTrip,
    UnknownStopCountry,
    NonAdjacentCountry,
    MissingTimetableKilometres,
    TripSpanExceedsLimit,
    ForeignDepthExceedsLimit,
    NoQualifyingCrossBorderTrip,
}

/// What R274 says of one trip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Disposition {
    /// The trip does not run (R272).
    NotRunning,
    Domestic,
    Qualifying {
        span_km: f64,
        depth_km: f64,
    },
    Rejected {
        reason: Reason,
        span_km: Option<f64>,
        depth_km: Option<f64>,
    },
    ForeignOnly,
}

/// One trip's fate, in the order of [`RouteView::trips`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TripDecision {
    pub trip: u64,
    pub keep: bool,
    pub disposition: Disposition,
}

/// A classified version's record (R281).
#[derive(Clone, Debug, PartialEq)]
pub struct Classified {
    pub reason: Reason,
    /// The distinct canonical countries of its running trips' emitted calls, sorted.
    pub countries: Vec<Box<str>>,
    pub integrated: bool,
    pub max_span_km: Option<f64>,
    pub max_depth_km: Option<f64>,
    pub domestic_trips: usize,
    pub qualifying_trips: usize,
    pub rejected_trips: usize,
    pub foreign_only_trips: usize,
    /// One per trip of the view, in its order.
    pub trips: Vec<TripDecision>,
}

impl Classified {
    /// The version carries regional cross-border service (F# set route type
    /// `RegionalInternational`, R280; a separate fact in the rewrite).
    pub fn is_regional_cross_border(&self) -> bool {
        self.qualifying_trips > 0
    }
}

/// The filter's decision on one line version.
#[derive(Clone, Debug, PartialEq)]
pub enum RouteDecision {
    /// Not classified (policy `keep-all`, or not a candidate: R271): the version and all its
    /// trips stay.
    Keep,
    /// Classified and kept: the trips whose decision says so stay (R277, R278).
    KeepTrips(Classified),
    /// Classified and dropped with all its trips (R277).
    Drop(Classified),
}

/// Whether a call is emitted (R273): its departure is a time, or it has no departure and has an
/// arrival. An empty cell is no time.
pub fn is_emitted(arrival: Option<TimeCell>, departure: Option<TimeCell>) -> bool {
    let given = |cell: Option<TimeCell>| !matches!(cell, None | Some(TimeCell::Empty));
    match departure {
        Some(TimeCell::At { .. }) => true,
        Some(TimeCell::Passes | TimeCell::OtherRoute) => false,
        None | Some(TimeCell::Empty) => given(arrival),
    }
}

/// The canonical country of a stop (R270).
fn country(stop: &StopCountry<'_>, policy: &Policy) -> Option<Box<str>> {
    let p = &policy.international;
    match stop.country.map(str::trim) {
        Some(code) if !code.is_empty() => {
            let code = code.to_uppercase();
            Some(
                p.country_aliases
                    .get(code.as_str())
                    .cloned()
                    .unwrap_or_else(|| code.into()),
            )
        }
        _ if stop.okres.is_some() => Some(p.domestic_country.clone()),
        _ => None,
    }
}

/// Decides one line version (R269–R278) and diagnoses what it drops (R278, R281).
pub fn classify(
    route: &RouteView<'_>,
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> RouteDecision {
    let p = &policy.international;
    if p.policy == FilterPolicy::KeepAll {
        return RouteDecision::Keep;
    }
    let domestic = &*p.domestic_country;
    let declared = route.route_type.is_some_and(|route_type| {
        p.declared_international_route_types
            .iter()
            .any(|code| &**code == route_type.code())
    });
    let candidate = declared
        || route
            .route_stops
            .iter()
            .any(|stop| country(stop, policy).as_deref() != Some(domestic));
    if !candidate {
        return RouteDecision::Keep;
    }

    let mut countries = BTreeSet::new();
    let mut trips = Vec::with_capacity(route.trips.len());
    for trip in &route.trips {
        let disposition = if trip.runs {
            let calls: Vec<(Option<Box<str>>, Option<Km>)> = trip
                .calls
                .iter()
                .filter(|call| call.emitted)
                .map(|call| (country(&call.stop, policy), call.km))
                .collect();
            countries.extend(calls.iter().filter_map(|(country, _)| country.clone()));
            classify_trip(&calls, route.integrated, policy)
        } else {
            Disposition::NotRunning
        };
        trips.push(TripDecision {
            trip: trip.trip,
            keep: false,
            disposition,
        });
    }

    let foreign = countries.iter().any(|country| &**country != domestic);
    let potentially_international = foreign || declared;
    let count = |test: fn(&Disposition) -> bool| {
        trips
            .iter()
            .filter(|decision| test(&decision.disposition))
            .count()
    };
    let qualifying: Vec<(f64, f64)> = trips
        .iter()
        .filter_map(|decision| match decision.disposition {
            Disposition::Qualifying { span_km, depth_km } => Some((span_km, depth_km)),
            _ => None,
        })
        .collect();
    let rejections: BTreeSet<Reason> = trips
        .iter()
        .filter_map(|decision| match decision.disposition {
            Disposition::Rejected { reason, .. } => Some(reason),
            _ => None,
        })
        .collect();
    let rejected_trips = count(|d| matches!(d, Disposition::Rejected { .. }));
    let foreign_only_trips = count(|d| matches!(d, Disposition::ForeignOnly));
    let domestic_trips = count(|d| matches!(d, Disposition::Domestic));
    let keep = !potentially_international || !qualifying.is_empty();
    let reason = if !potentially_international {
        Reason::Domestic
    } else if !qualifying.is_empty() {
        Reason::RegionalAdjacent
    } else if !foreign {
        Reason::InternationalMetadataWithoutForeignGeography
    } else if foreign_only_trips > 0 && rejected_trips == 0 {
        Reason::ForeignOnlyTrip
    } else if let (1, Some(&only)) = (rejections.len(), rejections.first()) {
        only
    } else {
        Reason::NoQualifyingCrossBorderTrip
    };

    for decision in &mut trips {
        decision.keep = keep
            && matches!(
                decision.disposition,
                Disposition::NotRunning | Disposition::Domestic | Disposition::Qualifying { .. }
            );
    }
    let classified = Classified {
        reason,
        countries: countries.into_iter().collect(),
        integrated: route.integrated,
        max_span_km: qualifying.iter().map(|q| q.0).reduce(f64::max),
        max_depth_km: qualifying.iter().map(|q| q.1).reduce(f64::max),
        domestic_trips,
        qualifying_trips: qualifying.len(),
        rejected_trips,
        foreign_only_trips,
        trips,
    };

    if keep {
        for decision in classified.trips.iter().filter(|decision| !decision.keep) {
            let why = match decision.disposition {
                Disposition::Rejected { reason, .. } => p.reason_text(reason),
                _ => p.reason_text(Reason::ForeignOnlyTrip),
            };
            diagnostics.push(
                Diagnostic::new(
                    "jdf_international_trip_dropped",
                    Severity::Warning,
                    format!("{}/{}", route.subject, decision.trip),
                    format!("trip dropped from a kept version: {why}"),
                )
                .rule(R278),
            );
        }
        RouteDecision::KeepTrips(classified)
    } else {
        diagnostics.push(
            Diagnostic::new(
                "jdf_international_route_dropped",
                Severity::Warning,
                route.subject,
                format!(
                    "{}: countries {}; {} domestic, {} rejected, {} foreign-only trips",
                    p.reason_text(classified.reason),
                    classified.countries.join(","),
                    classified.domestic_trips,
                    classified.rejected_trips,
                    classified.foreign_only_trips,
                ),
            )
            .rule(R281),
        );
        RouteDecision::Drop(classified)
    }
}

/// R274–R276 over a running trip's emitted calls (country, kilometre).
fn classify_trip(
    calls: &[(Option<Box<str>>, Option<Km>)],
    integrated: bool,
    policy: &Policy,
) -> Disposition {
    let p = &policy.international;
    let domestic = &*p.domestic_country;
    let rejected = |reason| Disposition::Rejected {
        reason,
        span_km: None,
        depth_km: None,
    };
    let is_domestic = |country: &Option<Box<str>>| country.as_deref() == Some(domestic);
    let unknown = calls.iter().any(|(country, _)| country.is_none());
    let has_domestic = calls.iter().any(|(country, _)| is_domestic(country));
    let foreign: BTreeSet<&str> = calls
        .iter()
        .filter_map(|(country, _)| country.as_deref())
        .filter(|&country| country != domestic)
        .collect();
    if foreign.is_empty() {
        return if has_domestic && !unknown {
            Disposition::Domestic
        } else {
            rejected(Reason::UnknownStopCountry)
        };
    }
    if !has_domestic {
        return Disposition::ForeignOnly;
    }
    if unknown {
        return rejected(Reason::UnknownStopCountry);
    }
    if !foreign
        .iter()
        .all(|country| p.adjacent_countries.iter().any(|a| &**a == *country))
    {
        return rejected(Reason::NonAdjacentCountry);
    }
    let Some(kms) = calls
        .iter()
        .map(|(country, km)| km.map(|km| (is_domestic(country), km.value())))
        .collect::<Option<Vec<(bool, f64)>>>()
    else {
        return rejected(Reason::MissingTimetableKilometres);
    };

    // R275.
    let all = kms.iter().map(|&(_, km)| km);
    let span = all.clone().fold(f64::NEG_INFINITY, f64::max) - all.fold(f64::INFINITY, f64::min);
    let depth = kms
        .iter()
        .filter(|(home, _)| !home)
        .map(|&(_, far)| {
            kms.iter()
                .filter(|(home, _)| *home)
                .map(|&(_, near)| (far - near).abs())
                .fold(f64::INFINITY, f64::min)
        })
        .fold(f64::NEG_INFINITY, f64::max);

    // R276.
    let (span_limit, depth_limit) = if integrated {
        (p.integrated_max_span_km, p.integrated_max_foreign_depth_km)
    } else {
        (p.max_span_km, p.max_foreign_depth_km)
    };
    let beyond = |reason| Disposition::Rejected {
        reason,
        span_km: Some(span),
        depth_km: Some(depth),
    };
    if span > span_limit {
        beyond(Reason::TripSpanExceedsLimit)
    } else if depth > depth_limit {
        beyond(Reason::ForeignDepthExceedsLimit)
    } else {
        Disposition::Qualifying {
            span_km: span,
            depth_km: depth,
        }
    }
}
