//! Checks that change nothing (JDF-R072, R073).

use std::collections::{BTreeSet, HashMap};

use jr_model::Severity;
use jr_model::geo::distance_m;

use super::geography::GeoStop;
use super::policy::Policy;
use super::{R072, R073, Report, trips};
use crate::batch::Batch;

/// R072: consecutive matched calls with kilometres whose matches lie too far apart, on every
/// trip, once per pair of stops.
pub(super) fn km_distances(
    batch: &Batch,
    states: &[GeoStop],
    policy: &Policy,
    report: &mut Report<'_>,
) {
    let check = &policy.file.diagnostics;
    let home = &*policy.file.home_country;
    let by_id: HashMap<u64, &GeoStop> = states.iter().map(|s| (s.stop.id, s)).collect();
    let international = states
        .iter()
        .any(|s| s.stop.country.as_deref().is_some_and(|c| c != home));
    let mut reported: BTreeSet<(u64, u64)> = BTreeSet::new();
    for calls in trips::trips_in_file_order(batch).values() {
        let mut last: Option<(u64, jr_model::Coord, f64)> = None;
        for &call in calls {
            let call = &batch.trip_stops[call];
            let Some(state) = by_id.get(&call.stop_id) else {
                continue;
            };
            let (Some(found), Some(km)) = (&state.matched, call.km) else {
                continue;
            };
            let km = km.value();
            if let Some((last_stop, last_coord, last_km)) = last {
                let distance_km = distance_m(last_coord, found.coord) / 1000.0;
                let allowed =
                    ((km - last_km).abs() + check.km_check_tolerance_km) * check.km_check_factor;
                if distance_km > allowed && reported.insert((last_stop, call.stop_id)) {
                    report.stop(
                        "jdf_match_km_distance",
                        Severity::Warning,
                        call.stop_id,
                        format!(
                            "match {distance_km:.1} km from the match of stop {last_stop}, {:.1} km by the \
                             timetable{}",
                            (km - last_km).abs(),
                            if international { " (international route)" } else { "" }
                        ),
                        R072,
                    );
                }
            }
            last = Some((call.stop_id, found.coord, km));
        }
    }
}

/// R073: stops without country, or in the home country without okres.
pub(super) fn missing_geography(
    states: &[GeoStop],
    called: &BTreeSet<u64>,
    policy: &Policy,
    report: &mut Report<'_>,
) {
    let home = &*policy.file.home_country;
    for state in states {
        let stop = &state.stop;
        let code = match (stop.country.as_deref(), stop.okres.as_deref()) {
            (None, None) => "jdf_stop_without_country",
            (Some(country), None) if country == home => "jdf_stop_without_okres",
            _ => continue,
        };
        let is_called = called.contains(&stop.id);
        report.stop(
            code,
            if is_called {
                Severity::Warning
            } else {
                Severity::Info
            },
            stop.id,
            format!(
                "{}{},{},{} has no {}",
                if is_called { "" } else { "unused stop " },
                stop.town,
                stop.district.as_deref().unwrap_or(""),
                stop.nearby.as_deref().unwrap_or(""),
                if code == "jdf_stop_without_country" {
                    "country"
                } else {
                    "okres"
                }
            ),
            R073,
        );
    }
}
