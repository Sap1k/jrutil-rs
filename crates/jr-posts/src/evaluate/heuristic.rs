//! POST-R080–R088: the heuristic decision, one per (stop, movement family) (F#
//! `evaluateReplayPolicy`).

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use jr_formats::post_evidence::{BlockRole, Pack};

use super::consolidate::{Consolidated, Hypothesis};
use super::geometry::{
    equirect, evidence_score, hard_gate, isolation_distance, median, mode_compatible,
    mode_incompatible, routed_fit_of, supporting_score,
};
use super::policy::{Constants, Settings};
use super::replay::{Point, Row};

/// A family's heuristic decision.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FamilyOutcome {
    pub family: Box<str>,
    /// The family's first row's context (mode, line and role come from it).
    pub first_context: u32,
    /// `Some` when physical: the hypothesis index, score, margin and first face.
    pub physical: Option<(usize, f64, f64, Option<Box<str>>)>,
}

/// R080: the selected corridor ranks of a context from its rows' variant facts.
pub(crate) fn selected_ranks(
    settings: &Settings,
    facts: &[(i32, Option<f64>, Option<f64>)],
) -> Vec<i32> {
    let mut seen: Vec<i32> = Vec::new();
    let mut ranks = Vec::new();
    for &(rank, cost, fraction) in facts {
        if seen.contains(&rank) {
            continue;
        }
        seen.push(rank);
        let tied = cost.is_some_and(|gap| gap <= settings.absolute_tie_metres)
            || fraction.is_some_and(|gap| gap <= settings.relative_tie_fraction);
        if rank == 0 || (rank < settings.maximum_variants && tied) {
            ranks.push(rank);
        }
    }
    ranks.sort_unstable();
    ranks
}

/// Groups of `items` by key, groups and members in first-occurrence order.
fn group_by<T, K: std::hash::Hash + Eq>(
    items: impl Iterator<Item = T>,
    key: impl Fn(&T) -> K,
) -> Vec<Vec<T>> {
    let mut index: HashMap<K, usize> = HashMap::new();
    let mut groups: Vec<Vec<T>> = Vec::new();
    for item in items {
        let group = *index.entry(key(&item)).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[group].push(item);
    }
    groups
}

/// R080 applied to consolidated rows: per context, per hypothesis the rank-0 row with the
/// alternative-corridor facts.
fn with_alternatives<'a>(settings: &Settings, rows: &[Row<'a>]) -> Vec<Row<'a>> {
    let mut result = Vec::new();
    for context_rows in group_by(rows.iter(), |row| row.context) {
        let facts: Vec<(i32, Option<f64>, Option<f64>)> = context_rows
            .iter()
            .map(|row| (row.rank, row.relative_cost, row.relative_fraction))
            .collect();
        let selected = selected_ranks(settings, &facts);
        for candidate_rows in group_by(context_rows.into_iter(), |row| row.candidate) {
            let Some(baseline) = candidate_rows.iter().find(|row| row.rank == 0) else {
                continue;
            };
            let chosen: Vec<&&Row> = candidate_rows
                .iter()
                .filter(|row| selected.contains(&row.rank))
                .collect();
            let complete = chosen.len() == selected.len();
            let valid = complete
                && chosen
                    .iter()
                    .all(|row| row.failure.is_none() && row.available && row.face.is_some());
            let mut faces: Vec<&str> = Vec::new();
            for face in chosen.iter().filter_map(|row| row.face) {
                if !faces.contains(&face) {
                    faces.push(face);
                }
            }
            let agree = valid && faces.len() == 1;
            result.push(Row {
                alternative_count: selected.len(),
                cost_gap: chosen
                    .iter()
                    .filter_map(|row| row.relative_cost)
                    .filter(|&cost| cost > 0.0)
                    .min_by(f64::total_cmp),
                tied_agree: agree,
                failure: if baseline.failure.is_some() {
                    baseline.failure
                } else if agree {
                    None
                } else {
                    Some("alternative-corridor-invariant")
                },
                ..(*baseline).clone()
            });
        }
    }
    result
}

