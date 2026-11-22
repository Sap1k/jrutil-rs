//! Builders for the merge: fixed batches with stops, versions and trips, the identity name
//! profile and the merge call.

use jiff::civil::{Date, date};
use jr_formats::geodata::registry;
use jr_formats::jdf::{self, Header, Km, NoteType, RouteType, TimeCell, Version};
use jr_geo::names::{Analyzer, Profile};
use jr_jdf::batch::{self, Batch};
use jr_jdf::fixed::{FixedBatch, FixedStop, Location, Origin, PostCandidate};
use jr_jdf::merge::{self, MergeInput, Merged, Registry};
use jr_model::{Coord, Diagnostics};

/// The stop-identity profile with the shipped synonym table.
pub fn names() -> Analyzer {
    #[derive(serde::Deserialize)]
    struct RoadNames {
        identity: Profile,
    }
    let names: RoadNames = toml::from_str(include_str!("../../../../data/names/road_names.toml"))
        .expect("road_names.toml");
    let lines = jr_formats::geodata::synonyms::parse(include_str!(
        "../../../../data/names/stop_synonyms.txt"
    ));
    Analyzer::new(
        names.identity,
        lines.into_iter().map(|line| line.alternatives),
    )
}

/// A stop in okres UL, CZ, without a position.
pub fn stop(town: &str, district: Option<&str>, nearby: Option<&str>) -> FixedStop {
    FixedStop {
        town: town.into(),
        district: district.map(Box::from),
        nearby_place: nearby.map(Box::from),
        okres: Some("UL".into()),
        country: Some("CZ".into()),
        location: None,
    }
}

pub fn at(mut stop: FixedStop, lat: f64, lon: f64) -> FixedStop {
    stop.location = Some(Location {
        coord: Coord::new(lon, lat),
        origin: Origin::Osm,
        source: "test".into(),
    });
    stop
}

pub fn estimated(mut stop: FixedStop, lat: f64, lon: f64) -> FixedStop {
    stop.location = Some(Location {
        coord: Coord::new(lon, lat),
        origin: Origin::RouteTime,
        source: "test".into(),
    });
    stop
}

pub const AGENCY: &str = "12345678";

pub fn agency(phone: &str) -> batch::Agency {
    batch::Agency {
        id: AGENCY.into(),
        tax_id: None,
        name: "Dopravce".into(),
        company_type: None,
        person_name: "".into(),
        office_address: "".into(),
        office_phone: phone.into(),
        control_phone: None,
        info_phone: None,
        fax: None,
        email: None,
        website: None,
        distinction: 1,
    }
}

pub struct B {
    pub fixed: FixedBatch,
}

/// An empty JDF 1.11 batch with one agency.
pub fn batch(name: &str) -> B {
    B {
        fixed: FixedBatch {
            batch: Batch {
                name: name.into(),
                version: Version::V1_11,
                header: Header {
                    version: "1.11".into(),
                    du_number: None,
                    region: None,
                    batch_id: None,
                    created: None,
                    generator: None,
                },
                stops: Vec::new(),
                stop_posts: Vec::new(),
                agencies: vec![agency("1")],
                routes: Vec::new(),
                route_integrations: Vec::new(),
                route_stops: Vec::new(),
                trips: Vec::new(),
                trip_groups: Vec::new(),
                trip_stops: Vec::new(),
                route_info: Vec::new(),
                attribute_refs: Vec::new(),
                service_notes: Vec::new(),
                transfers: Vec::new(),
                agency_alternations: Vec::new(),
                alternate_route_names: Vec::new(),
                reservations: Vec::new(),
            },
            stops: Vec::new(),
            post_candidates: Vec::new(),
        },
    }
}

impl B {
    pub fn code(&mut self, code: &str) -> u32 {
        let refs = &mut self.fixed.batch.attribute_refs;
        if let Some(found) = refs.iter().find(|r| &*r.code == code) {
            return found.id;
        }
        let id = refs.len() as u32 + 1;
        refs.push(jdf::AttributeRef {
            id,
            code: code.into(),
            reserved1: None,
        });
        id
    }

    pub fn slots<const N: usize>(&mut self, codes: &[&str]) -> [Option<u32>; N] {
        let mut slots = [None; N];
        for (slot, code) in slots.iter_mut().zip(codes) {
            *slot = Some(self.code(code));
        }
        slots
    }

