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
//! [[czptt.train_keys]]                  # optional: a regional feed's train trip IDs as keys
//! profile = "ids-jmk"                   # a source profile shipped with the compiler
//! feed = "sources/ids-jmk/gtfs.zip"
//!
//! [jdf]
//! batches = "sources/jdf/batches"       # one ZIP per batch; or the national exports below
//! catalogues = "geodata/other"          # stop catalogues, every *.csv below it
//! gazetteer = "geodata/gazetteer"       # cz_regions.json.gz, eur_countries.json.gz, cz_towns.json.gz,
//!                                       # eur_towns_countries.csv
//! osm = "osm/jdf-post-candidates.osm.pbf"
//! registry = "geodata/registry"         # optional: stops.csv
//! route_rules = "geodata/routes"        # optional: transport-modes.csv, presentation.csv
//! timetable_year = 2026
//! reference_date = "2026-11-21"
//! output = "out/jdf"
//! registry_candidates = "out/stop-candidates.csv"   # optional
//!
//! [[jdf.exports]]                       # a national export: a ZIP of batch ZIPs
//! name = "vld"                          # its batches are named vld-<member>
//! archive = "sources/JDF_VLD.zip"
//!
//! [jdf.shapes]                          # optional: generate route shapes, as for rail
//! companion = "bin/obehy-motis-shapes"
//! graph = "cache/osr-graph"
//!
//! [jdf.posts]                           # optional: estimate boarding posts
//! routing = "osm/jdf-transit-routing-demand.osm.pbf"
//! policy = "post-inference/learned-v1.json"
//! evidence = "out/posts-evidence"       # optional: write the evidence pack
//! reports = "out/posts-reports"         # optional: the review reports, one CSV each
//! cache = "cache/routing"               # optional: routed contexts kept between builds
//! cache_retention_days = 45             # optional: unused cache entries kept this long
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
    pub batches: Option<PathBuf>,
    /// National exports, each a ZIP of batch ZIPs.
    #[serde(default)]
    pub exports: Vec<ExportSpec>,
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
    /// Boarding-post estimation; none without it.
    pub posts: Option<PostsSpec>,
    /// Route shapes from the map-matching companion; none are generated without it.
    pub shapes: Option<ShapesSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportSpec {
    /// The prefix of the export's batch names.
    pub name: String,
    pub archive: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostsSpec {
    /// The road and tram OSM extract the estimator routes on.
    pub routing: PathBuf,
    /// The policy document with the scorer.
    pub policy: PathBuf,
    /// A directory for the evidence pack; it must not exist.
    pub evidence: Option<PathBuf>,
    /// A directory for the review reports.
    pub reports: Option<PathBuf>,
    /// A directory for the routing cache: routed contexts kept between builds, so a build
    /// routes only those whose surroundings changed. Output is the same with or without it.
    pub cache: Option<PathBuf>,
    /// How many days an unused cache entry is kept (counted from `reference_date`).
    #[serde(default = "default_cache_retention_days")]
    pub cache_retention_days: u32,
    /// Also report the scorer's training rows and each context's calls in `reports`
    /// (POST-R141).
    #[serde(default)]
    pub training: bool,
}

fn default_cache_retention_days() -> u32 {
    45
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
    /// Regional feeds whose train trip IDs key the national trains (OVL-R132).
    #[serde(default)]
    pub train_keys: Vec<TrainKeysSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrainKeysSpec {
    /// The name of a source profile shipped with the compiler; it gives the key namespace and
    /// the crosswalk.
    pub profile: String,
    /// The source's GTFS feed, a ZIP.
    pub feed: PathBuf,
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

impl ShapesSpec {
    fn resolve(&mut self, base: &Path) {
        for target in [
            Some(&mut self.companion),
            Some(&mut self.graph),
            self.cache.as_mut(),
        ]
        .into_iter()
        .flatten()
        {
            *target = base.join(&*target);
        }
    }
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
                shapes.resolve(base);
            }
            for source in &mut czptt.train_keys {
                source.feed = base.join(&source.feed);
            }
        }
        if let Some(jdf) = &mut spec.jdf {
            for target in [
                jdf.batches.as_mut(),
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
            for export in &mut jdf.exports {
                export.archive = base.join(&export.archive);
            }
            if let Some(shapes) = &mut jdf.shapes {
                shapes.resolve(base);
            }
            if let Some(posts) = &mut jdf.posts {
                posts.routing = base.join(&posts.routing);
                posts.policy = base.join(&posts.policy);
                for target in [
                    posts.evidence.as_mut(),
                    posts.reports.as_mut(),
                    posts.cache.as_mut(),
                ]
                .into_iter()
                .flatten()
                {
                    *target = base.join(&*target);
                }
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
