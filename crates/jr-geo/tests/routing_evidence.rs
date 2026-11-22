//! Routed evidence of a context (POST-R050–R053): corridor variants and attachments on small
//! hand-built networks, and its independence of the thread count.

use std::collections::BTreeSet;

use jr_geo::routing::{
    AttachmentFailure, Availability, ContextGeometry, FaceSide, GraphBuilder, OsmNode, OsmWay,
    RoutedEvidence, RoutingGraph, RoutingMode, RoutingPolicy, VariantFailure,
};
use jr_model::Coord;
use rayon::prelude::*;

type Tags = Vec<(&'static str, &'static str)>;

/// A position `north` and `east` metres from 50°N 14°E.
fn at(north: f64, east: f64) -> Coord {
    Coord::new(14.0 + east / 71_700.0, 50.0 + north / 111_250.0)
}

/// Nodes (id, north, east) and ways (id, nodes, tags).
fn graph(nodes: &[(i64, f64, f64)], ways: &[(i64, Vec<i64>, Tags)]) -> RoutingGraph {
    let policy = RoutingPolicy::builtin();
    let mut builder = GraphBuilder::new(&policy.graph);
    let mut nodes = nodes.to_vec();
    nodes.sort_by_key(|node| node.0);
    for &(id, north, east) in &nodes {
        let node = OsmNode {
            id,
            coord: at(north, east),
            tags: &[],
        };
        builder.node(&node).unwrap();
    }
    for (id, nodes, tags) in ways {
        builder
            .way(&OsmWay {
                id: *id,
                nodes,
                tags,
            })
            .unwrap();
    }
    builder.finish().unwrap()
}

fn residential() -> Tags {
    vec![("highway", "residential")]
}

fn one_way() -> Tags {
    vec![("highway", "residential"), ("oneway", "yes")]
}

fn evidence(
    graph: &RoutingGraph,
    previous: &[Coord],
    points: &[Coord],
    next: &[Coord],
) -> RoutedEvidence {
    evidence_with(graph, previous, points, next, &RoutingPolicy::builtin())
}

fn evidence_with(
    graph: &RoutingGraph,
    previous: &[Coord],
    points: &[Coord],
    next: &[Coord],
    policy: &RoutingPolicy,
) -> RoutedEvidence {
    let context = ContextGeometry {
        mode: RoutingMode::Bus,
        previous,
        route_points: points,
        next,
    };
    graph.routed_evidence(&context, policy)
}

/// A two-way street north from node 1 (0 m) to node 4 (333 m).
fn street() -> RoutingGraph {
    graph(
        &[
            (1, 0.0, 0.0),
            (2, 111.0, 0.0),
            (3, 222.0, 0.0),
            (4, 333.0, 0.0),
        ],
        &[(1, vec![1, 2, 3, 4], residential())],
    )
}

#[test]
fn post_r050_one_anchor_snap_per_directed_thread_and_a_ceiling() {
    let policy = RoutingPolicy::builtin();
    let street = street();
    let along = [at(100.0, 2.0), at(111.0, 0.0), at(120.0, 1.0)];
    let snaps = street.anchor_snaps(RoutingMode::Bus, &along, &policy);
    assert_eq!(snaps.len(), 2, "both directions of one thread");
    assert!(
        snaps.iter().all(|snap| snap.distance_m < 0.01),
        "the nearest kept"
    );

    // Ten one-way streets a metre apart: ten threads, eight kept.
    let mut nodes = Vec::new();
    let mut ways = Vec::new();
    for i in 0..10 {
        nodes.push((10 * i + 1, 0.0, i as f64));
        nodes.push((10 * i + 2, 100.0, i as f64));
        ways.push((i, vec![10 * i + 1, 10 * i + 2], one_way()));
    }
    let parallel = graph(&nodes, &ways);
    let across: Vec<Coord> = (0..10).map(|i| at(50.0, i as f64)).collect();
    let snaps = parallel.anchor_snaps(RoutingMode::Bus, &across, &policy);
    assert_eq!(snaps.len(), 8);
    let ways: BTreeSet<i64> = snaps.iter().map(|s| parallel.edge_way(s.edge)).collect();
    assert_eq!(ways.len(), 8);
}

