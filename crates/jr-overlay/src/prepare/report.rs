//! The review reports of preparation, in the F# column sets and value formats
//! (`stop_group_matches.csv`, `stop_context_inference.csv`, section 3.16) and the registry's
//! candidate rows (R048). Source group and stop IDs are written namespaced (`<source>:<id>`).

use std::collections::HashSet;

use jr_model::PlaceId;
use jr_model::timetable::Precision;
use sha2::{Digest, Sha256};

use super::Prepared;
use super::native;
use super::source::{PlaceRef, PreparedSource};
use crate::report::ReportTable;

fn sha256(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A base place's name as the package publishes it: with the base profile's suffix when its
/// position is estimated.
fn published_name(prepared: &Prepared<'_>, place: PlaceId) -> String {
    let base = &prepared.base;
    let row = &base.timetable.places[place];
    if row
        .position
        .is_some_and(|p| p.precision == Precision::Estimated)
    {
        format!("{}{}", row.name, base.profile.estimated_name_suffix)
    } else {
        row.name.to_string()
    }
}

/// One row per member stop of every scheduled group: sources in ID order, groups in order of
/// their first member in `stops.txt`, members in file order.
pub fn stop_group_matches(prepared: &Prepared<'_>) -> ReportTable {
    let base = &prepared.base;
    let mut rows = Vec::new();
    for source in &prepared.sources {
        let source_id = source.source_id();
        for group in &source.groups {
            let (target, name, digest) = match group.matched.map(|matched| matched.target) {
                Some(PlaceRef::Native(index)) => {
                    let place = &source.native_places[index as usize];
                    let key = prepared.names.stable_key(&group.name);
                    (
                        place.published_id().to_owned(),
                        group.name.to_string(),
                        sha256(&key),
                    )
                }
                Some(PlaceRef::Base(place)) => match base.place(place) {
                    Some(view) => {
                        let id = base.timetable.places[place].id.to_string();
                        let coordinate = view
                            .position
                            .map_or_else(String::new, |p| format!("{},{}", p.lat, p.lon));
                        let name = published_name(prepared, place);
                        (
                            id,
                            name,
                            sha256(&format!("{}|{coordinate}", view.stable_key)),
                        )
                    }
                    None => (
                        base.timetable.places[place].id.to_string(),
                        String::new(),
                        String::new(),
                    ),
                },
                None => (String::new(), String::new(), String::new()),
            };
            let method = group.matched.map_or_else(String::new, |matched| {
                let name = matched.method.as_str();
                if matched.corrected && !name.contains("gtfs_authoritative") {
                    format!("{name}+gtfs_authoritative")
                } else {
                    name.to_owned()
                }
            });
            let (status, disposition) = match group.matched {
                None => ("quarantined", "none"),
                Some(_) if method.contains("gtfs_authoritative") => {
                    ("corrected", "pid_name_and_coordinates")
                }
                Some(matched) if matched.method == super::source::MatchMethod::SourceNative => {
                    ("native", "pid_native")
                }
                Some(_) => ("matched", "matched_identity"),
            };
            let distance = group
                .matched
                .and_then(|matched| matched.distance)
                .map_or_else(String::new, |d| format!("{d:.1}"));
            let group_id = group.registry_key(source_id);
            for &member in &group.members {
                let stop = &source.feed.stops[member as usize];
                rows.push(vec![
                    group_id.clone(),
                    format!("{source_id}:{}", stop.stop_id),
                    stop.stop_name.as_deref().unwrap_or("").to_owned(),
                    target.clone(),
                    name.clone(),
                    digest.clone(),
                    method.clone(),
                    status.to_owned(),
                    distance.clone(),
                    disposition.to_owned(),
                ]);
            }
        }
    }
    ReportTable {
        name: "stop_group_matches",
        header: vec![
            "source_group_id",
            "source_stop_id",
            "source_stop_name",
            "target_stop_place_id",
            "target_stop_name",
            "target_stable_digest",
            "method",
            "status",
            "distance_metres",
            "geodata_disposition",
        ],
        rows,
    }
}

/// One row per accepted proposal, distinct: sources in ID order, then in order of acceptance
/// (pass, group's first proposal, trip order).
pub fn stop_context_inference(prepared: &Prepared<'_>) -> ReportTable {
    let base = &prepared.base;
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut seen: HashSet<Vec<String>> = HashSet::new();
    for source in &prepared.sources {
        let source_id = source.source_id();
        for inference in &source.inferences {
            let group = &source.groups[inference.group as usize];
            let trip = &source.trips[inference.trip as usize];
            let row = vec![
                group.registry_key(source_id),
                format!(
                    "{source_id}:{}",
                    source.feed.trips[trip.feed_index as usize].trip_id
                ),
                inference.ordinal.to_string(),
                base.timetable.places[inference.target].id.to_string(),
                base.place(inference.target)
                    .map_or_else(String::new, |_| published_name(prepared, inference.target)),
                inference.method.as_str().to_owned(),
            ];
            if seen.insert(row.clone()) {
                rows.push(row);
            }
        }
    }
    ReportTable {
        name: "stop_context_inference",
        header: vec![
            "source_group_id",
            "source_trip_id",
            "source_call_ordinal",
            "target_stop_place_id",
            "target_stop_name",
            "method",
        ],
        rows,
    }
}

/// R048.
pub fn overlay_place_candidates(prepared: &Prepared<'_>) -> ReportTable {
    let pins = native::pins(prepared.places);
    let rows = prepared
        .sources
        .iter()
        .flat_map(|source: &PreparedSource<'_>| native::registry_candidates(source, &pins))
        .collect();
    ReportTable {
        name: "overlay_place_candidates",
        header: vec![
            "source_id",
            "group_key",
            "place_id",
            "stop_name",
            "lat",
            "lon",
            "reason",
            "alias_of_suggestion",
        ],
        rows,
    }
}
