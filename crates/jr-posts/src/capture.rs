//! Capture (POST-R030–R057): from the road timetable and the post candidates to the evidence
//! pack, in memory (verdict Q8).
//!
//! Per stop: admitted observations, route points and the anchor (`observations`). Per trip
//! pattern: the contexts of its calls (`contexts`), once per distinct pattern (R041). Per
//! context at a stop with two or more route points: the anchors either side (R045), the work
//! key and the routing key (R046). Per distinct routing key: one call of the router, in
//! parallel, shared by every context with that geometry (R055, R057). The pack's rows come out
//! in exactly the order `post_evidence::check` enforces.
//!
//! With a routing cache ([`jr_geo::routing::ContextCache`], R056 replaced), a routing key whose
//! stored evidence is still valid on this graph is not routed again; the pack is the same
//! either way.
//!
//! Not ported: the capture region and source exclusions of training captures (R005, R047; a
//! capture with them is never publication input).

use std::collections::{HashMap, HashSet};

use jr_formats::post_evidence::ids::{movement_family_id, stable_id};
use jr_formats::post_evidence::{
    self, Availability, BlockRole, Contexts, CorridorVariants, Pack, PackIdentity,
    RoutePointEvidence,
};
use jr_geo::routing::{
    self, ContextCache, ContextGeometry, Lookup, Outcome, RoutedEvidence, RoutingGraph,
    RoutingMode, RoutingPolicy,
};
use jr_jdf::assemble::PostFacts;
use jr_model::{Coord, Rule, Timetable};
use rayon::prelude::*;
use serde::Serialize;

use crate::evaluate::Constants;

pub(crate) mod contexts;
mod observations;

pub(crate) use contexts::{FactIndex, trip_pattern};

/// An observation enters the pack when its stop has a precise position and it lies within
/// 300 m of it (equirectangular metric).
///
/// Candidates far from the stop belong to another stop of the same name or are mismatches.
pub const POST_R030: Rule = Rule {
    id: "POST-R030",
    summary: "observations within 300 m of the stop's precise position are admitted",
};

/// Admitted observations at one coordinate of a stop form a route point,
/// `route-point:sha(stop|lat|lon)` of the coordinate's shortest round-trip text; a stop's
/// route points are ordered by that ID. F# hashed the text of its own EPSG:3035 round trip of
/// the coordinate, so the IDs differ from F#'s (verdict Q1).
///
/// One physical post is often observed twice at exactly one coordinate.
pub const POST_R031: Rule = Rule {
    id: "POST-R031",
    summary: "route points: observations of a stop grouped by exact coordinate",
};

/// A route point's representative, members, lifecycle flags (B8 kept), support and upper-cased
/// modes come from its members.
///
/// The evaluator reads these facts per point, not per observation.
pub const POST_R032: Rule = Rule {
    id: "POST-R032",
    summary: "route point facts from its member observations",
};

/// Only stops with two or more route points get contexts (verdict Q7).
///
/// A single candidate cannot be chosen against anything.
pub const POST_R033: Rule = Rule {
    id: "POST-R033",
    summary: "stops with fewer than two route points are not estimated",
};

/// Contexts come from the published trips (verdict Q10) of bus, trolleybus and tram lines;
/// a trip's calls are its timetable calls in running order; direction is 1 for an even trip
/// number.
///
/// JDF trip-number parity gives direction (JDF-R440).
pub const POST_R040: Rule = Rule {
    id: "POST-R040",
    summary: "contexts from published bus, trolleybus and tram trips",
};

/// The pattern hash is `sha` of the calls' stop numbers joined `,`; trips with the same line,
/// distinction, direction, pattern and authored keys share their contexts.
///
/// Timetable call count is not evidence cardinality.
pub const POST_R041: Rule = Rule {
    id: "POST-R041",
    summary: "identical trip patterns share one set of contexts",
};

/// Consecutive calls at one stop form a block with roles through, incoming, outgoing,
/// interior and a block ID for blocks of two or more.
///
/// A vehicle waiting at a terminus may arrive and leave at different posts.
pub const POST_R042: Rule = Rule {
    id: "POST-R042",
    summary: "same-stop blocks and their roles",
};

/// A call's authored key is `id:<post>` for an authored post, else `num:<text>` for a textual
/// post, else none; authored contexts never receive an estimated post.
///
/// An authored post is the carrier's statement.
pub const POST_R043: Rule = Rule {
    id: "POST-R043",
    summary: "authored keys id:<post> / num:<text>",
};

