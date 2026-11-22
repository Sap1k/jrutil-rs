use std::collections::BTreeMap;

use jr_model::identity::czptt;
use jr_model::timetable::{
    Agency, Boarding, Call, Mode, Place, PlaceKind, Route, TimetableKind, Trip, Tristate,
};
use jr_model::{
    DateSet, Diagnostics, Feed, Horizon, OptTime, PlaceId, Precedence, RouteId, ServiceTime,
    Timetable, TripId, apply,
};

use super::*;

/// One route; places are letters (`czptt:stop:CZ:<letter>`), trips are given as stop letters
/// with a minute per call (`None`: no time). `*` before a letter marks a non-passenger call.
struct Fixture {
    t: Timetable,
    route: RouteId,
    places: BTreeMap<char, PlaceId>,
}

impl Fixture {
    fn new() -> Self {
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
        Self {
            t,
            route,
            places: BTreeMap::new(),
        }
    }

    fn place(&mut self, letter: char) -> PlaceId {
        if let Some(&place) = self.places.get(&letter) {
            return place;
        }
        let place = self.t.places.push(Place {
            id: czptt::station("CZ", &letter.to_string()),
            source_code: None,
            kind: PlaceKind::StopPlace,
            name: letter.to_string().into(),
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
        });
        self.places.insert(letter, place);
        place
    }

    fn trip(
        &mut self,
        id: &str,
        direction: Option<u8>,
        stops: &str,
        minutes: &[Option<i32>],
    ) -> TripId {
        self.trip_on(id, direction, stops, minutes, &[0, 1])
    }

    fn trip_on(
        &mut self,
        id: &str,
        direction: Option<u8>,
        stops: &str,
        minutes: &[Option<i32>],
        days: &[usize],
    ) -> TripId {
        let mut calls = Vec::new();
        let mut passenger = true;
        for letter in stops.chars() {
            if letter == '*' {
                passenger = false;
                continue;
            }
            let time = minutes
                .get(calls.len())
                .copied()
                .flatten()
                .map_or(OptTime::NONE, |minute| {
                    OptTime::some(ServiceTime::from_hms(8, minute, 0))
                });
            calls.push(Call {
                place: self.place(letter),
                boarding_point: None,
                track: None,
                route_stop: None,
                arrival: time,
                departure: time,
                sequence: calls.len() as i32 + 1,
                stop_headsign: None,
                line: None,
                zones: None,
                kilometre: None,
                pickup: Boarding::Regular,
                dropoff: Boarding::Regular,
                passenger,
                timepoint: true,
            });
            passenger = true;
        }
        let service = self
            .t
            .services
            .intern(DateSet::from_indices(days.iter().copied()));
        self.t.trips.push(Trip {
            id: czptt::trip(id, 1),
            route: self.route,
            service,
            direction,
            headsign: None,
            short_name: None,
            block: None,
            run: None,
            wheelchair_accessible: Tristate::Unknown,
            bikes_allowed: Tristate::Unknown,
            shape: None,
            calls,
        })
    }

    /// Runs the pass, applies its edits and checks the invariants.
    fn run(mut self) -> Outcome {
        let (edits, summary) = route_stops(&self.t);
        apply(
            &mut self.t,
            edits,
            &Precedence { position: vec![] },
            &mut Diagnostics::new(),
        )
        .unwrap();
        assert_eq!(self.t.check(), []);
        Outcome { t: self.t, summary }
    }
}

struct Outcome {
    t: Timetable,
    summary: Summary,
}

impl Outcome {
    /// The slot IDs with the route prefix cut off, in table order: `<dir>/<letter>/<visit>`.
    fn slots(&self) -> Vec<String> {
        self.t
            .route_stops
            .rows()
            .iter()
            .map(|slot| self.short(&slot.id))
            .collect()
    }

    fn short(&self, id: &jr_model::PublicId) -> String {
        let route = self.t.routes.rows()[0].id.as_str();
        id.as_str()
            .strip_prefix(route)
            .unwrap()
            .replace("/czptt:stop:CZ:", "/")
            .trim_start_matches('/')
            .to_owned()
    }

    /// The table position of each call's slot.
    fn calls(&self, trip: TripId) -> Vec<Option<usize>> {
        self.t.trips[trip]
            .calls
            .iter()
            .map(|call| call.route_stop.map(|slot| slot.index()))
            .collect()
    }

