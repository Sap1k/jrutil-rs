//! Hand-built rail timetables, SR70 rows and OSM nodes for the pass's tests.

use std::collections::BTreeMap;

use jr_formats::{osm, sr70};
use jr_model::identity::czptt;
use jr_model::timetable::{
    Agency, Boarding, Call, Mode, Place, PlaceKind, Position, Route, Run, TimetableKind, Trip,
    Tristate,
};
use jr_model::{
    Coord, DateSet, Diagnostics, Edit, Edits, Feed, Horizon, OptTime, PlaceId, Precedence, RouteId,
    ServiceTime, Timetable, apply,
};

use super::super::{Evidence, Policy, Summary, rail_coordinates};

/// `(country, code, name, arrival, departure, passenger)`; times `HH:MM[:SS]`, `""` for none.
pub type Stop<'a> = (&'a str, &'a str, &'a str, &'a str, &'a str, bool);

/// A stop with the same arrival and departure.
pub fn at<'a>(
    country: &'a str,
    code: &'a str,
    name: &'a str,
    time: &'a str,
    passenger: bool,
) -> Stop<'a> {
    (country, code, name, time, time, passenger)
}

/// The public ID of a railway point.
pub fn id(country: &str, code: &str) -> String {
    czptt::station(country, code).to_string()
}

pub struct Fixture {
    pub t: Timetable,
    route: RouteId,
    places: BTreeMap<(String, String), PlaceId>,
    pub sr70: Vec<sr70::Row>,
    pub nodes: Vec<osm::Node>,
}