/// `context_id = stableId("context:", [stop, mode, line, distinction, direction, pattern,
/// position, role, authored key or ""])`.
///
/// Training labels join on it.
pub const POST_R044: Rule = Rule {
    id: "POST-R044",
    summary: "context identity",
};

/// The previous (next) anchor is the first stop before (after) the block, skipping calls at
/// the stop itself and stops without an anchor; an anchor is the medoid of a stop's route
/// points, else its precise position. Outgoing and interior calls route from the stop's own
/// points, incoming and interior ones to them; a missing anchor falls back to them too.
///
/// The neighbours' posts approximate where the vehicle comes from and goes to.
pub const POST_R045: Rule = Rule {
    id: "POST-R045",
    summary: "anchors: nearest neighbour stops with an anchor, per role",
};

/// Contexts are ordered by the work key and equal keys collapse; routing keys hash the mode and
/// the exact coordinates routed.
///
/// One routed computation per distinct geometry.
pub const POST_R046: Rule = Rule {
    id: "POST-R046",
    summary: "canonical work order and routing keys",
};

/// A context's movement family is `stableId("movement-family:", [stop, mode, rank-0 ingress
/// thread, rank-0 egress thread, role])`.
///
/// The heuristic decides one post per family (R085).
pub const POST_R054: Rule = Rule {
    id: "POST-R054",
    summary: "movement family from the rank-0 corridor's threads",
};

/// Every context takes the routed evidence of its routing key; variants and attachments are
/// written per context.
///
/// Contexts with one geometry share one routed result.
pub const POST_R055: Rule = Rule {
    id: "POST-R055",
    summary: "routed evidence joined to contexts by routing key",
};

/// Distinct routing keys are routed in parallel; the pack is assembled in the canonical order,
/// so it does not depend on the thread count.
///
/// AGENTS.md rule 7.
pub const POST_R057: Rule = Rule {
    id: "POST-R057",
    summary: "parallel routing, deterministic pack",
};

/// A context's `route_distinction` is its line version's ordinal among the surviving merged
/// versions of the licence, from 1, in merge order (`TripFacts::distinction`). F#'s merged JDF
/// numbered every input version of a licence in batch order, dropped ones included, which the
/// Rust merge does not keep; context IDs then differ in that component only, and decisions do
/// not depend on it (the value only tells versions apart).
///
/// The distinction keeps two versions of one line apart; any stable numbering does that.
pub const POST_R190: Rule = Rule {
    id: "POST-R190",
    summary: "route distinction = ordinal among the licence's surviving versions",
};

/// When two or more route points tie as the medoid of a stop's anchor (R045; with two points
/// they always do), the anchor is the one nearest the stop's precise position, then the one
/// with the smaller latitude, then the smaller longitude. F# broke the tie by a hash of the
/// coordinate's text, an accident of identity; the choice differs from F#'s at about half of
/// the two-point stops.
///
/// The position stop matching chose is the best single statement of where the stop is; the
/// candidate nearest it is the best guess of where vehicles stand.
pub const POST_R191: Rule = Rule {
    id: "POST-R191",
    summary: "an anchor tie goes to the point nearest the stop, then south, then west",
};

/// The capture's rules.
pub const RULES: &[Rule] = &[
    POST_R030, POST_R031, POST_R032, POST_R033, POST_R040, POST_R041, POST_R042, POST_R043,
    POST_R044, POST_R045, POST_R046, POST_R054, POST_R055, POST_R057, POST_R190, POST_R191,
];

/// What the capture reads.
#[derive(Debug)]
pub struct CaptureInput<'a> {
    pub timetable: &'a Timetable,
    pub facts: &'a PostFacts,
    /// Freed when the contexts are routed, before the pack is put together.
    pub graph: RoutingGraph,
    pub routing: &'a RoutingPolicy,
    pub constants: &'a Constants,
    pub identity: PackIdentity,
    /// Routed evidence kept between builds; looked up per routing key and updated with what
    /// was routed. Its counts stay with it: they never enter the pack or the summary.
    pub cache: Option<&'a mut ContextCache>,
}

