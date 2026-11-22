//! The filtered road feed (`docs/rules/jdf-convert.md`, section 6; JDF-R479–R482).
//!
//! Consumers that merge the national road feed with regional feeds and with operators' own
//! feeds need the national one without the lines those feeds publish, and without calls at
//! stops nobody could place. The feed is the road timetable as it stands before the regional
//! overlay, minus those: this pass decides what goes and returns it as edits, to be applied to
//! a copy of the timetable that is then projected to GTFS.
//!
//! Which lines other feeds publish is read from facts of the line versions that the timetable
//! does not carry (alternate operators, the preferred integrated system): the assembly hands
//! them over as [`LineVersion`]s.

use std::collections::{BTreeMap, BTreeSet};

use jr_formats::geodata::filter_rules::FilterRules;
use jr_model::{Date, DateSet, Edit, EditBuilder, Edits, RouteId, Rule, Timetable};
use serde::Serialize;

/// The pass name recorded in the timetable's history.
pub const PASS: &str = "filtered_feed";

/// A line is one another feed publishes when one of its versions still valid on the reference
/// date or later names a listed operator, as the version's own or as an alternate operator
/// (`Altdop`), or has a listed integrated transport system as its preferred one (`LinExt`).
///
/// Every version still valid counts, so a line whose current version is a short detour still
/// matches through its regular version. An integrated system matches only through the
/// preferred row, as on the line portal: a line that merely also accepts another system's
/// tariff is kept. A licence that is no CIS line number takes no part.
pub const R479: Rule = Rule {
    id: "JDF-R479",
    summary: "lines other feeds publish: by operator or preferred integrated system of a valid version",
};

/// The trips of a route are removed when its line is one another feed publishes, or its
/// licence starts with a listed prefix (line families a regional feed covers whole).
pub const R480: Rule = Rule {
    id: "JDF-R480",
    summary: "trips of lines other feeds publish, and of listed licence prefixes, are removed",
};

/// A call at a stop without a position is removed; a trip left with fewer than two calls is
/// removed. The other calls keep their sequences.
///
/// The feed's consumers route on coordinates and reject a stop without them.
pub const R481: Rule = Rule {
    id: "JDF-R481",
    summary: "calls at unplaced stops are removed, and trips left with fewer than two calls",
};

/// A listed operator group or integrated system that matches no line, and a feed left without
/// a trip, fail the build: the rules or the input are not what the build assumes.
pub const R482: Rule = Rule {
    id: "JDF-R482",
    summary: "a selection that matches no line, or an empty feed, is an error",
};

pub const RULES: &[Rule] = &[R479, R480, R481, R482];

/// What the selection reads of one line version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineVersion {
    /// The published route its trips are on; `None` when none of its trips is published.
    pub route: Option<RouteId>,
    /// The licence as written.
    pub licence: Box<str>,
    /// The end of its published validity; `None` when open.
    pub valid_to: Option<Date>,
    /// The names of its operator and of its trips' alternate operators.
    pub operators: Vec<Box<str>>,
    /// The integrated transport systems it names as preferred.
    pub preferred_systems: Vec<u64>,
}

/// One selection of the rules and the lines it matched: `line-snapshot.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Query {
    /// `operators` or `ids`.
    pub kind: &'static str,
    /// The operator names joined with `;`, or the system's code.
    pub value: String,
    /// Six-digit line numbers, sorted.
    pub lines: Vec<String>,
}

/// The counts of `filter-report.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    pub matched_lines: usize,
    pub lines_removed: Vec<String>,
    pub routes_removed: usize,
    pub routes_kept: usize,
    pub trips_kept: usize,
    pub trips_removed_by_line: usize,
    pub trips_removed_too_few_calls: usize,
    pub calls_removed_missing_coordinates: usize,
    pub stops_without_coordinates: usize,
}

#[derive(Debug)]
pub struct Filtered {
    pub edits: Edits,
    pub queries: Vec<Query>,
    pub report: Report,
}

#[derive(Debug, thiserror::Error)]
pub enum FilterError {
    #[error("no line valid from {reference} matches {kind}={value}")]
    EmptySelection {
        kind: &'static str,
        value: String,
        reference: Date,
    },
    #[error("the filtered feed has no trip")]
    Empty,
}

/// The six-digit line number of a licence that is a CIS line number.
fn line_number(licence: &str) -> Option<String> {
    let digits = !licence.is_empty() && licence.bytes().all(|byte| byte.is_ascii_digit());
    (digits && licence.len() <= 6).then(|| format!("{licence:0>6}"))
}

