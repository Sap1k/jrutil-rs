//! Merging the fixed batches: stop identity, shared entities and line versions (JDF-R207–R254),
//! then the international filter on the result (R269–R281).
//!
//! Order is a rule (verdicts): batches are taken in ordinal order of their names, and where a
//! rule says "first" it means first in that order, then by position within the batch. Stops are
//! reconciled incrementally in that order ([`reconcile`], `stops`); every other relation of a
//! batch then refers to merged stops, agencies and versions by index. Versions are never copied
//! or cut: the per-date resolution decides on the published validity which dates each version
//! wins, and each trip runs on its calendar within those dates (`versions`).

mod assemble;
mod filter;
mod policy;
pub mod reconcile;
mod registry;
mod stops;
mod versions;

use std::collections::HashMap;

use jr_formats::jdf::{Km, TimeCell};
use jr_geo::names::Analyzer;
use jr_model::{Coord, Date, DateSet, Diagnostics, Horizon, Rule, Strings, Sym};

pub use filter::{FilterRecord, apply_international};
pub use policy::{CandidatesPolicy, Policy, StopsPolicy};
pub use reconcile::{MatchKind, Statistics};
pub use registry::Registry;
pub use stops::provisional_number;

use crate::batch::{self, AlternateRouteName, RouteIntegration, Transfer, TripGroup};
use crate::fixed::{FixedBatch, Location};

/// Every rule the merge implements.
pub const RULES: &[Rule] = &[
    registry::R207,
    registry::R208,
    registry::R209,
    registry::R210,
    stops::R213,
    reconcile::R216,
    reconcile::R217,
    reconcile::R218,
    reconcile::R219,
    reconcile::R220,
    reconcile::R221,
    reconcile::R222,
    stops::R223,
    stops::R224,
    reconcile::R225,
    stops::R226,
    stops::R227,
    stops::R229,
    stops::R230,
    stops::R231,
    stops::R232,
    stops::R233,
    stops::R234,
    R235,
    stops::R236,
    stops::R237,
    assemble::R238,
    assemble::R240,
    assemble::R241,
    assemble::R242,
    versions::R243,
    versions::R244,
    versions::R245,
    versions::R246,
    versions::R247,
    versions::R248,
    versions::R249,
    versions::R254,
    filter::R279,
    filter::R280,
    filter::R284,
    assemble::R285,
];

/// The reconciliation counts are reported with the result ([`Merged::statistics`]) and as
/// diagnostics per merged, ambiguous, quarantined and provisional spelling.
pub const R235: Rule = Rule {
    id: "JDF-R235",
    summary: "Reconciliation is counted by kind and diagnosed per spelling",
};

#[derive(Debug, thiserror::Error)]
pub enum MergeError {
    #[error("no timetable year {0}")]
    Year(i16),
    #[error("the reference date {date} is outside the timetable year {year} ({start}..={end})")]
    ReferenceDate {
        date: Date,
        year: i16,
        start: Date,
        end: Date,
    },
    #[error("two batches are named {0}")]
    DuplicateBatch(Box<str>),
    #[error("batch {batch} was added after {previous}: batches are merged in name order")]
    OutOfOrder { batch: Box<str>, previous: Box<str> },
    #[error("batch {batch}: {stops} stops but {fixed} fixed stops")]
    StopCount {
        batch: Box<str>,
        stops: usize,
        fixed: usize,
    },
}

/// What the merge reads besides the batches.
#[derive(Clone, Copy, Debug)]
pub struct MergeContext<'a> {
    pub registry: &'a Registry,
    /// The stop-identity name profile (`data/names/road_names.toml` `[identity]` with the
    /// synonym table; JDF-R214, R215).
    pub names: &'a Analyzer,
    /// The batch and calendar policy (`policy/jdf.toml`).
    pub jdf: &'a crate::Policy,
    pub timetable_year: i16,
    pub reference_date: Date,
}

/// The merged road facts: what the assembly into a `Timetable` reads.
#[derive(Clone, Debug, PartialEq)]
pub struct Merged {
    pub horizon: Horizon,
    /// The build's reference date (R249); it dates the merged export (JDF-R469).
    pub reference_date: Date,
    /// In number order.
    pub stops: Vec<Stop>,
    /// Deduplicated (R238), in order of first appearance.
    pub agencies: Vec<batch::Agency>,
    /// Surviving line versions in (licence, batch name, position) order.
    pub versions: Vec<Version>,
    /// R232–R234 rows, as data.
    pub registry_candidates: Vec<RegistryCandidate>,
    /// The fixed-code sets entities refer to.
    pub code_sets: CodeSets,
    /// Interned call texts (stanoviště).
    pub texts: Strings,
    pub statistics: Statistics,
}