/// R081: effective modes of each hypothesis (its own, or inherited within the radius).
pub(crate) fn effective_modes<'a>(
    constants: &Constants,
    points: &[Point],
    hypotheses: &[Hypothesis<'a>],
) -> Vec<Vec<&'a str>> {
    hypotheses
        .iter()
        .enumerate()
        .map(|(index, hypothesis)| {
            if !hypothesis.explicit_modes.is_empty() {
                return hypothesis.explicit_modes.clone();
            }
            let own = &points[hypothesis.medoid];
            let mut known: Vec<&'a str> = hypotheses
                .iter()
                .enumerate()
                .filter(|&(other_index, other)| {
                    let point = &points[other.medoid];
                    other_index != index
                        && !other.explicit_modes.is_empty()
                        && equirect(
                            constants,
                            own.latitude,
                            own.longitude,
                            point.latitude,
                            point.longitude,
                        )
                        .0 <= constants.modes.inheritance_radius_m
                })
                .flat_map(|(_, other)| other.explicit_modes.iter().copied())
                .collect();
            known.sort_unstable();
            known.dedup();
            known
        })
        .collect()
}

/// One scored row (F# `evaluated`).
struct Scored<'r, 'a> {
    row: &'r Row<'a>,
    rejected: bool,
    geometry: f64,
    final_score: f64,
    routed: f64,
}

/// A family's candidate hypothesis with its R085 scores.
struct FamilyCandidate<'a> {
    candidate: usize,
    geometry: f64,
    final_score: f64,
    routed: f64,
    faces: BTreeSet<&'a str>,
    alternatives: bool,
    agree: bool,
}

/// The heuristic context key (R084, B12 kept: without the authored key).
type ContextKey<'a> = (&'a str, &'a str, i32, i32, &'a str, i32, BlockRole, &'a str);

fn by_score(left: (f64, f64, &str), right: (f64, f64, &str)) -> Ordering {
    right
        .0
        .partial_cmp(&left.0)
        .unwrap_or(Ordering::Equal)
        .then(right.1.partial_cmp(&left.1).unwrap_or(Ordering::Equal))
        .then(left.2.cmp(right.2))
}

