//! Notes, features, connection claims and travel restrictions (JDF-R452–R465).

use std::collections::{BTreeSet, HashMap};

use jr_model::identity::{self, PublicId, escape};
use jr_model::timetable::{
    Assignment, AssignmentKind, ClaimDerivation, ClaimDirection, ClaimResolution, ConnectionClaim,
    Feature, Note, NoteKind, Restriction, RestrictionGroup, RestrictionScope, Scope,
};
use jr_model::{Diagnostic, Diagnostics, RouteId, Rule, Severity, Timetable, TripId};

use super::Policy;
use super::calls::{Published, PublishedTrip};
use super::lines::Lines;
use crate::merge::{Merged, Version};

/// `Udaje` rows with text are route information notes of the version's route; a blank one is
/// diagnosed. Versions of one route keep their own notes; two rows of one number with different
/// texts cannot share the ID the verdict prescribes, so the later is diagnosed and dropped.
pub const R452: Rule = Rule {
    id: "JDF-R452",
    summary: "Udaje rows are route information notes of their route",
};

/// `Caskody` rows with a text or a designation are timetable notes of their trip (label =
/// designation, text = note, validity = the row's dates, type = the type's name). A typed row
/// without text, which also shapes the calendar, is published too: the printed timetable's mark
/// with its days (`71`: does not run 24.12.) is what passengers are shown (B4 fixed,
/// `docs/roadmap.md`).
pub const R453: Rule = Rule {
    id: "JDF-R453",
    summary: "Caskody rows are trip notes, typed designations included",
};

/// `Mistenky` rows are reservation notes of their trip, numbered by their place among the
/// trip's rows; rows for trip 0 are notes of the whole route (B3 fixed, `docs/roadmap.md`).
pub const R454: Rule = Rule {
    id: "JDF-R454",
    summary: "Mistenky rows are reservation notes of their trip, or of the route for trip 0",
};

/// Every note is linked to its route or trip by a note assignment; its ID is built from the
/// owner's ID, its kind and the source row's own number (Q14).
pub const R455: Rule = Rule {
    id: "JDF-R455",
    summary: "Notes are linked to their owner and named after it and their row",
};

/// A published trip's fixed codes: calendar codes (`X`, `+`, day numbers) are calendar
/// designations, the others with a meaning in the policy table trip features; the exclusion
/// codes are restrictions, not features.
pub const R457: Rule = Rule {
    id: "JDF-R457",
    summary: "Trip codes are calendar designations or trip features",
};

/// Every integrated system a route's versions belong to (`LinExt`) is a route feature
/// `integrated_transport` with the system's number as its code.
pub const R458: Rule = Rule {
    id: "JDF-R458",
    summary: "LinExt systems are integrated-transport route features",
};

/// `Navaznosti` rows are connection claims at the trip's first published call of the route
/// stop: `m` waits for, `M` connects to, structured and unresolved. A claim whose trip is not
/// published, whose call is not served or cannot be found (no `origin_sequence = 0` fallback,
/// Q13), or whose kind is no JDF letter is dropped, diagnosed.
pub const R460: Rule = Rule {
    id: "JDF-R460",
    summary: "Navaznosti rows are claims at their published call; others are dropped",
};

/// The claim targets are CIS register numbers kept as source codes: the reader gives them as
/// numbers, so a target licence is zero-padded to six digits and `0` (the merged files'
/// placeholder) is absent.
pub const R461: Rule = Rule {
    id: "JDF-R461",
    summary: "Claim targets are source codes; the target licence is six digits; 0 is absent",
};

/// Exclusion codes `§`, `A`, `B`, `C` are restriction groups (policy table).
pub const R462: Rule = Rule {
    id: "JDF-R462",
    summary: "Exclusion codes are restriction groups",
};

/// A group written on a version's route stop restricts every published call of its trips at
/// that route stop; one with no such call is diagnosed.
pub const R463: Rule = Rule {
    id: "JDF-R463",
    summary: "Route-stop exclusion codes restrict the version's calls at that stop",
};

