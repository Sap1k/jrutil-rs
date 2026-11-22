//! The one way to read "the time of a call" and the one interpolation by source index, shared by
//! the time repair (R037), the chronology check (R039) and edge timing (R048, R049).

use crate::path::Call;

/// When the train reaches the call: its arrival, else its departure.
pub(crate) fn reached(call: &Call) -> Option<i32> {
    call.arrival.or(call.departure)
}

/// When the train leaves the call: its departure, else its arrival.
pub(crate) fn leaves(call: &Call) -> Option<i32> {
    call.departure.or(call.arrival)
}

/// Both times are known.
pub(crate) fn is_timed(call: &Call) -> bool {
    call.arrival.is_some() && call.departure.is_some()
}

/// The time at source index `index`, linear in the index between `left` and `right`
/// (`(index, seconds)`), truncated toward zero. `None` unless `left` lies before `right` and
/// time does not go backwards between them.
pub(crate) fn interpolate(left: (u32, i32), right: (u32, i32), index: u32) -> Option<i32> {
    let span = i64::from(right.0) - i64::from(left.0);
    if span <= 0 || right.1 < left.1 {
        return None;
    }
    let offset = i64::from(index) - i64::from(left.0);
    let value = i64::from(left.1) + (i64::from(right.1) - i64::from(left.1)) * offset / span;
    i32::try_from(value).ok()
}

/// Copies a call's times as `arrival ?? departure`, `departure ?? arrival`.
pub(crate) fn mirrored(arrival: Option<i32>, departure: Option<i32>) -> (Option<i32>, Option<i32>) {
    (arrival.or(departure), departure.or(arrival))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolation_is_linear_in_the_index_and_truncates() {
        assert_eq!(interpolate((0, 0), (3, 100), 1), Some(33));
        assert_eq!(interpolate((2, 600), (4, 600), 3), Some(600));
        assert_eq!(interpolate((2, 600), (4, 500), 3), None);
        assert_eq!(interpolate((4, 0), (4, 10), 4), None);
    }
}
