//! The time features of a candidate (OVL-R069) and the rank and margin that order candidates
//! (R070). Context inference (R044) ranks by them too.

use jr_model::Rule;

use crate::prepare::Times;
use crate::prepare::signature_present as present;

/// A candidate's time features: the first-departure delta, the duration delta (first departure
/// to last arrival), and over aligned call pairs with both times the sum, maximum and sum of
/// squares of the arrival and departure deltas, and the count of aligned target calls. Seconds,
/// unrounded; a negative time (a folded arrival before midnight) counts as absent. A missing
/// delta is `i32::MAX / 4`, a missing sum `i64::MAX / 4`. The tolerance on the first-departure
/// delta is R133's.
///
/// The nearest base trip in time is the most likely twin of a source trip.
pub const OVL_R069: Rule = Rule {
    id: "OVL-R069",
    summary: "time features: first departure, duration, aligned deltas (sum, max, squares)",
};

/// Candidates rank lexicographically by (edits, first-departure delta, sum, duration delta,
/// maximum, squares, more aligned calls). The margin to the runner-up is the difference in the
/// first component that differs.
pub const OVL_R070: Rule = Rule {
    id: "OVL-R070",
    summary: "candidates rank by edits then time features; the margin is the first difference",
};

pub const RULES: &[Rule] = &[OVL_R069, OVL_R070];

/// The time features of a candidate (R069) and its rank (R070): lower is better.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Rank {
    pub edits: u32,
    pub first_departure_delta: i64,
    pub aggregate_delta: i64,
    pub duration_delta: i64,
    pub maximum_delta: i64,
    pub squared_delta: i64,
    /// The negated count of aligned target calls.
    pub negative_aligned: i64,
}

/// What a missing delta counts as.
pub const MISSING_DELTA: i64 = (i32::MAX / 4) as i64;
/// What a missing sum counts as.
pub const MISSING_SUM: i64 = i64::MAX / 4;

/// R069, R070: the features of aligning `source` to `target`, `alignment[target call]` being the
/// source call it aligns to.
pub fn rank(edits: u32, source: Times<'_>, target: Times<'_>, alignment: &[Option<usize>]) -> Rank {
    let first = |times: &[Option<i32>]| times.iter().copied().find_map(present);
    let last = |times: &[Option<i32>]| times.iter().rev().copied().find_map(present);
    let (first_source, first_target) = (first(source.departures), first(target.departures));
    let (last_source, last_target) = (last(source.arrivals), last(target.arrivals));
    let first_departure_delta = match (first_source, first_target) {
        (Some(s), Some(t)) => i64::from((s - t).abs()),
        _ => MISSING_DELTA,
    };
    let duration_delta = match (first_source, first_target, last_source, last_target) {
        (Some(fs), Some(ft), Some(ls), Some(lt)) => i64::from(((ls - fs) - (lt - ft)).abs()),
        _ => MISSING_DELTA,
    };
    let (mut aggregate, mut maximum, mut squared, mut comparable) = (0i64, 0i64, 0i64, 0);
    for (target_index, aligned) in alignment.iter().enumerate() {
        let Some(source_index) = *aligned else {
            continue;
        };
        if source_index >= source.arrivals.len() || target_index >= target.arrivals.len() {
            continue;
        }
        for (s, t) in [
            (source.arrivals[source_index], target.arrivals[target_index]),
            (
                source.departures[source_index],
                target.departures[target_index],
            ),
        ] {
            if let (Some(s), Some(t)) = (present(s), present(t)) {
                let delta = i64::from((s - t).abs());
                aggregate += delta;
                maximum = maximum.max(delta);
                squared += delta * delta;
                comparable += 1;
            }
        }
    }
    if comparable == 0 {
        aggregate = MISSING_SUM;
        squared = MISSING_SUM;
    }
    Rank {
        edits,
        first_departure_delta,
        aggregate_delta: aggregate,
        duration_delta,
        maximum_delta: maximum,
        squared_delta: squared,
        negative_aligned: -(alignment.iter().filter(|a| a.is_some()).count() as i64),
    }
}

/// R070: the margin from `best` to a strictly worse `runner`.
pub fn margin(best: &Rank, runner: &Rank) -> i64 {
    if best.edits != runner.edits {
        i64::from(runner.edits) - i64::from(best.edits)
    } else if best.first_departure_delta != runner.first_departure_delta {
        runner.first_departure_delta - best.first_departure_delta
    } else if best.aggregate_delta != runner.aggregate_delta {
        runner.aggregate_delta - best.aggregate_delta
    } else if best.duration_delta != runner.duration_delta {
        runner.duration_delta - best.duration_delta
    } else if best.maximum_delta != runner.maximum_delta {
        runner.maximum_delta - best.maximum_delta
    } else if best.squared_delta != runner.squared_delta {
        runner.squared_delta - best.squared_delta
    } else {
        runner.negative_aligned - best.negative_aligned
    }
}
