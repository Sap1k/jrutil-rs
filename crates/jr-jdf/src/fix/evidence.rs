//! What stop matching reads besides the batch, built once per build (JDF-R006–R016): the
//! matcher corpus and its name index, the okres and municipality areas, the gazetteers, and the
//! memo of the context-free matching (verdict Q18).

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, RwLock};

use jr_formats::geodata::{catalogue, gazetteer, synonyms};
use jr_formats::osm::Node;
use jr_geo::areas::{Areas, Polygon};
use jr_geo::laea::{self, Laea};
use jr_geo::names::Analyzer;
use jr_geo::stop_index::{Candidate, StopIndex};
use jr_geo::towns::TownSynonyms;
use jr_model::{Coord, Diagnostic, Diagnostics, Severity};

use super::osm;
use super::policy::Policy;
use super::select::Selection;
use super::{R006, R007, R008, R015, R016};

/// The inputs of [`Evidence::build`], as the readers return them.
#[derive(Debug, Default)]
pub struct EvidenceInputs<'a> {
    /// (path relative to the catalogue directory, with `/` separators; its rows), in path order.
    pub catalogues: Vec<(&'a str, Vec<catalogue::Row>)>,
    pub osm_nodes: Vec<Node>,
    pub okresy: Vec<gazetteer::Okres>,
    pub towns: Vec<gazetteer::Town>,
    pub foreign_towns: Vec<gazetteer::ForeignTown>,
    pub synonyms: Vec<synonyms::Line>,
}

/// Where a corpus entry comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum SourceClass {
    /// A catalogue the policy lists as reviewed corrections.
    Gapfill,
    Catalogue,
    Osm,
}

impl SourceClass {
    /// R047, R070: a catalogue row (reviewed or not), as opposed to OSM.
    pub(crate) fn is_checked(self) -> bool {
        self != SourceClass::Osm
    }
}

/// One stop a timetable stop can be matched to.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    pub name: Box<str>,
    pub okres: Option<Box<str>>,
    pub country: Option<Box<str>>,
    pub coord: Coord,
    pub class: SourceClass,
    /// The source label F# wrote to `PolzastZdroj` (`external:{file stem}`, `osm:node:{id}`).
    pub source: Box<str>,
    /// The observation's identity, stable between builds: `osm:node:{id}`, or for a catalogue
    /// row F#'s `external:{path}:sha256:{sha(path|the five columns joined |)}` (JDF-R007), so
    /// the post estimator's hypothesis IDs equal F#'s.
    pub observation: Box<str>,
    /// R013: what the post estimator reads.
    pub facts: Vec<(Box<str>, Box<str>)>,
}

/// One municipality.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Municipality {
    pub name: Box<str>,
    pub okres: Option<Box<str>>,
}

/// Everything stop matching reads besides the batch: built once per build, shared by all
/// batches. `fix` must be called with the policy the evidence was built with (the memo assumes
/// it).
#[derive(Debug)]
pub struct Evidence {
    /// The corpus in R014's explicit order: catalogue rows in path and row order, then OSM
    /// nodes in file order. The entry number is the position.
    pub(crate) entries: Vec<Entry>,
    pub(crate) index: StopIndex,
    okres_by_code: HashMap<Box<str>, usize>,
    pub(crate) okres_areas: Areas,
    /// R046: pairs of okres areas (smaller first) within the adjacency distance.
    adjacency: BTreeSet<(usize, usize)>,
    pub(crate) municipalities: Vec<Municipality>,
    pub(crate) municipality_areas: Areas,
    municipality_index: StopIndex,
    /// Upper-cased ISO country of each European town name.
    foreign_countries: Vec<Box<str>>,
    foreign_index: StopIndex,
    town_synonyms: TownSynonyms,
    candidates: RwLock<HashMap<Box<str>, Arc<[Candidate]>>>,
    pub(crate) selections: RwLock<HashMap<SelectionKey, Selection>>,
}

/// A memoised selection: the name string, okres and country of the stop.
pub(crate) type SelectionKey = (Box<str>, Option<Box<str>>, Option<Box<str>>);