impl Fixture {
    pub fn new() -> Self {
        let mut t = Timetable::new(Feed::Czptt, Horizon::gvd(2026).unwrap());
        let agency = t.agencies.push(Agency {
            id: czptt::agency("0054"),
            name: "ČD".into(),
            url: None,
            timezone: "Europe/Prague".into(),
            language: None,
            phone: None,
            fare_url: None,
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
            sr70: Vec::new(),
            nodes: Vec::new(),
        }
    }

    /// A run of one part.
    pub fn path(&mut self, key: &str, stops: &[Stop]) {
        self.parts(key, &[stops]);
    }

    /// A run of several parts; each part after the first starts with the previous part's last
    /// stop, which keeps its sequence (one shared boundary call).
    pub fn parts(&mut self, key: &str, parts: &[&[Stop]]) {
        let run_key = self.t.strings.intern(key);
        let service = self.t.services.intern(DateSet::from_indices([0, 1]));
        let mut sequence = 0;
        for (index, stops) in parts.iter().enumerate() {
            let mut calls = Vec::new();
            for (position, &(country, code, name, arrival, departure, passenger)) in
                stops.iter().enumerate()
            {
                if !(index > 0 && position == 0) {
                    sequence += 1;
                }
                calls.push(Call {
                    place: self.place(country, code, name),
                    boarding_point: None,
                    track: None,
                    route_stop: None,
                    arrival: time(arrival),
                    departure: time(departure),
                    sequence,
                    stop_headsign: None,
                    line: None,
                    zones: None,
                    kilometre: None,
                    pickup: Boarding::Regular,
                    dropoff: Boarding::Regular,
                    passenger,
                    timepoint: true,
                });
            }
            let part = u16::try_from(index + 1).unwrap();
            self.t.trips.push(Trip {
                id: czptt::trip(key, part),
                route: self.route,
                service,
                direction: None,
                headsign: None,
                short_name: None,
                block: None,
                run: Some(Run { key: run_key, part }),
                wheelchair_accessible: Tristate::Unknown,
                bikes_allowed: Tristate::Unknown,
                shape: None,
                calls,
            });
        }
    }

    fn place(&mut self, country: &str, code: &str, name: &str) -> PlaceId {
        let key = (country.to_owned(), code.to_owned());
        if let Some(&place) = self.places.get(&key) {
            return place;
        }
        let place = self.t.places.push(Place {
            id: czptt::station(country, code),
            kind: PlaceKind::StopPlace,
            name: name.into(),
            source_code: Some(code.into()),
            description: None,
            municipality: None,
            district_name: None,
            district_code: None,
            nearby_place: None,
            country: Some(country.into()),
            position: None,
            url: None,
            timezone: None,
            wheelchair_boarding: Tristate::Unknown,
        });
        self.places.insert(key, place);
        place
    }

    pub fn sr70(&mut self, code: &str, name: &str, latitude: Option<f64>, longitude: Option<f64>) {
        self.sr70.push(sr70::Row {
            code: code.into(),
            name: Some(name).filter(|name| !name.is_empty()).map(Box::from),
            latitude,
            longitude,
            extra: Vec::new(),
            line: self.sr70.len() as u64 + 1,
        });
    }

    /// An SR70 row with a good coordinate.
    pub fn sr70_at(&mut self, code: &str, name: &str, latitude: f64, longitude: f64) {
        self.sr70(code, name, Some(latitude), Some(longitude));
    }

    pub fn node(&mut self, id: i64, latitude: f64, longitude: f64, tags: &[(&str, &str)]) {
        self.nodes.push(osm::Node {
            id,
            coord: Coord::new(longitude, latitude),
            tags: tags
                .iter()
                .map(|&(key, value)| (Box::from(key), Box::from(value)))
                .collect(),
        });
    }

    pub fn evidence(&self, diagnostics: &mut Diagnostics) -> Evidence {
        Evidence::build(
            &self.sr70,
            self.nodes.iter().cloned(),
            &Policy::builtin(),
            diagnostics,
        )
    }

    /// Builds the evidence and runs the pass with the built-in policy.
    pub fn run(&self) -> Outcome {
        self.run_with(&Policy::builtin())
    }

    pub fn run_with(&self, policy: &Policy) -> Outcome {
        let mut diagnostics = Diagnostics::new();
        let evidence = self.evidence(&mut diagnostics);
        let (edits, summary) = rail_coordinates(&self.t, &evidence, policy, &mut diagnostics);
        Outcome {
            edits,
            summary,
            diagnostics,
        }
    }

    /// Applies an outcome's edits and checks the invariants.
    pub fn apply(&mut self, outcome: Outcome) {
        let precedence = Precedence {
            position: vec![
                jr_model::timetable::CoordSource::Sr70,
                jr_model::timetable::CoordSource::Osm,
            ],
        };
        apply(
            &mut self.t,
            outcome.edits,
            &precedence,
            &mut Diagnostics::new(),
        )
        .unwrap();
        assert_eq!(self.t.check(), []);
    }

    pub fn position_of(&self, country: &str, code: &str) -> Option<Position> {
        let place = self.places[&(country.to_owned(), code.to_owned())];
        self.t.places[place].position
    }
}

fn time(text: &str) -> OptTime {
    if text.is_empty() {
        return OptTime::NONE;
    }
    let parts: Vec<i32> = text.split(':').map(|part| part.parse().unwrap()).collect();
    let seconds = parts.get(2).copied().unwrap_or(0);
    OptTime::some(ServiceTime::from_hms(parts[0], parts[1], seconds))
}

pub struct Outcome {
    pub edits: Edits,
    pub summary: Summary,
    pub diagnostics: Diagnostics,
}

impl Outcome {
    /// The position set for a place and the rule that set it.
    pub fn position(&self, t: &Timetable, country: &str, code: &str) -> Option<(Position, &str)> {
        let wanted = id(country, code);
        self.edits.iter().find_map(|(rule, edit)| match edit {
            Edit::SetPlacePosition { place, position }
                if t.places[*place].id.as_str() == wanted =>
            {
                Some((*position, rule))
            }
            _ => None,
        })
    }

    /// `(subject, message)` of the diagnostics with a code, in order.
    pub fn diagnostics(&self, code: &str) -> Vec<(&str, &str)> {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == code)
            .map(|diagnostic| (diagnostic.subject.as_str(), diagnostic.message.as_str()))
            .collect()
    }

    pub fn counts(&self) -> BTreeMap<&str, usize> {
        self.diagnostics.counts_by_code()
    }
}

/// Asserts a coordinate to a micro-degree.
pub fn assert_near(position: Position, longitude: f64, latitude: f64) {
    assert!(
        (position.coord.lon - longitude).abs() < 1e-6
            && (position.coord.lat - latitude).abs() < 1e-6,
        "{position:?} is not at {longitude}, {latitude}"
    );
}