    pub fn created(mut self, day: Date) -> Self {
        self.fixed.batch.header.created = Some(day);
        self
    }

    pub fn agency_phone(mut self, phone: &str) -> Self {
        self.fixed.batch.agencies = vec![agency(phone)];
        self
    }

    /// A stop numbered after the batch's last, with these fixed codes.
    pub fn stop_with(mut self, stop: FixedStop, codes: &[&str]) -> Self {
        let attributes = self.slots(codes);
        let id = self.fixed.batch.stops.len() as u64 + 1;
        self.fixed.batch.stops.push(jdf::Stop {
            id,
            town: stop.town.clone(),
            district: stop.district.clone(),
            nearby_place: stop.nearby_place.clone(),
            okres: stop.okres.clone(),
            country: stop.country.clone(),
            attributes,
        });
        self.fixed.stops.push(stop);
        self
    }

    pub fn stop(self, stop: FixedStop) -> Self {
        self.stop_with(stop, &[])
    }

    pub fn post(mut self, stop: u64, post_id: u64, name: &str) -> Self {
        self.fixed.batch.stop_posts.push(jdf::StopPost {
            stop_id: stop,
            post_id,
            name: None,
            description: None,
            post_name: Some(name.into()),
            reserved1: None,
            reserved2: None,
        });
        self
    }

    pub fn observation(mut self, stop: u32, observation: &str, lat: f64) -> Self {
        self.fixed.post_candidates.push(PostCandidate {
            stop,
            coord: Coord::new(14.0, lat),
            observation: observation.into(),
            facts: Vec::new(),
        });
        self
    }

    /// A line version (distinction 1, the batch's agency) whose route stops are the batch's
    /// stops so far.
    pub fn route(mut self, licence: &str, from: Date, to: Option<Date>, detour: bool) -> Self {
        self.fixed.batch.routes.push(batch::Route {
            id: licence.into(),
            name: "Test".into(),
            agency_id: AGENCY.into(),
            route_type: Some(RouteType::Regional),
            transport_mode: Some(jdf::TransportMode::Bus),
            detour,
            grouped: false,
            uses_stop_posts: None,
            one_way: None,
            reserved1: None,
            licence_number: None,
            licence_valid_from: None,
            licence_valid_to: None,
            timetable_valid_from: from,
            timetable_valid_to: to,
            agency_distinction: 1,
            distinction: 1,
        });
        for (index, stop) in self.fixed.batch.stops.clone().iter().enumerate() {
            self.fixed.batch.route_stops.push(batch::RouteStop {
                route_id: licence.into(),
                route_stop_id: index as u64 + 1,
                zone: None,
                stop_id: stop.id,
                approximate_minutes: None,
                attributes: [None; 3],
                route_distinction: 1,
            });
        }
        self
    }

    pub fn route_type(mut self, route_type: RouteType) -> Self {
        if let Some(route) = self.fixed.batch.routes.last_mut() {
            route.route_type = Some(route_type);
        }
        self
    }

    pub fn grouped(mut self) -> Self {
        if let Some(route) = self.fixed.batch.routes.last_mut() {
            route.grouped = true;
        }
        self
    }

    /// A trip of `licence` with these day codes, calling at every stop of the batch in order at
    /// kilometre 0, 10, 20, …
    pub fn trip(mut self, licence: &str, id: u64, codes: &[&str]) -> Self {
        let attributes = self.slots(codes);
        self.fixed.batch.trips.push(batch::Trip {
            route_id: licence.into(),
            id,
            attributes,
            trip_group_id: None,
            route_distinction: 1,
        });
        for (index, stop) in self.fixed.batch.stops.clone().iter().enumerate() {
            self.fixed.batch.trip_stops.push(batch::TripStop {
                route_id: licence.into(),
                trip_id: id,
                route_stop_id: index as u64 + 1,
                stop_id: stop.id,
                stop_post_id: None,
                stop_post_number: None,
                attributes: [None; 3],
                km: Some(Km {
                    units: index as u64 * 10,
                    scale: 0,
                }),
                arrival: None,
                departure: Some(TimeCell::At {
                    minutes: 480 + index as u16,
                }),
                earliest_arrival: None,
                latest_departure: None,
                route_distinction: 1,
            });
        }
        self
    }