/// F# "Raw corridor capture is deterministic complete and truncated to three": four separate
/// streets 72 m apart, an anchor and a route point on each.
#[test]
fn post_r051_three_cheapest_distinct_corridors_whatever_the_input_order() {
    let mut nodes = Vec::new();
    let mut ways = Vec::new();
    for lane in 0..4 {
        let east = lane as f64 * 71.7;
        let first = lane * 3 + 1;
        nodes.extend([
            (first, 0.0, east),
            (first + 1, 111.0, east),
            (first + 2, 222.0, east),
        ]);
        ways.push((10 + lane, vec![first, first + 1, first + 2], residential()));
    }
    let lanes = graph(&nodes, &ways);
    let column = |north: f64, offset: f64| -> Vec<Coord> {
        (0..4)
            .map(|lane| at(north, lane as f64 * 71.7 + offset))
            .collect()
    };
    let (previous, points, next) = (column(0.0, 0.0), column(111.0, 3.6), column(222.0, 0.0));
    let first = evidence(&lanes, &previous, &points, &next);
    assert_eq!(first.variants.len(), 3);
    let ranks: Vec<u32> = first.variants.iter().map(|v| v.variant_rank).collect();
    assert_eq!(ranks, [0, 1, 2]);
    assert!(
        first
            .variants
            .iter()
            .all(|v| v.routing_availability == Availability::Available)
    );
    let corridors: BTreeSet<_> = first.variants.iter().map(|v| v.corridor_id).collect();
    assert_eq!(corridors.len(), 3, "distinct corridors");
    let costs: Vec<f64> = first
        .variants
        .iter()
        .map(|v| v.absolute_cost_metres.unwrap())
        .collect();
    assert!(costs.windows(2).all(|pair| pair[0] <= pair[1]), "{costs:?}");
    assert_eq!(first.variants[0].relative_cost_metres, Some(0.0));
    assert_eq!(first.attachments.len(), 3);
    for (rank, rows) in first.attachments.iter().enumerate() {
        assert_eq!(rows.len(), points.len());
        assert!(rows.iter().all(|row| row.variant_rank == rank as u32));
    }
    let reversed = |coords: &[Coord]| coords.iter().rev().copied().collect::<Vec<_>>();
    let second = evidence(
        &lanes,
        &reversed(&previous),
        &reversed(&points),
        &reversed(&next),
    );
    assert_eq!(first.variants, second.variants);
}

#[test]
fn post_r051_fewer_variants_when_fewer_corridors_exist() {
    let street = graph(
        &[(1, 0.0, 0.0), (2, 111.0, 0.0), (3, 222.0, 0.0)],
        &[(1, vec![1, 2, 3], one_way())],
    );
    let found = evidence(
        &street,
        &[at(0.0, 0.0)],
        &[at(111.0, 3.0)],
        &[at(222.0, 0.0)],
    );
    assert_eq!(found.variants.len(), 1);
    let variant = &found.variants[0];
    assert!((variant.absolute_cost_metres.unwrap() - 222.0).abs() < 1.0);
    assert_eq!(variant.directed_edge_count, 2);
    assert_eq!(variant.repeated_directed_edge_count, 0);
    assert_eq!(variant.access_penalty_metres, Some(0.0));
    assert_eq!(variant.ingress_thread_id, variant.egress_thread_id);
}

#[test]
fn post_r051_unavailable_variants_say_why() {
    let street = street();
    let points = [at(111.0, 3.0), at(222.0, 3.0)];
    let far = evidence(&street, &[at(0.0, 500.0)], &points, &[at(333.0, 0.0)]);
    assert_eq!(far.variants.len(), 1);
    assert_eq!(
        far.variants[0].routing_availability,
        Availability::Unavailable
    );
    assert_eq!(
        far.variants[0].invariant_failure_reason,
        Some(VariantFailure::SnapUnavailable)
    );
    assert_eq!(far.attachments[0].len(), 2);
    assert!(
        far.attachments[0]
            .iter()
            .all(|row| row.invariant_failure_reason
                == Some(AttachmentFailure::BaselineUnavailable)
                && row.snap_edge_id.is_none())
    );

    let islands = graph(
        &[
            (1, 0.0, 0.0),
            (2, 100.0, 0.0),
            (3, 0.0, 300.0),
            (4, 100.0, 300.0),
        ],
        &[
            (1, vec![1, 2], residential()),
            (2, vec![3, 4], residential()),
        ],
    );
    let apart = evidence(
        &islands,
        &[at(0.0, 0.0)],
        &[at(50.0, 3.0)],
        &[at(100.0, 300.0)],
    );
    assert_eq!(
        apart.variants[0].invariant_failure_reason,
        Some(VariantFailure::Disconnected)
    );
}

