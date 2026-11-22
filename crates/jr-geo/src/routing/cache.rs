//! Routed context evidence kept between builds: a build routes only the contexts whose
//! surroundings changed. Pure: the caller reads and writes the bytes and passes the day.
//!
//! An entry is keyed by the capture's routing key (which names the context's mode and exact
//! coordinates) and holds the tile range the computation read, that range's digest at the
//! time, the day it was last used and the encoded evidence. A lookup reuses the evidence when
//! the current graph's digest of the same range is equal (see [`super::Regions`] for why that
//! is exact) and every snap edge it names resolves; then the evidence is what routing would
//! return, and a build with the cache is byte-identical to one without. The cache file's name
//! carries [`ContextCache::identity`], so another router build or routing policy never sees it.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::evidence::RoutedEvidence;
use super::graph::RoutingGraph;
use super::policy::RoutingPolicy;
use super::reader::ReadBox;
use super::region::{RegionDigest, Regions, StableEdge, TileRange};

/// The cache's encoding version; part of the identity and of the file header.
pub const CACHE_FORMAT: u32 = 1;

/// The file's first bytes.
const MAGIC: &[u8; 8] = b"jrroute\0";

/// The router's sources, hashed by `build.rs`.
const SOURCE_HASH: &str = env!("JR_GEO_SOURCE_HASH");

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("not a routing cache file")]
    NotACache,
    #[error("routing cache format {0}, expected {CACHE_FORMAT}")]
    Format(u32),
    #[error("routing cache does not decode: {0}")]
    Decode(#[from] postcard::Error),
}

/// What a run did with the cache.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheCounts {
    /// Contexts whose stored evidence was reused.
    pub reused: usize,
    /// Contexts found but routed again: their region or a snap edge changed.
    pub invalidated: usize,
    /// Contexts routed (the invalidated included).
    pub routed: usize,
    /// Entries the cache would write: used in this run, or unused within the retention.
    pub kept: usize,
}

/// The outcome of a lookup.
#[derive(Debug)]
pub enum Lookup {
    /// The stored evidence, valid on the current graph.
    Hit(RoutedEvidence),
    /// Stored, but its region or a snap edge changed.
    Invalidated,
    Miss,
}

/// A freshly routed context, ready to be recorded.
#[derive(Debug)]
pub struct Prepared {
    tiles: TileRange,
    digest: RegionDigest,
    payload: Vec<u8>,
}

/// What happened to one routing key, recorded after the parallel routing.
#[derive(Debug)]
pub enum Outcome {
    Reused,
    Routed { invalidated: bool, entry: Prepared },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Entry {
    tiles: TileRange,
    digest: RegionDigest,
    last_used_day: i32,
    payload: Vec<u8>,
}

/// The evidence as stored: snap edges by stable name (`snap_edge_id` indexes `edges`).
#[derive(Serialize, Deserialize)]
struct Payload {
    evidence: RoutedEvidence,
    edges: Vec<StableEdge>,
}

/// The cache of one run.
#[derive(Clone, Debug)]
pub struct ContextCache {
    day: i32,
    entries: BTreeMap<String, Entry>,
    /// Keys used (reused or routed) in this run.
    used: BTreeSet<String>,
    counts: CacheCounts,
}

impl ContextCache {
    /// An empty cache for a run on `day` (a day number the caller chooses).
    pub fn new(day: i32) -> Self {
        ContextCache {
            day,
            entries: BTreeMap::new(),
            used: BTreeSet::new(),
            counts: CacheCounts::default(),
        }
    }

    /// The identity of the caches this router and policy can use: a hash of the cache format,
    /// the router's sources and the routing policy.
    pub fn identity(policy: &RoutingPolicy) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"routing-contexts|");
        hasher.update(CACHE_FORMAT.to_le_bytes());
        hasher.update(SOURCE_HASH.as_bytes());
        hasher.update(postcard::to_stdvec(policy).expect("the policy encodes"));
        hasher.finalize()[..16]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    pub fn from_bytes(bytes: &[u8], day: i32) -> Result<Self, CacheError> {
        let rest = bytes.strip_prefix(MAGIC).ok_or(CacheError::NotACache)?;
        let (version, rest) = rest.split_at_checked(4).ok_or(CacheError::NotACache)?;
        let version = u32::from_le_bytes(version.try_into().expect("four bytes"));
        if version != CACHE_FORMAT {
            return Err(CacheError::Format(version));
        }
        let stored: Vec<(String, Entry)> = postcard::from_bytes(rest)?;
        let mut cache = ContextCache::new(day);
        cache.entries = stored.into_iter().collect();
        Ok(cache)
    }