    fn order(&self) -> String {
        self.t
            .route_stops
            .rows()
            .iter()
            .map(|slot| self.t.places[slot.place].name.to_string())
            .collect()
    }
}

/// A trip ID, direction, stops and minutes.
type TripSpec = (&'static str, Option<u8>, &'static str, Vec<Option<i32>>);

fn at(minutes: &[i32]) -> Vec<Option<i32>> {
    minutes.iter().copied().map(Some).collect()
}

#[test]
fn serving_r028_heaviest_pattern_wins_and_a_partial_stop_goes_between_its_neighbours_by_time() {
    let mut f = Fixture::new();
    let mut full = Vec::new();
    for index in 0..5 {
        full.push(f.trip(
            &format!("full{index}"),
            Some(0),
            "ABCD",
            &at(&[0, 30, 50, 100]),
        ));
    }
    let partial = f.trip("partial", Some(0), "ABXD", &at(&[0, 30, 60, 100]));
    let out = f.run();
    assert_eq!(out.order(), "ABCXD");
    assert_eq!(out.calls(full[0]), [Some(0), Some(1), Some(2), Some(4)]);
    assert_eq!(out.calls(partial), [Some(0), Some(1), Some(3), Some(4)]);
    let sequences: Vec<i32> = out
        .t
        .route_stops
        .rows()
        .iter()
        .map(|s| s.sequence)
        .collect();
    assert_eq!(sequences, [1, 2, 3, 4, 5]);
    assert_eq!(
        out.summary,
        Summary {
            route_directions: 1,
            patterns: 2,
            slots: 5,
            maximum_patterns_per_route_direction: 2,
        }
    );

    // An inserted stop that is strictly earlier goes before the existing one in its gap.
    let mut f = Fixture::new();
    for index in 0..5 {
        f.trip(&format!("main{index}"), Some(0), "ACD", &at(&[0, 50, 100]));
    }
    f.trip("early", Some(0), "ABD", &at(&[0, 20, 100]));
    assert_eq!(f.run().order(), "ABCD");

    // A short turn reuses the full line's slots.
    let mut f = Fixture::new();
    for index in 0..3 {
        f.trip(&format!("short{index}"), Some(0), "BC", &[]);
    }
    for index in 0..5 {
        f.trip(&format!("long{index}"), Some(0), "ABCD", &[]);
    }
    let out = f.run();
    assert_eq!(out.order(), "ABCD");
    assert_eq!(out.calls(TripId::from_index(0)), [Some(1), Some(2)]);
}

#[test]
fn serving_r028_equal_or_unknown_times_keep_the_existing_slot_first() {
    for partial in [at(&[0, 50, 100]), vec![Some(0), None, Some(100)]] {
        let mut f = Fixture::new();
        f.trip("main1", Some(0), "ACD", &at(&[0, 50, 100]));
        f.trip("main2", Some(0), "ACD", &at(&[0, 50, 100]));
        f.trip("other", Some(0), "ABD", &partial);
        assert_eq!(f.run().order(), "ACBD");
    }
}

#[test]
fn serving_r028_equal_weight_and_length_break_ties_by_place_ids() {
    // Neither time is known, so whichever pattern merges first keeps its stop first.
    let mut f = Fixture::new();
    f.trip("x", Some(0), "ACD", &[]);
    f.trip("y", Some(0), "ABD", &[]);
    assert_eq!(f.run().order(), "ABCD");
}

#[test]
fn serving_r028_non_passenger_calls_and_unpublished_trips_get_no_slot() {
    let mut f = Fixture::new();
    let through = f.trip("through", Some(0), "A*BC", &at(&[0, 5, 10]));
    let retired = f.trip_on("retired", Some(0), "AXC", &at(&[0, 5, 10]), &[]);
    let out = f.run();
    assert_eq!(out.order(), "AC");
    assert_eq!(out.calls(through), [Some(0), None, Some(1)]);
    assert_eq!(out.calls(retired), [None, None, None]);
    assert_eq!(out.summary.patterns, 1);
}

