//! Reading what the post estimator needs: the routing extract as a graph, the policy document
//! and the registry's pinned posts.

use std::fs;
use std::io::Cursor;
use std::path::Path;

use jr_formats::geodata::registry::{self, Post};
use jr_formats::osm::{self, Elements};
use jr_formats::post_policy::{self, PolicyDocument};
use jr_geo::routing::{
    GraphBuilder, OsmMember, OsmMemberType, OsmNode, OsmRelation, OsmWay, RoutingGraph,
    RoutingPolicy,
};
use sha2::{Digest, Sha256};

use super::LoadError;

const POSTS: &str = "posts.csv";

fn read(path: &Path) -> Result<Vec<u8>, LoadError> {
    fs::read(path).map_err(|source| LoadError::Read {
        path: path.to_owned(),
        source,
    })
}

fn invalid(path: &Path, problem: impl ToString) -> LoadError {
    LoadError::Invalid {
        path: path.to_owned(),
        problem: problem.to_string(),
    }
}

/// The routing graph of an OSM extract and the extract's SHA-256.
pub fn graph(path: &Path, policy: &RoutingPolicy) -> Result<(RoutingGraph, String), LoadError> {
    let bytes = read(path)?;
    let digest: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let mut builder = GraphBuilder::new(&policy.graph);
    let mut failure = None;
    osm::read_blocks(Cursor::new(&bytes), Elements::ALL, |block| {
        if failure.is_some() {
            return;
        }
        let mut feed = || -> Result<(), jr_geo::routing::GraphError> {
            for node in block.nodes() {
                let node_tags: Vec<(&str, &str)> = node.tags.iter().collect();
                builder.node(&OsmNode {
                    id: node.id,
                    coord: node.coord,
                    tags: &node_tags,
                })?;
            }
            for way in block.ways() {
                let way_tags: Vec<(&str, &str)> = way.tags.iter().collect();
                builder.way(&OsmWay {
                    id: way.id,
                    nodes: way.refs,
                    tags: &way_tags,
                })?;
            }
            for relation in block.relations() {
                let relation_tags: Vec<(&str, &str)> = relation.tags.iter().collect();
                let members: Vec<OsmMember<'_>> = relation
                    .members
                    .iter()
                    .map(|member| OsmMember {
                        kind: match member.kind {
                            osm::ElementKind::Node => OsmMemberType::Node,
                            osm::ElementKind::Way => OsmMemberType::Way,
                            osm::ElementKind::Relation => OsmMemberType::Relation,
                        },
                        id: member.id,
                        role: member.role,
                    })
                    .collect();
                builder.relation(&OsmRelation {
                    id: relation.id,
                    tags: &relation_tags,
                    members: &members,
                })?;
            }
            Ok(())
        };
        if let Err(error) = feed() {
            failure = Some(error);
        }
    })
    .map_err(|error| invalid(path, error))?;
    if let Some(error) = failure {
        return Err(invalid(path, error));
    }
    drop(bytes);
    let graph = builder.finish().map_err(|error| invalid(path, error))?;
    Ok((graph, digest))
}

pub fn policy(path: &Path) -> Result<PolicyDocument, LoadError> {
    post_policy::parse(&read(path)?).map_err(|error| invalid(path, error))
}

/// The registry's pinned posts; none when the registry has no such file.
pub fn registry_posts(registry: &Path) -> Result<Vec<Post>, LoadError> {
    let path = registry.join(POSTS);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    registry::parse_posts(&read(&path)?).map_err(|error| invalid(&path, error))
}
