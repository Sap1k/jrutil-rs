//! The review reports of matching, in the F# column sets and value formats (section 3.16).
//! Source trip IDs are namespaced (`<source>:<trip_id>`) where F# wrote them so.

use std::collections::HashSet;

use jr_model::timetable::Mode;
use sha2::{Digest, Sha256};

use super::{Candidate, Matches};
use crate::authority::trip_subject;
use crate::prepare::{Prepared, PreparedSource};
use crate::report::ReportTable;

fn date(prepared: &Prepared<'_>, index: usize) -> String {
    prepared
        .base
        .timetable
        .horizon
        .date_at(index)
        .strftime("%Y%m%d")
        .to_string()
}

fn base_trip_id(prepared: &Prepared<'_>, index: u32) -> String {
    let trip = prepared.base.trips[index as usize].trip;
    prepared.base.timetable.trips[trip].id.to_string()
}

fn subject(prepared: &Prepared<'_>, source: u32, trip: u32) -> String {
    let source = &prepared.sources[source as usize];
    trip_subject(source, &source.trips[trip as usize])
}

fn rank_columns(candidate: &Candidate) -> [String; 7] {
    let rank = candidate.rank;
    [
        rank.edits.to_string(),
        (-rank.negative_aligned).to_string(),
        rank.first_departure_delta.to_string(),
        rank.aggregate_delta.to_string(),
        rank.duration_delta.to_string(),
        rank.maximum_delta.to_string(),
        rank.squared_delta.to_string(),
    ]
}

/// One row per binding at its creation (first date), in binding order.
pub fn trip_candidate_scores(prepared: &Prepared<'_>, matches: &Matches) -> ReportTable {
    let rows = matches
        .bindings
        .iter()
        .map(|binding| {
            let method = if binding.by_availability {
                format!("{}+target_availability", binding.route_tier.as_str())
            } else {
                binding.route_tier.as_str().to_owned()
            };
            let mut row = vec![
                subject(prepared, binding.source, binding.trip),
                base_trip_id(prepared, binding.candidate.base_trip),
                date(prepared, binding.dates.first().unwrap_or_default()),
                method,
                binding.candidate.tier.as_str().to_owned(),
            ];
            row.extend(rank_columns(&binding.candidate));
            row.push(binding.margin.map_or_else(String::new, |m| m.to_string()));
            row
        })
        .collect();
    ReportTable {
        name: "trip_candidate_scores",
        header: vec![
            "source_trip_id",
            "base_trip_id",
            "first_accepted_date",
            "route_method",
            "trip_method",
            "pattern_edits",
            "aligned_target_calls",
            "first_departure_delta_seconds",
            "aggregate_time_delta_seconds",
            "duration_delta_seconds",
            "maximum_aligned_time_delta_seconds",
            "squared_aligned_time_delta",
            "runner_up_margin",
        ],
        rows,
    }
}

/// The base trip's stable line descriptor (`cis:<line>` or `route:<route>`) and its semantic
/// equivalence key (R071): the descriptor and the SHA-256 of its places and exact times.
fn equivalence(prepared: &Prepared<'_>, base_trip: u32) -> (String, String) {
    let base = &prepared.base;
    let trip = &base.trips[base_trip as usize];
    let line = match &base.route(trip.route).line {
        Some(line) => format!("cis:{line}"),
        None => format!("route:{}", base.timetable.routes[trip.route].id),
    };
    let mut text = String::new();
    for (index, place) in base.pattern(trip.pattern).iter().enumerate() {
        let time = |value: Option<i32>| value.unwrap_or(-1);
        text.push_str(&format!(
            "{}/{}/{};",
            base.timetable.places[*place].id,
            time(trip.arrivals[index]),
            time(trip.departures[index])
        ));
    }
    let digest: String = Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let key = format!("{line}|{digest}");
    (line, key)
}

/// One row per candidate still pending after the availability passes, in pending order.
pub fn ambiguous_trip_candidates(prepared: &Prepared<'_>, matches: &Matches) -> ReportTable {
    let mut rows = Vec::new();
    for pending in &matches.ambiguous {
        for candidate in &pending.candidates {
            let route = prepared.base.trips[candidate.base_trip as usize].route;
            let (line, key) = equivalence(prepared, candidate.base_trip);
            let mut row = vec![
                subject(prepared, pending.source, pending.trip),
                date(prepared, pending.date as usize),
                base_trip_id(prepared, candidate.base_trip),
                prepared.base.timetable.routes[route].id.to_string(),
                line,
                pending.route_tier.as_str().to_owned(),
                candidate.tier.as_str().to_owned(),
            ];
            row.extend(rank_columns(candidate));
            row.push(key);
            rows.push(row);
        }
    }
    ReportTable {
        name: "ambiguous_trip_candidates",
        header: vec![
            "source_trip_id",
            "date",
            "base_trip_id",
            "base_route_id",
            "stable_line_descriptor",
            "route_method",
            "trip_method",
            "pattern_edits",
            "aligned_target_calls",
            "first_departure_delta_seconds",
            "aggregate_time_delta_seconds",
            "duration_delta_seconds",
            "maximum_aligned_time_delta_seconds",
            "squared_aligned_time_delta",
            "semantic_equivalence_key",
        ],
        rows,
    }
}

/// One distinct row per binding with edits, in binding order.
pub fn pattern_edits(prepared: &Prepared<'_>, matches: &Matches) -> ReportTable {
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    for binding in matches
        .bindings
        .iter()
        .filter(|b| b.candidate.rank.edits > 0)
    {
        let candidate = &binding.candidate;
        let row = vec![
            subject(prepared, binding.source, binding.trip),
            base_trip_id(prepared, candidate.base_trip),
            candidate.tier.as_str().to_owned(),
            candidate.rank.edits.to_string(),
            candidate
                .alignment
                .iter()
                .filter(|a| a.is_some())
                .count()
                .to_string(),
            candidate.alignment.len().to_string(),
        ];
        if seen.insert(row.clone()) {
            rows.push(row);
        }
    }
    ReportTable {
        name: "pattern_edits",
        header: vec![
            "source_trip_id",
            "base_trip_id",
            "trip_method",
            "pattern_edits",
            "aligned_target_calls",
            "target_call_count",
        ],
        rows,
    }
}

/// The F# mode-class name of a source route (`ferry` for water, `heavy-rail` for rail,
/// `other:<route_type>` without a mode), for the report only.
pub(crate) fn mode_name(source: &PreparedSource<'_>, route: u32) -> String {
    let source_route = &source.routes[route as usize];
    match source_route.mode {
        Some(Mode::Bus) => "bus".into(),
        Some(Mode::Trolleybus) => "trolleybus".into(),
        Some(Mode::Tram) => "tram".into(),
        Some(Mode::Metro) => "metro".into(),
        Some(Mode::Rail) => "heavy-rail".into(),
        Some(Mode::Water) => "ferry".into(),
        Some(Mode::Cable | Mode::Funicular) | None => {
            let route_type = source.feed.routes[source_route.feed_index as usize].route_type;
            format!(
                "other:{}",
                route_type.map_or_else(String::new, |t| t.to_string())
            )
        }
    }
}
