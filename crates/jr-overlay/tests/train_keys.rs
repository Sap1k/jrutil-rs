//! OVL-R132: a regional feed's train trip IDs key the national trains of their course.

mod support;

use jr_model::edit::Precedence;
use jr_model::timetable::*;
use jr_model::{Date, DateSet, Diagnostics, PlaceId, RouteId, TripId};
use jr_overlay::train_keys::{Summary, train_keys};
use support::*;

const CROSSWALK: &str = "[hooks.operational_crosswalk]\nmember = \"api.txt\"\npattern = '^Linka/CVlaku = trip_id: (?<line>[^/]+)/(?<course>.+) = (?<trip>[^=\\s]+)$'\n";

fn yyyymmdd(date: Date) -> String {
    date.to_string().replace('-', "")
}

/// A national train: a trip on `dates`, keyed by its number over the dates' bounds.
fn train(
    base: &mut Base,
    route: RouteId,
    places: [PlaceId; 2],
    number: &str,
    dates: DateSet,
) -> TripId {
    let trip = base.trip(route, dates, &[(places[0], "08:00"), (places[1], "08:30")]);
    base.t.keys.push(SourceKey {
        namespace: Namespace::CZPTT_TRAIN_NUMBER,
        identifier: number.into(),
        entity: Entity::Trip(trip),
        valid_from: day(dates.first().unwrap()),
        valid_to: day(dates.last().unwrap()),
        binding: BindingMethod::Identity,
    });
    trip
}

#[test]
fn ovl_r132_train_trip_ids_key_the_national_trains_of_their_course() {
    let mut base = Base::new();
    let route = base.route("000001", "S2", Mode::Rail);
    let places = [
        base.place(1, "A", Some((49.0, 16.0))),
        base.place(2, "B", Some((49.1, 16.1))),
    ];
    // 4330 runs as one trip in the first week and as another in the second, and again on day
    // 20, after the source's line/course stops; 4331 is a run of two parts.
    let first_week = train(&mut base, route, places, "4330", days(0..=6));
    let second_week = train(&mut base, route, places, "4330", days((7..=13).chain([20])));
    let part_one = train(&mut base, route, places, "4331", days(0..=13));
    let part_two = train(&mut base, route, places, "4331", days(0..=13));
    train(&mut base, route, places, "1", days(0..=13));

    let profile = generic_profile(CROSSWALK);
    let calendar = |id: &str, from: usize, to: usize| {
        format!(
            "{id},1,1,1,1,1,1,1,{},{}\n",
            yyyymmdd(day(from)),
            yyyymmdd(day(to))
        )
    };
    let calendars = format!(
        "service_id,monday,tuesday,wednesday,thursday,friday,saturday,sunday,start_date,end_date\n{}{}{}",
        calendar("w1", 0, 6),
        calendar("w2", 7, 13),
        calendar("all", 0, 13)
    );
    let feed = feed(
        &profile,
        &[
            ("agency.txt", AGENCY),
            (
                "routes.txt",
                "route_id,agency_id,route_short_name,route_type\nrail,a,S2,2\nbus,a,104,3\n",
            ),
            (
                "trips.txt",
                concat!(
                    "route_id,service_id,trip_id\n",
                    "rail,w1,t1\nrail,w2,t2\nrail,all,t3\nrail,all,t4\nrail,all,t5\nbus,all,b1\n",
                ),
            ),
            (
                "stops.txt",
                "stop_id,stop_name,stop_lat,stop_lon\nx,X,49.0,16.0\ny,Y,49.1,16.1\n",
            ),
            (
                "stop_times.txt",
                concat!(
                    "trip_id,arrival_time,departure_time,stop_id,stop_sequence\n",
                    "t1,08:00:00,08:00:00,x,1\nt1,08:30:00,08:30:00,y,2\n",
                    "t2,08:00:00,08:00:00,x,1\nt2,08:30:00,08:30:00,y,2\n",
                    "t3,09:00:00,09:00:00,x,1\nt3,09:30:00,09:30:00,y,2\n",
                    "t4,10:00:00,10:00:00,x,1\nt4,10:30:00,10:30:00,y,2\n",
                    "t5,11:00:00,11:00:00,x,1\nt5,11:30:00,11:30:00,y,2\n",
                    "b1,12:00:00,12:00:00,x,1\nb1,12:30:00,12:30:00,y,2\n",
                ),
            ),
            ("calendar.txt", &calendars),
            (
                "api.txt",
                concat!(
                    "Linka/CVlaku = trip_id: 120/4330 = t1\n",
                    "Linka/CVlaku = trip_id: 120/4330 = t2\n",
                    "Linka/CVlaku = trip_id: 120/4331 = t3\n",
                    "Linka/CVlaku = trip_id: 120/77777 = t4\n",
                    "Linka/CVlaku = trip_id: 120/R5 = t5\n",
                    "Linka/CVlaku = trip_id: 104/1 = b1\n",
                ),
            ),
        ],
    );

    let mut diagnostics = Diagnostics::new();
    let (edits, summary) = train_keys(&base.t, &profile, &feed, &mut diagnostics);
    let mut t = base.t.clone();
    let precedence = Precedence { position: vec![] };
    jr_model::apply(&mut t, edits, &precedence, &mut diagnostics).unwrap();
    let offset = |date| horizon().index_of(date).unwrap();
    let mut keys: Vec<(String, TripId, usize, usize)> = t
        .keys
        .iter()
        .filter(|key| key.namespace != Namespace::CZPTT_TRAIN_NUMBER)
        .filter(|key| key.namespace != Namespace::CIS_LINE)
        .map(|key| {
            assert_eq!(key.namespace.as_str(), "test:gtfs_trip_line_course");
            assert_eq!(key.binding, BindingMethod::OperatorCrosswalk);
            let Entity::Trip(trip) = key.entity else {
                panic!("{key:?}")
            };
            (
                key.identifier.to_string(),
                trip,
                offset(key.valid_from),
                offset(key.valid_to),
            )
        })
        .collect();
    keys.sort();
    // Either calendar variant names the train of the day, in both weeks, never on day 20; a
    // run's parts are both keyed; a course that is no train number, a number no train has and
    // a bus are not.
    let mut expected = vec![
        ("t1".to_owned(), first_week, 0, 6),
        ("t1".to_owned(), second_week, 7, 13),
        ("t2".to_owned(), first_week, 0, 6),
        ("t2".to_owned(), second_week, 7, 13),
        ("t3".to_owned(), part_one, 0, 13),
        ("t3".to_owned(), part_two, 0, 13),
    ];
    expected.sort();
    assert_eq!(keys, expected);
    assert_eq!(
        summary,
        Summary {
            rail_trips: 5,
            numbered: 4,
            keyed: 3,
            courses: 3,
            courses_without_train: 1,
            keys_added: 6,
        }
    );
}
