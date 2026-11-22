//! Parquet `date32` (days since 1970-01-01) <-> `jiff::civil::Date`, without span arithmetic
//! per cell (H. Hinnant's civil-from-days algorithms).

use jiff::civil::Date;

/// The date `days` after 1970-01-01, `None` outside jiff's range.
pub fn from_days(days: i32) -> Option<Date> {
    let z = i64::from(days) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    Date::new(i16::try_from(year).ok()?, month as i8, day as i8).ok()
}

/// Days from 1970-01-01 to `date`.
pub fn to_days(date: Date) -> i32 {
    let month = i64::from(date.month());
    let year = i64::from(date.year()) - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(date.day()) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    // jiff's dates span about +-3.6 million days, well inside i32.
    (era * 146_097 + doe - 719_468) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_round_trip_across_eras_and_leap_days() {
        let epoch = Date::new(1970, 1, 1).expect("date");
        assert_eq!(from_days(0), Some(epoch));
        for days in [-719_468, -1, 0, 59, 60, 11_016, 20_513, 2_932_896] {
            let date = from_days(days).expect("in range");
            assert_eq!(to_days(date), days);
            assert_eq!(epoch.until(date).expect("span").get_days(), days);
        }
        assert_eq!(from_days(20_513), Date::new(2026, 3, 1).ok());
    }
}
