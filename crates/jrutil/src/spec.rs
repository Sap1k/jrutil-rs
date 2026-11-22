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
//! ```

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildSpec {
    /// The rail package.
    pub czptt: Option<CzpttSpec>,
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
        Ok(spec)
    }
}
