//! A batch with its format version's defaults applied (JDF-R025–R031): what every later stage
//! works on.
//!
//! The reader (`jr_formats::jdf`) returns each relation as written, with a field the batch's
//! version does not have as `None`. Here each such field that a later stage reads gets the value
//! the older version means (R028), and nothing else changes. Records without such a field are
//! the reader's own. An open validity end stays open (R030); 1.9 note types are read at field 5
//! by the reader (R029); attribute slots are fixed-size arrays (R031 is moot).
//!
//! The 1.8/1.9 defaults nobody reads are not applied: `Linky` `uses_stop_posts` and `one_way`
//! stay as written, and `Altdop`/`Altlinky` keep their optional distinctions (they are never
//! projected).

use std::collections::BTreeMap;

use jr_formats::jdf::{self, NoteType, RouteType, TransportMode, Version};
use jr_model::{Date, Diagnostic, Diagnostics, Rule, Severity};

pub use jr_formats::jdf::{
    AgencyAlternation, AlternateRouteName, AttributeRef, Header, Km, RouteIntegration, Stop,
    StopPost, TimeCell, Transfer, TripGroup,
};

use crate::policy::Policy;

mod checks;

pub(crate) use checks::{day_number, is_digits};

/// Pevnykod values are kept as written by the reader. Their meaning is the compiler's: the
/// known codes are the specification's list (1.10 and 1.11 together, in any version), an
/// all-digit code is a day of the week. An unknown code, a digit code that is no day number and
/// a second row for one attribute number (the first is used) are diagnosed per batch.
pub const R025: Rule = Rule {
    id: "JDF-R025",
    summary: "Fixed codes are checked against the specification's list; day digits must be days",
};

/// Route type, transport mode and Caskody type are closed code lists. The reader keeps an
/// unlisted code as written; it is diagnosed once per batch and code, and the stages treat it
/// as matching nothing (no declared international type, no note semantics).
pub const R026: Rule = Rule {
    id: "JDF-R026",
    summary: "Unlisted route types, transport modes and Caskody types are diagnosed",
};

/// JDF 1.8/1.9 have no route or agency distinction, transport mode, detour or grouping flag:
/// such a batch's routes are distinction 0 of their licence, buses, neither detours nor grouped
/// (the F# 1.9 → 1.10 upgrade). A 1.10/1.11 batch that leaves the detour or grouping flag empty
/// means `false` too (F# read an empty flag as `0`). Values in `[version_defaults]`.
pub const R028: Rule = Rule {
    id: "JDF-R028",
    summary: "Fields a 1.8/1.9 batch lacks take the version defaults; empty flags are false",
};

/// JDF 1.8–1.10 allow a timetable validity without an end. F# replaced it with 3000-01-01; here
/// an open end stays open (`None`) and the calendar runs it to the end of the horizon.
pub const R030: Rule = Rule {
    id: "JDF-R030",
    summary: "An open timetable validity end stays open and runs to the end of the horizon",
};

/// Attribute slots reference `Pevnykod.txt` numbers; F# aborted the batch on a number without a
/// row (`Array.find`). Such a slot is ignored, diagnosed once per batch and number.
pub const R283: Rule = Rule {
    id: "JDF-R283",
    summary: "An attribute number without a Pevnykod row is ignored",
};

/// `Dopravci.txt`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agency {
    pub id: Box<str>,
    pub tax_id: Option<Box<str>>,
    pub name: Box<str>,
    pub company_type: Option<jdf::CompanyType>,
    pub person_name: Box<str>,
    pub office_address: Box<str>,
    pub office_phone: Box<str>,
    pub control_phone: Option<Box<str>>,
    pub info_phone: Option<Box<str>>,
    pub fax: Option<Box<str>>,
    pub email: Option<Box<str>>,
    pub website: Option<Box<str>>,
    /// With `id` the key (R028 for 1.8/1.9).
    pub distinction: u32,
}

/// `Linky.txt`: one line version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub id: Box<str>,
    pub name: Box<str>,
    pub agency_id: Box<str>,
    pub route_type: Option<RouteType>,
    /// R028 for 1.8/1.9; `None` when a 1.10/1.11 batch leaves it empty.
    pub transport_mode: Option<TransportMode>,
    /// Výluka (R028).
    pub detour: bool,
    /// Seskupení spojů (R028).
    pub grouped: bool,
    pub uses_stop_posts: Option<bool>,
    pub one_way: Option<bool>,
    pub reserved1: Option<Box<str>>,
    pub licence_number: Option<Box<str>>,
    pub licence_valid_from: Option<Date>,
    pub licence_valid_to: Option<Date>,
    pub timetable_valid_from: Date,
    /// `None` is an open end (R030).
    pub timetable_valid_to: Option<Date>,
    /// R028.
    pub agency_distinction: u32,
    /// Rozlišení linky: with `id` the key (R028).
    pub distinction: u32,
}

/// `Zaslinky.txt`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteStop {
    pub route_id: Box<str>,
    pub route_stop_id: u64,
    pub zone: Option<Box<str>>,
    pub stop_id: u64,
    pub approximate_minutes: Option<u32>,
    pub attributes: [Option<u32>; 3],
    pub route_distinction: u32,
}

