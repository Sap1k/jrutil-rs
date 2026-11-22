//! One record type per relation: the superset of the fields of JDF 1.8/1.9, 1.10 and 1.11.
//!
//! Text fields the specification requires are `Box<str>` as written (possibly empty); optional
//! ones are `None` when empty. A field a version does not have is `None` in that version's
//! batches; the per-version source is in each field's comment ("1.9" covers 1.8 and 1.9, which
//! share one layout). Attribute slots hold the `Pevnykod.txt` numbers they reference.

use jr_model::Date;

use super::codes::{CompanyType, Km, NoteType, RouteType, TimeCell, TransportMode};

/// `VerzeJDF.txt`. 1.10 and 1.11 write six fields; real 1.8 batches only the version. Fields
/// 2–6 are read whenever the file has them, in any version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    /// Field 1, as written (`1.11`).
    pub version: Box<str>,
    /// Field 2, číslo dopravního úřadu.
    pub du_number: Option<u32>,
    /// Field 3, region.
    pub region: Option<Box<str>>,
    /// Field 4, identification of the batch.
    pub batch_id: Option<Box<str>>,
    /// Field 5, creation date.
    pub created: Option<Date>,
    /// Field 6, the generating program or person.
    pub generator: Option<Box<str>>,
}

/// `Zastavky.txt` (12 fields in every version).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stop {
    /// Stop number, local to the batch.
    pub id: u64,
    /// Obec.
    pub town: Box<str>,
    /// Část obce.
    pub district: Option<Box<str>>,
    /// Bližší místo (in practice the stop's own name).
    pub nearby_place: Option<Box<str>>,
    /// Okres code (`AB` is Praha), as written.
    pub okres: Option<Box<str>>,
    /// Country code as written (`CZ`, `D`, `A`, `SK`, `PL`, …).
    pub country: Option<Box<str>>,
    /// Fields 7–12.
    pub attributes: [Option<u32>; 6],
}

/// `Oznacniky.txt` (1.10 and later; 7 fields).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StopPost {
    pub stop_id: u64,
    pub post_id: u64,
    pub name: Option<Box<str>>,
    pub description: Option<Box<str>>,
    /// The post's name within the stop.
    pub post_name: Option<Box<str>>,
    pub reserved1: Option<Box<str>>,
    pub reserved2: Option<Box<str>>,
}

/// `Dopravci.txt` (1.9: 12 fields; 1.10, 1.11: 13).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agency {
    /// IČO.
    pub id: Box<str>,
    /// DIČ.
    pub tax_id: Option<Box<str>>,
    pub name: Box<str>,
    /// `None` when empty.
    pub company_type: Option<CompanyType>,
    pub person_name: Box<str>,
    pub office_address: Box<str>,
    pub office_phone: Box<str>,
    pub control_phone: Option<Box<str>>,
    pub info_phone: Option<Box<str>>,
    pub fax: Option<Box<str>>,
    pub email: Option<Box<str>>,
    pub website: Option<Box<str>>,
    /// Rozlišení dopravce: with `id` the key. Field 13 (1.10, 1.11); 1.9: `None`.
    pub distinction: Option<u32>,
}

/// `Linky.txt` (1.9: 10 fields; 1.10: 16; 1.11: 17).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    /// Licence line number (six digits in real data).
    pub id: Box<str>,
    pub name: Box<str>,
    pub agency_id: Box<str>,
    /// `None` when empty.
    pub route_type: Option<RouteType>,
    /// 1.10, 1.11 field 5; 1.9: `None`.
    pub transport_mode: Option<TransportMode>,
    /// Výluka. 1.10, 1.11 field 6; 1.9: `None`. `None` also when empty.
    pub detour: Option<bool>,
    /// Seskupení spojů. 1.10, 1.11 field 7; 1.9: `None`.
    pub grouped: Option<bool>,
    /// Uses stop posts (označníky). 1.10, 1.11 field 8; 1.9: `None`.
    pub uses_stop_posts: Option<bool>,
    /// 1.11 field 9; earlier: `None`.
    pub one_way: Option<bool>,
    pub reserved1: Option<Box<str>>,
    pub licence_number: Option<Box<str>>,
    pub licence_valid_from: Option<Date>,
    pub licence_valid_to: Option<Date>,
    pub timetable_valid_from: Date,
    /// Required in 1.11; optional before.
    pub timetable_valid_to: Option<Date>,
    /// Rozlišení dopravce. 1.10 field 15, 1.11 field 16; 1.9: `None`.
    pub agency_distinction: Option<u32>,
    /// Rozlišení linky: with `id` the key. 1.10 field 16, 1.11 field 17; 1.9: `None`.
    pub distinction: Option<u32>,
}