/// A group written on a trip's call restricts that call (fixes serving question 9); on an
/// unserved call or a trip not published it is dropped, diagnosed.
pub const R464: Rule = Rule {
    id: "JDF-R464",
    summary: "Call exclusion codes restrict that call",
};

/// A group reaching only one route stop of a published trip restricts nothing: diagnosed.
pub const R465: Rule = Rule {
    id: "JDF-R465",
    summary: "A restriction group with one route stop on a trip is diagnosed",
};

fn text(value: Option<&str>) -> Option<Box<str>> {
    value
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(Box::from)
}

/// What a note says, apart from its ID and where it came from.
type NoteContent = (NoteKind, Option<Box<str>>, Option<Box<str>>);

struct Notes<'t> {
    timetable: &'t mut Timetable,
    /// The contents published under each note number, in order of first appearance.
    ids: HashMap<PublicId, Vec<NoteContent>>,
}

impl Notes<'_> {
    /// Adds a note and its link. The versions of a line published as one route number their
    /// notes independently: a note saying what one already published under its number says is
    /// that note, published once; one saying something else is a further note under the
    /// number (`identity::jdf::note_variant`), so no text is lost.
    fn add(&mut self, mut note: Note, scope: Scope) {
        let content = (note.kind, note.label.clone(), note.text.clone());
        let known = self.ids.entry(note.id.clone()).or_default();
        if known.contains(&content) {
            return;
        }
        known.push(content);
        if known.len() > 1 {
            note.id = identity::jdf::note_variant(&note.id, known.len());
        }
        let source_object = note.source_object.clone();
        let handle = self.timetable.notes.push(note);
        self.timetable.assignments.push(Assignment {
            scope,
            kind: AssignmentKind::Note,
            source_code: None,
            note: Some(handle),
            source_object: Some(source_object),
        });
    }
}

