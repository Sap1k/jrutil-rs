//! A regional source's trains, keyed onto the national rail timetable (OVL-R132).
//!
//! Heavy rail never enters an overlay (OVL-R013), so a regional feed's train trips have no
//! output trip of their own. Where the source's crosswalk (R019) gives a train's line and course
//! and the course is its train number, the source's trip ID still names a national train: the
//! one with that number on each date the line/course runs. This pass publishes those keys on the
//! rail package, in the namespace R131 uses on the road package, so a realtime feed that names
//! trips by the source's IDs finds its trains as literally as its buses.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use jr_formats::gtfs::Feed;
use jr_model::timetable::{BindingMethod, Entity, Mode, Namespace, SourceKey};
use jr_model::{DateSet, Diagnostics, Edit, EditBuilder, Edits, Rule, Timetable, TripId};
use serde::Serialize;

use crate::prepare::{crosswalk, services};
use crate::profile::{BaseProfile, SourceProfile};

/// A source trip ID of a rail route with a crosswalk row whose course is all digits keys each
/// national trip with that `czptt:train_number`, on the dates both the trip and the source's
/// line/course run (`<namespace>:gtfs_trip_line_course`, `operator_crosswalk`).
pub const OVL_R132: Rule = Rule {
    id: "OVL-R132",
    summary: "a source's crosswalked train trip IDs key the national trains of their course",
};

pub const RULES: &[Rule] = &[OVL_R132];

/// The pass name recorded in the timetable's history.
pub const PASS: &str = "overlay-train-keys";

/// What the pass found, for the package's diagnostics summary.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    /// Source trips of rail routes.
    pub rail_trips: usize,
    /// Of those, with a crosswalk row whose course is a train number.
    pub numbered: usize,
    /// Of those, keyed to at least one national trip.
    pub keyed: usize,
    /// Distinct line/course pairs numbered, and those no national train runs for on any date.
    pub courses: usize,
    pub courses_without_train: usize,
    pub keys_added: usize,
}

/// R132 over one source: the keys, as edits to the rail timetable.
pub fn train_keys(
    timetable: &Timetable,
    profile: &SourceProfile,
    feed: &Feed,
    diagnostics: &mut Diagnostics,
) -> (Edits, Summary) {
    let mut summary = Summary::default();
    let horizon = timetable.horizon;
    let road = BaseProfile::road().expect("the built-in road profile parses");
    let rail_routes: BTreeSet<&str> = feed
        .routes
        .iter()
        .filter(|route| road.mode_of_route_type(route.route_type) == Some(Mode::Rail))
        .map(|route| &*route.route_id)
        .collect();
    let days = services(feed, horizon);
    let dates_of = |trip: u32| -> DateSet {
        let trip = &feed.trips[trip as usize];
        days.get(&*trip.service_id).copied().unwrap_or_default()
    };

    // The source's numbered train trips and the dates of each line/course.
    let mut rows: Vec<(u32, &str, &str)> = Vec::new();
    let mut courses: BTreeMap<(&str, &str), DateSet> = BTreeMap::new();
    let crosswalk = crosswalk(profile, feed, diagnostics);
    for row in &crosswalk {
        if !rail_routes.contains(&*feed.trips[row.trip as usize].route_id) {
            continue;
        }
        if !row.course.bytes().all(|b| b.is_ascii_digit()) || row.course.is_empty() {
            continue;
        }
        rows.push((row.trip, &row.line, &row.course));
        let dates = courses.entry((&row.line, &row.course)).or_default();
        *dates = *dates | dates_of(row.trip);
    }
    summary.rail_trips = feed
        .trips
        .iter()
        .filter(|trip| rail_routes.contains(&*trip.route_id))
        .count();
    summary.numbered = rows.len();
    summary.courses = courses.len();

    // The national trains by number, with the dates each trip runs under it.
    let mut trains: HashMap<&str, Vec<(TripId, DateSet)>> = HashMap::new();
    for key in &timetable.keys {
        let Entity::Trip(trip) = key.entity else {
            continue;
        };
        if key.namespace != Namespace::CZPTT_TRAIN_NUMBER {
            continue;
        }
        let runs = timetable.services.get(timetable.trips[trip].service)
            & DateSet::range(horizon, key.valid_from, key.valid_to);
        if !runs.is_empty() {
            trains
                .entry(&key.identifier)
                .or_default()
                .push((trip, runs));
        }
    }
    for found in trains.values_mut() {
        found.sort_by_key(|(trip, _)| *trip);
    }

    let namespace = Namespace(format!("{}:gtfs_trip_line_course", profile.key_namespace).into());
    let mut keys: BTreeSet<SourceKey> = BTreeSet::new();
    let mut matched: BTreeSet<(&str, &str)> = BTreeSet::new();
    for &(trip, line, course) in &rows {
        let dates = courses[&(line, course)];
        let mut keyed = false;
        for (national, runs) in trains.get(course).map_or(&[][..], Vec::as_slice) {
            for (from, to) in contiguous(dates & *runs) {
                keyed = true;
                keys.insert(SourceKey {
                    namespace: namespace.clone(),
                    identifier: feed.trips[trip as usize].trip_id.clone(),
                    entity: Entity::Trip(*national),
                    valid_from: horizon.date_at(from),
                    valid_to: horizon.date_at(to),
                    binding: BindingMethod::OperatorCrosswalk,
                });
            }
        }
        if keyed {
            summary.keyed += 1;
            matched.insert((line, course));
        }
    }
    summary.courses_without_train = summary.courses - matched.len();
    summary.keys_added = keys.len();
    let mut edits = EditBuilder::new(timetable, PASS);
    for key in keys {
        edits.push(OVL_R132, Edit::AddKey(key));
    }
    (edits.finish(), summary)
}

/// Contiguous runs of a date set, as day offsets.
fn contiguous(dates: DateSet) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for day in dates.iter() {
        match runs.last_mut() {
            Some((_, last)) if *last + 1 == day => *last = day,
            _ => runs.push((day, day)),
        }
    }
    runs
}