impl Evidence {
    pub fn build(
        inputs: EvidenceInputs<'_>,
        policy: &Policy,
        diagnostics: &mut Diagnostics,
    ) -> Evidence {
        let file = &policy.file;
        let analyzer = Analyzer::new(
            policy.names.matching.clone(),
            inputs.synonyms.iter().map(|line| &line.alternatives),
        );
        for conflict in analyzer.synonym_conflicts() {
            let line = |index: usize| inputs.synonyms.get(index).map_or(0, |line| line.line);
            diagnostics.push(
                Diagnostic::new(
                    "jdf_names_synonym_conflict",
                    Severity::Info,
                    "stop_synonyms.txt",
                    format!(
                        "`{}` is named on line {} and again on line {}; the later line wins",
                        conflict.alternative,
                        line(conflict.first_line),
                        line(conflict.line)
                    ),
                )
                .rule(jr_geo::names::JDF_R215),
            );
        }

        // R015: one area per okres code, every polygon of it (B2 fixed).
        let okres_codes: Vec<Box<str>> = inputs.okresy.iter().map(|o| o.code.clone()).collect();
        let okres_polygons: Vec<Vec<Polygon>> = inputs
            .okresy
            .iter()
            .map(|okres| {
                okres
                    .polygons
                    .iter()
                    .map(|p| Polygon {
                        exterior: p.exterior.clone(),
                        holes: p.holes.clone(),
                    })
                    .collect()
            })
            .collect();
        let okres_areas = Areas::new(&okres_polygons);
        let mut adjacency = BTreeSet::new();
        for area in 0..okres_areas.len() {
            for other in okres_areas.within_m(area, file.matching.okres_adjacency_m) {
                adjacency.insert((area.min(other), area.max(other)));
            }
        }
        let okres_by_code = okres_codes
            .iter()
            .enumerate()
            .map(|(index, code)| (code.clone(), index))
            .collect();
        diagnostics.push(
            Diagnostic::new(
                "jdf_okres_areas",
                Severity::Info,
                "cz_regions",
                format!(
                    "{} okres codes, {} polygons, {} adjacent pairs",
                    okres_codes.len(),
                    okres_polygons.iter().map(Vec::len).sum::<usize>(),
                    adjacency.len()
                ),
            )
            .rule(R015),
        );

        // R016: municipalities, brought to WGS84 once.
        let mut municipalities = Vec::with_capacity(inputs.towns.len());
        let mut town_polygons = Vec::with_capacity(inputs.towns.len());
        for town in inputs.towns {
            municipalities.push(Municipality {
                name: town.name,
                okres: town.okres,
            });
            let ring = |ring: Vec<gazetteer::Laea>| -> Vec<Coord> {
                ring.into_iter()
                    .map(|p| {
                        laea::to_wgs84(Laea {
                            easting: p.easting,
                            northing: p.northing,
                        })
                    })
                    .collect()
            };
            town_polygons.push(
                town.polygons
                    .into_iter()
                    .map(|p| Polygon {
                        exterior: ring(p.exterior),
                        holes: p.holes.into_iter().map(ring).collect(),
                    })
                    .collect::<Vec<_>>(),
            );
        }
        let municipality_areas = Areas::new(&town_polygons);
        drop(town_polygons);
        let municipality_names: Vec<&str> = municipalities.iter().map(|m| &*m.name).collect();
        let municipality_index = StopIndex::new(analyzer.clone(), &municipality_names);
        let foreign_names: Vec<&str> = inputs.foreign_towns.iter().map(|t| &*t.name).collect();
        let foreign_index = StopIndex::new(analyzer.clone(), &foreign_names);
        let foreign_countries = inputs
            .foreign_towns
            .iter()
            .map(|t| t.country.to_uppercase().into_boxed_str())
            .collect();
        diagnostics.push(
            Diagnostic::new(
                "jdf_gazetteers",
                Severity::Info,
                "gazetteers",
                format!(
                    "{} municipalities, {} European town names",
                    municipalities.len(),
                    foreign_names.len()
                ),
            )
            .rule(R016),
        );

        let mut entries = catalogue_entries(inputs.catalogues, policy, diagnostics);
        let town_lookup = MunicipalityLookup {
            municipalities: &municipalities,
            areas: &municipality_areas,
            analyzer: &analyzer,
        };
        entries.extend(osm::entries(
            &inputs.osm_nodes,
            &town_lookup,
            policy,
            diagnostics,
        ));
        let names: Vec<&str> = entries.iter().map(|entry| &*entry.name).collect();
        let index = StopIndex::new(analyzer, &names);
        Evidence {
            entries,
            index,
            okres_by_code,
            okres_areas,
            adjacency,
            municipalities,
            municipality_areas,
            municipality_index,
            foreign_countries,
            foreign_index,
            town_synonyms: policy.names.town_synonyms.clone(),
            candidates: RwLock::new(HashMap::new()),
            selections: RwLock::new(HashMap::new()),
        }
    }

    pub(crate) fn analyzer(&self) -> &Analyzer {
        self.index.analyzer()
    }

    /// R041, R042: every candidate of a name string, memoised (pure in the name).
    pub(crate) fn candidates(&self, name: &str) -> Arc<[Candidate]> {
        if let Some(found) = self.candidates.read().expect("memo").get(name) {
            return found.clone();
        }
        let computed: Arc<[Candidate]> = self.index.candidates(name).into();
        self.candidates
            .write()
            .expect("memo")
            .entry(name.into())
            .or_insert(computed)
            .clone()
    }