/// A name triple with its okres and country, as published.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Spelling {
    pub town: Box<str>,
    pub district: Option<Box<str>>,
    pub nearby_place: Option<Box<str>>,
    pub okres: Option<Box<str>>,
    pub country: Option<Box<str>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProvisionalReason {
    /// No stop matched (R224).
    New,
    /// Quarantined: several stops matched (R224).
    Ambiguous,
}

/// One merged stop.
#[derive(Clone, Debug, PartialEq)]
pub struct Stop {
    /// The registered or provisional number (`jdf:stop:<number>`).
    pub number: u64,
    /// `None` for a registered number.
    pub provisional: Option<ProvisionalReason>,
    /// The published spelling (R226).
    pub name: Spelling,
    /// R231.
    pub location: Option<Location>,
    /// R227.
    pub codes: CodeSetId,
    /// R236, by observation.
    pub post_candidates: Vec<PostCandidate>,
    /// R237, by post number.
    pub posts: Vec<Post>,
}

/// An observed position that may be one of the stop's posts.
#[derive(Clone, Debug, PartialEq)]
pub struct PostCandidate {
    pub coord: Coord,
    pub observation: Box<str>,
    pub facts: Vec<(Box<str>, Box<str>)>,
}

/// An `Oznacniky.txt` post of the stop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Post {
    pub post_id: u64,
    pub name: Option<Box<str>>,
    pub description: Option<Box<str>>,
    pub post_name: Option<Box<str>>,
}

/// One row of the registry candidates file.
#[derive(Clone, Debug, PartialEq)]
pub struct RegistryCandidate {
    /// The provisional number; `None` for an alias row.
    pub provisional_number: Option<u64>,
    pub spelling: Spelling,
    /// The stop's position when precise.
    pub location: Option<Coord>,
    pub reason: Box<str>,
    /// Suggested registered numbers (R234); the number itself for an alias row.
    pub suggestions: Vec<u64>,
}

/// One surviving line version.
#[derive(Clone, Debug, PartialEq)]
pub struct Version {
    /// The batch it came from.
    pub batch: Box<str>,
    /// Its row in the batch's `Linky.txt`.
    pub position: u32,
    /// The batch's creation date (R240).
    pub created: Option<Date>,
    /// The `Linky` row as published.
    pub route: batch::Route,
    /// Index into [`Merged::agencies`].
    pub agency: usize,
    pub key: VersionKey,
    /// The intervals it won (R247), bounded to the timetable year (R249).
    pub won: Vec<(Date, Date)>,
    /// Those dates.
    pub dates: DateSet,
    /// Regional cross-border service (R280), set by the international filter.
    pub regional_cross_border: bool,
    pub integrations: Vec<RouteIntegration>,
    pub route_stops: Vec<RouteStop>,
    pub trips: Vec<Trip>,
    pub route_info: Vec<batch::RouteInfo>,
    pub notes: Vec<batch::ServiceNote>,
    pub transfers: Vec<Transfer>,
    pub agency_alternations: Vec<AgencyAlternation>,
    pub alternate_names: Vec<AlternateRouteName>,
    pub reservations: Vec<batch::Reservation>,
}

impl Version {
    /// How diagnostics name the version.
    pub fn subject(&self) -> String {
        format!(
            "{}/{}@{}",
            self.route.id, self.route.distinction, self.batch
        )
    }
}

/// The parts of a version's trip identity (R254): `jr_model::identity` builds the ID.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VersionKey {
    pub licence: Box<str>,
    /// The published validity start.
    pub valid_from: Date,
    pub detour: bool,
    /// Eight lower-case hex characters, for a version sharing start and detour flag with an
    /// earlier-ending one.
    pub hash: Option<Box<str>>,
    /// `n ≥ 2` for the n-th side-by-side version of identical published validity.
    pub part: Option<u32>,
}

/// A `Zaslinky.txt` row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteStop {
    pub route_stop_id: u64,
    pub zone: Option<Box<str>>,
    /// Index into [`Merged::stops`].
    pub stop: u32,
    pub approximate_minutes: Option<u32>,
    pub codes: CodeSetId,
}

