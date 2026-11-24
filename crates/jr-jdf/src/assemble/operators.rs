//! Alternate operators (JDF-R483, R484): who runs a trip on which days, and the trip split by
//! operator.
//!
//! Assembly decides ([`decide`]); the split is a pass of its own ([`edits`]) that the pipeline
//! applies after the post estimator, so a part of a trip carries the boarding points the
//! estimator chose for it.

use std::collections::HashMap;

use jr_model::edit::{Edit, EditBuilder, Edits};
use jr_model::timetable::{Agency, Entity, Scope, SourceKey};
use jr_model::{
    AgencyId, DateSet, Diagnostic, Diagnostics, PublicId, RouteId, Rule, Severity, Timetable,
    TripId, identity,
};

use super::Policy;
use super::calls::Published;
use crate::holidays::Holidays;
use crate::merge::{AgencyAlternation, Merged};
use crate::policy::NoteKind;

/// An `Altdop` row names the operator of a trip (its trip number, or `0`: every trip of the
/// line) on the days it describes: its day attributes (`X`, `+`, day numbers; none: every day),
/// its odd or even weeks (time code type 5 or 6) and its period (`date_from ..= date_to`; one
/// day without `date_to`). Per day the first matching row in file order decides; a day no row
/// matches is the line's operator's (JDF 1.11, `Altdop`).
pub const R483: Rule = Rule {
    id: "JDF-R483",
    summary: "Altdop rows name a trip's operator day by day; the first matching row decides",
};

/// A trip that other operators run on some of its days is split by operator: each operator's
/// days are a trip of their own, under that operator's variant of the route (same line, same
/// notes and integrations, the operator's agency; `identity::jdf::operator_route`). The line's
/// operator keeps the trip's ID, or, where it runs none of the days, the first alternate
/// operator in `Altdop` order; the other parts are `identity::jdf::operator_trip`. A trip run
/// by one alternate operator on all its days only moves to that operator's route.
///
/// GTFS knows one operator per route, and passengers board the operator that runs the day.
pub const R484: Rule = Rule {
    id: "JDF-R484",
    summary: "A trip other operators run is split by operator, each part under its operator's route",
};

/// What assembly decided, for [`edits`].
#[derive(Debug, Default)]
pub struct OperatorSplits {
    splits: Vec<Split>,
}

impl OperatorSplits {
    pub fn is_empty(&self) -> bool {
        self.splits.is_empty()
    }
}

#[derive(Debug)]
struct Split {
    trip: TripId,
    trip_id: PublicId,
    route: RouteId,
    /// The days the line's operator runs it.
    own: DateSet,
    /// The other operators, in `Altdop` order of their first row.
    parts: Vec<Part>,
}

#[derive(Debug)]
struct Part {
    agency: Agency,
    registration: Box<str>,
    distinction: u32,
    days: DateSet,
}

