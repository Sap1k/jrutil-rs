//! A batch's agencies, versions and their relations, referring to merged stops and agencies by
//! index (JDF-R238, R240–R242, R285).

use std::collections::{BTreeMap, HashMap};

use jr_model::{DateSet, Diagnostic, Diagnostics, Rule, Severity, Strings};

use super::{AgencyAlternation, Call, CodeSetId, CodeSets, RouteStop, Trip, Version, VersionKey};
use crate::batch::{self, AttributeCodes, Batch};

/// An operator row is the same agency as an earlier one of its IČO when every field but the
/// distinction is equal; otherwise it is a new variant numbered after the IČO's last. Rows that
/// refer to a batch's agency refer to the merged one (verdict: agency identity as in R238).
pub const R238: Rule = Rule {
    id: "JDF-R238",
    summary: "Agencies are deduplicated per IČO by equal fields; new variants are numbered",
};

/// A version keeps the facts it was published with: its batch and position (its identity; no
/// merged distinction exists), the batch's creation date and the published validity.
pub const R240: Rule = Rule {
    id: "JDF-R240",
    summary: "A version keeps its batch, position, creation date and published validity",
};

/// Each relation's rows follow their version (by its batch-local key) and refer to stops,
/// agencies and fixed codes by merged index.
pub const R241: Rule = Rule {
    id: "JDF-R241",
    summary: "Relation rows follow their version and refer to merged entities by index",
};

/// A trip of a grouped version carries its `SpojSkup` group as a fact; a grouped version's trip
/// without a known group is diagnosed (F# aborted). Trips of ungrouped versions carry none.
pub const R242: Rule = Rule {
    id: "JDF-R242",
    summary: "Trips of grouped versions carry their group; a missing group is diagnosed",
};

/// A row naming a version, trip, stop, agency or post's stop the batch does not have, or a
/// second version or trip with a key already taken, cannot be placed: it is dropped, diagnosed
/// once per batch and relation with the count (F# aborted the merge).
pub const R285: Rule = Rule {
    id: "JDF-R285",
    summary: "Rows with references the batch cannot resolve are dropped",
};

/// The merged agencies.
#[derive(Debug, Default)]
pub(crate) struct Agencies {
    pub rows: Vec<batch::Agency>,
    by_ico: HashMap<Box<str>, Vec<usize>>,
}

impl Agencies {
    /// R238 for one batch: batch-local key to merged index.
    fn add_batch(&mut self, agencies: Vec<batch::Agency>) -> HashMap<(Box<str>, u32), usize> {
        let mut map = HashMap::new();
        for agency in agencies {
            let local = (agency.id.clone(), agency.distinction);
            let variants = self.by_ico.entry(agency.id.clone()).or_default();
            let same = variants.iter().copied().find(|&index| {
                let existing = &self.rows[index];
                batch::Agency {
                    distinction: agency.distinction,
                    ..existing.clone()
                } == agency
            });
            let index = match same {
                Some(index) => index,
                None => {
                    let distinction = variants
                        .iter()
                        .map(|&index| self.rows[index].distinction)
                        .max()
                        .map_or(1, |last| last + 1);
                    let index = self.rows.len();
                    variants.push(index);
                    self.rows.push(batch::Agency {
                        distinction,
                        ..agency
                    });
                    index
                }
            };
            map.insert(local, index);
        }
        map
    }
}

pub(crate) struct Context<'c> {
    pub stop_map: &'c [usize],
    pub stop_position: &'c HashMap<u64, usize>,
    pub agencies: &'c mut Agencies,
    pub code_sets: &'c mut CodeSets,
    pub texts: &'c mut Strings,
    pub jdf: &'c crate::Policy,
    /// `Oznacniky` rows whose stop the batch does not have.
    pub unknown_posts: usize,
}

/// Counts of dropped rows by (relation, what was missing).
#[derive(Default)]
struct Unknown(BTreeMap<(&'static str, &'static str), usize>);

impl Unknown {
    fn add(&mut self, relation: &'static str, missing: &'static str) {
        *self.0.entry((relation, missing)).or_default() += 1;
    }
}