/// A `Spoje.txt` row with its calls.
#[derive(Clone, Debug, PartialEq)]
pub struct Trip {
    pub id: u64,
    pub codes: CodeSetId,
    /// Its `SpojSkup` group, for a grouped version (R242).
    pub group: Option<TripGroup>,
    /// Its calendar within the dates its version won (R248, R255–R264).
    pub days: DateSet,
    /// In `Zasspoje.txt` order.
    pub calls: Vec<Call>,
}

/// A `Zasspoje.txt` row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Call {
    /// Index into [`Merged::stops`].
    pub stop: u32,
    pub codes: CodeSetId,
    pub route_stop_id: u64,
    pub stop_post_id: Option<u64>,
    /// In [`Merged::texts`].
    pub stop_post_number: Option<Sym>,
    pub km: Option<Km>,
    pub arrival: Option<TimeCell>,
    pub departure: Option<TimeCell>,
    pub earliest_arrival: Option<TimeCell>,
    pub latest_departure: Option<TimeCell>,
}

/// An `Altdop.txt` row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgencyAlternation {
    pub trip_id: u64,
    /// Index into [`Merged::agencies`].
    pub agency: usize,
    pub codes: CodeSetId,
    pub time_type: Option<Box<str>>,
    pub reserved1: Option<Box<str>>,
    pub date_from: Option<Date>,
    pub date_to: Option<Date>,
}

/// A handle of a fixed-code set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CodeSetId(u32);

/// Interned fixed-code lists (R213): attribute slots resolved to codes, in slot order.
#[derive(Clone, Debug, PartialEq)]
pub struct CodeSets {
    sets: Vec<Box<[Box<str>]>>,
    /// Looked up, never iterated.
    index: HashMap<Box<[Box<str>]>, CodeSetId>,
}

impl Default for CodeSets {
    fn default() -> Self {
        let mut sets = CodeSets {
            sets: Vec::new(),
            index: HashMap::new(),
        };
        sets.intern(std::iter::empty());
        sets
    }
}

impl CodeSets {
    /// The empty set.
    pub const EMPTY: CodeSetId = CodeSetId(0);

    pub fn intern<'s>(&mut self, codes: impl IntoIterator<Item = &'s str>) -> CodeSetId {
        let key: Box<[Box<str>]> = codes.into_iter().map(Box::from).collect();
        if let Some(&id) = self.index.get(&key) {
            return id;
        }
        let id = CodeSetId(self.sets.len() as u32);
        self.sets.push(key.clone());
        self.index.insert(key, id);
        id
    }

    pub fn get(&self, id: CodeSetId) -> &[Box<str>] {
        &self.sets[id.0 as usize]
    }
}

/// The merge in progress: batches are added one at a time in ordinal order of their names (the
/// stated order of every "first" in the rules), so a caller can match batches' stops in
/// parallel, in bounded groups, and hand each over as soon as its turn comes. What a batch
/// contributes is kept in the merged form only; the batch itself is dropped.
pub struct Merger<'a> {
    context: MergeContext<'a>,
    horizon: Horizon,
    stops: stops::Stops<'a>,
    agencies: assemble::Agencies,
    code_sets: CodeSets,
    texts: Strings,
    versions: Vec<Version>,
    last: Option<Box<str>>,
}

impl std::fmt::Debug for Merger<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Merger")
            .field("last", &self.last)
            .field("versions", &self.versions.len())
            .finish_non_exhaustive()
    }
}