    /// The entries used in this run and the unused ones last used within `retention_days`,
    /// sorted by key: the same cache gives the same bytes.
    pub fn to_bytes(&self, retention_days: u32) -> Vec<u8> {
        let kept: Vec<(&String, &Entry)> = self
            .entries
            .iter()
            .filter(|(key, entry)| self.keeps(key, entry, retention_days))
            .collect();
        let mut bytes = MAGIC.to_vec();
        bytes.extend(CACHE_FORMAT.to_le_bytes());
        bytes.extend(postcard::to_stdvec(&kept).expect("the cache encodes"));
        bytes
    }

    fn keeps(&self, key: &str, entry: &Entry, retention_days: u32) -> bool {
        self.used.contains(key)
            || i64::from(self.day) - i64::from(entry.last_used_day) <= i64::from(retention_days)
    }

    /// The counts of this run, `kept` for the given retention.
    pub fn counts(&self, retention_days: u32) -> CacheCounts {
        CacheCounts {
            kept: self
                .entries
                .iter()
                .filter(|(key, entry)| self.keeps(key, entry, retention_days))
                .count(),
            ..self.counts
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The stored evidence of a routing key if it is valid on this graph. Read-only: call it
    /// from any number of threads.
    pub fn lookup(&self, key: &str, graph: &RoutingGraph, regions: &Regions) -> Lookup {
        let Some(entry) = self.entries.get(key) else {
            return Lookup::Miss;
        };
        if regions.digest(entry.tiles) != entry.digest {
            return Lookup::Invalidated;
        }
        let Ok(Payload {
            mut evidence,
            edges,
        }) = postcard::from_bytes::<Payload>(&entry.payload)
        else {
            return Lookup::Invalidated;
        };
        let Some(resolved) = edges
            .iter()
            .map(|stable| graph.resolve_edge(stable))
            .collect::<Option<Vec<u32>>>()
        else {
            return Lookup::Invalidated;
        };
        for attachment in evidence.attachments.iter_mut().flatten() {
            if let Some(index) = attachment.snap_edge_id {
                attachment.snap_edge_id = Some(resolved[index as usize]);
            }
        }
        Lookup::Hit(evidence)
    }

    /// Encodes freshly routed evidence with the region it read. Pure: call it from the
    /// routing workers.
    pub fn prepare(
        graph: &RoutingGraph,
        regions: &Regions,
        evidence: &RoutedEvidence,
        read: ReadBox,
    ) -> Prepared {
        let tiles = regions.tiles(read);
        let mut edges: Vec<StableEdge> = Vec::new();
        let mut stored = evidence.clone();
        for attachment in stored.attachments.iter_mut().flatten() {
            if let Some(edge) = attachment.snap_edge_id {
                let stable = graph.stable_edge(edge);
                let index = match edges.iter().position(|known| *known == stable) {
                    Some(index) => index,
                    None => {
                        edges.push(stable);
                        edges.len() - 1
                    }
                };
                attachment.snap_edge_id = Some(index as u32);
            }
        }
        let payload = postcard::to_stdvec(&Payload {
            evidence: stored,
            edges,
        })
        .expect("evidence encodes");
        Prepared {
            tiles,
            digest: regions.digest(tiles),
            payload,
        }
    }

    /// Records what happened to a routing key.
    pub fn record(&mut self, key: &str, outcome: Outcome) {
        match outcome {
            Outcome::Reused => {
                self.counts.reused += 1;
                if let Some(entry) = self.entries.get_mut(key) {
                    entry.last_used_day = self.day;
                }
            }
            Outcome::Routed { invalidated, entry } => {
                self.counts.routed += 1;
                if invalidated {
                    self.counts.invalidated += 1;
                }
                self.entries.insert(
                    key.to_owned(),
                    Entry {
                        tiles: entry.tiles,
                        digest: entry.digest,
                        last_used_day: self.day,
                        payload: entry.payload,
                    },
                );
            }
        }
        self.used.insert(key.to_owned());
    }
}
