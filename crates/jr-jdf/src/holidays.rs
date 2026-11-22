//! The public-holiday table (JDF-R265) and the Easter computus (JDF-R266), with the other day
//! classes of a horizon the calendar ladder asks about: weekdays and ISO week parity.

use jiff::Span;
use jr_model::{Date, DateSet, Horizon, Rule};

use crate::policy::Policy;

/// The day attributes `+` and `X` depend on Czech public holidays (zákon 245/2000 Sb.): eleven
/// fixed dates and two relative to Easter. The table is data (`[holidays]` in policy), so a
/// change in the law is a policy change; a date applies from its `since` year, where given.
pub const R265: Rule = Rule {
    id: "JDF-R265",
    summary: "Czech public holidays are the policy table of fixed and Easter-relative dates",
};

/// Good Friday and Easter Monday follow Easter Sunday. F# used the "New Scientist" algorithm
/// with month arithmetic that treats every month as 31 days and crashed on "April 0" (Easter on
/// 2 April or 30 March, first in 2051). The anonymous Gregorian computus (Meeus/Jones/Butcher)
/// gives Easter Sunday; holidays are whole-day offsets from it in calendar arithmetic.
pub const R266: Rule = Rule {
    id: "JDF-R266",
    summary: "Easter Sunday by the Gregorian computus; Easter holidays are day offsets from it",
};

/// Easter Sunday of a Gregorian year (JDF-R266).
pub fn easter_sunday(year: i16) -> Option<Date> {
    let y = i32::from(year);
    let a = y % 19;
    let b = y / 100;
    let c = y % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = (h + l - 7 * m + 114) % 31 + 1;
    Date::new(year, month as i8, day as i8).ok()
}

/// The public holidays of one year, in date order (JDF-R265, R266).
pub fn holidays_of_year(policy: &Policy, year: i16) -> Vec<Date> {
    let applies = |since: Option<i16>| since.is_none_or(|since| year >= since);
    let table = &policy.holidays;
    let mut dates: Vec<Date> = table
        .fixed
        .iter()
        .filter(|holiday| applies(holiday.since))
        .filter_map(|holiday| Date::new(year, holiday.month, holiday.day).ok())
        .collect();
    if let Some(easter) = easter_sunday(year) {
        dates.extend(
            table
                .easter
                .iter()
                .filter(|holiday| applies(holiday.since))
                .filter_map(|holiday| easter.checked_add(Span::new().days(holiday.offset)).ok()),
        );
    }
    dates.sort();
    dates.dedup();
    dates
}

/// The day classes of one horizon: its public holidays, its days of each weekday and its days in
/// odd ISO weeks. Built once per horizon and shared by every trip's calendar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Holidays {
    horizon: Horizon,
    holidays: DateSet,
    /// Monday first.
    weekdays: [DateSet; 7],
    odd_iso_weeks: DateSet,
}

impl Holidays {
    pub fn new(policy: &Policy, horizon: Horizon) -> Holidays {
        let mut holidays = DateSet::EMPTY;
        for year in horizon.start().year()..=horizon.end().year() {
            let (set, _) = DateSet::from_dates(horizon, holidays_of_year(policy, year));
            holidays = holidays | set;
        }
        let mut weekdays = [DateSet::EMPTY; 7];
        let mut odd_iso_weeks = DateSet::EMPTY;
        for index in 0..horizon.days() {
            let date = horizon.date_at(index);
            weekdays[date.weekday().to_monday_zero_offset() as usize].insert(index);
            if date.iso_week_date().week() % 2 == 1 {
                odd_iso_weeks.insert(index);
            }
        }
        Holidays {
            horizon,
            holidays,
            weekdays,
            odd_iso_weeks,
        }
    }

    pub fn horizon(&self) -> Horizon {
        self.horizon
    }

    /// The public holidays inside the horizon.
    pub fn holidays(&self) -> DateSet {
        self.holidays
    }

    pub fn dates(&self) -> impl Iterator<Item = Date> + use<> {
        self.holidays.dates(self.horizon)
    }

    /// The days of a weekday, 0 = Monday … 6 = Sunday.
    pub fn weekday(&self, monday_zero: usize) -> DateSet {
        self.weekdays[monday_zero]
    }

    pub fn odd_iso_weeks(&self) -> DateSet {
        self.odd_iso_weeks
    }

    pub fn even_iso_weeks(&self) -> DateSet {
        DateSet::full(self.horizon.days()) - self.odd_iso_weeks
    }
}