    /// The municipalities whose name the town matches exactly, after the town table (R054).
    pub(crate) fn municipalities_named(&self, town: &str) -> Vec<u32> {
        self.municipality_index
            .exact(self.town_synonyms.resolve(town))
    }

    /// R051, R056, R058, R059: whether the town is exactly a municipality's name.
    pub(crate) fn is_municipality(&self, town: &str) -> bool {
        !self.municipalities_named(town).is_empty()
    }

    /// R050, R053: the upper-cased ISO country of the first European town (entry order) the
    /// name matches exactly.
    pub(crate) fn first_foreign_town(&self, town: &str) -> Option<&str> {
        self.foreign_index
            .exact(town)
            .first()
            .map(|&entry| &*self.foreign_countries[entry as usize])
    }

    /// The okres area of a code, after the alias of R045 (verdict Q9).
    pub(crate) fn okres_area(&self, code: &str, policy: &Policy) -> Option<usize> {
        self.okres_by_code
            .get(policy.file.okres_alias(code))
            .copied()
    }

    /// R046: whether two okres codes name adjacent okresy.
    pub(crate) fn okresy_adjacent(&self, left: &str, right: &str, policy: &Policy) -> bool {
        match (
            self.okres_area(left, policy),
            self.okres_area(right, policy),
        ) {
            (Some(a), Some(b)) => self.adjacency.contains(&(a.min(b), a.max(b))),
            _ => false,
        }
    }
}

/// R011, R012: the municipality containing a point.
pub(crate) struct MunicipalityLookup<'a> {
    pub municipalities: &'a [Municipality],
    pub areas: &'a Areas,
    pub analyzer: &'a Analyzer,
}

/// R008: a row's name, coordinate bits, okres and country.
type RowKey = (Box<str>, u64, u64, Option<Box<str>>, Option<Box<str>>);

/// R006–R008: catalogue rows with valid coordinates, exact duplicates removed, in path and row
/// order.
fn catalogue_entries(
    catalogues: Vec<(&str, Vec<catalogue::Row>)>,
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> Vec<Entry> {
    let file = &policy.file;
    let mut entries = Vec::new();
    let mut seen: BTreeSet<RowKey> = BTreeSet::new();
    let mut duplicates = 0usize;
    for (path, rows) in catalogues {
        let stem = path
            .rsplit('/')
            .next()
            .map(|name| name.rsplit_once('.').map_or(name, |(stem, _)| stem))
            .unwrap_or(path);
        let class = if file.catalogues.gapfill.iter().any(|g| &**g == path) {
            SourceClass::Gapfill
        } else {
            SourceClass::Catalogue
        };
        let source: Box<str> = format!("{}{stem}", file.catalogues.source_prefix).into();
        for row in rows {
            let coord = match (row.latitude, row.longitude) {
                (Some(lat), Some(lon)) => Coord::new(lon, lat),
                _ => Coord::new(f64::NAN, f64::NAN),
            };
            if !coord.is_valid() {
                diagnostics.push(
                    Diagnostic::new(
                        "jdf_catalogue_row_invalid",
                        Severity::Error,
                        format!("{path}:{}", row.line),
                        format!(
                            "`{}` has no valid latitude and longitude: dropped",
                            row.name
                        ),
                    )
                    .rule(R006),
                );
                continue;
            }
            let key = (
                row.name.clone(),
                coord.lat.to_bits(),
                coord.lon.to_bits(),
                row.okres.clone(),
                row.country.clone(),
            );
            if !seen.insert(key) {
                duplicates += 1;
                continue;
            }
            entries.push(Entry {
                name: row.name,
                okres: row.okres,
                country: row.country,
                coord,
                class,
                source: source.clone(),
                observation: catalogue_observation(
                    &file.catalogues.source_prefix,
                    path,
                    &row.columns,
                ),
                facts: vec![
                    ("source_kind".into(), "external".into()),
                    ("lifecycle".into(), file.osm.post_evidence.active.clone()),
                ],
            });
        }
    }
    if duplicates > 0 {
        diagnostics.push(
            Diagnostic::new(
                "jdf_catalogue_duplicates",
                Severity::Info,
                "catalogues",
                format!("{duplicates} exact duplicate rows dropped (the first is kept)"),
            )
            .rule(R008),
        );
    }
    diagnostics.push(
        Diagnostic::new(
            "jdf_catalogue_rows",
            Severity::Info,
            "catalogues",
            format!("{} catalogue rows in the matcher corpus", entries.len()),
        )
        .rule(R007),
    );
    entries
}

/// JDF-R007: F#'s identity of a catalogue row,
/// `external:{path}:sha256:{sha(path|the five columns joined |)}`.
fn catalogue_observation(prefix: &str, path: &str, columns: &[Box<str>; 5]) -> Box<str> {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(format!("{path}|{}", columns.join("|")).as_bytes());
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{prefix}{path}:sha256:{hex}").into()
}
