//! Candidate base routes of source routes (OVL-R036–R038, R049–R052).

use jr_model::{Diagnostic, Diagnostics, RouteId, Rule, Severity};

use super::base::BaseView;
use super::source::PreparedSource;
use crate::profile::RouteTier;

/// Two modes are compatible when equal or of one class of the base profile.
///
/// JDF represents trolleybus service as road/bus routes.
pub const OVL_R036: Rule = Rule {
    id: "OVL-R036",
    summary: "modes are compatible when equal or in one class of the base profile",
};

/// Two public line numbers are structurally compatible when their letters and digits (upper
/// case) are equal, or when one is a single letter that the other begins with only digits after
/// it (`H` and `H4`, never `2` and `25`).
///
/// Regional feeds and CIS differ in how they write a line's letter variants.
pub const OVL_R037: Rule = Rule {
    id: "OVL-R037",
    summary: "line labels are compatible when equal or a letter and its numbered variants",
};

/// A source route's mode is its GTFS `route_type` read by the base profile's table; a base
/// route's is its model mode.
///
/// JDF long-distance lines are coach (200–209) where PID publishes the same line as plain bus.
pub const OVL_R038: Rule = Rule {
    id: "OVL-R038",
    summary: "a source route's mode is its route type read by the base profile's table",
};

/// A source trip's direct base routes are the published base routes of the line it asserts
/// (R018) whose mode is compatible (R036), sorted by ID; a source route's are the union over
/// its trips. Non-empty and with the tier in the profile, they are the route's primary
/// candidates (`companion_assertion`).
///
/// A documented line claim is the strongest route evidence (BASE_PLAN 12.3 step 1).
pub const OVL_R049: Rule = Rule {
    id: "OVL-R049",
    summary: "the base routes of a trip's asserted line are its route's primary candidates",
};

/// Otherwise the primary candidates are the published base routes of compatible mode whose
/// public line number equals the source's (`structural_trip_evidence`), none when the tier is
/// not in the profile. A route without a public line number has none (B21 fixed, diagnosed).
///
/// The public designation is the next best evidence (BASE_PLAN 12.3 step 3).
pub const OVL_R050: Rule = Rule {
    id: "OVL-R050",
    summary: "base routes with the same public line number and a compatible mode",
};

/// Every source route also has relaxed structural candidates: published base routes of
/// compatible mode with a structurally compatible line number (R037). They are the fallback of
/// trip matching and context inference, never of authority.
pub const OVL_R051: Rule = Rule {
    id: "OVL-R051",
    summary: "relaxed structural candidates by compatible line labels",
};

/// A source route with no candidates is not an error; what becomes of its trips is decided by
/// matching (R073) and authority (R059).
pub const OVL_R052: Rule = Rule {
    id: "OVL-R052",
    summary: "a source route without candidates is left to trip matching and authority",
};

pub const RULES: &[Rule] = &[
    OVL_R036, OVL_R037, OVL_R038, OVL_R049, OVL_R050, OVL_R051, OVL_R052,
];

/// R037.
pub fn labels_compatible(left: &str, right: &str) -> bool {
    let normalize = |text: &str| -> String {
        text.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_uppercase)
            .collect()
    };
    let (left, right) = (normalize(left), normalize(right));
    let variant = |shorter: &str, longer: &str| {
        let mut chars = shorter.chars();
        matches!((chars.next(), chars.next()), (Some(letter), None) if letter.is_alphabetic())
            && longer.starts_with(shorter)
            && longer[shorter.len()..].chars().all(|c| c.is_ascii_digit())
    };
    left == right
        || (!left.is_empty()
            && !right.is_empty()
            && (variant(&left, &right) || variant(&right, &left)))
}

/// R049–R052: fills each trip's direct routes and each route's candidates.
pub fn candidates(
    source: &mut PreparedSource<'_>,
    base: &BaseView<'_>,
    diagnostics: &mut Diagnostics,
) {
    let profile = source.profile;
    let compatible = |left, right| base.profile.compatible(left, right);
    let by_id = |route: &RouteId| base.timetable.routes[*route].id.as_str();
    for trip in &mut source.trips {
        let mode = source.routes[trip.route as usize].mode;
        let Some(line) = &trip.line_claim else {
            continue;
        };
        let mut direct: Vec<RouteId> = base
            .routes_by_line
            .get(line)
            .into_iter()
            .flatten()
            .copied()
            .filter(|&route| compatible(Some(base.route(route).mode), mode))
            .collect();
        direct.sort_by(|a, b| by_id(a).cmp(by_id(b)));
        direct.dedup();
        trip.direct_routes = direct;
    }
    let mut trips_of_route: Vec<Vec<u32>> = vec![Vec::new(); source.routes.len()];
    for (index, trip) in source.trips.iter().enumerate() {
        trips_of_route[trip.route as usize].push(index as u32);
    }
    let mut unlabelled = 0usize;
    for (index, route) in source.routes.iter_mut().enumerate() {
        let feed_route = &source.feed.routes[route.feed_index as usize];
        let label = feed_route.route_short_name.as_deref().unwrap_or("").trim();
        let mut direct: Vec<RouteId> = Vec::new();
        for &trip in &trips_of_route[index] {
            for &candidate in &source.trips[trip as usize].direct_routes {
                if !direct.contains(&candidate) {
                    direct.push(candidate);
                }
            }
        }
        let mut exact = Vec::new();
        let mut relaxed = Vec::new();
        if label.is_empty() {
            unlabelled += 1;
        } else {
            for &candidate in &base.published_routes {
                let target = base.route(candidate);
                if !compatible(Some(target.mode), route.mode) {
                    continue;
                }
                let number = target.public_number.as_deref().unwrap_or("");
                if number == label {
                    exact.push(candidate);
                }
                if labels_compatible(label, number) {
                    relaxed.push(candidate);
                }
            }
        }
        route.structural = relaxed;
        if !direct.is_empty()
            && profile
                .routes
                .tiers
                .contains(&RouteTier::CompanionAssertion)
        {
            route.primary = direct;
            route.primary_tier = RouteTier::CompanionAssertion;
        } else {
            route.primary_tier = RouteTier::StructuralTripEvidence;
            route.primary = if profile
                .routes
                .tiers
                .contains(&RouteTier::StructuralTripEvidence)
            {
                exact
            } else {
                Vec::new()
            };
        }
    }
    if unlabelled > 0 {
        diagnostics.push(
            Diagnostic::new(
                "overlay_route_label_missing",
                Severity::Warning,
                profile.source_id.to_string(),
                format!("{unlabelled} routes have no public line number: no structural candidates"),
            )
            .rule(OVL_R050),
        );
    }
}