/// `LinExt.txt` (1.10 and later; 7 fields).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteIntegration {
    pub route_id: Box<str>,
    /// Pořadové číslo, per route.
    pub entry_number: u32,
    /// The integrated transport system's number.
    pub transport_system_id: u64,
    /// The route's designation in that system.
    pub route_name: Box<str>,
    /// Preferential designation. `None` when empty.
    pub preferential: Option<bool>,
    pub reserved1: Box<str>,
    pub route_distinction: u32,
}

/// `Zaslinky.txt` (1.9: 7 fields; 1.10, 1.11: 9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteStop {
    pub route_id: Box<str>,
    /// Tarifní číslo: the stop's place in the printed timetable.
    pub route_stop_id: u64,
    /// Tariff zone(s), comma-separated as written. Field 3 in every version (the F# 1.9 model
    /// calls it `reserved1`; real 1.8 batches carry zones there).
    pub zone: Option<Box<str>>,
    pub stop_id: u64,
    /// Minutes from the route's start. 1.10, 1.11 field 5; 1.9: `None`.
    pub approximate_minutes: Option<u32>,
    /// 1.9 fields 5–7; 1.10, 1.11 fields 6–8.
    pub attributes: [Option<u32>; 3],
    /// 1.10, 1.11 field 9; 1.9: `None`.
    pub route_distinction: Option<u32>,
}

/// `Spoje.txt` (1.9: 12 fields; 1.10, 1.11: 14).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trip {
    pub route_id: Box<str>,
    /// Trip number: odd runs forward, even in reverse.
    pub id: u64,
    /// Fields 3–12.
    pub attributes: [Option<u32>; 10],
    /// 1.10, 1.11 field 13; 1.9: `None`.
    pub trip_group_id: Option<u64>,
    /// 1.10, 1.11 field 14; 1.9: `None`.
    pub route_distinction: Option<u32>,
}

/// `SpojSkup.txt` (1.10 and later; 5 fields).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TripGroup {
    pub id: u64,
    pub entry_number: u32,
    pub name: Box<str>,
    pub description: Option<Box<str>>,
    pub reserved1: Box<str>,
}

/// `Zasspoje.txt`: a call (1.9: 10 fields; 1.10: 12; 1.11: 15).
///
/// A time field is `None` when the version does not have it or when its text is not a time
/// cell (reported); a written empty cell is `Some(TimeCell::Empty)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TripStop {
    pub route_id: Box<str>,
    pub trip_id: u64,
    pub route_stop_id: u64,
    pub stop_id: u64,
    /// The `Oznacniky.txt` post number. 1.10, 1.11 field 5; 1.9: `None`.
    pub stop_post_id: Option<u64>,
    /// Stanoviště, as written. 1.9 field 5; 1.10, 1.11 field 6.
    pub stop_post_number: Option<Box<str>>,
    /// 1.9 fields 6–7, 1.10 fields 7–8 (third slot `None`); 1.11 fields 7–9.
    pub attributes: [Option<u32>; 3],
    /// 1.9 field 8, 1.10 field 9, 1.11 field 10.
    pub km: Option<Km>,
    /// 1.9 field 9, 1.10 field 10, 1.11 field 11.
    pub arrival: Option<TimeCell>,
    /// 1.9 field 10, 1.10 field 11, 1.11 field 12.
    pub departure: Option<TimeCell>,
    /// 1.11 field 13; earlier: `None`.
    pub earliest_arrival: Option<TimeCell>,
    /// 1.11 field 14; earlier: `None`.
    pub latest_departure: Option<TimeCell>,
    /// 1.10 field 12, 1.11 field 15; 1.9: `None`.
    pub route_distinction: Option<u32>,
}

