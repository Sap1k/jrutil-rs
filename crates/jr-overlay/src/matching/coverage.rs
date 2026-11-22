//! Coverage counts (OVL-R076).

use std::collections::{BTreeMap, BTreeSet};

use jr_model::Rule;
use jr_model::timetable::Mode;

use super::Matches;
use crate::authority::Authority;
use crate::prepare::Prepared;

/// Per source: the calls of active trips, their call-dates, the calls of trips with a binding or
/// a projection; per mode, the stop groups active trips call and how many are placed, and the
/// calls and how many belong to such trips.
pub const OVL_R076: Rule = Rule {
    id: "OVL-R076",
    summary: "coverage counts of calls and stop groups per source and mode",
};

pub const RULES: &[Rule] = &[OVL_R076];

/// One mode's counts (`None`: a route type without a mode).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModeCoverage {
    pub groups_placed: usize,
    pub groups: usize,
    pub calls_matched: usize,
    pub calls: usize,
}

/// One source's counts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    pub calls: usize,
    pub call_dates: usize,
    pub matched_calls: usize,
    pub by_mode: BTreeMap<Option<Mode>, ModeCoverage>,
}

pub fn coverage(
    prepared: &Prepared<'_>,
    authority: &Authority,
    matches: &Matches,
) -> Vec<Coverage> {
    prepared
        .sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            let mut accepted = vec![false; source.trips.len()];
            for binding in matches
                .bindings
                .iter()
                .filter(|b| b.source as usize == index)
            {
                accepted[binding.trip as usize] = true;
            }
            for projection in authority
                .projections
                .iter()
                .filter(|p| p.source as usize == index)
            {
                accepted[projection.trip as usize] = true;
            }
            let mut result = Coverage::default();
            let mut groups: BTreeMap<Option<Mode>, BTreeSet<u32>> = BTreeMap::new();
            for (trip_index, trip) in source.trips.iter().enumerate() {
                let calls = source.trip_calls(trip);
                result.call_dates += trip.dates.count() * calls.len();
                if trip.dates.is_empty() {
                    continue;
                }
                result.calls += calls.len();
                let mode = source.routes[trip.route as usize].mode;
                let entry = result.by_mode.entry(mode).or_default();
                entry.calls += calls.len();
                if accepted[trip_index] {
                    result.matched_calls += calls.len();
                    entry.calls_matched += calls.len();
                }
                let set = groups.entry(mode).or_default();
                set.extend(
                    calls
                        .iter()
                        .filter_map(|call| source.group_of_stop[call.stop as usize]),
                );
            }
            for (mode, set) in groups {
                let entry = result.by_mode.entry(mode).or_default();
                entry.groups = set.len();
                entry.groups_placed = set
                    .iter()
                    .filter(|&&g| source.groups[g as usize].matched.is_some())
                    .count();
            }
            result
        })
        .collect()
}
