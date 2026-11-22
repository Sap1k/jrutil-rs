//! Matching source stop groups to base stop places by reviewed overrides, names and distance,
//! and coordinates (OVL-R040–R043).

use std::collections::BTreeSet;

use jr_formats::geodata::stop_overrides;
use jr_model::{Diagnostic, Diagnostics, Horizon, PlaceId, Rule, Severity};
use rayon::prelude::*;

use super::base::{BaseView, point_set_distance};
use super::names::Names;
use super::source::{MatchMethod, PlaceRef, PreparedSource, StopGroup, StopMatch};

/// A reviewed override names a group's base place: by its stable name key
/// (`jdf-stop-place-name`, exactly one base place) or by its ID (`jdf-stop-place`). Rows whose
/// validity overlaps the horizon at all apply. One distinct target places the group (no
/// distance check); a target that does not resolve, several targets or an unsupported namespace
/// leave the group unplaced (B11 fixed: no fall-through to automatic matching).
///
/// Some source platforms lie beyond the automatic radius of their national stop ("PID
/// platforms remain just beyond the automatic 125 m radius").
pub const OVL_R040: Rule = Rule {
    id: "OVL-R040",
    summary: "a reviewed override places a group at the base place it names",
};

/// An override whose source group is not a scheduled group of the source is diagnosed.
///
/// A renamed or retired source stop leaves its review row behind.
pub const OVL_R041: Rule = Rule {
    id: "OVL-R041",
    summary: "an override for a group the source does not schedule is diagnosed",
};

/// Base places with a name that ranks (R033) and a point within the profile's distance of a
/// group point are candidates; those of the best rank remain. One places the group, at the
/// smallest point distance; several leave it unplaced (ambiguous) and stop the tiers.
pub const OVL_R042: Rule = Rule {
    id: "OVL-R042",
    summary: "a unique best-ranked name within the radius places a group",
};

/// With the profile's coordinate identity tier: base places with a point within its maximum
/// distance, nearest first (ties by ID). One places the group; of several the nearest does when
/// the runner-up is at least the margin farther; otherwise the group stays unplaced.
///
/// Regional names can differ from CIS names entirely while the coordinates coincide.
pub const OVL_R043: Rule = Rule {
    id: "OVL-R043",
    summary: "a unique nearest base place within the identity distance places a group",
};

pub const RULES: &[Rule] = &[OVL_R040, OVL_R041, OVL_R042, OVL_R043];

/// The override namespaces (R040).
const BY_NAME: &str = "jdf-stop-place-name";
const BY_ID: &str = "jdf-stop-place";

/// Places every group the initial tiers can (R040–R043).
pub fn match_groups(
    source: &mut PreparedSource<'_>,
    base: &BaseView<'_>,
    names: &Names,
    diagnostics: &mut Diagnostics,
) {
    let source_id = source.profile.source_id.to_string();
    let horizon = base.timetable.horizon;
    let known: BTreeSet<&str> = source.groups.iter().map(|g| &*g.local_id).collect();
    let mut reported = BTreeSet::new();
    for row in &source.overrides {
        if !known.contains(&*row.source_id) && reported.insert(&*row.source_id) {
            diagnostics.push(
                Diagnostic::new(
                    "stop_override_source_missing",
                    Severity::Warning,
                    format!("{source_id}:{}", row.source_id),
                    "No scheduled source stop group has this ID",
                )
                .rule(OVL_R041),
            );
        }
    }
    let profile = source.profile;
    let overrides = &source.overrides;
    let outcomes: Vec<(Option<StopMatch>, Vec<Diagnostic>)> = source
        .groups
        .par_iter()
        .map(|group| {
            let subject = group.registry_key(&source_id);
            let rows: Vec<&stop_overrides::Row> = overrides
                .iter()
                .filter(|row| row.source_id == group.local_id && in_force(row, horizon))
                .collect();
            let mut found = Vec::new();
            let placed = if rows.is_empty() {
                automatic(group, profile, base, names, &subject, &mut found)
            } else {
                reviewed(&rows, base, names, &subject, &mut found)
            };
            (placed, found)
        })
        .collect();
    for (group, (placed, found)) in source.groups.iter_mut().zip(outcomes) {
        group.matched = placed;
        for diagnostic in found {
            diagnostics.push(diagnostic);
        }
    }
}

fn in_force(row: &stop_overrides::Row, horizon: Horizon) -> bool {
    row.valid_from.is_none_or(|from| from <= horizon.end())
        && row.valid_to.is_none_or(|to| to >= horizon.start())
}