    pub fn note(
        mut self,
        licence: &str,
        trip: u64,
        note_type: NoteType,
        from: Option<Date>,
        to: Option<Date>,
    ) -> Self {
        self.fixed.batch.service_notes.push(batch::ServiceNote {
            route_id: licence.into(),
            trip_id: trip,
            id: 1,
            designation: "1".into(),
            note_type: Some(note_type),
            date_from: from,
            date_to: to,
            note: None,
            route_distinction: 1,
        });
        self
    }

    pub fn build(self) -> FixedBatch {
        self.fixed
    }
}

/// A batch of these stops and one version (licence = the batch name) using all of them, valid
/// over GVD 2026.
pub fn stops_batch(name: &str, stops: Vec<FixedStop>) -> FixedBatch {
    let mut builder = batch(name);
    for stop in stops {
        builder = builder.stop(stop);
    }
    builder
        .route(name, date(2025, 12, 14), Some(date(2026, 12, 12)), false)
        .build()
}

pub fn registry_stop(
    id: i64,
    town: &str,
    district: Option<&str>,
    nearby: Option<&str>,
    reference: Option<(f64, f64)>,
) -> registry::Stop {
    registry::Stop {
        id,
        town: town.into(),
        district: district.map(Box::from),
        nearby_place: nearby.map(Box::from),
        okres: Some("UL".into()),
        country: Some("CZ".into()),
        reference: reference.map(|(lat, lon)| Coord::new(lon, lat)),
        status: registry::Status::Active,
        merged_into: None,
        note: None,
        line: 2,
    }
}

pub fn registry(rows: &[registry::Stop]) -> Registry {
    Registry::new(rows, &merge::Policy::builtin(), &mut Diagnostics::new())
}

/// Merges over GVD `year` with the reference date at its start.
pub fn merge_in(year: i16, batches: Vec<FixedBatch>, registry: &Registry) -> (Merged, Diagnostics) {
    let reference = jr_model::Horizon::gvd(year).expect("year").start();
    merge_at(year, reference, batches, registry)
}

pub fn merge_at(
    year: i16,
    reference: Date,
    batches: Vec<FixedBatch>,
    registry: &Registry,
) -> (Merged, Diagnostics) {
    let names = names();
    let jdf = jr_jdf::Policy::builtin();
    let mut diagnostics = Diagnostics::new();
    let merged = merge::merge(
        MergeInput {
            batches,
            registry,
            names: &names,
            jdf: &jdf,
            timetable_year: year,
            reference_date: reference,
        },
        &merge::Policy::builtin(),
        &mut diagnostics,
    )
    .expect("merges");
    (merged, diagnostics)
}

pub fn merge_stops(batches: Vec<FixedBatch>) -> (Merged, Diagnostics) {
    merge_in(2026, batches, &Registry::empty())
}

pub fn display_names(merged: &Merged) -> Vec<String> {
    merged.stops.iter().map(|s| s.name.display_name()).collect()
}

pub fn count(diagnostics: &Diagnostics, code: &str) -> usize {
    diagnostics.iter().filter(|d| d.code == code).count()
}

/// Merges over GVD 2026 and assembles with the built-in policies; the timetable passes
/// `check()`.
pub fn assemble(batches: Vec<FixedBatch>) -> (jr_jdf::assemble::Assembled, Diagnostics) {
    let (assembled, diagnostics) = assemble_with(batches, &[]);
    (assembled.expect("assembles"), diagnostics)
}

pub fn assemble_with(
    batches: Vec<FixedBatch>,
    rules: &[jr_formats::geodata::route_rules::TransportModeRule],
) -> (
    Result<jr_jdf::assemble::Assembled, jr_jdf::assemble::AssembleError>,
    Diagnostics,
) {
    let (merged, mut diagnostics) = merge_in(2026, batches, &Registry::empty());
    let jdf = jr_jdf::Policy::builtin();
    let assembled = jr_jdf::assemble::assemble(
        jr_jdf::assemble::AssembleInput {
            merged,
            transport_modes: rules,
            jdf: &jdf,
        },
        &jr_jdf::assemble::Policy::builtin(),
        &mut diagnostics,
    );
    if let Ok(assembled) = &assembled {
        assert_eq!(assembled.timetable.check(), Vec::new());
    }
    (assembled, diagnostics)
}

/// The whole GVD 2026.
pub fn year_route(builder: B, licence: &str) -> B {
    builder.route(licence, date(2025, 12, 14), Some(date(2026, 12, 12)), false)
}
