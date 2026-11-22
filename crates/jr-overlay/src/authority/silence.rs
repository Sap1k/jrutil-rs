//! OVL-R062: the integrated-system silence.

use std::collections::{BTreeMap, BTreeSet};

use jr_model::{DateSet, Diagnostic, Diagnostics, Severity};

use super::{AuthorityLine, OVL_R062};
use crate::prepare::Prepared;

/// R062 for every source's scopes, in source and scope order.
pub fn extend(
    prepared: &Prepared<'_>,
    lines: &mut BTreeMap<AuthorityLine, DateSet>,
    diagnostics: &mut Diagnostics,
) {
    let base = &prepared.base;
    let horizon = base.timetable.horizon;
    let mut running: Option<BTreeMap<&str, DateSet>> = None;
    for source in &prepared.sources {
        let scopes = &source.profile.authority.integrated_systems;
        let Some((start, end)) = source.window else {
            continue;
        };
        if scopes.is_empty() {
            continue;
        }
        // The days each national line runs in the base.
        let running = running.get_or_insert_with(|| {
            let mut days: BTreeMap<&str, DateSet> = BTreeMap::new();
            for trip in &base.trips {
                if let Some(line) = &base.route(trip.route).line {
                    let entry = days.entry(line).or_default();
                    *entry = *entry | trip.dates;
                }
            }
            days
        });
        let window = DateSet::range(horizon, start, end);
        for scope in scopes {
            let mut scoped: BTreeSet<&str> = BTreeSet::new();
            for route in base.routes.iter().filter(|route| route.published) {
                let Some(line) = route.line.as_deref() else {
                    continue;
                };
                if route.systems.iter().any(|system| **system == *scope.system)
                    && source.profile.authority.modes.contains(&route.mode)
                    && !scope
                        .excluded_lines
                        .iter()
                        .any(|excluded| **excluded == *line)
                    && running
                        .get(line)
                        .is_some_and(|days| days.intersects(&window))
                {
                    scoped.insert(line);
                }
            }
            let silenced: Vec<&str> = scoped
                .iter()
                .copied()
                .filter(|line| !lines.contains_key(&AuthorityLine::National((*line).into())))
                .collect();
            if silenced.len() > scope.maximum_silenced_lines as usize {
                let sample: Vec<&str> = silenced.iter().copied().take(20).collect();
                diagnostics.push(
                    Diagnostic::new(
                        "trip_set_authority_silence_withheld",
                        Severity::Warning,
                        source.source_id(),
                        format!(
                            "{} runs none of {} {} lines (maximum {}); national trips kept: {}",
                            source.source_id(),
                            silenced.len(),
                            scope.system,
                            scope.maximum_silenced_lines,
                            sample.join(";")
                        ),
                    )
                    .rule(OVL_R062),
                );
                continue;
            }
            for line in scoped {
                let key = AuthorityLine::National(line.into());
                if !lines.contains_key(&key) {
                    diagnostics.push(
                        Diagnostic::new(
                            "trip_set_authority_silent",
                            Severity::Warning,
                            line,
                            format!(
                                "{} runs no trips of this {} line between {} and {}; national trips withdrawn",
                                source.source_id(),
                                scope.system,
                                start.strftime("%Y%m%d"),
                                end.strftime("%Y%m%d")
                            ),
                        )
                        .rule(OVL_R062),
                    );
                }
                let dates = lines.entry(key).or_default();
                *dates = *dates | window;
            }
        }
    }
}