pub fn filtered_feed(
    timetable: &Timetable,
    lines: &[LineVersion],
    rules: &FilterRules,
    reference: Date,
) -> Result<Filtered, FilterError> {
    // R479.
    let valid: Vec<(&LineVersion, String)> = lines
        .iter()
        .filter(|line| line.valid_to.is_none_or(|to| to >= reference))
        .filter_map(|line| Some((line, line_number(&line.licence)?)))
        .collect();
    let select = |matches: &dyn Fn(&LineVersion) -> bool| -> Vec<String> {
        let found: BTreeSet<&String> = valid
            .iter()
            .filter(|(line, _)| matches(line))
            .map(|(_, number)| number)
            .collect();
        found.into_iter().cloned().collect()
    };
    let mut queries = Vec::new();
    if !rules.operators.is_empty() {
        queries.push(Query {
            kind: "operators",
            value: rules.operators.join(";"),
            lines: select(&|line| {
                line.operators
                    .iter()
                    .any(|name| rules.operators.contains(name))
            }),
        });
    }
    for &code in &rules.ids_codes {
        queries.push(Query {
            kind: "ids",
            value: code.to_string(),
            lines: select(&|line| line.preferred_systems.contains(&u64::from(code))),
        });
    }
    // R482.
    if let Some(empty) = queries.iter().find(|query| query.lines.is_empty()) {
        return Err(FilterError::EmptySelection {
            kind: empty.kind,
            value: empty.value.clone(),
            reference,
        });
    }
    let removed_lines: BTreeSet<&str> = queries
        .iter()
        .flat_map(|query| query.lines.iter().map(String::as_str))
        .collect();

    // R480: a route goes with its line.
    let mut route_lines: BTreeMap<RouteId, String> = BTreeMap::new();
    for line in lines {
        if let (Some(route), Some(number)) = (line.route, line_number(&line.licence)) {
            route_lines.entry(route).or_insert(number);
        }
    }
    let removed_routes: BTreeSet<RouteId> = route_lines
        .iter()
        .filter(|(_, number)| {
            removed_lines.contains(number.as_str())
                || rules
                    .line_prefixes
                    .iter()
                    .any(|prefix| number.starts_with(&**prefix))
        })
        .map(|(&route, _)| route)
        .collect();

    let mut builder = EditBuilder::new(timetable, PASS);
    let retired = builder.service(DateSet::EMPTY);
    let mut report = Report {
        matched_lines: removed_lines.len(),
        lines_removed: removed_lines
            .iter()
            .map(|line| (*line).to_owned())
            .collect(),
        ..Report::default()
    };
    let mut routes_seen = BTreeSet::new();
    let mut routes_kept = BTreeSet::new();
    let mut unplaced = BTreeSet::new();
    for (trip, row) in timetable.trips.iter() {
        if timetable.services.get(row.service).is_empty() {
            continue;
        }
        routes_seen.insert(row.route);
        if removed_routes.contains(&row.route) {
            report.trips_removed_by_line += 1;
            builder.push(
                R480,
                Edit::SetTripService {
                    trip,
                    service: retired,
                },
            );
            continue;
        }
        // R481.
        let gone: Vec<i32> = row
            .calls
            .iter()
            .filter(|call| {
                timetable
                    .places
                    .get(call.place)
                    .is_none_or(|place| place.position.is_none())
            })
            .map(|call| {
                unplaced.insert(call.place);
                call.sequence
            })
            .collect();
        if row.calls.len() - gone.len() < 2 {
            report.trips_removed_too_few_calls += 1;
            builder.push(
                R481,
                Edit::SetTripService {
                    trip,
                    service: retired,
                },
            );
            continue;
        }
        report.trips_kept += 1;
        routes_kept.insert(row.route);
        if !gone.is_empty() {
            report.calls_removed_missing_coordinates += gone.len();
            builder.push(
                R481,
                Edit::RemoveCalls {
                    trip,
                    sequences: gone,
                },
            );
        }
    }
    report.routes_kept = routes_kept.len();
    report.routes_removed = routes_seen.intersection(&removed_routes).count();
    report.stops_without_coordinates = unplaced.len();
    // R482.
    if report.trips_kept == 0 {
        return Err(FilterError::Empty);
    }
    Ok(Filtered {
        edits: builder.finish(),
        queries,
        report,
    })
}
