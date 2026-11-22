//! National keys (JDF-R466).

use std::collections::BTreeMap;

use jr_model::timetable::{BindingMethod, Entity, Namespace, SourceKey};
use jr_model::{Date, Diagnostic, Diagnostics, RouteId, Rule, Severity, Timetable};

use super::Policy;
use super::calls::Published;
use crate::merge::Merged;

/// A licence of one to six ASCII digits is a CIS line: each published trip gets
/// `cis:line_trip` `<licence padded to six>:<trip number>` and each route with trips `cis:line`
/// `<licence padded>`, binding by identity. A trip's key holds from the first to the last day
/// its line version is in force (the dates the version won, JDF-R247), a route's over its
/// trips' keys. Any other licence gets no key, diagnosed once per version.
///
/// The line and trip number identify a trip only within one version of the line: the next
/// version reuses the numbers. The version's period is therefore the period the identifier
/// means this trip, whether or not the trip runs on its first and last day.
pub const R466: Rule = Rule {
    id: "JDF-R466",
    summary: "CIS licences give cis:line and cis:line_trip keys over the version's period",
};

pub(crate) fn build(
    merged: &Merged,
    routes: &[Option<RouteId>],
    published: &Published,
    policy: &Policy,
    timetable: &mut Timetable,
    diagnostics: &mut Diagnostics,
) {
    let width = policy.keys.licence_digits;
    let mut route_dates: BTreeMap<RouteId, (Box<str>, Date, Date)> = BTreeMap::new();
    for (v, version) in merged.versions.iter().enumerate() {
        let (Some(route), trips) = (routes[v], &published.versions[v]) else {
            continue;
        };
        if trips.is_empty() {
            continue;
        }
        let licence = &*version.route.id;
        if licence.is_empty()
            || licence.len() > width
            || !licence.bytes().all(|b| b.is_ascii_digit())
        {
            diagnostics.push(
                Diagnostic::new(
                    "jdf_no_cis_key",
                    Severity::Warning,
                    version.subject(),
                    format!("licence {licence:?} is no CIS line number: no keys"),
                )
                .rule(R466),
            );
            continue;
        }
        let padded = format!("{licence:0>width$}");
        let (Some(from), Some(to)) = (
            version.won.iter().map(|&(from, _)| from).min(),
            version.won.iter().map(|&(_, to)| to).max(),
        ) else {
            continue;
        };
        for trip in trips {
            timetable.keys.push(SourceKey {
                namespace: Namespace::CIS_LINE_TRIP,
                identifier: format!("{padded}:{}", version.trips[trip.trip].id).into(),
                entity: Entity::Trip(trip.handle),
                valid_from: from,
                valid_to: to,
                binding: BindingMethod::Identity,
            });
            let entry = route_dates
                .entry(route)
                .or_insert_with(|| (padded.clone().into(), from, to));
            entry.1 = entry.1.min(from);
            entry.2 = entry.2.max(to);
        }
    }
    for (route, (identifier, from, to)) in route_dates {
        timetable.keys.push(SourceKey {
            namespace: Namespace::CIS_LINE,
            identifier,
            entity: Entity::Route(route),
            valid_from: from,
            valid_to: to,
            binding: BindingMethod::Identity,
        });
    }
}