impl<'a> Merger<'a> {
    pub fn new(context: MergeContext<'a>, policy: &'a Policy) -> Result<Merger<'a>, MergeError> {
        let year = context.timetable_year;
        let horizon = Horizon::gvd(year).map_err(|_| MergeError::Year(year))?;
        if !horizon.contains(context.reference_date) {
            return Err(MergeError::ReferenceDate {
                date: context.reference_date,
                year,
                start: horizon.start(),
                end: horizon.end(),
            });
        }
        Ok(Merger {
            stops: stops::Stops::new(&context.registry.rows, context.names, policy),
            context,
            horizon,
            agencies: assemble::Agencies::default(),
            code_sets: CodeSets::default(),
            texts: Strings::new(),
            versions: Vec::new(),
            last: None,
        })
    }

    /// Reconciles the batch's stops and takes its line versions (R207–R242).
    pub fn add(
        &mut self,
        fixed: FixedBatch,
        diagnostics: &mut Diagnostics,
    ) -> Result<(), MergeError> {
        let name = &fixed.batch.name;
        match &self.last {
            Some(previous) if previous == name => {
                return Err(MergeError::DuplicateBatch(name.clone()));
            }
            Some(previous) if previous > name => {
                return Err(MergeError::OutOfOrder {
                    batch: name.clone(),
                    previous: previous.clone(),
                });
            }
            _ => self.last = Some(name.clone()),
        }
        if fixed.stops.len() != fixed.batch.stops.len() {
            return Err(MergeError::StopCount {
                batch: name.clone(),
                stops: fixed.batch.stops.len(),
                fixed: fixed.stops.len(),
            });
        }
        let map = self.stops.add_batch(&fixed, diagnostics);
        let mut position = HashMap::with_capacity(fixed.batch.stops.len());
        for (at, stop) in fixed.batch.stops.iter().enumerate() {
            position.entry(stop.id).or_insert(at);
        }
        let unknown_posts = self.stops.add_posts(&fixed, &map, &position, diagnostics);
        let mut context = assemble::Context {
            stop_map: &map,
            stop_position: &position,
            agencies: &mut self.agencies,
            code_sets: &mut self.code_sets,
            texts: &mut self.texts,
            jdf: self.context.jdf,
            unknown_posts,
        };
        assemble::add_batch(fixed.batch, &mut context, &mut self.versions, diagnostics);
        Ok(())
    }

    /// Decides which dates each version wins and each trip runs (R243–R264), numbers the
    /// stops and drops what nothing uses (R284).
    pub fn finish(self, diagnostics: &mut Diagnostics) -> Merged {
        let Merger {
            context,
            horizon,
            stops,
            agencies,
            mut code_sets,
            texts,
            versions,
            last: _,
        } = self;
        let holidays = crate::holidays::Holidays::new(context.jdf, horizon);
        let mut versions = versions::resolve(
            versions,
            horizon,
            context.reference_date,
            &holidays,
            &code_sets,
            context.jdf,
            diagnostics,
        );

        // Every draft stop in number order, then what no version uses goes (R284).
        let mut order: Vec<usize> = (0..stops.drafts.len()).collect();
        order.sort_by_key(|&index| stops.drafts[index].number);
        let mut final_index = vec![0u32; stops.drafts.len()];
        for (position, &index) in order.iter().enumerate() {
            final_index[index] = position as u32;
        }
        let merged_stops: Vec<Stop> = order
            .iter()
            .map(|&index| {
                let draft = &stops.drafts[index];
                Stop {
                    number: draft.number,
                    provisional: draft.provisional,
                    name: draft.name.clone(),
                    location: draft.location.clone(),
                    codes: stops::Stops::code_set(draft, &mut code_sets),
                    post_candidates: draft.post_candidates.values().cloned().collect(),
                    posts: draft.posts.values().cloned().collect(),
                }
            })
            .collect();
        for version in &mut versions {
            for route_stop in &mut version.route_stops {
                route_stop.stop = final_index[route_stop.stop as usize];
            }
            for trip in &mut version.trips {
                for call in &mut trip.calls {
                    call.stop = final_index[call.stop as usize];
                }
            }
        }
        let mut merged = Merged {
            horizon,
            reference_date: context.reference_date,
            stops: merged_stops,
            agencies: agencies.rows,
            versions,
            registry_candidates: Vec::new(),
            code_sets,
            texts,
            statistics: stops.statistics(),
        };
        filter::prune(&mut merged, diagnostics);
        let surviving: std::collections::HashSet<u64> =
            merged.stops.iter().map(|stop| stop.number).collect();
        let kept: Vec<bool> = stops
            .drafts
            .iter()
            .map(|draft| surviving.contains(&draft.number))
            .collect();
        merged.registry_candidates = stops.candidates(&kept);
        merged
    }
}

/// Merges fixed batches held together (R207–R254): they are taken in name order.
pub fn merge(
    mut batches: Vec<FixedBatch>,
    context: MergeContext<'_>,
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> Result<Merged, MergeError> {
    batches.sort_by(|a, b| a.batch.name.cmp(&b.batch.name));
    let mut merger = Merger::new(context, policy)?;
    for fixed in batches {
        merger.add(fixed, diagnostics)?;
    }
    Ok(merger.finish(diagnostics))
}
