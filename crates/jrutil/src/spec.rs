//! The build specification: which pinned inputs to compile into which packages.
//!
//! A TOML file. Paths are relative to the file's own directory. The compiler reads nothing the
//! specification does not name, and never the network.
//!
//! ```toml
//! [czptt]
//! snapshot = "sources/czptt"            # sources.json, annual/, changes/, kadr/, sr70/
//! osm = "osm/railway-locations.osm.pbf"
//! output = "out/czptt"
//!
//! [czptt.shapes]                        # optional: generate route shapes
//! companion = "bin/obehy-motis-shapes"
//! graph = "cache/osr-graph"
//! cache = "cache/route-shapes"          # optional
//!
//! [jdf]
//! batches = "sources/jdf/batches"       # one ZIP per batch
//! catalogues = "geodata/other"          # stop catalogues, every *.csv below it
//! gazetteer = "geodata/gazetteer"       # cz_regions.json.gz, cz_towns.json.gz, eur_towns_countries.csv
//! osm = "osm/jdf-post-candidates.osm.pbf"
//! registry = "geodata/registry"         # optional: stops.csv
//! route_rules = "geodata/routes"        # optional: transport-modes.csv, presentation.csv
//! timetable_year = 2026
//! reference_date = "2026-11-21"
//! output = "out/jdf"
//! registry_candidates = "out/stop-candidates.csv"   # optional
//!
//! [jdf.overlay]                         # optional: regional feeds overlaid on the road timetable
//! reports = "out/overlay-reports"       # optional: the review reports, one CSV each
//!
//! [[jdf.overlay.sources]]
//! profile = "pid"                       # a source profile shipped with the compiler
//! feed = "sources/pid/gtfs.zip"
//! overrides = "geodata/overlay/pid-stop-overrides.csv"   # optional
//!
//! [jdf.filtered]                        # optional: the road feed without other feeds' lines
//! rules = "geodata/filtered-jdf/rules-v1.json"
//! output = "out/jdf-filtered"           # gtfs.zip, line-snapshot.json, filter-report.json
//! ```

use std::path::{Path, PathBuf};

use jr_model::Date;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildSpec {
    /// The rail package.
    pub czptt: Option<CzpttSpec>,
    /// The road package.
    pub jdf: Option<JdfSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JdfSpec {
    /// A directory of JDF batches, one ZIP each. A batch is named after its file.
    pub batches: PathBuf,
    /// The stop catalogues: a CSV file, or a directory whose `*.csv` files are all read.
    pub catalogues: PathBuf,
    /// The directory of the okres, municipality and European town gazetteers.
    pub gazetteer: PathBuf,
    /// The OSM extract road stops are located from.
    pub osm: PathBuf,
    /// The reviewed stop registry directory; without one every stop number is provisional.
    pub registry: Option<PathBuf>,
    /// The reviewed route rules directory.
    pub route_rules: Option<PathBuf>,
    pub timetable_year: i16,
    /// The day the build is for: it decides between overlapping line versions.
    pub reference_date: Date,
    /// The package directory to write; it must not exist.
    pub output: PathBuf,
    /// Where to write what the stop registry is missing.
    pub registry_candidates: Option<PathBuf>,
    /// The filtered road feed; not built without it.
    pub filtered: Option<FilteredSpec>,
    /// Regional feeds overlaid on the road timetable before it is published.
    pub overlay: Option<OverlaySpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverlaySpec {
    pub sources: Vec<OverlaySourceSpec>,
    /// A directory for the review reports; it must not exist.
    pub reports: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverlaySourceSpec {
    /// The name of a source profile shipped with the compiler.
    pub profile: String,
    /// The source's GTFS feed, a ZIP.
    pub feed: PathBuf,
    /// The reviewed stop overrides for the source.
    pub overrides: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilteredSpec {
    /// The reviewed rules naming the operators, integrated systems and line families other
    /// feeds publish.
    pub rules: PathBuf,
    /// The directory to write; it must not exist.
    pub output: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CzpttSpec {
    /// The CZPTT source snapshot directory.
    pub snapshot: PathBuf,
    /// The OSM extract railway points are located from.
    pub osm: PathBuf,
    /// The package directory to write; it must not exist.
    pub output: PathBuf,
    /// The timetable year; the snapshot's own when absent.
    pub timetable_year: Option<i16>,
    /// Route shapes from the map-matching companion; none are generated without it.
    pub shapes: Option<ShapesSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShapesSpec {
    /// The `obehy-motis-shapes` program.
    pub companion: PathBuf,
    /// The osr graph it routes on.
    pub graph: PathBuf,
    /// Its cache directory.
    pub cache: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum SpecError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
}

impl BuildSpec {
    pub fn read(path: &Path) -> Result<Self, SpecError> {
        let text = std::fs::read_to_string(path).map_err(|source| SpecError::Read {
            path: path.to_owned(),
            source,
        })?;
        let mut spec: Self = toml::from_str(&text).map_err(|source| SpecError::Parse {
            path: path.to_owned(),
            source,
        })?;
        let base = path.parent().unwrap_or(Path::new("."));
        if let Some(czptt) = &mut spec.czptt {
            for target in [&mut czptt.snapshot, &mut czptt.osm, &mut czptt.output] {
                *target = base.join(&*target);
            }
            if let Some(shapes) = &mut czptt.shapes {
                for target in [
                    Some(&mut shapes.companion),
                    Some(&mut shapes.graph),
                    shapes.cache.as_mut(),
                ]
                .into_iter()
                .flatten()
                {
                    *target = base.join(&*target);
                }
            }
        }
        if let Some(jdf) = &mut spec.jdf {
            for target in [
                Some(&mut jdf.batches),
                Some(&mut jdf.catalogues),
                Some(&mut jdf.gazetteer),
                Some(&mut jdf.osm),
                Some(&mut jdf.output),
                jdf.registry.as_mut(),
                jdf.route_rules.as_mut(),
                jdf.registry_candidates.as_mut(),
            ]
            .into_iter()
            .flatten()
            {
                *target = base.join(&*target);
            }
            if let Some(overlay) = &mut jdf.overlay {
                if let Some(reports) = &mut overlay.reports {
                    *reports = base.join(&*reports);
                }
                for source in &mut overlay.sources {
                    source.feed = base.join(&source.feed);
                    if let Some(overrides) = &mut source.overrides {
                        *overrides = base.join(&*overrides);
                    }
                }
            }
            if let Some(filtered) = &mut jdf.filtered {
                filtered.rules = base.join(&filtered.rules);
                filtered.output = base.join(&filtered.output);
            }
        }
        Ok(spec)
    }
}
