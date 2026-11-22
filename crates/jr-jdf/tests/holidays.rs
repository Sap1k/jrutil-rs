//! Public holidays (R265) and Easter (R266).

mod support;

use jiff::civil::date;
use jr_jdf::Policy;
use jr_jdf::holidays::{Holidays, easter_sunday, holidays_of_year};

#[test]
fn r266_easter_sunday() {
    // F# HolidayTests "Easter Sunday for 2020", and the years the F# arithmetic broke on.
    for (year, month, day) in [
        (2020, 4, 12),
        (2024, 3, 31),
        (2025, 4, 20),
        (2026, 4, 5),
        (2038, 4, 25),
        (2051, 4, 2),
        (2059, 3, 30),
        (2285, 3, 22),
    ] {
        assert_eq!(easter_sunday(year), Some(date(year, month, day)), "{year}");
    }
}

#[test]
fn r265_czech_holidays_for_2020() {
    // F# HolidayTests "Czech holidays for 2020".
    let expected: Vec<_> = [
        (1, 1),
        (4, 10),
        (4, 13),
        (5, 1),
        (5, 8),
        (7, 5),
        (7, 6),
        (9, 28),
        (10, 28),
        (11, 17),
        (12, 24),
        (12, 25),
        (12, 26),
    ]
    .into_iter()
    .map(|(month, day)| date(2020, month, day))
    .collect();
    assert_eq!(holidays_of_year(&Policy::builtin(), 2020), expected);
}

#[test]
fn r266_easter_holidays_across_month_ends() {
    // Easter on 2 April 2051 (Good Friday 31 March) and 30 March 2059 (Easter Monday 31 March):
    // F# built "April 0" and crashed.
    let policy = Policy::builtin();
    let y2051 = holidays_of_year(&policy, 2051);
    assert!(y2051.contains(&date(2051, 3, 31)) && y2051.contains(&date(2051, 4, 3)));
    let y2059 = holidays_of_year(&policy, 2059);
    assert!(y2059.contains(&date(2059, 3, 28)) && y2059.contains(&date(2059, 3, 31)));
}

#[test]
fn r265_good_friday_is_a_holiday_since_2016() {
    let policy = Policy::builtin();
    assert!(!holidays_of_year(&policy, 2015).contains(&date(2015, 4, 3)));
    assert!(holidays_of_year(&policy, 2016).contains(&date(2016, 3, 25)));
}

#[test]
fn r265_a_horizon_holds_the_holidays_of_both_its_years() {
    let holidays = Holidays::new(&Policy::builtin(), support::gvd());
    let dates: Vec<_> = holidays.dates().collect();
    assert_eq!(dates.len(), 3 + 10);
    assert_eq!(
        dates[..4],
        [
            date(2025, 12, 24),
            date(2025, 12, 25),
            date(2025, 12, 26),
            date(2026, 1, 1)
        ]
    );
    // 2026-12-24 is after the horizon's last day (12 December).
    assert_eq!(dates.last(), Some(&date(2026, 11, 17)));
}