#[test]
fn serving_r028_result_is_independent_of_trip_order_and_thread_count() {
    let trips: [TripSpec; 7] = [
        ("t1", Some(0), "ABCDE", at(&[0, 20, 40, 70, 100])),
        ("t2", Some(0), "ABCDE", at(&[0, 20, 40, 70, 100])),
        ("t3", Some(0), "ABXDE", at(&[0, 20, 50, 70, 100])),
        ("t4", Some(0), "ABXDE", at(&[0, 20, 50, 70, 100])),
        ("t5", Some(0), "BCD", vec![]),
        ("t6", Some(1), "EDCBA", vec![]),
        ("t7", None, "AYE", at(&[0, 90, 100])),
    ];
    let render = |order: &[usize], threads: usize| {
        let mut f = Fixture::new();
        for &index in order {
            let (id, direction, stops, minutes) = &trips[index];
            f.trip(id, *direction, stops, minutes);
        }
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        let out = pool.install(|| f.run());
        let mut calls: Vec<(String, Vec<Option<String>>)> = out
            .t
            .trips
            .iter()
            .map(|(handle, trip)| {
                let slots = out
                    .calls(handle)
                    .into_iter()
                    .map(|slot| slot.map(|slot| out.short(&out.t.route_stops.rows()[slot].id)));
                (trip.id.to_string(), slots.collect())
            })
            .collect();
        calls.sort();
        (out.slots(), calls)
    };
    let expected = render(&[0, 1, 2, 3, 4, 5, 6], 1);
    assert_eq!(render(&[6, 4, 2, 0, 5, 3, 1], 4), expected);
    assert_eq!(render(&[3, 5, 1, 6, 0, 2, 4], 3), expected);
}

#[test]
fn serving_r029_pattern_times_come_from_the_smallest_trip_id() {
    // The pattern ABD runs twice; its smallest trip ID ("a") puts B after C, the other before.
    let mut f = Fixture::new();
    for index in 0..3 {
        f.trip(&format!("main{index}"), Some(0), "ACD", &at(&[0, 50, 100]));
    }
    f.trip("b", Some(0), "ABD", &at(&[0, 40, 100]));
    f.trip("a", Some(0), "ABD", &at(&[0, 60, 100]));
    assert_eq!(f.run().order(), "ACBD");
}

#[test]
fn serving_r003_a_loop_line_keeps_both_visits_with_visit_numbers() {
    let mut f = Fixture::new();
    for index in 0..4 {
        f.trip(&format!("loop{index}"), Some(0), "ABCA", &[]);
    }
    let short = f.trip("short", Some(0), "ABC", &[]);
    let out = f.run();
    assert_eq!(out.slots(), ["0/A/1", "0/B/1", "0/C/1", "0/A/2"]);
    assert_eq!(
        out.calls(TripId::from_index(0)),
        [Some(0), Some(1), Some(2), Some(3)]
    );
    assert_eq!(out.calls(short), [Some(0), Some(1), Some(2)]);
    assert_eq!(
        out.t.route_stops.rows()[0].id.as_str(),
        "czptt:route:line:S1:0054:106/0/czptt:stop:CZ:A/1"
    );
}

#[test]
fn serving_r003_each_direction_and_the_trips_without_one_get_their_own_list() {
    let mut f = Fixture::new();
    let back = f.trip("back", Some(1), "CBA", &[]);
    let there = f.trip("there", Some(0), "ABC", &[]);
    let undirected = f.trip("undirected", None, "AB", &[]);
    let out = f.run();
    assert_eq!(
        out.slots(),
        [
            "-/A/1", "-/B/1", "0/A/1", "0/B/1", "0/C/1", "1/C/1", "1/B/1", "1/A/1"
        ]
    );
    let directions: Vec<Option<u8>> = out
        .t
        .route_stops
        .rows()
        .iter()
        .map(|s| s.direction)
        .collect();
    assert_eq!(
        directions,
        [
            None,
            None,
            Some(0),
            Some(0),
            Some(0),
            Some(1),
            Some(1),
            Some(1)
        ]
    );
    let sequences: Vec<i32> = out
        .t
        .route_stops
        .rows()
        .iter()
        .map(|s| s.sequence)
        .collect();
    assert_eq!(sequences, [1, 2, 1, 2, 3, 1, 2, 3]);
    assert_eq!(out.calls(undirected), [Some(0), Some(1)]);
    assert_eq!(out.calls(there), [Some(2), Some(3), Some(4)]);
    assert_eq!(out.calls(back), [Some(5), Some(6), Some(7)]);
    assert_eq!(out.summary.route_directions, 3);
    assert_eq!(out.summary.maximum_patterns_per_route_direction, 1);
}