/// `Spoje.txt`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trip {
    pub route_id: Box<str>,
    pub id: u64,
    pub attributes: [Option<u32>; 10],
    pub trip_group_id: Option<u64>,
    pub route_distinction: u32,
}

/// `Zasspoje.txt`: a call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TripStop {
    pub route_id: Box<str>,
    pub trip_id: u64,
    pub route_stop_id: u64,
    pub stop_id: u64,
    pub stop_post_id: Option<u64>,
    pub stop_post_number: Option<Box<str>>,
    pub attributes: [Option<u32>; 3],
    pub km: Option<Km>,
    pub arrival: Option<TimeCell>,
    pub departure: Option<TimeCell>,
    pub earliest_arrival: Option<TimeCell>,
    pub latest_departure: Option<TimeCell>,
    pub route_distinction: u32,
}

/// `Udaje.txt`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteInfo {
    pub route_id: Box<str>,
    pub id: u64,
    pub text: Box<str>,
    pub route_distinction: u32,
}

/// `Caskody.txt`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceNote {
    pub route_id: Box<str>,
    pub trip_id: u64,
    pub id: u64,
    pub designation: Box<str>,
    pub note_type: Option<NoteType>,
    pub date_from: Option<Date>,
    pub date_to: Option<Date>,
    pub note: Option<Box<str>>,
    pub route_distinction: u32,
}

/// `Mistenky.txt`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reservation {
    pub route_id: Box<str>,
    pub trip_id: u64,
    pub note: Box<str>,
    pub route_distinction: u32,
}

/// One batch, every relation in file order, with the version defaults applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Batch {
    /// The export's identifier of the batch (its file name); batches are taken in ordinal order
    /// of it.
    pub name: Box<str>,
    /// The version the batch declares.
    pub version: Version,
    pub header: Header,
    pub stops: Vec<Stop>,
    pub stop_posts: Vec<StopPost>,
    pub agencies: Vec<Agency>,
    pub routes: Vec<Route>,
    pub route_integrations: Vec<RouteIntegration>,
    pub route_stops: Vec<RouteStop>,
    pub trips: Vec<Trip>,
    pub trip_groups: Vec<TripGroup>,
    pub trip_stops: Vec<TripStop>,
    pub route_info: Vec<RouteInfo>,
    pub attribute_refs: Vec<AttributeRef>,
    pub service_notes: Vec<ServiceNote>,
    pub transfers: Vec<Transfer>,
    pub agency_alternations: Vec<AgencyAlternation>,
    pub alternate_route_names: Vec<AlternateRouteName>,
    pub reservations: Vec<Reservation>,
}

impl Batch {
    /// The fixed code of each attribute number; the first row wins (R025).
    pub fn attribute_codes(&self) -> AttributeCodes<'_> {
        let mut codes = BTreeMap::new();
        for attribute in &self.attribute_refs {
            codes.entry(attribute.id).or_insert(&*attribute.code);
        }
        AttributeCodes { codes }
    }
}

/// Attribute number to fixed code, for one batch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AttributeCodes<'a> {
    codes: BTreeMap<u32, &'a str>,
}

impl<'a> AttributeCodes<'a> {
    pub fn get(&self, id: u32) -> Option<&'a str> {
        self.codes.get(&id).copied()
    }

    /// The codes of an attribute array, skipping empty slots and numbers without a row (R283).
    pub fn resolve<'s>(&'s self, slots: &'s [Option<u32>]) -> impl Iterator<Item = &'a str> + 's {
        slots.iter().flatten().filter_map(|&id| self.get(id))
    }
}

