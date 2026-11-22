//! The routing context cache: stored evidence is reused exactly when the region it read is
//! unchanged, and only then.

use jr_geo::routing::{
    ContextCache, ContextGeometry, GraphBuilder, Lookup, OsmMember, OsmMemberType, OsmNode,
    OsmRelation, OsmWay, Outcome, RoutedEvidence, RoutingGraph, RoutingMode, RoutingPolicy,
};
use jr_model::Coord;

type Tags = Vec<(&'static str, &'static str)>;

/// A position `north` and `east` metres from 50°N 14°E.
fn at(north: f64, east: f64) -> Coord {
    Coord::new(14.0 + east / 71_700.0, 50.0 + north / 111_250.0)
}

fn residential() -> Tags {
    vec![("highway", "residential")]
}

/// A hand-built extract: nodes (id, north, east), ways, restrictions (value, from, via, to).
#[derive(Clone, Default)]
struct Osm {
    nodes: Vec<(i64, f64, f64)>,
    ways: Vec<(i64, Vec<i64>, Tags)>,
    restrictions: Vec<(&'static str, i64, i64, i64)>,
}

impl Osm {
    fn build(&self) -> RoutingGraph {
        let policy = RoutingPolicy::builtin();
        let mut builder = GraphBuilder::new(&policy.graph);
        let mut nodes = self.nodes.clone();
        nodes.sort_by_key(|node| node.0);
        for &(id, north, east) in &nodes {
            let node = OsmNode {
                id,
                coord: at(north, east),
                tags: &[],
            };
            builder.node(&node).unwrap();
        }
        for (id, nodes, tags) in &self.ways {
            builder
                .way(&OsmWay {
                    id: *id,
                    nodes,
                    tags,
                })
                .unwrap();
        }
        for (i, &(value, from, via, to)) in self.restrictions.iter().enumerate() {
            let members = [
                OsmMember {
                    kind: OsmMemberType::Way,
                    id: from,
                    role: "from",
                },
                OsmMember {
                    kind: OsmMemberType::Node,
                    id: via,
                    role: "via",
                },
                OsmMember {
                    kind: OsmMemberType::Way,
                    id: to,
                    role: "to",
                },
            ];
            let tags = [("type", "restriction"), ("restriction", value)];
            builder
                .relation(&OsmRelation {
                    id: i as i64 + 1,
                    tags: &tags,
                    members: &members,
                })
                .unwrap();
        }
        builder.finish().unwrap()
    }
}

/// A grid of two-way streets 100 m apart: node `row * 10 + column + 1`, ways 100 + row
/// (west–east) and 200 + column (south–north).
fn grid() -> Osm {
    let id = |row: i64, column: i64| row * 10 + column + 1;
    let mut osm = Osm::default();
    for row in 0..5 {
        for column in 0..5 {
            osm.nodes
                .push((id(row, column), row as f64 * 100.0, column as f64 * 100.0));
        }
    }
    for row in 0..5 {
        osm.ways.push((
            100 + row,
            (0..5).map(|c| id(row, c)).collect(),
            residential(),
        ));
    }
    for column in 0..5 {
        osm.ways.push((
            200 + column,
            (0..5).map(|r| id(r, column)).collect(),
            residential(),
        ));
    }
    osm
}

/// Contexts along the grid's streets: a stop between two crossings with posts either side.
fn contexts() -> Vec<(Vec<Coord>, Vec<Coord>, Vec<Coord>)> {
    (0..4)
        .map(|i| {
            let east = i as f64 * 100.0;
            (
                vec![at(100.0, east)],
                vec![at(150.0, east + 4.0), at(150.0, east - 4.0)],
                vec![at(200.0, east)],
            )
        })
        .collect()
}

fn geometry<'a>(context: &'a (Vec<Coord>, Vec<Coord>, Vec<Coord>)) -> ContextGeometry<'a> {
    ContextGeometry {
        mode: RoutingMode::Bus,
        previous: &context.0,
        route_points: &context.1,
        next: &context.2,
    }
}

fn fresh(graph: &RoutingGraph) -> Vec<RoutedEvidence> {
    let policy = RoutingPolicy::builtin();
    contexts()
        .iter()
        .map(|context| graph.routed_evidence(&geometry(context), &policy))
        .collect()
}

/// What the capture does: look each context up, route the others, record the outcomes.
/// Returns the evidence and each lookup's kind.
fn run(cache: &mut ContextCache, graph: &RoutingGraph) -> (Vec<RoutedEvidence>, Vec<&'static str>) {
    let policy = RoutingPolicy::builtin();
    let regions = graph.regions();
    let mut evidence = Vec::new();
    let mut kinds = Vec::new();
    for (i, context) in contexts().iter().enumerate() {
        let key = format!("context-{i}");
        let (routed, outcome, kind) = match cache.lookup(&key, graph, &regions) {
            Lookup::Hit(hit) => (hit, Outcome::Reused, "hit"),
            other => {
                let invalidated = matches!(other, Lookup::Invalidated);
                let (routed, read) = graph.routed_evidence_read(&geometry(context), &policy);
                let entry = ContextCache::prepare(graph, &regions, &routed, read);
                let kind = if invalidated { "invalidated" } else { "miss" };
                (routed, Outcome::Routed { invalidated, entry }, kind)
            }
        };
        cache.record(&key, outcome);
        evidence.push(routed);
        kinds.push(kind);
    }
    (evidence, kinds)
}