/// Counts of the capture.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct CaptureCounts {
    pub observations: usize,
    pub route_points: usize,
    /// Stops with route points (one-point stops included).
    pub candidate_stops: usize,
    /// Stops with a precise position whose candidates all lie beyond the radius (R030).
    pub stops_beyond_radius: usize,
    /// Trips of bus, trolleybus and tram lines, and distinct patterns among them (R041).
    pub trips: usize,
    pub patterns: usize,
    /// Trips with a call at a place that is not a JDF stop (none expected).
    pub trips_skipped: usize,
    /// Work rows before (R041) and contexts after the collapse of equal keys (R046).
    pub source_contexts: usize,
    pub contexts: usize,
    pub routing_keys: usize,
}

/// The pack and the counts.
#[derive(Debug)]
pub struct Capture {
    pub pack: Pack,
    pub counts: CaptureCounts,
}

/// One context before routing.
#[derive(Clone, Debug)]
struct Work {
    stop: u64,
    mode: Box<str>,
    line: Box<str>,
    distinction: i32,
    direction: i32,
    pattern: String,
    position: i32,
    role: BlockRole,
    authored: Option<Box<str>>,
    context_id: String,
    block_id: Option<String>,
    previous_stop: Option<u64>,
    next_stop: Option<u64>,
    previous: Vec<Coord>,
    next: Vec<Coord>,
}

impl Work {
    #[allow(clippy::type_complexity)]
    fn key(&self) -> (u64, &str, &str, i32, i32, &str, i32, &str, Option<&str>) {
        (
            self.stop,
            &self.mode,
            &self.line,
            self.distinction,
            self.direction,
            &self.pattern,
            self.position,
            self.role.as_str(),
            self.authored.as_deref(),
        )
    }
}

fn coordinate_key(coords: &[Coord]) -> String {
    let parts: Vec<String> = coords
        .iter()
        .map(|c| format!("{:016x}{:016x}", c.lon.to_bits(), c.lat.to_bits()))
        .collect();
    parts.join(",")
}

