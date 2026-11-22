//! The road and tram graph (POST-R010–R023): small hand-built networks in, routes, snaps and
//! threads out.

use std::collections::BTreeSet;

use jr_geo::laea;
use jr_geo::routing::{
    self, GraphBuilder, OsmMember, OsmMemberType, OsmNode, OsmRelation, OsmWay, RouteFailure,
    RoutingGraph, RoutingMode, RoutingPolicy, Snap,
};
use jr_model::Coord;

const LEDGER: &str = include_str!("../../../docs/rules/posts.md");

type Tags = Vec<(&'static str, &'static str)>;
/// (type, ID, role).
type Members = Vec<(OsmMemberType, i64, &'static str)>;

/// A position `north` and `east` metres from 50°N 14°E.
fn at(north: f64, east: f64) -> Coord {
    Coord::new(14.0 + east / 71_700.0, 50.0 + north / 111_250.0)
}

/// A hand-built extract.
#[derive(Default)]
struct Osm {
    nodes: Vec<(i64, Coord, Tags)>,
    ways: Vec<(i64, Vec<i64>, Tags)>,
    relations: Vec<(i64, Tags, Members)>,
}

impl Osm {
    fn node(mut self, id: i64, north: f64, east: f64) -> Self {
        self.nodes.push((id, at(north, east), Vec::new()));
        self
    }

    fn tagged(mut self, id: i64, north: f64, east: f64, tags: Tags) -> Self {
        self.nodes.push((id, at(north, east), tags));
        self
    }

    fn way(mut self, id: i64, nodes: &[i64], tags: Tags) -> Self {
        self.ways.push((id, nodes.to_vec(), tags));
        self
    }

    fn restriction(mut self, id: i64, value: &'static str, from: i64, via: i64, to: i64) -> Self {
        self.relations.push((
            id,
            vec![("type", "restriction"), ("restriction", value)],
            vec![
                (OsmMemberType::Way, from, "from"),
                (OsmMemberType::Node, via, "via"),
                (OsmMemberType::Way, to, "to"),
            ],
        ));
        self
    }

    fn build(&self) -> RoutingGraph {
        let policy = RoutingPolicy::builtin();
        let mut builder = GraphBuilder::new(&policy.graph);
        for (id, coord, tags) in &self.nodes {
            builder
                .node(&OsmNode {
                    id: *id,
                    coord: *coord,
                    tags,
                })
                .unwrap();
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
        for (id, tags, members) in &self.relations {
            let members: Vec<OsmMember<'_>> = members
                .iter()
                .map(|&(kind, id, role)| OsmMember { kind, id, role })
                .collect();
            builder
                .relation(&OsmRelation {
                    id: *id,
                    tags,
                    members: &members,
                })
                .unwrap();
        }
        builder.finish().unwrap()
    }
}

/// Four nodes 100 m apart going north.
fn line() -> Osm {
    Osm::default()
        .node(1, 0.0, 0.0)
        .node(2, 100.0, 0.0)
        .node(3, 200.0, 0.0)
        .node(4, 300.0, 0.0)
}

fn snaps(graph: &RoutingGraph, mode: RoutingMode, north: f64, east: f64) -> Vec<Snap> {
    graph.snap(
        mode,
        laea::from_wgs84(at(north, east)),
        &RoutingPolicy::builtin().snap,
    )
}

/// The routed cost between two positions over their nearest snaps.
fn route(graph: &RoutingGraph, mode: RoutingMode, from: (f64, f64), to: (f64, f64)) -> Option<f64> {
    let policy = RoutingPolicy::builtin();
    let start = snaps(graph, mode, from.0, from.1);
    let finish = snaps(graph, mode, to.0, to.1);
    start
        .iter()
        .flat_map(|s| finish.iter().map(move |f| (s, f)))
        .filter_map(|(s, f)| graph.route(mode, s, f, &policy.search).ok())
        .map(|path| path.cost)
        .min_by(f64::total_cmp)
}

fn residential() -> Tags {
    vec![("highway", "residential")]
}

#[test]
fn every_rule_has_a_heading_in_the_ledger() {
    let headings: BTreeSet<&str> = LEDGER
        .lines()
        .filter_map(|line| line.strip_prefix("#### "))
        .filter_map(|heading| heading.split_whitespace().next())
        .collect();
    let mut seen = BTreeSet::new();
    for rule in routing::RULES {
        assert!(headings.contains(rule.id), "{} is not in posts.md", rule.id);
        assert!(seen.insert(rule.id), "{} is declared twice", rule.id);
    }
}

#[test]
fn post_r010_vertices_are_the_nodes_edges_touch_in_osm_order() {
    let graph = Osm::default()
        .node(30, 200.0, 0.0)
        .node(10, 0.0, 0.0)
        .node(20, 100.0, 0.0)
        .node(99, 500.0, 500.0)
        .way(1, &[10, 20, 30], residential())
        .build();
    assert_eq!(graph.node_count(), 3);
    let ids: Vec<i64> = (0..3).map(|n| graph.node_osm_id(n)).collect();
    assert_eq!(ids, [10, 20, 30]);
    let first = graph.outgoing(graph.node_of(10).unwrap()).start;
    let length = graph.edge_length(first);
    assert!((length - 100.0).abs() < 0.5, "{length}");
}

fn flags(tags: Tags) -> (bool, bool) {
    let graph = Osm::default()
        .node(1, 0.0, 0.0)
        .node(2, 100.0, 0.0)
        .way(1, &[1, 2], tags)
        .build();
    let counts = graph.counts();
    (counts.road_edges > 0, counts.tram_edges > 0)
}

#[test]
fn post_r011_road_way_eligibility_and_access() {
    let road = |tags: Tags| flags(tags).0;
    assert!(road(residential()));
    assert!(road(vec![("highway", " Busway ")]));
    assert!(!road(vec![("highway", "track")]));
    assert!(!road(vec![("highway", "motorway_link")]), "B5 kept");
    assert!(!road(vec![("highway", "construction")]));
    assert!(!road(vec![("highway", "residential"), ("disused", "no")]));
    assert!(!road(vec![
        ("highway", "residential"),
        ("access", "private")
    ]));
    assert!(!road(vec![
        ("highway", "residential"),
        ("motor_vehicle", "no")
    ]));
    assert!(road(vec![
        ("highway", "residential"),
        ("access", "no"),
        ("bus", "yes")
    ]));
    assert!(road(vec![
        ("highway", "residential"),
        ("vehicle", "no"),
        ("psv", "designated")
    ]));
    assert!(road(vec![
        ("highway", "residential"),
        ("access", "destination")
    ]));
    assert!(road(vec![
        ("highway", "residential"),
        ("access:conditional", "no @ (Mo-Fr)")
    ]));
}

#[test]
fn post_r012_a_road_closed_to_buses_costs_eight_times_its_length() {
    // A 200 m street closed to buses, and a 400 m detour around it.
    let graph = line()
        .node(5, 0.0, 100.0)
        .node(6, 200.0, 100.0)
        .way(
            1,
            &[1, 2, 3],
            vec![("highway", "residential"), ("bus", "no")],
        )
        .way(2, &[1, 5, 6, 3], residential())
        .build();
    assert_eq!(graph.counts().severe_edges, 4);
    let cost = route(&graph, RoutingMode::Bus, (0.0, 0.0), (200.0, 0.0)).unwrap();
    assert!((cost - 400.0).abs() < 2.0, "the detour: {cost}");
    // Alone, the closed street is still routable at the penalised cost.
    let alone = line()
        .way(
            1,
            &[1, 2, 3],
            vec![("highway", "residential"), ("bus", "no")],
        )
        .build();
    let cost = route(&alone, RoutingMode::Bus, (0.0, 0.0), (200.0, 0.0)).unwrap();
    assert!((cost - 1600.0).abs() < 8.0, "{cost}");
}

#[test]
fn post_r013_service_roads_are_flagged() {
    let graph = line()
        .way(1, &[1, 2], vec![("highway", "service")])
        .way(2, &[2, 3], residential())
        .build();
    let counts = graph.counts();
    assert_eq!((counts.service_edges, counts.edges), (2, 4));
}

#[test]
fn post_r014_tram_track_and_street_track() {
    assert_eq!(flags(vec![("railway", "tram")]), (false, true));
    assert_eq!(
        flags(vec![("railway", "tram"), ("disused", "yes")]),
        (false, false)
    );
    assert_eq!(
        flags(vec![("highway", "primary"), ("railway", "tram")]),
        (true, true)
    );
    // A tram-only edge does not route a bus (F# "respects direction mode and access").
    let graph = line()
        .way(1, &[1, 2, 3], residential())
        .way(2, &[3, 4], vec![("railway", "tram")])
        .build();
    assert!(route(&graph, RoutingMode::Tram, (200.0, 0.0), (300.0, 0.0)).is_some());
    assert!(route(&graph, RoutingMode::Bus, (200.0, 0.0), (300.0, 0.0)).is_none());
}

#[test]
fn post_r015_one_way_in_both_spellings_and_roundabouts() {
    let routes = |tags: Tags| {
        let graph = line().way(1, &[1, 2, 3], tags).build();
        (
            route(&graph, RoutingMode::Bus, (0.0, 0.0), (200.0, 0.0)).is_some(),
            route(&graph, RoutingMode::Bus, (200.0, 0.0), (0.0, 0.0)).is_some(),
        )
    };
    let with = |extra: (&'static str, &'static str)| vec![("highway", "residential"), extra];
    assert_eq!(routes(residential()), (true, true));
    for forward in ["yes", "true", "1", " YES "] {
        assert_eq!(
            routes(with(("oneway", forward))),
            (true, false),
            "{forward}"
        );
    }
    for backward in ["-1", "reverse"] {
        assert_eq!(
            routes(with(("oneway", backward))),
            (false, true),
            "{backward}"
        );
    }
    assert_eq!(routes(with(("junction", "roundabout"))), (true, false));
    assert_eq!(routes(with(("oneway", "no"))), (true, true));
    // Contraflow for buses is not read (B7 kept).
    let contraflow = vec![
        ("highway", "residential"),
        ("oneway", "yes"),
        ("oneway:bus", "no"),
    ];
    assert_eq!(routes(contraflow), (true, false));
}

#[test]
fn post_r016_barrier_nodes_with_and_without_a_bus_exemption() {
    let through = |tags: Tags| {
        let graph = Osm::default()
            .node(1, 0.0, 0.0)
            .tagged(2, 100.0, 0.0, tags)
            .node(3, 200.0, 0.0)
            .way(1, &[1, 2, 3], residential())
            .build();
        route(&graph, RoutingMode::Bus, (0.0, 0.0), (200.0, 0.0)).is_some()
    };
    assert!(through(Vec::new()));
    assert!(!through(vec![("barrier", "bollard")]));
    assert!(!through(vec![("barrier", "lift_gate")]), "B6 kept");
    assert!(through(vec![("barrier", "lift_gate"), ("bus", "yes")]));
    assert!(through(vec![
        ("barrier", "bus_trap"),
        ("psv", "designated")
    ]));
    assert!(!through(vec![("barrier", "gate"), ("bus", "no")]));
}

#[test]
fn post_r017_missing_and_repeated_nodes_break_the_chain() {
    // Node 9 is not in the extract; 2 → 2 repeats a node.
    let graph = line().way(1, &[1, 9, 2, 2, 3], residential()).build();
    assert_eq!(graph.counts().edges, 2, "only 2–3 in both directions");
    assert!(route(&graph, RoutingMode::Bus, (100.0, 0.0), (200.0, 0.0)).is_some());
    assert!(route(&graph, RoutingMode::Bus, (0.0, 0.0), (100.0, 0.0)).is_none());
}

/// A crossroads at node 5: way 1 from the south, way 2 to the north, way 3 to the east,
/// way 4 to the west.
fn crossroads() -> Osm {
    Osm::default()
        .node(1, -100.0, 0.0)
        .node(5, 0.0, 0.0)
        .node(2, 100.0, 0.0)
        .node(3, 0.0, 100.0)
        .node(4, 0.0, -100.0)
        .way(1, &[1, 5], residential())
        .way(2, &[5, 2], residential())
        .way(3, &[5, 3], residential())
        .way(4, &[5, 4], residential())
}

#[test]
fn post_r018_turn_restrictions_refuse_and_allow_turns() {
    let from_south = (-100.0, 0.0);
    let (north, east, west) = ((100.0, 0.0), (0.0, 100.0), (0.0, -100.0));
    // Every arm is a dead end, where a vehicle may turn: a refused turn costs a 200 m
    // detour to the end of another arm and back.
    let cost = |graph: &RoutingGraph, from, to| route(graph, RoutingMode::Bus, from, to).unwrap();
    let open = crossroads().build();
    assert!((cost(&open, from_south, east) - 200.0).abs() < 1.0);

    let no_right = crossroads()
        .restriction(10, "no_right_turn", 1, 5, 3)
        .build();
    assert!((cost(&no_right, from_south, east) - 400.0).abs() < 1.0);
    assert!((cost(&no_right, from_south, north) - 200.0).abs() < 1.0);
    assert!(
        (cost(&no_right, west, east) - 200.0).abs() < 1.0,
        "other approaches"
    );

    let only_straight = crossroads()
        .restriction(10, "Only_Straight_On", 1, 5, 2)
        .build();
    assert!((cost(&only_straight, from_south, north) - 200.0).abs() < 1.0);
    assert!((cost(&only_straight, from_south, east) - 400.0).abs() < 1.0);
    assert!((cost(&only_straight, from_south, west) - 400.0).abs() < 1.0);

    // Restrictions of other ways at the node, and a via way, change nothing.
    let unrelated = crossroads()
        .restriction(10, "no_right_turn", 77, 5, 3)
        .restriction(11, "no_u_turn", 2, 5, 2)
        .build();
    assert!((cost(&unrelated, from_south, east) - 200.0).abs() < 1.0);
    assert_eq!(unrelated.counts().restrictions, 2);
}

#[test]
fn post_r019_trolleybuses_use_roads_and_mode_codes_map() {
    let graph = line()
        .way(1, &[1, 2], residential())
        .way(2, &[2, 3], vec![("railway", "tram")])
        .build();
    assert!(route(&graph, RoutingMode::Trolleybus, (0.0, 0.0), (100.0, 0.0)).is_some());
    assert!(route(&graph, RoutingMode::Trolleybus, (100.0, 0.0), (200.0, 0.0)).is_none());
    assert_eq!(RoutingMode::for_context_mode("E"), RoutingMode::Tram);
    assert_eq!(RoutingMode::for_context_mode("T"), RoutingMode::Trolleybus);
    assert_eq!(RoutingMode::for_context_mode("A"), RoutingMode::Bus);
}

#[test]
fn post_r020_snaps_the_nearest_edges_of_the_mode_within_the_band() {
    // Northbound streets 10 m, 20 m and 40 m east of the point; tram track 5 m west.
    let mut osm = Osm::default();
    for (i, east) in [10.0, 20.0, 40.0, -5.0].into_iter().enumerate() {
        let i = i as i64;
        osm = osm
            .node(10 * i + 1, -100.0, east)
            .node(10 * i + 2, 100.0, east);
    }
    let graph = osm
        .way(
            1,
            &[1, 2],
            vec![("highway", "residential"), ("oneway", "yes")],
        )
        .way(
            2,
            &[11, 12],
            vec![("highway", "residential"), ("oneway", "yes")],
        )
        .way(
            3,
            &[21, 22],
            vec![("highway", "residential"), ("oneway", "yes")],
        )
        .way(4, &[31, 32], vec![("railway", "tram"), ("oneway", "yes")])
        .build();
    let bus = snaps(&graph, RoutingMode::Bus, 0.0, 0.0);
    let ways: Vec<i64> = bus.iter().map(|snap| graph.edge_way(snap.edge)).collect();
    assert_eq!(ways, [1, 2], "the 40 m street is outside the 15 m band");
    assert!(
        (bus[0].distance_m - 10.0).abs() < 0.5,
        "{}",
        bus[0].distance_m
    );
    assert!((bus[0].fraction - 0.5).abs() < 0.01);
    let tram = snaps(&graph, RoutingMode::Tram, 0.0, 0.0);
    assert_eq!(tram.len(), 1);
    assert_eq!(graph.edge_way(tram[0].edge), 4);
    // Nothing within 75 m.
    assert!(snaps(&graph, RoutingMode::Bus, 0.0, 200.0).is_empty());
    // Past the end of a segment the snap is its end node.
    let beyond = snaps(&graph, RoutingMode::Bus, 130.0, 10.0);
    assert_eq!(beyond[0].fraction, 1.0);
}

#[test]
fn post_r021_routes_between_fractional_positions() {
    // F# "uses fractional edge positions": 50.0005 to 50.0015 along a north street, ≈ 111 m.
    let graph = line().way(1, &[1, 2, 3, 4], residential()).build();
    let cost = route(&graph, RoutingMode::Bus, (55.6, 0.0), (166.9, 0.0)).unwrap();
    assert!((cost - 111.3).abs() < 1.0, "{cost}");
    // A target behind the start on a one-way edge is not reached.
    let one_way = line()
        .way(
            1,
            &[1, 2, 3, 4],
            vec![("highway", "residential"), ("oneway", "yes")],
        )
        .build();
    let policy = RoutingPolicy::builtin();
    let ahead = &snaps(&one_way, RoutingMode::Bus, 160.0, 0.0)[0];
    let behind = &snaps(&one_way, RoutingMode::Bus, 120.0, 0.0)[0];
    assert_eq!(
        one_way.route(RoutingMode::Bus, ahead, behind, &policy.search),
        Err(RouteFailure::Disconnected)
    );
    let path = one_way
        .route(RoutingMode::Bus, behind, ahead, &policy.search)
        .unwrap();
    assert!((path.cost - 40.0).abs() < 0.5);
    assert_eq!(path.edges.len(), 1);
}

#[test]
fn post_r021_the_expansion_ceiling_is_a_search_limit() {
    let graph = line()
        .way(1, &[1, 2, 3, 4], residential())
        .node(8, 0.0, 300.0)
        .node(9, 100.0, 300.0)
        .way(2, &[8, 9], residential())
        .build();
    let mut policy = RoutingPolicy::builtin();
    let start = &snaps(&graph, RoutingMode::Bus, 10.0, 0.0)[0];
    let island = &snaps(&graph, RoutingMode::Bus, 50.0, 300.0)[0];
    assert_eq!(
        graph.route(RoutingMode::Bus, start, island, &policy.search),
        Err(RouteFailure::Disconnected)
    );
    policy.search.max_expansions = 2;
    let far = &snaps(&graph, RoutingMode::Bus, 290.0, 0.0)[0];
    assert_eq!(
        graph.route(RoutingMode::Bus, start, far, &policy.search),
        Err(RouteFailure::SearchLimit)
    );
}

fn thread_of_way(graph: &RoutingGraph, way: i64) -> Vec<String> {
    (0..graph.edge_count() as u32)
        .filter(|&edge| graph.edge_way(edge) == way)
        .map(|edge| graph.directed_thread(edge).to_string())
        .collect()
}

#[test]
fn post_r022_a_t_junction_splits_the_bar_and_not_the_stem() {
    // The bar runs west–east through node 5; the stem runs south from 5 over two ways.
    let graph = Osm::default()
        .node(1, 0.0, -100.0)
        .node(5, 0.0, 0.0)
        .node(2, 0.0, 100.0)
        .node(3, -100.0, 0.0)
        .node(4, -200.0, 0.0)
        .way(1, &[1, 5, 2], residential())
        .way(2, &[5, 3], residential())
        .way(3, &[3, 4], residential())
        .build();
    let thread = |way| -> BTreeSet<String> {
        thread_of_way(&graph, way)
            .into_iter()
            .map(|t| t[..32].to_owned())
            .collect()
    };
    let bar = thread(1);
    assert_eq!(bar.len(), 2, "west and east of the junction");
    assert_eq!(
        thread(2),
        thread(3),
        "the stem is one thread across its way split"
    );
    assert!(bar.is_disjoint(&thread(2)));
    // Both directions of an edge: one thread, opposite signs.
    let stem = thread_of_way(&graph, 2);
    assert_eq!(stem.len(), 2);
    assert_ne!(stem[0], stem[1]);
    assert_eq!(stem[0][..32], stem[1][..32]);
}

#[test]
fn post_r022_a_dual_carriageway_shares_a_thread_in_opposite_directions() {
    // Junctions 1 and 2 have side roads; carriageway 10 runs north east of carriageway 11.
    let graph = Osm::default()
        .node(1, 0.0, 0.0)
        .node(2, 300.0, 0.0)
        .node(3, 150.0, 10.0)
        .node(4, 150.0, -10.0)
        .node(5, 0.0, -100.0)
        .node(6, 300.0, 100.0)
        .way(
            10,
            &[1, 3, 2],
            vec![("highway", "primary"), ("oneway", "yes")],
        )
        .way(
            11,
            &[2, 4, 1],
            vec![("highway", "primary"), ("oneway", "yes")],
        )
        .way(12, &[5, 1], residential())
        .way(13, &[2, 6], residential())
        .build();
    let north: BTreeSet<String> = thread_of_way(&graph, 10).into_iter().collect();
    let south: BTreeSet<String> = thread_of_way(&graph, 11).into_iter().collect();
    assert_eq!((north.len(), south.len()), (1, 1));
    let (north, south) = (north.first().unwrap(), south.first().unwrap());
    assert_eq!(north[..32], south[..32], "same ends, same thread");
    assert_ne!(north, south, "opposite directions along it");
}

#[test]
fn post_r022_a_service_bay_is_a_thread_of_its_own() {
    // F# "keeps a service bay distinct from its through road".
    let graph = Osm::default()
        .node(1, 0.0, 0.0)
        .node(2, 111.0, 0.0)
        .node(3, 222.0, 0.0)
        .node(4, 111.0, 21.0)
        .node(5, 166.0, 21.0)
        .way(10, &[1, 2, 3], vec![("highway", "secondary")])
        .way(11, &[2, 4, 5, 3], vec![("highway", "service")])
        .build();
    let road = &snaps(&graph, RoutingMode::Bus, 111.0, 0.0)[0];
    let bay = &snaps(&graph, RoutingMode::Bus, 139.0, 21.0)[0];
    assert_ne!(
        graph.directed_thread(road.edge).thread,
        graph.directed_thread(bay.edge).thread
    );
}

#[test]
fn post_r022_a_ring_is_a_looped_thread() {
    let graph = Osm::default()
        .node(1, 0.0, 0.0)
        .node(2, 100.0, 0.0)
        .node(3, 100.0, 100.0)
        .node(4, 0.0, 100.0)
        .way(
            1,
            &[1, 2, 3],
            vec![("highway", "service"), ("oneway", "yes")],
        )
        .way(
            2,
            &[3, 4, 1],
            vec![("highway", "service"), ("oneway", "yes")],
        )
        .build();
    let threads: BTreeSet<String> = (0..graph.edge_count() as u32)
        .map(|edge| graph.directed_thread(edge).thread.to_string())
        .collect();
    assert_eq!(threads.len(), 1, "one ring");
}

#[test]
fn post_r023_corridor_identity_does_not_depend_on_edge_numbering() {
    let policy = RoutingPolicy::builtin();
    let corridor = |osm: Osm| {
        let graph = osm.build();
        let start = &snaps(&graph, RoutingMode::Bus, -50.0, 0.0)[0];
        let finish = &snaps(&graph, RoutingMode::Bus, 50.0, 0.0)[0];
        let path = graph
            .route(RoutingMode::Bus, start, finish, &policy.search)
            .unwrap();
        graph.corridor_id(&path.edges).to_string()
    };
    let a = corridor(crossroads());
    let mut reordered = crossroads();
    reordered.ways.reverse();
    reordered.nodes.reverse();
    let b = corridor(reordered.node(100, 500.0, 500.0).node(0, -500.0, 500.0));
    assert_eq!(a, b);
    assert_eq!(a.len(), 16);
}
