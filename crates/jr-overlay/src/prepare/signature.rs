//! Signatures of call patterns (OVL-R055) and the time features trips are ranked by (the
//! features of R069 and the rank of R070, which trip matching declares; context inference, R044,
//! ranks by them too).

use jr_model::Rule;

/// A trip's signatures compare its place pattern and its times rounded down to the profile's
/// resolution: `full` every call's arrival and departure, `endpoints` the first departure and the
/// last arrival, `first` the first departure only. They are compared directly, not hashed; equal
/// signatures are equal patterns with equal rounded times.
pub const OVL_R055: Rule = Rule {
    id: "OVL-R055",
    summary: "signatures compare place patterns and times rounded to the profile's resolution",
};

pub const RULES: &[Rule] = &[OVL_R055];

/// A time present for scoring and signatures: GTFS times are never negative, so a negative one
/// (a folded arrival before midnight, R056) counts as absent, as in the reference.
fn present(time: Option<i32>) -> Option<i32> {
    time.filter(|&time| time >= 0)
}

/// R055: a time rounded down to `resolution` seconds.
pub fn rounded(time: Option<i32>, resolution: u32) -> Option<i32> {
    let resolution = resolution as i32;
    time.map(|time| time / resolution * resolution)
}

/// Times of one call sequence.
#[derive(Clone, Copy, Debug)]
pub struct Times<'t> {
    pub arrivals: &'t [Option<i32>],
    pub departures: &'t [Option<i32>],
}

impl Times<'_> {
    /// R055 `full`: every call's rounded arrival and departure are equal (patterns compared by
    /// the caller).
    pub fn full_equal(&self, other: &Times<'_>, resolution: u32) -> bool {
        let same = |a: &[Option<i32>], b: &[Option<i32>]| {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(x, y)| rounded(*x, resolution) == rounded(*y, resolution))
        };
        same(self.arrivals, other.arrivals) && same(self.departures, other.departures)
    }

    /// R055 `first`: equal rounded first departures.
    pub fn first_equal(&self, other: &Times<'_>, resolution: u32) -> bool {
        rounded(self.departures.first().copied().flatten(), resolution)
            == rounded(other.departures.first().copied().flatten(), resolution)
    }

    /// R055 `endpoints`: equal rounded first departures and last arrivals.
    pub fn endpoints_equal(&self, other: &Times<'_>, resolution: u32) -> bool {
        self.first_equal(other, resolution)
            && rounded(self.arrivals.last().copied().flatten(), resolution)
                == rounded(other.arrivals.last().copied().flatten(), resolution)
    }
}

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

/// R069: the features of aligning `source` to `target`, `alignment[target call]` being the
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