/// Adds one batch's versions and relations (in batch order) to `versions`.
pub(crate) fn add_batch(
    batch: Batch,
    context: &mut Context<'_>,
    versions: &mut Vec<Version>,
    diagnostics: &mut Diagnostics,
) {
    let Batch {
        name,
        header,
        agencies,
        routes,
        route_integrations,
        route_stops,
        trips,
        trip_groups,
        trip_stops,
        route_info,
        attribute_refs,
        service_notes,
        transfers,
        agency_alternations,
        alternate_route_names,
        reservations,
        ..
    } = batch;
    let codes = AttributeCodes::new(&attribute_refs);
    let mut unknown = Unknown::default();
    for _ in 0..context.unknown_posts {
        unknown.add("Oznacniky", "stop");
    }
    let agency_map = context.agencies.add_batch(agencies);
    let defaults = &context.jdf.version_defaults;

    // Versions, by their batch-local key.
    let first = versions.len();
    let mut by_key: HashMap<(Box<str>, u32), usize> = HashMap::new();
    for (position, route) in routes.into_iter().enumerate() {
        let Some(&agency) = agency_map.get(&(route.agency_id.clone(), route.agency_distinction))
        else {
            unknown.add("Linky", "agency");
            continue;
        };
        let key = (route.id.clone(), route.distinction);
        if by_key.contains_key(&key) {
            unknown.add("Linky", "unique licence and distinction");
            continue;
        }
        by_key.insert(key, versions.len());
        versions.push(Version {
            batch: name.clone(),
            position: position as u32,
            created: header.created,
            key: VersionKey {
                licence: route.id.clone(),
                valid_from: route.timetable_valid_from,
                detour: route.detour,
                hash: None,
                part: None,
            },
            route,
            agency,
            won: Vec::new(),
            dates: DateSet::EMPTY,
            regional_cross_border: false,
            integrations: Vec::new(),
            route_stops: Vec::new(),
            trips: Vec::new(),
            route_info: Vec::new(),
            notes: Vec::new(),
            transfers: Vec::new(),
            agency_alternations: Vec::new(),
            alternate_names: Vec::new(),
            reservations: Vec::new(),
        });
    }
    let version_of =
        |route: &str, distinction: u32| by_key.get(&(route.into(), distinction)).copied();

    let stop_of = |number: u64| -> Option<u32> {
        context
            .stop_position
            .get(&number)
            .map(|&at| context.stop_map[at] as u32)
    };
    let mut slot_sets: HashMap<Vec<Option<u32>>, CodeSetId> = HashMap::new();
    let mut code_set = |slots: &[Option<u32>], code_sets: &mut CodeSets| -> CodeSetId {
        if let Some(&id) = slot_sets.get(slots) {
            return id;
        }
        let id = code_sets.intern(codes.resolve(slots));
        slot_sets.insert(slots.to_vec(), id);
        id
    };

    for row in route_integrations {
        match version_of(&row.route_id, row.route_distinction) {
            Some(v) => versions[v].integrations.push(row),
            None => unknown.add("LinExt", "version"),
        }
    }
    for row in route_stops {
        let Some(v) = version_of(&row.route_id, row.route_distinction) else {
            unknown.add("Zaslinky", "version");
            continue;
        };
        let Some(stop) = stop_of(row.stop_id) else {
            unknown.add("Zaslinky", "stop");
            continue;
        };
        versions[v].route_stops.push(RouteStop {
            route_stop_id: row.route_stop_id,
            zone: row.zone,
            stop,
            approximate_minutes: row.approximate_minutes,
            codes: code_set(&row.attributes, context.code_sets),
        });
    }

    let groups: HashMap<u64, &batch::TripGroup> =
        trip_groups.iter().rev().map(|g| (g.id, g)).collect();
    let mut trip_index: HashMap<(usize, u64), usize> = HashMap::new();
    for row in trips {
        let Some(v) = version_of(&row.route_id, row.route_distinction) else {
            unknown.add("Spoje", "version");
            continue;
        };
        if trip_index.contains_key(&(v, row.id)) {
            unknown.add("Spoje", "unique trip number");
            continue;
        }
        let group = if versions[v].route.grouped {
            let group = row.trip_group_id.and_then(|id| groups.get(&id));
            if group.is_none() {
                diagnostics.push(
                    Diagnostic::new(
                        "jdf_trip_group_missing",
                        Severity::Warning,
                        format!("{}/{}", versions[v].subject(), row.id),
                        "a trip of a grouped version has no known trip group",
                    )
                    .rule(R242),
                );
            }
            group.map(|g| (*g).clone())
        } else {
            None
        };
        trip_index.insert((v, row.id), versions[v].trips.len());
        versions[v].trips.push(Trip {
            id: row.id,
            codes: code_set(&row.attributes, context.code_sets),
            group,
            days: DateSet::EMPTY,
            calls: Vec::new(),
        });
    }
    for row in trip_stops {
        let Some(v) = version_of(&row.route_id, row.route_distinction) else {
            unknown.add("Zasspoje", "version");
            continue;
        };
        let Some(&t) = trip_index.get(&(v, row.trip_id)) else {
            unknown.add("Zasspoje", "trip");
            continue;
        };
        let Some(stop) = stop_of(row.stop_id) else {
            unknown.add("Zasspoje", "stop");
            continue;
        };
        let call = Call {
            stop,
            codes: code_set(&row.attributes, context.code_sets),
            route_stop_id: row.route_stop_id,
            stop_post_id: row.stop_post_id,
            stop_post_number: context.texts.intern_opt(row.stop_post_number.as_deref()),
            km: row.km,
            arrival: row.arrival,
            departure: row.departure,
            earliest_arrival: row.earliest_arrival,
            latest_departure: row.latest_departure,
        };
        versions[v].trips[t].calls.push(call);
    }
    for row in route_info {
        match version_of(&row.route_id, row.route_distinction) {
            Some(v) => versions[v].route_info.push(row),
            None => unknown.add("Udaje", "version"),
        }
    }
    for row in service_notes {
        match version_of(&row.route_id, row.route_distinction) {
            Some(v) => versions[v].notes.push(row),
            None => unknown.add("Caskody", "version"),
        }
    }
    for row in transfers {
        match version_of(&row.route_id, row.route_distinction) {
            Some(v) => versions[v].transfers.push(row),
            None => unknown.add("Navaznosti", "version"),
        }
    }
    for row in reservations {
        match version_of(&row.route_id, row.route_distinction) {
            Some(v) => versions[v].reservations.push(row),
            None => unknown.add("Mistenky", "version"),
        }
    }
    // Altdop and Altlinky keep their optional distinctions (batch module); the 1.8/1.9 default
    // (R028) keys them here.
    for row in agency_alternations {
        let distinction = row.route_distinction.unwrap_or(defaults.route_distinction);
        let Some(v) = version_of(&row.route_id, distinction) else {
            unknown.add("Altdop", "version");
            continue;
        };
        let agency_distinction = row
            .agency_distinction
            .unwrap_or(defaults.agency_distinction);
        let Some(&agency) = agency_map.get(&(row.agency_id.clone(), agency_distinction)) else {
            unknown.add("Altdop", "agency");
            continue;
        };
        versions[v].agency_alternations.push(AgencyAlternation {
            trip_id: row.trip_id,
            agency,
            codes: code_set(&row.attributes, context.code_sets),
            time_type: row.time_type,
            reserved1: row.reserved1,
            date_from: row.date_from,
            date_to: row.date_to,
        });
    }
    for row in alternate_route_names {
        let distinction = row.route_distinction.unwrap_or(defaults.route_distinction);
        match version_of(&row.route_id, distinction) {
            Some(v) => versions[v].alternate_names.push(row),
            None => unknown.add("Altlinky", "version"),
        }
    }
    debug_assert!(versions[first..].iter().all(|v| *v.batch == *name));

    for ((relation, missing), count) in unknown.0 {
        diagnostics.push(
            Diagnostic::new(
                "jdf_reference_unknown",
                Severity::Warning,
                &*name,
                format!(
                    "{count} {relation} rows name a {missing} the batch does not have: dropped"
                ),
            )
            .rule(R285),
        );
    }
}
