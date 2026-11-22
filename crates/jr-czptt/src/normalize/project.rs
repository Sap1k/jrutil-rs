//! Projecting line, route type and category changes onto passenger calls (R040).

use std::collections::BTreeMap;

use jr_model::{Diagnostic, Diagnostics, Rule, Severity};

use crate::path::{Call, ServiceKey};

/// A line or category change written at a point where no passenger can board or alight has no
/// visible boundary. It moves to an operator change between the surrounding passenger calls, if
/// there is one (nearest first), else to the next passenger call; after the last passenger call
/// it is ignored. Of several changes landing on one call, the latest in the source wins.
pub(crate) const R040: Rule = Rule {
    id: "CZPTT-R040",
    summary: "A line or category change at a non-passenger point moves to a passenger call",
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reason {
    CoalescedWithOperatorBoundary,
    DeferredToNextPassengerCall,
    IgnoredAfterFinalPassengerCall,
}

/// A change that did not stay where the source wrote it.
#[derive(Clone, Debug)]
pub(crate) struct Adjustment {
    pub source_sequence: u32,
    pub applied_sequence: Option<u32>,
    pub reason: Reason,
}

fn key(call: &Call) -> ServiceKey {
    ServiceKey {
        line: call.line.clone(),
        route_type: call.route_type,
        category: call.category.clone(),
    }
}

/// Sets every call's `projected` key and returns the moves.
pub(crate) fn project(calls: &mut [Call]) -> Vec<Adjustment> {
    if calls.len() < 2 {
        return Vec::new();
    }
    let passengers: Vec<usize> = (0..calls.len()).filter(|&i| calls[i].passenger).collect();
    let operator_changes: Vec<usize> = (1..calls.len())
        .filter(|&i| calls[i - 1].responsible_ru != calls[i].responsible_ru)
        .collect();
    // Applied index -> (source index, key); the latest source index wins.
    let mut events: BTreeMap<usize, (usize, ServiceKey)> = BTreeMap::new();
    let mut adjustments = Vec::new();
    for source in 1..calls.len() {
        let current = key(&calls[source]);
        if key(&calls[source - 1]) == current {
            continue;
        }
        let applied = if calls[source].passenger {
            Some(source)
        } else {
            passengers
                .iter()
                .copied()
                .find(|&i| i > source)
                .map(|next| {
                    let previous = passengers
                        .iter()
                        .copied()
                        .rfind(|&i| i < source)
                        .map_or(-1, |i| i as i64);
                    operator_changes
                        .iter()
                        .copied()
                        .filter(|&t| t as i64 > previous && t <= next)
                        .min_by_key(|&t| (t.abs_diff(source), t))
                        .unwrap_or(next)
                })
        };
        match applied {
            Some(applied) => {
                if applied != source {
                    adjustments.push(Adjustment {
                        source_sequence: source as u32 + 1,
                        applied_sequence: Some(applied as u32 + 1),
                        reason: if operator_changes.contains(&applied) {
                            Reason::CoalescedWithOperatorBoundary
                        } else {
                            Reason::DeferredToNextPassengerCall
                        },
                    });
                }
                let entry = events.entry(applied).or_insert((source, current.clone()));
                if source >= entry.0 {
                    *entry = (source, current);
                }
            }
            None => adjustments.push(Adjustment {
                source_sequence: source as u32 + 1,
                applied_sequence: None,
                reason: Reason::IgnoredAfterFinalPassengerCall,
            }),
        }
    }
    let mut projected = key(&calls[0]);
    for (index, call) in calls.iter_mut().enumerate() {
        if let Some((_, key)) = events.get(&index) {
            projected = key.clone();
        }
        call.projected = projected.clone();
    }
    adjustments
}

pub(crate) fn report_adjustments(
    pa: &str,
    adjustments: &[Adjustment],
    diagnostics: &mut Diagnostics,
) {
    for adjustment in adjustments {
        let (code, text) = match (adjustment.reason, adjustment.applied_sequence) {
            (Reason::CoalescedWithOperatorBoundary, Some(applied)) => (
                "czptt_line_boundary_moved",
                format!("to the operator change at sequence {applied}"),
            ),
            (Reason::DeferredToNextPassengerCall, Some(applied)) => (
                "czptt_line_boundary_moved",
                format!("to the next passenger call at sequence {applied}"),
            ),
            _ => (
                "czptt_line_boundary_ignored",
                "ignored: no passenger call follows".to_owned(),
            ),
        };
        diagnostics.push(
            Diagnostic::new(
                code,
                Severity::Info,
                pa,
                format!(
                    "line change at sequence {} {text}",
                    adjustment.source_sequence
                ),
            )
            .rule(R040),
        );
    }
}