/// R483 for every published trip of a line with `Altdop` rows.
pub(crate) fn decide(
    merged: &Merged,
    routes: &[Option<RouteId>],
    published: &Published,
    jdf: &crate::Policy,
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> OperatorSplits {
    let holidays = Holidays::new(jdf, merged.horizon);
    let mut splits = Vec::new();
    for (v, version) in merged.versions.iter().enumerate() {
        let Some(route) = routes[v] else { continue };
        if version.agency_alternations.is_empty() {
            continue;
        }
        let rows: Vec<(&AgencyAlternation, DateSet)> = version
            .agency_alternations
            .iter()
            .map(|row| (row, row_days(row, merged, &holidays, jdf)))
            .collect();
        for trip in &published.versions[v] {
            let source = &version.trips[trip.trip];
            let mut remaining = source.days;
            let mut own = DateSet::EMPTY;
            let mut parts: Vec<(usize, DateSet)> = Vec::new();
            for &(row, days) in &rows {
                if row.trip_id != 0 && row.trip_id != source.id {
                    continue;
                }
                let matched = remaining & days;
                if matched.is_empty() {
                    continue;
                }
                remaining = remaining - matched;
                if row.agency == version.agency {
                    own = own | matched;
                } else if let Some((_, days)) = parts.iter_mut().find(|(a, _)| *a == row.agency) {
                    *days = *days | matched;
                } else {
                    parts.push((row.agency, matched));
                }
            }
            if parts.is_empty() {
                continue;
            }
            diagnostics.push(
                Diagnostic::new(
                    "jdf_trip_alternate_operator",
                    Severity::Info,
                    format!("{}/{}", version.subject(), source.id),
                    format!(
                        "run by {} other operator(s) on some days: split by operator",
                        parts.len()
                    ),
                )
                .rule(R483),
            );
            splits.push(Split {
                trip: trip.handle,
                trip_id: trip.id.clone(),
                route,
                own: own | remaining,
                parts: parts
                    .into_iter()
                    .map(|(agency, days)| {
                        let row = &merged.agencies[agency];
                        Part {
                            agency: super::lines::agency_row(row, policy),
                            registration: row.id.clone(),
                            distinction: row.distinction,
                            days,
                        }
                    })
                    .collect(),
            });
        }
    }
    OperatorSplits { splits }
}

/// R483: the days one `Altdop` row describes.
fn row_days(
    row: &AgencyAlternation,
    merged: &Merged,
    holidays: &Holidays,
    jdf: &crate::Policy,
) -> DateSet {
    let horizon = merged.horizon;
    let full = DateSet::full(horizon.days());
    let codes: Vec<&str> = merged
        .code_sets
        .get(row.codes)
        .iter()
        .map(|code| &**code)
        .collect();
    let by_codes = crate::calendar::day_attribute_days(&codes, holidays, jdf).unwrap_or(full);
    let weeks = match row
        .time_type
        .as_deref()
        .and_then(|code| jdf.calendar.note_types.get(code.trim()))
    {
        Some(NoteKind::OddWeeks) => holidays.odd_iso_weeks(),
        Some(NoteKind::EvenWeeks) => holidays.even_iso_weeks(),
        _ => full,
    };
    let period = match (row.date_from, row.date_to) {
        (Some(from), to) => DateSet::range(horizon, from, to.unwrap_or(from)),
        (None, Some(to)) => DateSet::range(horizon, horizon.start(), to),
        (None, None) => full,
    };
    by_codes & weeks & period
}

/// R484: the split, as edits, and each variant route with the route it is a variant of (what
/// belongs to a route's line belongs to its variants: the filtered feed's line selection).
pub fn edits(timetable: &Timetable, splits: &OperatorSplits) -> (Edits, Vec<(RouteId, RouteId)>) {
    let mut b = EditBuilder::new(timetable, "jdf-operators");
    let mut agencies: HashMap<PublicId, AgencyId> = timetable
        .agencies
        .iter()
        .map(|(handle, agency)| (agency.id.clone(), handle))
        .collect();
    let mut variants: HashMap<(RouteId, PublicId), RouteId> = HashMap::new();
    for split in &splits.splits {
        // The line's operator keeps the ID; without days of its own, the first other one.
        let keeper = split.own.is_empty().then_some(0);
        for (index, part) in split.parts.iter().enumerate() {
            let agency = match agencies.get(&part.agency.id) {
                Some(&agency) => agency,
                None => {
                    let agency = b.add_agency(R484, part.agency.clone());
                    agencies.insert(part.agency.id.clone(), agency);
                    agency
                }
            };
            let key = (split.route, part.agency.id.clone());
            let variant = match variants.get(&key) {
                Some(&variant) => variant,
                None => {
                    let variant = variant_route(&mut b, timetable, split.route, agency, part);
                    variants.insert(key, variant);
                    variant
                }
            };
            let service = b.service(part.days);
            let trip = if keeper == Some(index) {
                b.push(
                    R484,
                    Edit::SetTripService {
                        trip: split.trip,
                        service,
                    },
                );
                split.trip
            } else {
                let id = identity::jdf::operator_trip(
                    &split.trip_id,
                    &part.registration,
                    part.distinction,
                );
                b.split_trip(R484, split.trip, id, service)
            };
            b.push(
                R484,
                Edit::SetTripRoute {
                    trip,
                    route: variant,
                },
            );
        }
        if keeper.is_none() {
            let service = b.service(split.own);
            b.push(
                R484,
                Edit::SetTripService {
                    trip: split.trip,
                    service,
                },
            );
        }
    }
    let mut variants: Vec<(RouteId, RouteId)> = variants
        .into_iter()
        .map(|((route, _), variant)| (route, variant))
        .collect();
    variants.sort();
    (b.finish(), variants)
}

/// R484: the route under another operator, with what is stated for the route.
fn variant_route(
    b: &mut EditBuilder<'_>,
    timetable: &Timetable,
    route: RouteId,
    agency: AgencyId,
    part: &Part,
) -> RouteId {
    let original = timetable.routes.get(route).expect("a published route");
    let mut copy = original.clone();
    copy.id = identity::jdf::operator_route(&original.id, &part.registration, part.distinction);
    copy.agency = agency;
    let variant = b.add_route(R484, copy);
    for assignment in &timetable.assignments {
        if assignment.scope == Scope::Route(route) {
            let mut stated = assignment.clone();
            stated.scope = Scope::Route(variant);
            b.push(R484, Edit::AddAssignment(stated));
        }
    }
    for key in &timetable.keys {
        if key.entity == Entity::Route(route) {
            b.push(
                R484,
                Edit::AddKey(SourceKey {
                    entity: Entity::Route(variant),
                    ..key.clone()
                }),
            );
        }
    }
    variant
}
