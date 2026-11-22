//! The source hooks of a profile (OVL-R015, R017, R018, R019): source quirks named by the
//! profile, with their data there.

use std::collections::HashMap;

use jr_formats::gtfs::{Feed, Stop, Trip};
use jr_model::{Date, Diagnostic, Diagnostics, Rule, Severity};

use super::columns::{StopColumns, TripColumns};
use crate::profile::SourceProfile;

/// A trip variant's revision is the date the profile's pattern captures from one of its columns;
/// newer revisions win conflicting claims (R082). Without the hook, or without a match, a trip
/// has no revision (no ordering; the F# sentinel 700101 is dropped).
///
/// PID publishes revisions of one trip with different validity (`991_11915_260207`).
pub const OVL_R015: Rule = Rule {
    id: "OVL-R015",
    summary: "a trip's revision date read from a column by the profile's pattern",
};

/// A source stop's post key is the profile's post column, else its own stop ID; a stop place
/// (`location_type 1`) has none. It is the source code a post the source adds carries.
///
/// PID keys posts by ASW stop numbers.
pub const OVL_R017: Rule = Rule {
    id: "OVL-R017",
    summary: "a source post's key: the profile's post column, else the stop ID",
};

/// A source trip asserts a national line when the profile's companion table, joined on its key
/// columns, gives the trip exactly one distinct non-blank value. Keys are compared as composite
/// keys (B10 fixed).
///
/// PID's `route_sub_agencies.route_licence_number` is the CIS licence of a route a sub-agency
/// runs.
pub const OVL_R018: Rule = Rule {
    id: "OVL-R018",
    summary: "a trip's national line asserted by the profile's companion table",
};

/// The operator's crosswalk maps (line, course) to trip IDs, one line of the profile's member per
/// trip in the profile's grammar. A malformed line, an unknown trip or a trip mapped twice is a
/// diagnostic and the line is skipped (B20, verdict Q11).
///
/// IDS JMK `api.txt` maps its line codes and course (train) numbers to GTFS trips.
pub const OVL_R019: Rule = Rule {
    id: "OVL-R019",
    summary: "operator line/course to trip crosswalk read by the profile's grammar",
};

pub const RULES: &[Rule] = &[OVL_R015, OVL_R017, OVL_R018, OVL_R019];

/// R015.
pub fn revision(profile: &SourceProfile, columns: &TripColumns, trip: &Trip) -> Option<Date> {
    let hook = profile.hooks.revision.as_ref()?;
    let regex = hook.compiled.as_ref()?;
    let value = columns.get(trip, &hook.column)?;
    let captured = regex.captures(value)?.name("revision")?.as_str();
    Date::strptime(&*hook.date_format, captured).ok()
}

/// R017.
pub fn post_key<'s>(
    profile: &SourceProfile,
    columns: &StopColumns,
    stop: &'s Stop,
) -> Option<&'s str> {
    if stop.location_type == Some(1) {
        return None;
    }
    profile
        .hooks
        .post_key
        .as_ref()
        .and_then(|hook| columns.get(stop, &hook.column))
        .or(Some(&*stop.stop_id))
}

/// R018: the national line each trip asserts, by `feed.trips` index.
pub fn line_claims(
    profile: &SourceProfile,
    feed: &Feed,
    columns: &TripColumns,
    diagnostics: &mut Diagnostics,
) -> Vec<Option<Box<str>>> {
    let mut claims = vec![None; feed.trips.len()];
    let Some(hook) = &profile.hooks.cis_line_claim else {
        return claims;
    };
    let Some(table) = feed.other.get(&*hook.table) else {
        diagnostics.push(
            Diagnostic::new(
                "overlay_line_claim_table_missing",
                Severity::Error,
                profile.source_id.to_string(),
                format!("the feed has no {}: no trip asserts a line", hook.table),
            )
            .rule(OVL_R018),
        );
        return claims;
    };
    let lookup: Vec<Option<usize>> = hook.lookup_keys.iter().map(|k| table.column(k)).collect();
    let value_column = table.column(&hook.value_column);
    let mut values: HashMap<Vec<&str>, Vec<&str>> = HashMap::new();
    if let (Some(lookup), Some(value_column)) = (
        lookup.iter().copied().collect::<Option<Vec<usize>>>(),
        value_column,
    ) {
        for row in &table.rows {
            let value = &*row[value_column];
            if value.trim().is_empty() {
                continue;
            }
            let key: Vec<&str> = lookup.iter().map(|&column| &*row[column]).collect();
            let entry = values.entry(key).or_default();
            if !entry.contains(&value) {
                entry.push(value);
            }
        }
    }
    for (index, trip) in feed.trips.iter().enumerate() {
        let key: Vec<&str> = hook
            .source_keys
            .iter()
            .map(|column| columns.get(trip, column).unwrap_or(""))
            .collect();
        if let Some([value]) = values.get(&key).map(Vec::as_slice) {
            claims[index] = Some((*value).into());
        }
    }
    claims
}

/// One crosswalk line (R019).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrosswalkRow {
    pub line: Box<str>,
    pub course: Box<str>,
    /// Index into the feed's trips.
    pub trip: u32,
}

/// R019, rows in member order.
pub fn crosswalk(
    profile: &SourceProfile,
    feed: &Feed,
    diagnostics: &mut Diagnostics,
) -> Vec<CrosswalkRow> {
    let Some(hook) = &profile.hooks.operational_crosswalk else {
        return Vec::new();
    };
    let source = profile.source_id.to_string();
    let problem = |diagnostics: &mut Diagnostics, message: String| {
        diagnostics.push(
            Diagnostic::new(
                "overlay_crosswalk_line_rejected",
                Severity::Warning,
                source.clone(),
                message,
            )
            .rule(OVL_R019),
        );
    };
    let Some(bytes) = feed.raw.get(&*hook.member) else {
        problem(diagnostics, format!("the feed has no {}", hook.member));
        return Vec::new();
    };
    let Some(regex) = &hook.compiled else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let trips: HashMap<&str, u32> = feed
        .trips
        .iter()
        .enumerate()
        .rev()
        .map(|(index, trip)| (&*trip.trip_id, index as u32))
        .collect();
    let mut seen = vec![false; feed.trips.len()];
    let mut rows = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(captures) = regex.captures(line) else {
            problem(
                diagnostics,
                format!("{} line {} is malformed", hook.member, number + 1),
            );
            continue;
        };
        let group = |name: &str| captures.name(name).map_or("", |m| m.as_str());
        let Some(&trip) = trips.get(group("trip")) else {
            problem(
                diagnostics,
                format!(
                    "{} line {} names unknown trip {}",
                    hook.member,
                    number + 1,
                    group("trip")
                ),
            );
            continue;
        };
        if std::mem::replace(&mut seen[trip as usize], true) {
            problem(
                diagnostics,
                format!(
                    "{} line {} maps trip {} again",
                    hook.member,
                    number + 1,
                    group("trip")
                ),
            );
            continue;
        }
        rows.push(CrosswalkRow {
            line: group("line").into(),
            course: group("course").into(),
            trip,
        });
    }
    let unmapped = seen.iter().filter(|seen| !**seen).count();
    if unmapped > 0 {
        problem(
            diagnostics,
            format!(
                "{} maps no line and course for {unmapped} trips",
                hook.member
            ),
        );
    }
    rows
}