/// A straight road north and, between 100 m and 200 m, a one-way service bay 20 m east of it.
fn road_with_bay() -> RoutingGraph {
    graph(
        &[
            (1, 0.0, 0.0),
            (2, 100.0, 0.0),
            (3, 200.0, 0.0),
            (4, 300.0, 0.0),
            (5, 400.0, 0.0),
            (6, 130.0, 20.0),
            (7, 170.0, 20.0),
        ],
        &[
            (1, vec![1, 2, 3, 4, 5], vec![("highway", "secondary")]),
            (
                2,
                vec![2, 6, 7, 3],
                vec![("highway", "service"), ("oneway", "yes")],
            ),
        ],
    )
}

#[test]
fn post_r052_attachment_is_the_cheapest_detour_within_the_horizon() {
    let road = road_with_bay();
    let kerb = at(150.0, 4.0);
    let bay = at(150.0, 24.0);
    let nowhere = at(150.0, 300.0);
    let found = evidence(
        &road,
        &[at(0.0, 0.0)],
        &[kerb, bay, nowhere],
        &[at(400.0, 0.0)],
    );
    let base = &found.variants[0];
    assert!((base.absolute_cost_metres.unwrap() - 400.0).abs() < 1.0);
    let rows = &found.attachments[0];
    assert!(rows[0].routed_excess_metres.unwrap() < 1e-6);
    assert_eq!(rows[0].snap_way_id, Some(1));
    // The bay is 2 × 36.1 + 40 = 112.1 m against 100 m of road.
    assert_eq!(rows[1].snap_way_id, Some(2));
    let excess = rows[1].routed_excess_metres.unwrap();
    assert!((excess - 12.1).abs() < 0.5, "{excess}");
    assert!((rows[1].snap_distance_metres.unwrap() - 4.0).abs() < 0.3);
    assert_eq!(
        rows[1].corridor_distance_metres,
        rows[1].snap_distance_metres
    );
    assert_eq!(
        rows[1].proximity_distance_metres,
        rows[1].snap_distance_metres
    );
    assert_eq!(
        rows[2].invariant_failure_reason,
        Some(AttachmentFailure::RoutedExcessOrDisconnected)
    );

    let mut tight = RoutingPolicy::builtin();
    tight.evidence.horizon_m = 5.0;
    let found = evidence_with(
        &road,
        &[at(0.0, 0.0)],
        &[kerb, bay],
        &[at(400.0, 0.0)],
        &tight,
    );
    assert!(found.attachments[0][0].routed_excess_metres.unwrap() < 1e-6);
    assert_eq!(
        found.attachments[0][1].invariant_failure_reason,
        Some(AttachmentFailure::EvidenceHorizon)
    );
}

#[test]
fn post_r052_a_route_point_behind_the_origin_is_not_attached() {
    let street = graph(
        &[(1, 0.0, 0.0), (2, 200.0, 0.0), (3, 300.0, 0.0)],
        &[(1, vec![1, 2, 3], one_way())],
    );
    let found = evidence(
        &street,
        &[at(100.0, 0.0)],
        &[at(50.0, 3.0)],
        &[at(300.0, 0.0)],
    );
    assert_eq!(
        found.attachments[0][0].invariant_failure_reason,
        Some(AttachmentFailure::RoutedExcessOrDisconnected)
    );
}

/// F# "uses fractional edge positions and directed right side", re-expressed on the
/// production path: posts on both sides of a two-way street, travelled both ways.
#[test]
fn post_r053_side_of_the_road_in_both_directions() {
    let street = street();
    let east = at(166.8, 3.6);
    let west = at(166.8, -3.6);
    let (south_end, north_end) = (at(0.0, 0.0), at(333.0, 0.0));
    let north = evidence(&street, &[south_end], &[east, west], &[north_end]);
    let south = evidence(&street, &[north_end], &[east, west], &[south_end]);
    let (n, s) = (&north.attachments[0], &south.attachments[0]);
    assert!(
        n[0].signed_lateral_offset_metres.unwrap() < -3.0,
        "east is right going north"
    );
    assert!(n[1].signed_lateral_offset_metres.unwrap() > 3.0);
    assert!(
        s[0].signed_lateral_offset_metres.unwrap() > 3.0,
        "east is left going south"
    );
    assert!(s[1].signed_lateral_offset_metres.unwrap() < -3.0);
    for row in n.iter().chain(s) {
        assert!(row.routed_excess_metres.unwrap() < 1e-6);
        assert!((row.snap_distance_metres.unwrap() - 3.6).abs() < 0.2);
        assert!(row.heading_difference_degrees.unwrap() < 1e-9);
    }
    // Faces belong to the road, not to the direction of travel.
    let face = |row: &jr_geo::routing::Attachment| row.corridor_face_id.unwrap();
    assert_ne!(face(&n[0]), face(&n[1]));
    assert_eq!(face(&n[0]), face(&s[0]));
    assert_eq!(face(&n[1]), face(&s[1]));
    assert_ne!(face(&n[0]).side, FaceSide::Centre);
    // Grid north is about 3° off true north at 14°E in EPSG:3035.
    let heading = n[0].corridor_heading_degrees.unwrap();
    assert!(!(5.0..355.0).contains(&heading), "{heading}");
    let heading = s[0].corridor_heading_degrees.unwrap();
    assert!((heading - 180.0).abs() < 5.0, "{heading}");
    // A post on the centre line is the centre face.
    let centre = evidence(&street, &[south_end], &[at(166.8, 1.0)], &[north_end]);
    assert_eq!(face(&centre.attachments[0][0]).side, FaceSide::Centre);
}

