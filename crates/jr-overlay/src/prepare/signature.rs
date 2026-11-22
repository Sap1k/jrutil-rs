//! Signatures of call patterns (OVL-R055). The time features and rank trips are compared by
//! are trip matching's (R069, R070, `crate::matching::score`).

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
pub(crate) fn present(time: Option<i32>) -> Option<i32> {
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