/// Applies the version defaults to a batch as read and diagnoses its codes (R025, R026, R028,
/// R030, R283). Diagnostics name the batch.
pub fn normalise(
    name: &str,
    source: jdf::Batch,
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> Batch {
    let defaults = &policy.version_defaults;
    let old = matches!(source.version, Version::V1_8 | Version::V1_9);
    let distinction = |value: Option<u32>| value.unwrap_or(defaults.route_distinction);
    let default_mode = transport_mode(&defaults.transport_mode);
    let mut empty_flags = 0usize;
    let mut open_ends = 0usize;

    let routes: Vec<Route> = source
        .routes
        .into_iter()
        .map(|r| {
            if !old && (r.detour.is_none() || r.grouped.is_none()) {
                empty_flags += 1;
            }
            if r.timetable_valid_to.is_none() {
                open_ends += 1;
            }
            Route {
                id: r.id,
                name: r.name,
                agency_id: r.agency_id,
                route_type: r.route_type,
                transport_mode: match r.transport_mode {
                    None if old => Some(default_mode.clone()),
                    mode => mode,
                },
                detour: r.detour.unwrap_or(defaults.detour),
                grouped: r.grouped.unwrap_or(defaults.grouped),
                uses_stop_posts: r.uses_stop_posts,
                one_way: r.one_way,
                reserved1: r.reserved1,
                licence_number: r.licence_number,
                licence_valid_from: r.licence_valid_from,
                licence_valid_to: r.licence_valid_to,
                timetable_valid_from: r.timetable_valid_from,
                timetable_valid_to: r.timetable_valid_to,
                agency_distinction: r.agency_distinction.unwrap_or(defaults.agency_distinction),
                distinction: distinction(r.distinction),
            }
        })
        .collect();

    let batch = Batch {
        name: name.into(),
        version: source.version,
        header: source.header,
        stops: source.stops,
        stop_posts: source.stop_posts,
        agencies: source
            .agencies
            .into_iter()
            .map(|a| Agency {
                id: a.id,
                tax_id: a.tax_id,
                name: a.name,
                company_type: a.company_type,
                person_name: a.person_name,
                office_address: a.office_address,
                office_phone: a.office_phone,
                control_phone: a.control_phone,
                info_phone: a.info_phone,
                fax: a.fax,
                email: a.email,
                website: a.website,
                distinction: a.distinction.unwrap_or(defaults.agency_distinction),
            })
            .collect(),
        routes,
        route_integrations: source.route_integrations,
        route_stops: source
            .route_stops
            .into_iter()
            .map(|s| RouteStop {
                route_id: s.route_id,
                route_stop_id: s.route_stop_id,
                zone: s.zone,
                stop_id: s.stop_id,
                approximate_minutes: s.approximate_minutes,
                attributes: s.attributes,
                route_distinction: distinction(s.route_distinction),
            })
            .collect(),
        trips: source
            .trips
            .into_iter()
            .map(|t| Trip {
                route_id: t.route_id,
                id: t.id,
                attributes: t.attributes,
                trip_group_id: t.trip_group_id,
                route_distinction: distinction(t.route_distinction),
            })
            .collect(),
        trip_groups: source.trip_groups,
        trip_stops: source
            .trip_stops
            .into_iter()
            .map(|c| TripStop {
                route_id: c.route_id,
                trip_id: c.trip_id,
                route_stop_id: c.route_stop_id,
                stop_id: c.stop_id,
                stop_post_id: c.stop_post_id,
                stop_post_number: c.stop_post_number,
                attributes: c.attributes,
                km: c.km,
                arrival: c.arrival,
                departure: c.departure,
                earliest_arrival: c.earliest_arrival,
                latest_departure: c.latest_departure,
                route_distinction: distinction(c.route_distinction),
            })
            .collect(),
        route_info: source
            .route_info
            .into_iter()
            .map(|i| RouteInfo {
                route_id: i.route_id,
                id: i.id,
                text: i.text,
                route_distinction: distinction(i.route_distinction),
            })
            .collect(),
        attribute_refs: source.attribute_refs,
        service_notes: source
            .service_notes
            .into_iter()
            .map(|n| ServiceNote {
                route_id: n.route_id,
                trip_id: n.trip_id,
                id: n.id,
                designation: n.designation,
                note_type: n.note_type,
                date_from: n.date_from,
                date_to: n.date_to,
                note: n.note,
                route_distinction: distinction(n.route_distinction),
            })
            .collect(),
        transfers: source.transfers,
        agency_alternations: source.agency_alternations,
        alternate_route_names: source.alternate_route_names,
        reservations: source
            .reservations
            .into_iter()
            .map(|r| Reservation {
                route_id: r.route_id,
                trip_id: r.trip_id,
                note: r.note,
                route_distinction: distinction(r.route_distinction),
            })
            .collect(),
    };

    let info = |code, message: String, rule| {
        Diagnostic::new(code, Severity::Info, name, message).rule(rule)
    };
    if old && !batch.routes.is_empty() {
        diagnostics.push(info(
            "jdf_version_defaults",
            format!(
                "JDF {}: {} routes are distinction {}, agency distinction {}, transport mode {}, \
                 detour {}, grouped {}",
                batch.version,
                batch.routes.len(),
                defaults.route_distinction,
                defaults.agency_distinction,
                defaults.transport_mode,
                defaults.detour,
                defaults.grouped
            ),
            R028,
        ));
    }
    if empty_flags > 0 {
        diagnostics.push(info(
            "jdf_route_flag_empty",
            format!(
                "{empty_flags} routes leave the detour or grouping flag empty: detour {}, \
                 grouped {}",
                defaults.detour, defaults.grouped
            ),
            R028,
        ));
    }
    if open_ends > 0 {
        diagnostics.push(info(
            "jdf_validity_open_end",
            format!("{open_ends} routes have no timetable validity end: open"),
            R030,
        ));
    }
    checks::check_fixed_codes(&batch, policy, diagnostics);
    checks::check_codes(&batch, diagnostics);
    checks::check_attribute_numbers(&batch, diagnostics);
    batch
}

/// The transport mode of a `Linky.txt` code.
fn transport_mode(code: &str) -> TransportMode {
    [
        TransportMode::Bus,
        TransportMode::Tram,
        TransportMode::CableCar,
        TransportMode::Metro,
        TransportMode::Ferry,
        TransportMode::Trolleybus,
    ]
    .into_iter()
    .find(|mode| mode.code() == code)
    .unwrap_or_else(|| TransportMode::Other(code.into()))
}