#[test]
fn post_r053_east_west_street_and_a_bay_off_the_path() {
    let street = graph(
        &[
            (1, 0.0, 0.0),
            (2, 0.0, 100.0),
            (3, 0.0, 200.0),
            (4, 0.0, 300.0),
        ],
        &[(1, vec![1, 2, 3, 4], residential())],
    );
    let (south, north) = (at(-4.0, 150.0), at(4.0, 150.0));
    let east = evidence(&street, &[at(0.0, 0.0)], &[south, north], &[at(0.0, 300.0)]);
    assert!(east.attachments[0][0].signed_lateral_offset_metres.unwrap() < 0.0);
    assert!(east.attachments[0][1].signed_lateral_offset_metres.unwrap() > 0.0);
    let west = evidence(&street, &[at(0.0, 300.0)], &[south, north], &[at(0.0, 0.0)]);
    assert!(west.attachments[0][0].signed_lateral_offset_metres.unwrap() > 0.0);

    // In the bay the snap edge is not on the variant: the geometry follows the detour.
    let road = road_with_bay();
    let found = evidence(
        &road,
        &[at(0.0, 0.0)],
        &[at(150.0, 24.0)],
        &[at(400.0, 0.0)],
    );
    let row = &found.attachments[0][0];
    assert_eq!(row.routing_availability, Availability::Available);
    assert!(
        row.signed_lateral_offset_metres.unwrap() < -3.0,
        "right of the bay"
    );
    assert_eq!(row.corridor_face_id.unwrap().side, FaceSide::Right);
}

/// A grid of two-way streets 100 m apart with one-way and service segments.
fn grid() -> RoutingGraph {
    let id = |row: i64, column: i64| row * 10 + column + 1;
    let mut nodes = Vec::new();
    for row in 0..6 {
        for column in 0..6 {
            nodes.push((id(row, column), row as f64 * 100.0, column as f64 * 100.0));
        }
    }
    let mut ways = Vec::new();
    for row in 0..6 {
        let tags = if row % 2 == 1 {
            one_way()
        } else {
            residential()
        };
        ways.push((100 + row, (0..6).map(|c| id(row, c)).collect(), tags));
    }
    for column in 0..6 {
        let tags = if column == 3 {
            vec![("highway", "service")]
        } else {
            residential()
        };
        ways.push((200 + column, (0..6).map(|r| id(r, column)).collect(), tags));
    }
    graph(&nodes, &ways)
}

#[test]
fn post_r057_routed_evidence_is_the_same_on_one_and_four_threads() {
    let grid = grid();
    let policy = RoutingPolicy::builtin();
    let contexts: Vec<(Vec<Coord>, Vec<Coord>, Vec<Coord>)> = (0..40)
        .map(|i| {
            let (row, column) = ((i % 5) as f64, (i / 8) as f64);
            let stop = (row * 100.0 + 50.0, column * 100.0);
            (
                vec![
                    at(row * 100.0, column * 100.0 + 50.0),
                    at(row * 100.0 + 5.0, column * 100.0),
                ],
                vec![at(stop.0, stop.1 + 4.0), at(stop.0 + 3.0, stop.1 - 4.0)],
                vec![at(row * 100.0 + 100.0, column * 100.0 + 100.0)],
            )
        })
        .collect();
    let run = |threads: usize| -> Vec<RoutedEvidence> {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        pool.install(|| {
            contexts
                .par_iter()
                .map(|(previous, points, next)| {
                    let context = ContextGeometry {
                        mode: RoutingMode::Bus,
                        previous,
                        route_points: points,
                        next,
                    };
                    grid.routed_evidence(&context, &policy)
                })
                .collect()
        })
    };
    let one = run(1);
    assert!(one.iter().any(|e| e.variants.len() > 1));
    assert_eq!(one, run(4));
}