/// R040.
fn reviewed(
    rows: &[&stop_overrides::Row],
    base: &BaseView<'_>,
    names: &Names,
    subject: &str,
    found: &mut Vec<Diagnostic>,
) -> Option<StopMatch> {
    let mut targets: Vec<PlaceId> = Vec::new();
    let mut failed = false;
    let mut report = |code: &'static str, message: String| {
        found.push(Diagnostic::new(code, Severity::Warning, subject, message).rule(OVL_R040));
    };
    for row in rows {
        let target = match &*row.target_namespace {
            BY_NAME => {
                let key = names.stable_key(&row.target_id);
                let places: Vec<PlaceId> = base
                    .places_by_stable_key(&key)
                    .map(|place| place.place)
                    .collect();
                match places.as_slice() {
                    [place] => Some(*place),
                    [] => {
                        report(
                            "stop_override_target_missing",
                            format!("Stable target name not found: {}", row.target_id),
                        );
                        None
                    }
                    _ => {
                        report(
                            "stop_override_target_ambiguous",
                            format!(
                                "Stable target name {} resolves to {} base stop places",
                                row.target_id,
                                places.len()
                            ),
                        );
                        None
                    }
                }
            }
            BY_ID => {
                let place = base.places.iter().find(|place| {
                    base.timetable.places[place.place].id.as_str() == &*row.target_id
                });
                if place.is_none() {
                    report("stop_override_target_missing", row.target_id.to_string());
                }
                place.map(|place| place.place)
            }
            other => {
                report("stop_override_namespace_unsupported", other.to_owned());
                None
            }
        };
        match target {
            Some(place) if !targets.contains(&place) => targets.push(place),
            Some(_) => {}
            None => failed = true,
        }
    }
    match targets.as_slice() {
        [place] if !failed => Some(StopMatch {
            target: PlaceRef::Base(*place),
            method: MatchMethod::ReviewedOverride,
            distance: None,
            corrected: false,
        }),
        [_, _, ..] => {
            report(
                "stop_override_ambiguous",
                "Multiple reviewed bindings overlap the GVD".to_owned(),
            );
            None
        }
        _ => None,
    }
}

/// R042, R043.
fn automatic(
    group: &StopGroup,
    profile: &crate::profile::SourceProfile,
    base: &BaseView<'_>,
    names: &Names,
    subject: &str,
    found: &mut Vec<Diagnostic>,
) -> Option<StopMatch> {
    let maximum = profile.stops.maximum_distance_metres;
    // R042.
    let mut ranked: Vec<(PlaceId, u32, f64)> = Vec::new();
    for place in base.places_near(&group.points, maximum) {
        let Some(distance) = point_set_distance(&group.points, &place.points) else {
            continue;
        };
        if distance > maximum {
            continue;
        }
        if let Some(rank) = names.rank(&group.name, &place.name) {
            ranked.push((place.place, rank, distance));
        }
    }
    if let Some(best) = ranked.iter().map(|(_, rank, _)| *rank).min() {
        ranked.retain(|(_, rank, _)| *rank == best);
        if let [(place, _, distance)] = ranked.as_slice() {
            return Some(StopMatch {
                target: PlaceRef::Base(*place),
                method: MatchMethod::NameGeoUnique,
                distance: Some(*distance),
                corrected: false,
            });
        }
        found.push(
            Diagnostic::new(
                "stop_match_ambiguous",
                Severity::Warning,
                subject,
                format!("{} name/geography candidates", ranked.len()),
            )
            .rule(OVL_R042),
        );
        return None;
    }
    // R043.
    let identity = profile
        .stops
        .coordinate_identity
        .filter(|_| !group.points.is_empty());
    let mut candidates: Vec<(f64, &str, PlaceId, &str)> = Vec::new();
    if let Some(identity) = identity {
        for place in base.places_near(&group.points, identity.maximum_metres) {
            if let Some(distance) = point_set_distance(&group.points, &place.points)
                && distance <= identity.maximum_metres
            {
                let id = base.timetable.places[place.place].id.as_str();
                candidates.push((distance, id, place.place, &place.name));
            }
        }
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(b.1)));
        let chosen = match candidates.as_slice() {
            [] => None,
            [only] => Some(only),
            [best, runner, ..] => {
                (runner.0 - best.0 >= identity.minimum_margin_metres).then_some(best)
            }
        };
        if let Some(&(distance, _, place, name)) = chosen {
            found.push(
                Diagnostic::new(
                    "stop_coordinate_identity_selected",
                    Severity::Info,
                    subject,
                    format!("{} -> {name} at {distance:.1} m", group.name),
                )
                .rule(OVL_R043),
            );
            return Some(StopMatch {
                target: PlaceRef::Base(place),
                method: MatchMethod::CoordinateIdentityUnique,
                distance: Some(distance),
                corrected: false,
            });
        }
        if !candidates.is_empty() {
            found.push(
                Diagnostic::new(
                    "stop_match_ambiguous",
                    Severity::Warning,
                    subject,
                    format!(
                        "No coordinate candidate has the required {:.1} m margin",
                        identity.minimum_margin_metres
                    ),
                )
                .rule(OVL_R043),
            );
            return None;
        }
    }
    found.push(
        Diagnostic::new(
            "stop_match_unresolved",
            Severity::Warning,
            subject,
            "No unique name/geography or coordinate-identity candidate",
        )
        .rule(OVL_R043),
    );
    None
}
