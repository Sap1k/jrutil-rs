//! Reading an OSM extract.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use jr_formats::osm::{self, Node};

use super::LoadError;

/// Every tagged node of a PBF file, in file order.
pub fn tagged_nodes(path: &Path) -> Result<Vec<Node>, LoadError> {
    let file = File::open(path).map_err(|source| LoadError::Read {
        path: path.to_owned(),
        source,
    })?;
    let mut nodes = Vec::new();
    osm::tagged_nodes(BufReader::new(file), |node| nodes.push(node)).map_err(|error| {
        LoadError::Invalid {
            path: path.to_owned(),
            problem: error.to_string(),
        }
    })?;
    Ok(nodes)
}