#[test]
fn cache_bytes_round_trip_and_are_deterministic() {
    let graph = grid().build();
    let mut first = ContextCache::new(100);
    run(&mut first, &graph);
    let mut second = ContextCache::new(100);
    run(&mut second, &graph);
    let bytes = first.to_bytes(45);
    assert_eq!(bytes, second.to_bytes(45), "same inputs, same bytes");
    let decoded = ContextCache::from_bytes(&bytes, 100).expect("decodes");
    assert_eq!(decoded.len(), contexts().len());
    assert_eq!(
        decoded.to_bytes(45),
        bytes,
        "decode and encode give the same cache"
    );
    assert!(ContextCache::from_bytes(&bytes[..bytes.len() - 3], 100).is_err());
    assert!(ContextCache::from_bytes(b"something else", 100).is_err());
}

#[test]
fn cache_on_the_same_graph_reuses_every_context_exactly() {
    let graph = grid().build();
    let mut cache = ContextCache::new(100);
    let (cold, kinds) = run(&mut cache, &graph);
    assert!(kinds.iter().all(|&kind| kind == "miss"));
    assert_eq!(cold, fresh(&graph));
    let mut warm = ContextCache::from_bytes(&cache.to_bytes(45), 101).unwrap();
    let (reused, kinds) = run(&mut warm, &graph);
    assert!(kinds.iter().all(|&kind| kind == "hit"), "{kinds:?}");
    assert_eq!(reused, cold);
    let counts = warm.counts(45);
    assert_eq!((counts.reused, counts.routed, counts.kept), (4, 0, 4));
}

/// Routes on `before`, then looks up on `after`: every context must be invalidated and
/// re-routed to what a cold run on `after` gives.
fn assert_invalidated(before: &Osm, after: &Osm) {
    let mut cache = ContextCache::new(100);
    run(&mut cache, &before.build());
    let changed = after.build();
    let (evidence, kinds) = run(&mut cache, &changed);
    assert!(kinds.iter().all(|&kind| kind == "invalidated"), "{kinds:?}");
    assert_eq!(evidence, fresh(&changed));
}

#[test]
fn cache_is_invalidated_by_a_change_in_the_read_region() {
    let before = grid();

    // An edge added: a diagonal across the grid.
    let mut added = before.clone();
    added.ways.push((300, vec![1, 13, 25], residential()));
    assert_invalidated(&before, &added);

    // A way's flags changed: the streets the contexts run along become service roads.
    let mut flags = before.clone();
    for way in &mut flags.ways {
        if way.0 >= 200 {
            way.2 = vec![("highway", "service")];
        }
    }
    assert_invalidated(&before, &flags);

    // A restriction added at a crossing every route passes.
    let mut restricted = before.clone();
    for column in 0..5 {
        restricted
            .restrictions
            .push(("no_left_turn", 200 + column, 10 + column + 1, 101));
    }
    assert_invalidated(&before, &restricted);
}

#[test]
fn cache_survives_a_change_far_away_and_renumbered_edges() {
    let near = grid();
    let mut far = near.clone();
    // 30 km away: a street on nodes with smaller OSM IDs, which shifts every edge number.
    far.nodes.push((-2, 30_000.0, 30_000.0));
    far.nodes.push((-1, 30_100.0, 30_000.0));
    far.ways.push((1, vec![-2, -1], residential()));
    let mut cache = ContextCache::new(100);
    let (cold, _) = run(&mut cache, &near.build());
    let far_graph = far.build();
    let (evidence, kinds) = run(&mut cache, &far_graph);
    assert!(kinds.iter().all(|&kind| kind == "hit"), "{kinds:?}");
    let expected = fresh(&far_graph);
    assert_eq!(
        evidence, expected,
        "snap edge numbers of the new graph included"
    );
    let edges = |all: &[RoutedEvidence]| -> Vec<Option<u32>> {
        all.iter()
            .flat_map(|e| e.attachments.iter().flatten().map(|a| a.snap_edge_id))
            .collect()
    };
    assert_ne!(edges(&cold), edges(&expected), "the numbering did change");
}

#[test]
fn cache_drops_entries_unused_past_the_retention() {
    let graph = grid().build();
    let mut cache = ContextCache::new(100);
    run(&mut cache, &graph);
    let bytes = cache.to_bytes(45);
    let within = ContextCache::from_bytes(&bytes, 145).unwrap();
    assert_eq!(within.counts(45).kept, 4);
    let past = ContextCache::from_bytes(&bytes, 146).unwrap();
    assert_eq!(past.counts(45).kept, 0);
    let written = ContextCache::from_bytes(&past.to_bytes(45), 146).unwrap();
    assert!(written.is_empty());
}