/// R030–R057: the evidence pack of the timetable.
pub fn capture(input: CaptureInput<'_>) -> Capture {
    let constants = input.constants;
    let admitted = observations::admit(input.facts, constants);
    let index = FactIndex::new(input.facts);
    let mut counts = CaptureCounts {
        observations: admitted.observations.len(),
        route_points: admitted.route_points.len(),
        candidate_stops: admitted.points.len(),
        stops_beyond_radius: admitted.stops_without_points,
        ..CaptureCounts::default()
    };
    let points = &admitted.route_points;
    let stop_coords = |stop: u64| -> Option<Vec<Coord>> {
        let range = admitted.points.get(&stop)?;
        (range.len() >= constants.capture.minimum_route_points).then(|| {
            range
                .clone()
                .map(|row| Coord::new(points.longitude[row], points.latitude[row]))
                .collect()
        })
    };

    // R040–R045: work rows per distinct pattern.
    let mut seen = HashSet::new();
    let mut work: Vec<Work> = Vec::new();
    for trip in &input.facts.trips {
        if !constants.capture.modes.contains(&trip.mode) {
            continue;
        }
        counts.trips += 1;
        let Some(pattern) = trip_pattern(input.timetable, &index, trip, &constants.capture.modes)
        else {
            counts.trips_skipped += 1;
            continue;
        };
        if !seen.insert(pattern.identity()) {
            continue;
        }
        counts.patterns += 1;
        let stops = &pattern.stops;
        for call in &pattern.calls {
            let Some(own) = stop_coords(call.stop) else {
                continue;
            };
            let anchor = |indexes: Vec<usize>| {
                indexes.into_iter().find_map(|i| {
                    (stops[i] != call.stop)
                        .then(|| admitted.anchors.get(&stops[i]).map(|&c| (stops[i], c)))
                        .flatten()
                })
            };
            let previous = anchor((0..call.start).rev().collect());
            let next = anchor((call.finish..stops.len()).collect());
            let (previous_stop, previous) = match call.role {
                BlockRole::Outgoing | BlockRole::Interior => (None, own.clone()),
                _ => previous.map_or((None, own.clone()), |(stop, c)| (Some(stop), vec![c])),
            };
            let (next_stop, next) = match call.role {
                BlockRole::Incoming | BlockRole::Interior => (None, own.clone()),
                _ => next.map_or((None, own.clone()), |(stop, c)| (Some(stop), vec![c])),
            };
            work.push(Work {
                stop: call.stop,
                mode: trip.mode.clone(),
                line: trip.line.clone(),
                distinction: trip.distinction,
                direction: pattern.direction,
                pattern: pattern.pattern_hash.clone(),
                position: call.position,
                role: call.role,
                authored: call.authored.clone(),
                context_id: call.context_id.clone(),
                block_id: call.block_id.clone(),
                previous_stop,
                next_stop,
                previous,
                next,
            });
        }
    }
    counts.source_contexts = work.len();
    // R046: the canonical order; equal keys are one context (their routing inputs are equal,
    // being functions of the key's pattern).
    work.sort_by(|a, b| a.key().cmp(&b.key()));
    work.dedup_by(|b, a| a.key() == b.key());
    counts.contexts = work.len();

    // R046, R055, R057: one routed computation per distinct routing key.
    let mut keys: HashMap<String, usize> = HashMap::new();
    let mut unique: Vec<(usize, String)> = Vec::new();
    let context_keys: Vec<usize> = work
        .iter()
        .enumerate()
        .map(|(row, w)| {
            let own = coordinate_key(&stop_coords(w.stop).unwrap_or_default());
            let key = stable_id(
                "routing-evidence:",
                &[
                    &w.stop.to_string(),
                    &w.mode,
                    &coordinate_key(&w.previous),
                    &own,
                    &coordinate_key(&w.next),
                ],
            );
            *keys.entry(key.clone()).or_insert_with(|| {
                unique.push((row, key));
                unique.len() - 1
            })
        })
        .collect();
    counts.routing_keys = unique.len();
    let cache = input.cache;
    let regions = cache.as_ref().map(|_| input.graph.regions());
    let shared = cache.as_deref();
    let graph = &input.graph;
    let routed: Vec<(RoutedEvidence, Option<Outcome>)> = unique
        .par_iter()
        .map(|(row, key)| {
            let w = &work[*row];
            let own = stop_coords(w.stop).unwrap_or_default();
            let context = ContextGeometry {
                mode: RoutingMode::for_context_mode(&w.mode),
                previous: &w.previous,
                route_points: &own,
                next: &w.next,
            };
            let (Some(cache), Some(regions)) = (shared, regions.as_ref()) else {
                return (graph.routed_evidence(&context, input.routing), None);
            };
            let invalidated = match cache.lookup(key, graph, regions) {
                Lookup::Hit(evidence) => return (evidence, Some(Outcome::Reused)),
                Lookup::Invalidated => true,
                Lookup::Miss => false,
            };
            let (evidence, read) = graph.routed_evidence_read(&context, input.routing);
            let entry = ContextCache::prepare(graph, regions, &evidence, read);
            (evidence, Some(Outcome::Routed { invalidated, entry }))
        })
        .collect();
    let routed: Vec<RoutedEvidence> = match cache {
        Some(cache) => routed
            .into_iter()
            .zip(&unique)
            .map(|((evidence, outcome), (_, key))| {
                if let Some(outcome) = outcome {
                    cache.record(key, outcome);
                }
                evidence
            })
            .collect(),
        None => routed.into_iter().map(|(evidence, _)| evidence).collect(),
    };
    drop(input.graph);

    let mut contexts = Contexts::default();
    let mut variants = CorridorVariants::default();
    let mut attachments = RoutePointEvidence::default();
    for (row, w) in work.iter().enumerate() {
        let evidence = &routed[context_keys[row]];
        let first_point = admitted.points.get(&w.stop).map_or(0, |range| range.start);
        let baseline = evidence.variants.first();
        let thread =
            |select: fn(&routing::CorridorVariant) -> Option<String>| baseline.and_then(select);
        let ingress = thread(|v| v.ingress_thread_id.map(|t| t.to_string()));
        let egress = thread(|v| v.egress_thread_id.map(|t| t.to_string()));
        contexts.context_id.push(w.context_id.as_str().into());
        contexts.stop.push(w.stop);
        contexts.mode.push(w.mode.clone());
        contexts.line_id.push(w.line.clone());
        contexts.route_distinction.push(w.distinction);
        contexts.direction.push(w.direction);
        contexts.pattern_hash.push(w.pattern.as_str().into());
        contexts.pattern_position.push(w.position);
        contexts.role.push(w.role);
        contexts.movement_family_id.push(
            movement_family_id(
                w.stop,
                &w.mode,
                ingress.as_deref(),
                egress.as_deref(),
                w.role.as_str(),
            )
            .into(),
        );
        contexts.previous_stop.push(w.previous_stop);
        contexts.next_stop.push(w.next_stop);
        contexts.authored_post_key.push(w.authored.clone());
        contexts
            .same_stop_block_id
            .push(w.block_id.as_deref().map(Into::into));
        push_evidence(
            &mut variants,
            &mut attachments,
            row as u32,
            first_point,
            evidence,
        );
    }
    let pack = Pack {
        identity: input.identity,
        observations: admitted.observations,
        route_points: admitted.route_points,
        contexts,
        corridor_variants: variants,
        route_point_evidence: attachments,
    };
    Capture { pack, counts }
}

