//! The international filter on the merged versions (JDF-R269–R281 through
//! [`crate::international`]) and pruning what no version uses (R279, R284).

use jr_model::{Diagnostic, Diagnostics, Rule, Severity};

use super::Merged;
use crate::international::{
    CallView, RouteDecision, RouteView, StopCountry, TripView, classify, is_emitted,
};

/// The filter removes the trips and versions it rejects; what only they used (stops, agencies)
/// goes with them by R284. Nothing else is pruned (F# pruned each relation by hand).
pub const R279: Rule = Rule {
    id: "JDF-R279",
    summary: "Rejected versions and trips are removed; their unused stops and agencies follow",
};

/// A kept version with a qualifying cross-border trip is marked as regional cross-border
/// service, a fact next to its published route type (F# overwrote the route type).
pub const R280: Rule = Rule {
    id: "JDF-R280",
    summary: "A kept version with a qualifying trip is marked regional cross-border",
};

/// Stops and agencies that no surviving version uses are not published and offer no registry
/// candidates (verdict Q20); counted in one diagnostic.
pub const R284: Rule = Rule {
    id: "JDF-R284",
    summary: "Stops and agencies no surviving version uses are dropped",
};

/// The filter's decision on one version, for the manifest.
#[derive(Clone, Debug, PartialEq)]
pub struct FilterRecord {
    pub batch: Box<str>,
    pub licence: Box<str>,
    /// The published distinction.
    pub distinction: u32,
    pub decision: RouteDecision,
}

/// R269–R281 on the merged versions: views, decisions, removal and pruning. Returns one record
/// per version (none under `keep-all`).
pub fn apply_international(
    merged: &mut Merged,
    jdf_policy: &crate::Policy,
    diagnostics: &mut Diagnostics,
) -> Vec<FilterRecord> {
    if jdf_policy.international.policy == crate::policy::FilterPolicy::KeepAll {
        return Vec::new();
    }
    let country = |stop: u32| {
        let stop = &merged.stops[stop as usize];
        StopCountry {
            country: stop.name.country.as_deref(),
            okres: stop.name.okres.as_deref(),
        }
    };
    let mut decisions = Vec::with_capacity(merged.versions.len());
    for version in &merged.versions {
        let subject = version.subject();
        let view = RouteView {
            subject: &subject,
            route_type: version.route.route_type.as_ref(),
            integrated: !version.integrations.is_empty(),
            route_stops: version
                .route_stops
                .iter()
                .map(|s| country(s.stop))
                .collect(),
            trips: version
                .trips
                .iter()
                .map(|trip| TripView {
                    trip: trip.id,
                    runs: !trip.days.is_empty(),
                    calls: trip
                        .calls
                        .iter()
                        .map(|call| CallView {
                            stop: country(call.stop),
                            km: call.km,
                            emitted: is_emitted(call.arrival, call.departure),
                        })
                        .collect(),
                })
                .collect(),
        };
        decisions.push(classify(&view, jdf_policy, diagnostics));
    }

    let mut records = Vec::with_capacity(decisions.len());
    let mut versions = Vec::with_capacity(merged.versions.len());
    for (mut version, decision) in std::mem::take(&mut merged.versions)
        .into_iter()
        .zip(decisions)
    {
        records.push(FilterRecord {
            batch: version.batch.clone(),
            licence: version.route.id.clone(),
            distinction: version.route.distinction,
            decision: decision.clone(),
        });
        match decision {
            RouteDecision::Keep => versions.push(version),
            RouteDecision::KeepTrips(classified) => {
                let mut keep = classified.trips.iter().map(|t| t.keep);
                version.trips.retain(|_| keep.next().unwrap_or(true));
                version.regional_cross_border = classified.is_regional_cross_border();
                versions.push(version);
            }
            RouteDecision::Drop(_) => {}
        }
    }
    merged.versions = versions;
    prune(merged, diagnostics);
    records
}

/// R284: drops stops and agencies no version uses, renumbering the references.
pub(crate) fn prune(merged: &mut Merged, diagnostics: &mut Diagnostics) {
    let mut used_stops = vec![false; merged.stops.len()];
    let mut used_agencies = vec![false; merged.agencies.len()];
    for version in &merged.versions {
        used_agencies[version.agency] = true;
        for alternation in &version.agency_alternations {
            used_agencies[alternation.agency] = true;
        }
        for route_stop in &version.route_stops {
            used_stops[route_stop.stop as usize] = true;
        }
        for trip in &version.trips {
            for call in &trip.calls {
                used_stops[call.stop as usize] = true;
            }
        }
    }
    let renumber = |used: &[bool]| {
        let mut next = 0usize;
        used.iter()
            .map(|&kept| {
                let index = next;
                next += usize::from(kept);
                kept.then_some(index)
            })
            .collect::<Vec<Option<usize>>>()
    };
    let stop_index = renumber(&used_stops);
    let agency_index = renumber(&used_agencies);
    let dropped_stops = used_stops.iter().filter(|&&used| !used).count();
    let dropped_agencies = used_agencies.iter().filter(|&&used| !used).count();
    if dropped_stops == 0 && dropped_agencies == 0 {
        return;
    }
    let mut position = 0;
    merged.stops.retain(|_| {
        position += 1;
        used_stops[position - 1]
    });
    let mut position = 0;
    merged.agencies.retain(|_| {
        position += 1;
        used_agencies[position - 1]
    });
    let stop = |old: u32| stop_index[old as usize].map_or(old, |new| new as u32);
    let agency = |old: usize| agency_index[old].unwrap_or(old);
    for version in &mut merged.versions {
        version.agency = agency(version.agency);
        for alternation in &mut version.agency_alternations {
            alternation.agency = agency(alternation.agency);
        }
        for route_stop in &mut version.route_stops {
            route_stop.stop = stop(route_stop.stop);
        }
        for trip in &mut version.trips {
            for call in &mut trip.calls {
                call.stop = stop(call.stop);
            }
        }
    }
    let numbers: std::collections::HashSet<u64> =
        merged.stops.iter().map(|stop| stop.number).collect();
    merged.registry_candidates.retain(|candidate| {
        candidate
            .provisional_number
            .or_else(|| candidate.suggestions.first().copied())
            .is_none_or(|number| numbers.contains(&number))
    });
    diagnostics.push(
        Diagnostic::new(
            "jdf_unused_dropped",
            Severity::Info,
            "merge",
            format!("{dropped_stops} stops and {dropped_agencies} agencies no version uses"),
        )
        .rule(R284),
    );
}