/// `Udaje.txt` (1.9: 3 fields; 1.10, 1.11: 4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteInfo {
    pub route_id: Box<str>,
    pub id: u64,
    pub text: Box<str>,
    /// 1.10, 1.11 field 4; 1.9: `None`.
    pub route_distinction: Option<u32>,
}

/// `Pevnykod.txt` (3 fields in every version).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttributeRef {
    /// The number the attribute slots reference.
    pub id: u32,
    /// The fixed code as written (`X`, `+`, `1`–`7`, `@`, …); mapping it is the compiler's.
    pub code: Box<str>,
    pub reserved1: Option<Box<str>>,
}

/// `Caskody.txt` (1.9: 8 fields; 1.10, 1.11: 9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceNote {
    pub route_id: Box<str>,
    pub trip_id: u64,
    pub id: u64,
    /// Označení časového kódu, as written.
    pub designation: Box<str>,
    /// Field 5 in every version (the F# 1.9 model calls it `timeType`). `None` when empty.
    pub note_type: Option<NoteType>,
    pub date_from: Option<Date>,
    pub date_to: Option<Date>,
    pub note: Option<Box<str>>,
    /// 1.10, 1.11 field 9; 1.9: `None`.
    pub route_distinction: Option<u32>,
}

/// `Navaznosti.txt` (1.10: 11 fields; 1.11: 12). The transfer IDs point into the national
/// registers, not into the batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transfer {
    /// Druh návaznosti (`m` or `M` in real data), as written.
    pub kind: Box<str>,
    pub route_id: Box<str>,
    pub trip_id: u64,
    pub route_stop_id: u64,
    pub transfer_route_id: Option<u64>,
    pub transfer_stop_id: Option<u64>,
    pub transfer_stop_post_id: Option<u64>,
    pub transfer_end_stop_id: Option<u64>,
    pub transfer_end_stop_post_id: Option<u64>,
    pub wait_minutes: Option<u32>,
    /// 1.11 field 11; 1.10: `None`.
    pub note: Option<Box<str>>,
    /// 1.10 field 11, 1.11 field 12.
    pub route_distinction: u32,
}

/// `Altdop.txt` (1.9: 13 fields; 1.10, 1.11: 15).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgencyAlternation {
    pub route_id: Box<str>,
    pub trip_id: u64,
    pub agency_id: Box<str>,
    /// Fields 4–9.
    pub attributes: [Option<u32>; 6],
    /// Typ časového kódu, as written.
    pub time_type: Option<Box<str>>,
    pub reserved1: Option<Box<str>>,
    pub date_from: Option<Date>,
    pub date_to: Option<Date>,
    /// 1.10, 1.11 field 14; 1.9: `None`.
    pub agency_distinction: Option<u32>,
    /// 1.10, 1.11 field 15; 1.9: `None`.
    pub route_distinction: Option<u32>,
}

/// `Altlinky.txt` (1.9: 3 fields; 1.10, 1.11: 4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlternateRouteName {
    pub route_id: Box<str>,
    pub alt_route_number: u32,
    pub country: Box<str>,
    /// 1.10, 1.11 field 4; 1.9: `None`.
    pub route_distinction: Option<u32>,
}

/// `Mistenky.txt` (1.9: 3 fields; 1.10, 1.11: 4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reservation {
    pub route_id: Box<str>,
    pub trip_id: u64,
    pub note: Box<str>,
    /// 1.10, 1.11 field 4; 1.9: `None`.
    pub route_distinction: Option<u32>,
}
