//! `service_calendar` and `service_exception`: each published date set as a weekday mask over
//! its first..last date plus the dates where set and mask disagree.
//!
//! The mask is fixed by the set: a weekday's bit is set iff that weekday occurs in the range
//! and at least half of its occurrences there are in the set.

use jiff::civil::Date;
use jr_model::handle::Handle;
use jr_model::{DateSet, PublicId, ServiceId, identity};

use super::PublishError;
use super::assemble::{self, boolean, date, int16, text};
use super::scope::Published;
use crate::table::Table;

/// One published service.
#[derive(Debug)]
struct Service {
    id: PublicId,
    valid_from: Date,
    valid_to: Date,
    mask: i16,
    /// `(date, added)`, in date order.
    exceptions: Vec<(Date, bool)>,
}

#[derive(Debug)]
pub struct Services {
    /// By `ServiceId` index; `None` for a service no published trip uses.
    by_handle: Vec<Option<usize>>,
    services: Vec<Service>,
}

impl Services {
    pub fn of(published: &Published) -> Services {
        let t = published.t;
        let mut by_handle = vec![None; t.services.len()];
        let mut used = vec![false; t.services.len()];
        for &id in &published.trips {
            used[t.trips[id].service.index()] = true;
        }
        for &index in &published.claims {
            if let Some(dates) = t.connection_claims[index].dates {
                used[dates.index()] = true;
            }
        }
        let mut services = Vec::new();
        for (handle, set) in t.services.iter() {
            if !used[handle.index()] {
                continue;
            }
            by_handle[handle.index()] = Some(services.len());
            services.push(service(published, set));
        }
        Services {
            by_handle,
            services,
        }
    }

    /// The published ID of a service a published trip or claim uses.
    pub fn id(&self, handle: ServiceId) -> Option<&str> {
        self.by_handle[handle.index()].map(|index| self.services[index].id.as_str())
    }

    pub fn calendar(&self) -> Result<Table, PublishError> {
        let rows = &self.services;
        assemble::finish(
            "service_calendar",
            vec![
                ("service_id", text(rows.iter().map(|s| Some(s.id.as_str())))),
                ("valid_from", date(rows.iter().map(|s| Some(s.valid_from)))),
                ("valid_to", date(rows.iter().map(|s| Some(s.valid_to)))),
                ("weekday_mask", int16(rows.iter().map(|s| Some(s.mask)))),
            ],
        )
    }

    pub fn exceptions(&self) -> Result<Table, PublishError> {
        let rows: Vec<(&str, Date, bool)> = self
            .services
            .iter()
            .flat_map(|s| {
                s.exceptions
                    .iter()
                    .map(|&(day, added)| (s.id.as_str(), day, added))
            })
            .collect();
        assemble::finish(
            "service_exception",
            vec![
                ("service_id", text(rows.iter().map(|r| Some(r.0)))),
                ("service_date", date(rows.iter().map(|r| Some(r.1)))),
                ("added", boolean(rows.iter().map(|r| Some(r.2)))),
            ],
        )
    }
}

fn service(published: &Published, set: DateSet) -> Service {
    let t = published.t;
    let horizon = t.horizon;
    // Only used services are built, and a used service is not empty.
    let (first, last) = (set.first().unwrap_or(0), set.last().unwrap_or(0));
    let mut occurrences = [0_u32; 7];
    let mut running = [0_u32; 7];
    for day in first..=last {
        let weekday = usize::from(horizon.weekday_at(day));
        occurrences[weekday] += 1;
        if set.contains(day) {
            running[weekday] += 1;
        }
    }
    let mask: i16 = (0..7)
        .filter(|&w| occurrences[w] > 0 && 2 * running[w] >= occurrences[w])
        .map(|w| 1 << w)
        .sum();
    let exceptions = (first..=last)
        .filter_map(|day| {
            let in_mask = mask & (1 << horizon.weekday_at(day)) != 0;
            let in_set = set.contains(day);
            (in_mask != in_set).then(|| (horizon.date_at(day), in_set))
        })
        .collect();
    Service {
        id: identity::service(t.feed, &set),
        valid_from: horizon.date_at(first),
        valid_to: horizon.date_at(last),
        mask,
        exceptions,
    }
}