/// R080–R088 for one stop: the family decisions in (mode, line, family) order.
pub(crate) fn decide<'a>(
    settings: &Settings,
    constants: &Constants,
    pack: &'a Pack,
    points: &[Point<'a>],
    consolidated: &Consolidated<'a>,
) -> Vec<FamilyOutcome> {
    let contexts = &pack.contexts;
    let hypotheses = &consolidated.hypotheses;
    let rows = with_alternatives(settings, &consolidated.rows);
    let modes = effective_modes(constants, points, hypotheses);
    let isolation: Vec<f64> = hypotheses
        .iter()
        .enumerate()
        .map(|(index, hypothesis)| {
            let own = &points[hypothesis.medoid];
            let nearest = hypotheses
                .iter()
                .enumerate()
                .filter(|&(other, _)| other != index)
                .map(|(_, other)| {
                    let point = &points[other.medoid];
                    isolation_distance(
                        constants,
                        own.latitude,
                        own.longitude,
                        point.latitude,
                        point.longitude,
                    )
                })
                .min_by(f64::total_cmp);
            match nearest {
                None => 0.0,
                Some(_) if settings.minimum_separation_metres == 0.0 => 0.0,
                Some(distance) => {
                    settings.maximum_isolation_adjustment
                        * (distance / settings.minimum_separation_metres).min(1.0)
                }
            }
        })
        .collect();
    let scored: Vec<Scored> = rows
        .iter()
        .map(|row| {
            let hypothesis = &hypotheses[row.candidate];
            let mode = &*contexts.mode[row.context as usize];
            let lifecycle = if hypothesis.obsolete && !hypothesis.current {
                Some("obsolete-lifecycle")
            } else if mode_incompatible(mode, &modes[row.candidate]) {
                Some("mode-incompatible")
            } else {
                None
            };
            let rejected = hard_gate(
                settings,
                lifecycle.or(row.failure),
                row.corridor_distance,
                row.offset,
                row.excess,
            )
            .is_some();
            let routed = routed_fit_of(settings, row.excess);
            let geometry = evidence_score(
                settings,
                constants,
                row.corridor_distance,
                row.offset,
                row.corridor_heading,
                row.attachment_heading,
                routed,
            );
            let source = settings
                .maximum_source_support_adjustment
                .min((hypothesis.support - 1.0).max(0.0) * settings.source_support_per_weight);
            let compatible = |value: &&str| mode_compatible(mode, value);
            let modality = if hypothesis.denied_modes.iter().any(compatible) {
                settings.conflict_adjustment
            } else if hypothesis.explicit_modes.iter().any(compatible) {
                settings.explicit_support_adjustment
            } else if hypothesis.explicit_modes.is_empty() {
                settings.estimated_support_adjustment
            } else {
                0.0
            };
            let support = (source + modality).clamp(
                settings.conflict_adjustment,
                settings.maximum_combined_supporting_adjustment,
            );
            Scored {
                row,
                rejected,
                geometry,
                final_score: supporting_score(geometry, support),
                routed,
            }
        })
        .collect();

    let key = |row: &Row| -> ContextKey<'a> {
        let c = row.context as usize;
        (
            &contexts.mode[c],
            &contexts.line_id[c],
            contexts.route_distinction[c],
            contexts.direction[c],
            &contexts.pattern_hash[c],
            contexts.pattern_position[c],
            contexts.role[c],
            &contexts.movement_family_id[c],
        )
    };
    let id = |candidate: usize| hypotheses[candidate].id.as_str();

    // R084: the winner of each context key: (candidate, final, lead).
    let mut winners: BTreeMap<ContextKey<'a>, (usize, f64, f64)> = BTreeMap::new();
    for group in group_by(scored.iter(), |scored| key(scored.row)) {
        let mut ranked: Vec<&&Scored> = group.iter().filter(|scored| !scored.rejected).collect();
        ranked.sort_by(|a, b| {
            by_score(
                (a.final_score, a.geometry, id(a.row.candidate)),
                (b.final_score, b.geometry, id(b.row.candidate)),
            )
        });
        let Some(winner) = ranked.first() else {
            continue;
        };
        let lead = ranked
            .get(1)
            .map(|runner| winner.final_score - runner.final_score)
            .unwrap_or(winner.final_score);
        winners.insert(
            key(winner.row),
            (winner.row.candidate, winner.final_score, lead),
        );
    }

    let mut outcomes: Vec<FamilyOutcome> = Vec::new();
    for family_rows in group_by(scored.iter(), |scored| {
        &*contexts.movement_family_id[scored.row.context as usize]
    }) {
        let first = family_rows[0].row.context;
        let family = &*contexts.movement_family_id[first as usize];
        let mut family_keys: Vec<ContextKey> = Vec::new();
        for scored in &family_rows {
            let k = key(scored.row);
            if !family_keys.contains(&k) {
                family_keys.push(k);
            }
        }
        let family_winners: Vec<(usize, f64, f64)> = family_keys
            .iter()
            .filter_map(|k| winners.get(k).copied())
            .collect();
        // R085: contexts of other families won by each hypothesis, capped.
        let mut external: BTreeMap<usize, usize> = BTreeMap::new();
        for (k, &(candidate, _, _)) in &winners {
            if k.7 != family {
                *external.entry(candidate).or_insert(0) += 1;
            }
        }
        for count in external.values_mut() {
            *count = (*count).min(constants.heuristic.popularity_context_cap);
        }
        let mut candidates: Vec<FamilyCandidate> =
            group_by(family_rows.iter().filter(|s| !s.rejected), |s| {
                s.row.candidate
            })
            .into_iter()
            .map(|values| {
                let candidate = values[0].row.candidate;
                let geometry = median(values.iter().map(|s| s.geometry).collect());
                let final_score = median(values.iter().map(|s| s.final_score).collect());
                let popularity = settings.maximum_popularity_adjustment.min(
                    external.get(&candidate).copied().unwrap_or(0) as f64
                        * settings.per_context_adjustment,
                );
                let final_score = supporting_score(final_score, popularity);
                let final_score = supporting_score(final_score, isolation[candidate]);
                let routed = median(values.iter().map(|s| s.routed).collect());
                let faces: BTreeSet<&str> = values.iter().filter_map(|s| s.row.face).collect();
                let alternatives = values.iter().any(|s| s.row.alternative_count > 1);
                let agree = values
                    .iter()
                    .all(|s| s.row.alternative_count <= 1 || s.row.tied_agree);
                FamilyCandidate {
                    candidate,
                    geometry,
                    final_score,
                    routed,
                    faces,
                    alternatives,
                    agree,
                }
            })
            .collect();
        candidates.sort_by(|a, b| {
            by_score(
                (a.final_score, a.geometry, id(a.candidate)),
                (b.final_score, b.geometry, id(b.candidate)),
            )
        });
        let physical = candidates.first().and_then(|top| {
            let FamilyCandidate {
                candidate,
                geometry,
                final_score,
                routed,
                faces,
                alternatives,
                agree,
            } = top;
            let runner = candidates.get(1);
            let margin = runner.map_or(*final_score, |r| final_score - r.final_score);
            let geometry_margin = runner.map_or(*geometry, |r| geometry - r.geometry);
            let routed_advantage = runner.map_or(*routed, |r| routed - r.routed);
            let support = family_winners
                .iter()
                .filter(|winner| winner.0 == *candidate)
                .count();
            let share = if family_keys.is_empty() {
                0.0
            } else {
                support as f64 / family_keys.len() as f64
            };
            let contradictory = family_winners.iter().any(|&(other, score, lead)| {
                other != *candidate
                    && score >= settings.minimum_per_context_score
                    && lead >= settings.minimum_per_context_lead
            });
            let established = external.get(candidate).copied().unwrap_or(0);
            let runner_up = external
                .iter()
                .filter(|(other, _)| *other != candidate)
                .map(|(_, &count)| count)
                .max()
                .unwrap_or(0);
            let established_strong = established as i64
                >= i64::from(settings.minimum_supporting_contexts)
                && established as f64 >= settings.minimum_runner_up_ratio * runner_up.max(1) as f64
                && geometry_margin >= -settings.maximum_geometry_disadvantage
                && !contradictory
                && (!alternatives || *agree);
            let context_count = family_keys.len() as i64;
            let consensus = context_count >= i64::from(settings.minimum_contexts)
                && share >= settings.minimum_winning_share
                && !contradictory;
            let material_routed = routed_advantage >= settings.material_routed_advantage
                && geometry_margin >= -constants.heuristic.geometry_margin_tolerance;
            let eligible = *final_score >= settings.minimum_physical_score
                && (margin >= settings.minimum_physical_margin || material_routed)
                && !contradictory
                && (context_count < i64::from(settings.minimum_contexts)
                    || consensus
                    || established_strong)
                && (!alternatives || *agree);
            eligible.then(|| {
                (
                    *candidate,
                    *final_score,
                    margin,
                    faces.first().map(|face| Box::<str>::from(*face)),
                )
            })
        });
        outcomes.push(FamilyOutcome {
            family: family.into(),
            first_context: first,
            physical,
        });
    }
    outcomes.sort_by(|a, b| {
        let (ca, cb) = (a.first_context as usize, b.first_context as usize);
        (&contexts.mode[ca], &contexts.line_id[ca], &a.family).cmp(&(
            &contexts.mode[cb],
            &contexts.line_id[cb],
            &b.family,
        ))
    });
    outcomes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(absolute: f64, relative: f64, maximum: i32) -> Settings {
        let text = format!(
            r#"{{"schema_version": 2, "policy_id": "p",
            "consolidation": {{"maximum_diameter_metres": 2, "distinguished_context_minimum_count": 2}},
            "geometry_weights": {{"alignment": 1.0}},
            "hard_gates": {{"maximum_corridor_distance_metres": 1, "maximum_routed_excess_metres": 1}},
            "consensus": {{"minimum_contexts": 1}},
            "established_post": {{"minimum_supporting_contexts": 1, "minimum_runner_up_ratio": 1.0}},
            "alternative_corridors": {{"absolute_tie_metres": {absolute}, "relative_tie_fraction": {relative},
                                       "maximum_variants": {maximum}}}}}"#
        );
        let document = jr_formats::post_policy::parse(text.as_bytes()).unwrap();
        Settings::new(&document.policy).unwrap()
    }

    #[test]
    fn post_r080_alternative_corridor_thresholds_reinterpret_captured_costs() {
        let facts = [
            (0, Some(0.0), Some(0.0)),
            (1, Some(20.0), Some(0.20)),
            (2, Some(100.0), Some(0.01)),
        ];
        assert_eq!(
            selected_ranks(&settings(20.0, 0.02, 3), &facts),
            vec![0, 1, 2]
        );
        assert_eq!(selected_ranks(&settings(1.0, 0.001, 3), &facts), vec![0]);
        assert_eq!(selected_ranks(&settings(20.0, 0.02, 2), &facts), vec![0, 1]);
    }
}
