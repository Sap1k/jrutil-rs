//! The direction of each train on its route (CZPTT-R121).

use std::collections::{BTreeMap, HashMap};

use jr_model::{Diagnostic, Diagnostics, PlaceId, RouteId, Rule, Severity, Timetable};

/// CZPTT gives a train no direction. Per route, the trip with the most distinct passenger
/// places (then the smallest public ID) runs in direction 0; a trip takes the direction of an
/// already directed trip it shares at least two passenger places with (the one sharing most,
/// then the smallest public ID), or the other one when it visits them in the reverse order (more
/// reversed than kept consecutive pairs of the shared places). Rounds repeat until nothing
/// changes; a trip still undecided (or whose comparison ties) has no direction, diagnosed.
///
/// A line's route stops are ordered per direction (serving verdict 1); without directions a
/// rail line's list held both directions of travel (`docs/roadmap.md`).
pub(crate) const R121: Rule = Rule {
    id: "CZPTT-R121",
    summary: "A train's direction follows the order of the stations it shares with its route's longest trip",
};

/// R121 over the whole timetable.
pub(crate) fn assign(timetable: &mut Timetable, diagnostics: &mut Diagnostics) {
    let mut by_route: BTreeMap<RouteId, Vec<usize>> = BTreeMap::new();
    for (index, trip) in timetable.trips.rows().iter().enumerate() {
        by_route.entry(trip.route).or_default().push(index);
    }
    let mut decided: Vec<(usize, Option<u8>)> = Vec::new();
    for trips in by_route.values() {
        let sequences: Vec<Vec<PlaceId>> = trips
            .iter()
            .map(|&index| {
                let mut seen = Vec::new();
                for call in &timetable.trips.rows()[index].calls {
                    if call.passenger && !seen.contains(&call.place) {
                        seen.push(call.place);
                    }
                }
                seen
            })
            .collect();
        let id = |at: usize| timetable.trips.rows()[trips[at]].id.as_str();
        let Some(reference) = (0..trips.len()).min_by(|&a, &b| {
            sequences[b]
                .len()
                .cmp(&sequences[a].len())
                .then_with(|| id(a).cmp(id(b)))
        }) else {
            continue;
        };
        let mut direction: Vec<Option<u8>> = vec![None; trips.len()];
        direction[reference] = Some(0);
        let positions: Vec<HashMap<PlaceId, usize>> = sequences
            .iter()
            .map(|places| places.iter().enumerate().map(|(i, &p)| (p, i)).collect())
            .collect();
        loop {
            let mut changed = false;
            for trip in 0..trips.len() {
                if direction[trip].is_some() {
                    continue;
                }
                let best = (0..trips.len())
                    .filter(|&other| direction[other].is_some())
                    .map(|other| {
                        let shared = sequences[trip]
                            .iter()
                            .filter(|place| positions[other].contains_key(place))
                            .count();
                        (other, shared)
                    })
                    .filter(|&(_, shared)| shared >= 2)
                    .min_by(|&(a, shared_a), &(b, shared_b)| {
                        shared_b.cmp(&shared_a).then_with(|| id(a).cmp(id(b)))
                    });
                let Some((other, _)) = best else { continue };
                let order: Vec<usize> = sequences[trip]
                    .iter()
                    .filter_map(|place| positions[other].get(place).copied())
                    .collect();
                let kept = order.windows(2).filter(|pair| pair[0] < pair[1]).count();
                let reversed = order.len().saturating_sub(1) - kept;
                if kept == reversed {
                    continue;
                }
                let theirs = direction[other].expect("directed");
                direction[trip] = Some(if kept > reversed { theirs } else { 1 - theirs });
                changed = true;
            }
            if !changed {
                break;
            }
        }
        decided.extend(trips.iter().copied().zip(direction));
    }
    for (index, direction) in decided {
        let trip = &mut timetable.trips.rows_mut()[index];
        trip.direction = direction;
        if direction.is_none() {
            diagnostics.push(
                Diagnostic::new(
                    "czptt_direction_undecided",
                    Severity::Info,
                    trip.id.as_str(),
                    "no directed trip of its route shares two of its stations: no direction",
                )
                .rule(R121),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use jr_model::identity::czptt;
    use jr_model::timetable::{
        Agency, Boarding, Call, Mode, Place, PlaceKind, Route, TimetableKind, Trip, Tristate,
    };
    use jr_model::{DateSet, Feed, Horizon, OptTime};

    use super::*;

    fn timetable(trips: &[(&str, &[&str])]) -> Timetable {
        let mut t = Timetable::new(Feed::Czptt, Horizon::gvd(2026).unwrap());
        let agency = t.agencies.push(Agency {
            id: czptt::agency("0054"),
            name: "ČD".into(),
            url: None,
            timezone: "Europe/Prague".into(),
            language: None,
            phone: None,
            fare_url: None,
            registration: None,
            email: None,
        });
        let route = t.routes.push(Route {
            id: czptt::line_route("S1", "0054", 106),
            agency,
            mode: Mode::Rail,
            gtfs_route_type: 106,
            short_name: Some("S1".into()),
            long_name: None,
            description: None,
            url: None,
            color: None,
            text_color: None,
            sort_order: None,
            timetable_kind: TimetableKind::Regular,
        });
        let service = t.services.intern(DateSet::from_indices([0]));
        let mut places: HashMap<&str, PlaceId> = HashMap::new();
        for (id, stops) in trips {
            let mut calls = Vec::new();
            for name in stops.iter() {
                let place = *places.entry(name).or_insert_with(|| {
                    t.places.push(Place {
                        id: czptt::station("CZ", name),
                        source_code: None,
                        kind: PlaceKind::StopPlace,
                        name: (*name).into(),
                        description: None,
                        municipality: None,
                        district_name: None,
                        district_code: None,
                        nearby_place: None,
                        country: Some("CZ".into()),
                        position: None,
                        url: None,
                        timezone: None,
                        wheelchair_boarding: Tristate::Unknown,
                    })
                });
                calls.push(Call {
                    place,
                    boarding_point: None,
                    track: None,
                    route_stop: None,
                    arrival: OptTime::NONE,
                    departure: OptTime::NONE,
                    sequence: calls.len() as i32 + 1,
                    stop_headsign: None,
                    line: None,
                    zones: None,
                    kilometre: None,
                    pickup: Boarding::Regular,
                    dropoff: Boarding::Regular,
                    passenger: true,
                    timepoint: true,
                });
            }
            t.trips.push(Trip {
                id: czptt::trip(id, 1),
                route,
                service,
                direction: None,
                headsign: None,
                short_name: None,
                block: None,
                run: None,
                wheelchair_accessible: Tristate::Unknown,
                bikes_allowed: Tristate::Unknown,
                shape: None,
                calls,
            });
        }
        t
    }

    #[test]
    fn czptt_r121_trips_take_the_direction_of_the_shared_stations() {
        let mut t = timetable(&[
            ("a", &["A", "B", "C", "D"]),
            ("b", &["D", "C", "B"]),
            ("c", &["B", "C"]),
            // Runs from D to C, as b does.
            ("d", &["E", "D", "C"]),
            ("e", &["X", "Y"]),
        ]);
        let mut diagnostics = Diagnostics::new();
        assign(&mut t, &mut diagnostics);
        let directions: Vec<Option<u8>> =
            t.trips.rows().iter().map(|trip| trip.direction).collect();
        assert_eq!(directions, [Some(0), Some(1), Some(0), Some(1), None]);
        assert_eq!(
            diagnostics
                .counts_by_code()
                .get("czptt_direction_undecided"),
            Some(&1)
        );
    }
}