fn dropped(subject: String, what: &str, rule: Rule) -> Diagnostic {
    Diagnostic::new(
        "jdf_enrichment_dropped",
        Severity::Warning,
        subject,
        format!("{what} of a trip or route that is not published"),
    )
    .rule(rule)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build(
    merged: &Merged,
    lines: &Lines,
    routes: &[Option<RouteId>],
    published: &Published,
    policy: &Policy,
    timetable: &mut Timetable,
    diagnostics: &mut Diagnostics,
) {
    let mut notes = Notes {
        timetable,
        ids: HashMap::new(),
    };
    for (v, version) in merged.versions.iter().enumerate() {
        let trips: HashMap<u64, &PublishedTrip> = published.versions[v]
            .iter()
            .map(|p| (version.trips[p.trip].id, p))
            .collect();
        let route = routes[v].map(|handle| (handle, notes.timetable.routes[handle].id.clone()));
        version_notes(
            version,
            route.as_ref(),
            &trips,
            policy,
            &mut notes,
            diagnostics,
        );
        let timetable = &mut *notes.timetable;
        trip_features(merged, version, &published.versions[v], policy, timetable);
        claims(version, &trips, policy, timetable, diagnostics);
        if let Some((handle, route_id)) = &route {
            restrictions(
                merged,
                version,
                *handle,
                route_id,
                &published.versions[v],
                policy,
                timetable,
                diagnostics,
            );
        }
    }
    integrations(merged, lines, routes, notes.timetable);
}

fn version_notes(
    version: &Version,
    route: Option<&(RouteId, PublicId)>,
    trips: &HashMap<u64, &PublishedTrip>,
    policy: &Policy,
    notes: &mut Notes<'_>,
    diagnostics: &mut Diagnostics,
) {
    let subject = version.subject();
    let p = &policy.notes;
    for row in &version.route_info {
        let source_object = format!("{subject}:Udaje:{}", row.id);
        let Some((handle, route_id)) = route else {
            diagnostics.push(dropped(source_object, "route information", R452));
            continue;
        };
        let Some(body) = text(Some(&row.text)) else {
            diagnostics.push(
                Diagnostic::new(
                    "jdf_note_blank",
                    Severity::Warning,
                    source_object,
                    "blank note",
                )
                .rule(R452),
            );
            continue;
        };
        let note = Note {
            id: identity::jdf::note(route_id, &p.route_kind, row.id),
            kind: NoteKind::RouteInformation,
            label: None,
            text: Some(body),
            valid_from: None,
            valid_to: None,
            note_type: None,
            source_object: source_object.into(),
        };
        notes.add(note, Scope::Route(*handle));
    }
    for row in &version.notes {
        let source_object = format!("{subject}/{}:Caskody:{}", row.trip_id, row.id);
        let body = text(row.note.as_deref());
        let label = text(Some(&row.designation));
        if body.is_none() && label.is_none() {
            diagnostics.push(
                Diagnostic::new(
                    "jdf_note_blank",
                    Severity::Warning,
                    source_object,
                    "blank note",
                )
                .rule(R453),
            );
            continue;
        }
        let Some(trip) = trips.get(&row.trip_id) else {
            diagnostics.push(dropped(source_object, "timetable note", R453));
            continue;
        };
        let note = Note {
            id: identity::jdf::note(&trip.id, &p.trip_kind, row.id),
            kind: NoteKind::TimetableNote,
            label,
            text: body,
            valid_from: row.date_from,
            valid_to: row.date_to,
            note_type: row
                .note_type
                .as_ref()
                .and_then(|t| p.note_types.get(t.code()).cloned()),
            source_object: source_object.into(),
        };
        notes.add(note, Scope::Trip(trip.handle));
    }
    let mut ordinals: HashMap<u64, u64> = HashMap::new();
    for row in &version.reservations {
        let ordinal = ordinals.entry(row.trip_id).or_default();
        *ordinal += 1;
        let source_object = format!("{subject}/{}:Mistenky:{ordinal}", row.trip_id);
        let Some(body) = text(Some(&row.note)) else {
            diagnostics.push(
                Diagnostic::new(
                    "jdf_note_blank",
                    Severity::Warning,
                    source_object,
                    "blank note",
                )
                .rule(R454),
            );
            continue;
        };
        let (owner_id, scope) = if row.trip_id == 0 {
            let Some((handle, route_id)) = route else {
                diagnostics.push(dropped(source_object, "reservation note", R454));
                continue;
            };
            (route_id, Scope::Route(*handle))
        } else {
            let Some(trip) = trips.get(&row.trip_id) else {
                diagnostics.push(dropped(source_object, "reservation note", R454));
                continue;
            };
            (&trip.id, Scope::Trip(trip.handle))
        };
        let note = Note {
            id: identity::jdf::note(owner_id, &p.reservation_kind, *ordinal),
            kind: NoteKind::ReservationNote,
            label: None,
            text: Some(body),
            valid_from: None,
            valid_to: None,
            note_type: None,
            source_object: source_object.into(),
        };
        notes.add(note, scope);
    }
}

/// R457.
fn trip_features(
    merged: &Merged,
    version: &Version,
    published: &[PublishedTrip],
    policy: &Policy,
    timetable: &mut Timetable,
) {
    for trip in published {
        let mut seen = BTreeSet::new();
        for code in merged.code_sets.get(version.trips[trip.trip].codes) {
            if !seen.insert(code) {
                continue;
            }
            let kind = if policy.codes.is_calendar(code) {
                AssignmentKind::CalendarDesignation
            } else if let Some(&feature) = policy.trip_features.get(code) {
                AssignmentKind::Feature(feature)
            } else {
                continue;
            };
            timetable.assignments.push(Assignment {
                scope: Scope::Trip(trip.handle),
                kind,
                source_code: Some(code.clone()),
                note: None,
                source_object: Some(format!("{}:attribute:{}", trip.id, escape(code)).into()),
            });
        }
    }
}

/// R458.
fn integrations(
    merged: &Merged,
    lines: &Lines,
    routes: &[Option<RouteId>],
    timetable: &mut Timetable,
) {
    for group in &lines.groups {
        let Some(route) = group.members.iter().find_map(|&v| routes[v]) else {
            continue;
        };
        let systems: BTreeSet<u64> = group
            .members
            .iter()
            .flat_map(|&v| merged.versions[v].integrations.iter())
            .map(|row| row.transport_system_id)
            .collect();
        for system in systems {
            timetable.assignments.push(Assignment {
                scope: Scope::Route(route),
                kind: AssignmentKind::Feature(Feature::IntegratedTransport),
                source_code: Some(system.to_string().into()),
                note: None,
                source_object: Some(format!("{}:integration:{system}", group.id).into()),
            });
        }
    }
}

/// The published sequence of the first call at a route stop; `Err(true)` when the route stop's
/// calls are all unserved, `Err(false)` when the trip has none.
fn first_sequence(trip: &PublishedTrip, route_stop: u64) -> Result<i32, bool> {
    let mut seen = false;
    for &(rs, sequence, served) in &trip.ordered {
        if rs == route_stop {
            if served {
                return Ok(sequence);
            }
            seen = true;
        }
    }
    Err(seen)
}

/// R460, R461.
fn claims(
    version: &Version,
    trips: &HashMap<u64, &PublishedTrip>,
    policy: &Policy,
    timetable: &mut Timetable,
    diagnostics: &mut Diagnostics,
) {
    let subject = version.subject();
    let mut ordinals: HashMap<u64, u64> = HashMap::new();
    let target = |value: Option<u64>| value.filter(|&v| v != 0).map(|v| v.to_string().into());
    for row in &version.transfers {
        let ordinal = ordinals.entry(row.trip_id).or_default();
        *ordinal += 1;
        let source = format!("{subject}/{}:Navaznosti:{ordinal}", row.trip_id);
        let warn = |code, message: String| {
            Diagnostic::new(code, Severity::Warning, source.clone(), message).rule(R460)
        };
        let Some(trip) = trips.get(&row.trip_id) else {
            diagnostics.push(dropped(source, "connection claim", R460));
            continue;
        };
        let direction = if row.kind == policy.claims.waits_for {
            ClaimDirection::WaitsFor
        } else if row.kind == policy.claims.connects_to {
            ClaimDirection::ConnectsTo
        } else {
            diagnostics.push(warn(
                "jdf_claim_kind_unknown",
                format!("transfer kind {:?} is no JDF kind: dropped", row.kind),
            ));
            continue;
        };
        let sequence = match first_sequence(trip, row.route_stop_id) {
            Ok(sequence) => sequence,
            Err(true) => {
                diagnostics.push(warn(
                    "jdf_claim_call_unserved",
                    format!("route stop {} is not served: dropped", row.route_stop_id),
                ));
                continue;
            }
            Err(false) => {
                diagnostics.push(warn(
                    "jdf_claim_call_unknown",
                    format!(
                        "the trip has no call at route stop {}: dropped",
                        row.route_stop_id
                    ),
                ));
                continue;
            }
        };
        timetable.connection_claims.push(ConnectionClaim {
            direction,
            origin_trip: trip.handle,
            origin_sequence: sequence,
            dates: None,
            target_source_route: row
                .transfer_route_id
                .filter(|&v| v != 0)
                .map(|v| format!("{v:0width$}", width = policy.keys.licence_digits).into()),
            target_source_trip: None,
            target_source_stop: target(row.transfer_stop_id),
            target_source_post: target(row.transfer_stop_post_id),
            target_source_end_stop: target(row.transfer_end_stop_id),
            target_source_end_post: target(row.transfer_end_stop_post_id),
            wait_minutes: row.wait_minutes.and_then(|m| i32::try_from(m).ok()),
            note: text(row.note.as_deref()),
            target_public_line: None,
            target_destination_text: None,
            derivation: ClaimDerivation::Structured,
            resolution: ClaimResolution::Unresolved,
            target_route: None,
            target_trip: None,
            target_location: None,
            source_object: format!("{}:transfer:{ordinal}", trip.id).into(),
        });
    }
}

/// R462: the groups among a code list.
fn groups(codes: &[Box<str>], policy: &Policy) -> BTreeSet<RestrictionGroup> {
    codes
        .iter()
        .filter_map(|code| policy.codes.restrictions.get(code).copied())
        .collect()
}

/// R463–R465.
#[allow(clippy::too_many_arguments)]
fn restrictions(
    merged: &Merged,
    version: &Version,
    route: RouteId,
    route_id: &PublicId,
    published: &[PublishedTrip],
    policy: &Policy,
    timetable: &mut Timetable,
    diagnostics: &mut Diagnostics,
) {
    let subject = version.subject();
    let mut route_stop_groups: HashMap<u64, BTreeSet<RestrictionGroup>> = HashMap::new();
    for row in &version.route_stops {
        let found = groups(merged.code_sets.get(row.codes), policy);
        if found.is_empty() {
            continue;
        }
        route_stop_groups
            .entry(row.route_stop_id)
            .or_default()
            .extend(found.iter().copied());
        for group in found {
            let calls: Vec<(TripId, i32)> = published
                .iter()
                .flat_map(|trip| {
                    trip.ordered
                        .iter()
                        .filter(|&&(rs, _, served)| rs == row.route_stop_id && served)
                        .map(|&(_, sequence, _)| (trip.handle, sequence))
                })
                .collect();
            let source = format!(
                "{route_id}:route-stop:{}:{}",
                row.route_stop_id,
                group.as_str()
            );
            if calls.is_empty() {
                diagnostics.push(
                    Diagnostic::new(
                        "jdf_restriction_unplaced",
                        Severity::Warning,
                        source,
                        format!("{subject}: no published call at the route stop"),
                    )
                    .rule(R463),
                );
                continue;
            }
            timetable.restrictions.push(Restriction {
                group,
                scope: RestrictionScope::RouteStop {
                    route,
                    calls: calls.into(),
                },
                source_route_stop: row.route_stop_id.to_string().into(),
                source_object: source.into(),
            });
        }
    }
    let by_trip: HashMap<usize, &PublishedTrip> = published.iter().map(|p| (p.trip, p)).collect();
    for (index, trip) in version.trips.iter().enumerate() {
        let published_trip = by_trip.get(&index);
        let mut reached: HashMap<RestrictionGroup, BTreeSet<u64>> = HashMap::new();
        for call in &trip.calls {
            let call_groups = groups(merged.code_sets.get(call.codes), policy);
            let source = |group: RestrictionGroup| {
                format!(
                    "{subject}/{}:restriction:{}:{}",
                    trip.id,
                    call.route_stop_id,
                    group.as_str()
                )
            };
            let Some(published_trip) = published_trip else {
                for group in call_groups {
                    diagnostics.push(dropped(source(group), "travel restriction", R464));
                }
                continue;
            };
            let sequence = first_sequence(published_trip, call.route_stop_id);
            if sequence.is_ok() {
                let mut effective = call_groups.clone();
                effective.extend(
                    route_stop_groups
                        .get(&call.route_stop_id)
                        .into_iter()
                        .flatten(),
                );
                for group in effective {
                    reached.entry(group).or_default().insert(call.route_stop_id);
                }
            }
            for group in call_groups {
                match sequence {
                    Ok(sequence) => timetable.restrictions.push(Restriction {
                        group,
                        scope: RestrictionScope::Call {
                            trip: published_trip.handle,
                            sequence,
                        },
                        source_route_stop: call.route_stop_id.to_string().into(),
                        source_object: format!(
                            "{}:restriction:{}:{}",
                            published_trip.id,
                            call.route_stop_id,
                            group.as_str()
                        )
                        .into(),
                    }),
                    Err(_) => diagnostics.push(
                        Diagnostic::new(
                            "jdf_restriction_call_unserved",
                            Severity::Warning,
                            source(group),
                            "the restricted call is not served: dropped",
                        )
                        .rule(R464),
                    ),
                }
            }
        }
        if let Some(published_trip) = published_trip {
            let mut singles: Vec<RestrictionGroup> = reached
                .iter()
                .filter(|(_, stops)| stops.len() == 1)
                .map(|(group, _)| *group)
                .collect();
            singles.sort();
            for group in singles {
                diagnostics.push(
                    Diagnostic::new(
                        "jdf_restriction_singleton",
                        Severity::Warning,
                        format!(
                            "{}:restriction-group:{}",
                            published_trip.id,
                            escape(group.as_str())
                        ),
                        "the restriction group reaches only one published call",
                    )
                    .rule(R465),
                );
            }
        }
    }
}
