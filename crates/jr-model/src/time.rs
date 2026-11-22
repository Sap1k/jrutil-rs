//! Service time and the timetable horizon.
//!
//! A time is seconds from local midnight of the service date, as in GTFS `stop_times` and the
//! serving contract: it equals the wall-clock reading except on DST change days, and values from
//! 86,400 up run into the next day. There are no timestamps in the model.

use jiff::civil::Weekday;
use serde::{Deserialize, Serialize};

use crate::dateset::CAPACITY;

pub type Date = jiff::civil::Date;

/// Seconds from local midnight of the service date. May be negative (a train that leaves the
/// day before its service date) or above a day.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ServiceTime(i32);

impl ServiceTime {
    pub const DAY: i32 = 86_400;

    pub const fn from_seconds(seconds: i32) -> Self {
        Self(seconds)
    }

    pub const fn from_hms(hours: i32, minutes: i32, seconds: i32) -> Self {
        Self(hours * 3600 + minutes * 60 + seconds)
    }

    pub const fn seconds(self) -> i32 {
        self.0
    }

    pub const fn plus(self, seconds: i32) -> Self {
        Self(self.0 + seconds)
    }
}

/// An optional [`ServiceTime`] in four bytes, for the call table.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "Option<ServiceTime>", into = "Option<ServiceTime>")]
pub struct OptTime(i32);

impl OptTime {
    pub const NONE: Self = Self(i32::MIN);

    pub const fn some(time: ServiceTime) -> Self {
        Self(time.0)
    }

    pub const fn get(self) -> Option<ServiceTime> {
        if self.0 == i32::MIN {
            None
        } else {
            Some(ServiceTime(self.0))
        }
    }

    pub const fn is_some(self) -> bool {
        self.0 != i32::MIN
    }
}

impl From<Option<ServiceTime>> for OptTime {
    fn from(time: Option<ServiceTime>) -> Self {
        time.map_or(Self::NONE, Self::some)
    }
}

impl From<OptTime> for Option<ServiceTime> {
    fn from(time: OptTime) -> Self {
        time.get()
    }
}

impl std::fmt::Debug for OptTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.get() {
            Some(time) => write!(f, "{}", time.seconds()),
            None => f.write_str("-"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HorizonError {
    #[error("horizon ends ({end}) before it starts ({start})")]
    Reversed { start: Date, end: Date },
    #[error("horizon of {days} days exceeds the {CAPACITY} a date set holds")]
    TooLong { days: i64 },
    #[error("no timetable year {0}")]
    Year(i16),
}

/// The inclusive range of service dates a timetable covers. Every [`crate::DateSet`] of the
/// timetable is a set of day offsets from its start.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Horizon {
    start: Date,
    days: u16,
}

impl Horizon {
    pub fn new(start: Date, end: Date) -> Result<Self, HorizonError> {
        if end < start {
            return Err(HorizonError::Reversed { start, end });
        }
        let days = i64::from(days_between(start, end)) + 1;
        if days > CAPACITY as i64 {
            return Err(HorizonError::TooLong { days });
        }
        Ok(Self {
            start,
            days: days as u16,
        })
    }

    /// The timetable year (GVD) `year`: from the second Sunday of December of the year before
    /// to the day before the second Sunday of December of `year`. GVD 2026 is 2025-12-14
    /// through 2026-12-12.
    pub fn gvd(year: i16) -> Result<Self, HorizonError> {
        let change = |december_of: i16| {
            Date::new(december_of, 12, 1)
                .and_then(|first| first.nth_weekday_of_month(2, Weekday::Sunday))
                .map_err(|_| HorizonError::Year(year))
        };
        let start = change(year - 1)?;
        let end = change(year)?
            .yesterday()
            .map_err(|_| HorizonError::Year(year))?;
        Self::new(start, end)
    }

    /// The same horizon with `before` and `after` extra days, for sources whose runs start
    /// the day before their service date or end past the last one.
    pub fn widened(self, before: u16, after: u16) -> Result<Self, HorizonError> {
        let start = shift(self.start, -i32::from(before));
        let end = shift(self.end(), i32::from(after));
        Self::new(start, end)
    }

    pub fn start(self) -> Date {
        self.start
    }

    pub fn end(self) -> Date {
        shift(self.start, i32::from(self.days) - 1)
    }

    pub fn days(self) -> usize {
        usize::from(self.days)
    }

    pub fn contains(self, date: Date) -> bool {
        self.index_of(date).is_some()
    }

    /// The day offset of `date`, or `None` outside the horizon.
    pub fn index_of(self, date: Date) -> Option<usize> {
        let offset = days_between(self.start, date);
        (0..i32::from(self.days))
            .contains(&offset)
            .then_some(offset as usize)
    }

    /// The date at a day offset inside the horizon.
    pub fn date_at(self, index: usize) -> Date {
        debug_assert!(index < self.days());
        shift(self.start, index as i32)
    }

    /// The weekday of a day offset: 0 is Monday, 6 is Sunday.
    pub fn weekday_at(self, index: usize) -> u8 {
        self.date_at(index).weekday().to_monday_zero_offset() as u8
    }
}

fn days_between(from: Date, to: Date) -> i32 {
    // Civil dates are at most a few thousand years apart; the span is always representable.
    from.until(to).expect("civil dates are in range").get_days()
}

fn shift(date: Date, days: i32) -> Date {
    date.checked_add(jiff::Span::new().days(days))
        .expect("a horizon stays inside jiff's range")
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn gvd_runs_between_second_december_sundays() {
        let gvd = Horizon::gvd(2026).unwrap();
        assert_eq!(gvd.start(), date(2025, 12, 14));
        assert_eq!(gvd.end(), date(2026, 12, 12));
        assert_eq!(gvd.days(), 364);
        // 2024 has its second December Sunday on the 8th, 2023 on the 10th: a 52-week year.
        let gvd = Horizon::gvd(2024).unwrap();
        assert_eq!(
            (gvd.start(), gvd.end()),
            (date(2023, 12, 10), date(2024, 12, 7))
        );
    }

    #[test]
    fn offsets_and_dates_round_trip() {
        let gvd = Horizon::gvd(2026).unwrap();
        assert_eq!(gvd.index_of(date(2025, 12, 14)), Some(0));
        assert_eq!(gvd.index_of(date(2025, 12, 13)), None);
        assert_eq!(gvd.index_of(date(2026, 12, 13)), None);
        assert_eq!(gvd.date_at(363), date(2026, 12, 12));
        assert_eq!(gvd.weekday_at(0), 6);
        let wide = gvd.widened(1, 2).unwrap();
        assert_eq!(
            (wide.start(), wide.end()),
            (date(2025, 12, 13), date(2026, 12, 14))
        );
    }
}