fn availability(value: routing::Availability) -> Availability {
    match value {
        routing::Availability::Available => Availability::Available,
        routing::Availability::Unavailable => Availability::Unavailable,
    }
}

fn int(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

/// The router's variants and attachments of one context as pack rows.
fn push_evidence(
    variants: &mut CorridorVariants,
    attachments: &mut RoutePointEvidence,
    context: u32,
    first_point: usize,
    evidence: &RoutedEvidence,
) {
    for (rank, variant) in evidence.variants.iter().enumerate() {
        let row = variants.len() as u32;
        variants.context.push(context);
        variants.variant_rank.push(int(variant.variant_rank));
        variants
            .corridor_id
            .push(variant.corridor_id.map(|id| id.to_string().into()));
        variants
            .absolute_cost_metres
            .push(variant.absolute_cost_metres);
        variants
            .relative_cost_metres
            .push(variant.relative_cost_metres);
        variants
            .relative_cost_fraction
            .push(variant.relative_cost_fraction);
        variants.path_length_metres.push(variant.path_length_metres);
        variants
            .directed_edge_count
            .push(int(variant.directed_edge_count));
        variants
            .repeated_directed_edge_count
            .push(int(variant.repeated_directed_edge_count));
        variants
            .service_edge_count
            .push(int(variant.service_edge_count));
        variants
            .restricted_access_edge_count
            .push(int(variant.restricted_access_edge_count));
        variants
            .access_penalty_metres
            .push(variant.access_penalty_metres);
        variants
            .ingress_thread_id
            .push(variant.ingress_thread_id.map(|t| t.to_string().into()));
        variants
            .egress_thread_id
            .push(variant.egress_thread_id.map(|t| t.to_string().into()));
        variants
            .routing_availability
            .push(availability(variant.routing_availability));
        variants
            .invariant_failure_reason
            .push(variant.invariant_failure_reason.map(|r| r.as_str().into()));
        for (point, attachment) in evidence
            .attachments
            .get(rank)
            .into_iter()
            .flatten()
            .enumerate()
        {
            attachments.variant.push(row);
            attachments.route_point.push((first_point + point) as u32);
            attachments
                .routed_excess_metres
                .push(attachment.routed_excess_metres);
            attachments
                .snap_edge_id
                .push(attachment.snap_edge_id.and_then(|e| i32::try_from(e).ok()));
            attachments.snap_way_id.push(attachment.snap_way_id);
            attachments.snap_fraction.push(attachment.snap_fraction);
            attachments
                .snap_projected_longitude
                .push(attachment.snap_projected_longitude);
            attachments
                .snap_projected_latitude
                .push(attachment.snap_projected_latitude);
            attachments
                .snap_distance_metres
                .push(attachment.snap_distance_metres);
            attachments
                .corridor_face_id
                .push(attachment.corridor_face_id.map(|f| f.to_string().into()));
            attachments
                .corridor_distance_metres
                .push(attachment.corridor_distance_metres);
            attachments
                .signed_lateral_offset_metres
                .push(attachment.signed_lateral_offset_metres);
            attachments
                .corridor_heading_degrees
                .push(attachment.corridor_heading_degrees);
            attachments
                .attachment_heading_degrees
                .push(attachment.attachment_heading_degrees);
            attachments
                .heading_difference_degrees
                .push(attachment.heading_difference_degrees);
            attachments
                .proximity_distance_metres
                .push(attachment.proximity_distance_metres);
            attachments
                .routing_availability
                .push(availability(attachment.routing_availability));
            attachments.invariant_failure_reason.push(
                attachment
                    .invariant_failure_reason
                    .map(|r| r.as_str().into()),
            );
        }
    }
}

/// Whether a pack built in memory satisfies the reader's invariants.
pub fn check(pack: &Pack) -> Result<(), post_evidence::PackError> {
    post_evidence::check(pack)
}
