//! Source calendars into the base horizon (OVL-R029), the feed window (R020) and the
//! after-midnight fold (R056).

use std::collections::HashMap;

use jr_formats::gtfs::Feed;
use jr_model::{Date, DateSet, Horizon, Rule};

/// A service's days inside the base horizon: `calendar.txt` rows set each day of their range to
/// the weekday flag (`1` runs), later rows of a service overwriting earlier ones; then
/// `calendar_dates.txt` sets a day to whether its exception type is `1`. Days outside the
/// horizon are dropped.
///
/// GTFS semantics; the overlay compares calendars as date sets over the base's horizon.
pub const OVL_R029: Rule = Rule {
    id: "OVL-R029",
    summary: "GTFS calendars become date sets over the base horizon",
};

/// The dates a source publishes: the first `feed_info.txt` row's start and end, else the extent
/// of its calendar ranges and added dates. What an integrated-system scope covers (R062).
///
/// A feed's window says how far its silence about a line can be read as "not running".
pub const OVL_R020: Rule = Rule {
    id: "OVL-R020",
    summary: "a source's feed window: feed_info's dates, else its calendar's extent",
};

/// A source trip whose first call departs at or after 24:00 runs on the next day of its base:
/// its days move one later (days leaving the horizon are dropped and counted) and every call
/// time loses 24 hours. Applied when the base profile says the base dates trips by departure.
/// A first call arriving before midnight keeps its arrival as negative seconds of the new day.
///
/// GTFS dates a trip by its service day, so PID runs Monday night's 00:30 tram as Monday 24:30;
/// JDF dates it by departure: Tuesday 00:30.
pub const OVL_R056: Rule = Rule {
    id: "OVL-R056",
    summary: "a source trip departing after midnight moves to the next day, times less 24 h",
};

pub const RULES: &[Rule] = &[OVL_R020, OVL_R029, OVL_R056];

/// Seconds in a day.
pub const DAY: i32 = 86_400;

/// R029: every service of the feed with its days.
pub fn services(feed: &Feed, horizon: Horizon) -> HashMap<&str, DateSet> {
    let mut sets: HashMap<&str, DateSet> = HashMap::new();
    for row in &feed.calendars {
        let set = sets.entry(&row.service_id).or_default();
        let (Some(start), Some(end)) = (row.start_date, row.end_date) else {
            continue;
        };
        for index in DateSet::range(horizon, start, end).iter() {
            let weekday = usize::from(horizon.weekday_at(index));
            if row.weekdays[weekday] == Some(1) {
                set.insert(index);
            } else {
                set.remove(index);
            }
        }
    }
    for row in &feed.calendar_dates {
        let set = sets.entry(&row.service_id).or_default();
        let Some(index) = row.date.and_then(|date| horizon.index_of(date)) else {
            continue;
        };
        if row.exception_type == Some(1) {
            set.insert(index);
        } else {
            set.remove(index);
        }
    }
    sets
}

/// R020.
pub fn feed_window(feed: &Feed) -> Option<(Date, Date)> {
    if let Some(info) = &feed.feed_info
        && let (Some(start), Some(end)) = (info.feed_start_date, info.feed_end_date)
    {
        return Some((start, end));
    }
    let dates = feed
        .calendars
        .iter()
        .flat_map(|row| [row.start_date, row.end_date])
        .chain(
            feed.calendar_dates
                .iter()
                .filter(|row| row.exception_type == Some(1))
                .map(|row| row.date),
        )
        .flatten();
    let mut window: Option<(Date, Date)> = None;
    for date in dates {
        window = Some(match window {
            None => (date, date),
            Some((first, last)) => (first.min(date), last.max(date)),
        });
    }
    window
}
