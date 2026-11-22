//! The evaluator's rules, one behaviour each, on small hand-built packs.

mod common;

use std::collections::BTreeSet;

use common::*;
use jr_formats::post_evidence::{self, BlockRole, Pack};
use jr_posts::evaluate::{self, DecisionKind, Decisions, EvaluationPolicy};

const STOP: u64 = 10;

fn run(pack: &Pack, policy: &EvaluationPolicy) -> Decisions {
    evaluate::evaluate(pack, policy).expect("evaluates")
}

/// An attachment 3 m left of travel (rejected).
fn left(face: &str, distance: f64) -> Att {
    Att {
        offset: 3.0,
        ..good(face, distance)
    }
}

fn observations(decisions: &Decisions, context: usize) -> Option<Vec<String>> {
    decisions
        .chosen(&decisions.contexts[context])
        .map(|hypothesis| {
            hypothesis
                .observation_ids
                .iter()
                .map(|id| id.to_string())
                .collect()
        })
}

fn hypothesis_sets(decisions: &Decisions) -> BTreeSet<Vec<String>> {
    decisions.stops[0]
        .hypotheses
        .iter()
        .map(|h| h.observation_ids.iter().map(|id| id.to_string()).collect())
        .collect()
}

/// Two points 30 m apart: `a` with the given attachment, `b` on the left (rejected).
fn two_points(a: Att) -> Pack {
    let points = vec![
        point("a", 0.0, 0.0, vec![osm(1)]),
        point("b", 0.0, 30.0, vec![osm(2)]),
    ];
    pack(
        &[(STOP, points)],
        vec![context(
            STOP,
            "L1",
            vec![variant("in", vec![a, left("f:L", 3.0)])],
        )],
    )
}

#[test]
fn post_r062_an_unavailable_baseline_corridor_decides_nothing() {
    let points = vec![
        point("a", 0.0, 0.0, vec![osm(1)]),
        point("b", 0.0, 30.0, vec![osm(2)]),
    ];
    let mut corridor = variant("in", vec![unreachable(), unreachable()]);
    corridor.unavailable = Some("disconnected".into());
    let pack = pack(&[(STOP, points)], vec![context(STOP, "L1", vec![corridor])]);
    let decisions = run(&pack, &heuristic());
    assert_eq!(decisions.contexts[0].kind, DecisionKind::Centroid);
}

#[test]
fn post_r070_close_points_merge_unless_lifecycle_or_side_tells_them_apart() {
    let build = |second: Point, second_att: Att| {
        let points = vec![point("a", 0.0, 0.0, vec![osm(1)]), second];
        pack(
            &[(STOP, points)],
            vec![context(
                STOP,
                "L1",
                vec![variant("in", vec![good("f:R", 3.0), second_att])],
            )],
        )
    };
    let merged = run(
        &build(point("b", 0.0, 1.0, vec![osm(2)]), good("f:R", 3.0)),
        &heuristic(),
    );
    assert_eq!(merged.stops[0].hypotheses.len(), 1);
    let far = run(
        &build(point("b", 0.0, 3.0, vec![osm(2)]), good("f:R", 3.0)),
        &heuristic(),
    );
    assert_eq!(far.stops[0].hypotheses.len(), 2);
    let mut obsolete = point("b", 0.0, 1.0, vec![osm(2)]);
    obsolete.obsolete = true;
    let lifecycle = run(&build(obsolete, good("f:R", 3.0)), &heuristic());
    assert_eq!(lifecycle.stops[0].hypotheses.len(), 2);
    let sides = run(
        &build(point("b", 0.0, 1.0, vec![osm(2)]), left("f:R", 3.0)),
        &heuristic(),
    );
    assert_eq!(sides.stops[0].hypotheses.len(), 2);
}

#[test]
fn post_r071_the_closest_compatible_pair_merges_first() {
    let points = vec![
        point("a", 0.0, 0.0, vec![osm(1)]),
        point("b", 0.0, 1.5, vec![osm(2)]),
        point("c", 0.0, 2.6, vec![osm(3)]),
    ];
    let att = || good("f:R", 3.0);
    let pack = pack(
        &[(STOP, points)],
        vec![context(
            STOP,
            "L1",
            vec![variant("in", vec![att(), att(), att()])],
        )],
    );
    let decisions = run(&pack, &heuristic());
    let expected: BTreeSet<Vec<String>> = [
        vec!["osm:node:1".to_owned()],
        vec!["osm:node:2".to_owned(), "osm:node:3".to_owned()],
    ]
    .into();
    assert_eq!(hypothesis_sets(&decisions), expected);
    assert_eq!(decisions.counts.merges, 1);
}

#[test]
fn post_r072_a_hypothesis_is_named_by_its_stop_and_observations_and_sits_at_its_medoid() {
    let points = vec![
        point("a", 0.0, 0.0, vec![osm(1)]),
        point("b", 0.0, 0.6, vec![osm(2)]),
        point("c", 0.0, 1.0, vec![osm(3)]),
    ];
    let att = || good("f:R", 3.0);
    let pack = pack(
        &[(STOP, points)],
        vec![context(
            STOP,
            "L1",
            vec![variant("in", vec![att(), att(), att()])],
        )],
    );
    let decisions = run(&pack, &heuristic());
    let hypothesis = &decisions.stops[0].hypotheses[0];
    assert_eq!(
        hypothesis.id,
        post_evidence::ids::sha256_hex(b"10|osm:node:1|osm:node:2|osm:node:3")
    );
    assert_eq!(
        hypothesis.representative, 1,
        "the middle point is the medoid"
    );
}

#[test]
fn post_r073_merged_points_with_different_headings_lose_alignment() {
    let build = |heading: f64| {
        let points = vec![
            point("a", 0.0, 0.0, vec![osm(1)]),
            point("b", 0.0, 1.0, vec![osm(2)]),
            point("c", 0.0, 30.0, vec![osm(3)]),
        ];
        let turned = Att {
            corridor_heading: heading,
            attachment_heading: heading,
            ..good("f:R", 3.0)
        };
        pack(
            &[(STOP, points)],
            vec![context(
                STOP,
                "L1",
                vec![variant(
                    "in",
                    vec![good("f:R", 3.0), turned, left("f:L", 3.0)],
                )],
            )],
        )
    };
    let agreeing = run(&build(90.0), &heuristic());
    assert_eq!(agreeing.contexts[0].kind, DecisionKind::Physical);
    let disagreeing = run(&build(100.0), &heuristic());
    assert_eq!(disagreeing.stops[0].hypotheses.len(), 2, "still merged");
    assert_eq!(disagreeing.contexts[0].kind, DecisionKind::Centroid);
}

#[test]
fn post_r080_near_tied_corridors_must_agree_on_the_face() {
    let build = |face: &str| {
        let points = vec![
            point("a", 0.0, 0.0, vec![osm(1)]),
            point("b", 0.0, 30.0, vec![osm(2)]),
        ];
        let mut tied = variant("in", vec![good(face, 3.0), left("f:L", 3.0)]);
        tied.relative_cost = 1.0;
        tied.relative_fraction = 0.01;
        pack(
            &[(STOP, points)],
            vec![context(
                STOP,
                "L1",
                vec![
                    variant("in", vec![good("f:R", 3.0), left("f:L", 3.0)]),
                    tied,
                ],
            )],
        )
    };
    assert_eq!(
        run(&build("f:R"), &heuristic()).contexts[0].kind,
        DecisionKind::Physical
    );
    assert_eq!(
        run(&build("g:R"), &heuristic()).contexts[0].kind,
        DecisionKind::Centroid
    );
}

#[test]
fn post_r081_tram_posts_and_points_beside_them_are_not_bus_posts() {
    let build = |modeless_east: f64| {
        let mut tram = point("a", 0.0, 0.0, vec![osm(1)]);
        tram.explicit_modes = "TRAM".into();
        let points = vec![tram, point("b", 0.0, modeless_east, vec![osm(2)])];
        pack(
            &[(STOP, points)],
            vec![context(
                STOP,
                "L1",
                vec![variant("in", vec![good("f:R", 1.0), good("g:R", 3.0)])],
            )],
        )
    };
    let beside = run(&build(4.0), &heuristic());
    assert_eq!(beside.contexts[0].kind, DecisionKind::Centroid);
    let apart = run(&build(20.0), &heuristic());
    assert_eq!(observations(&apart, 0), Some(vec!["osm:node:2".to_owned()]));
}

#[test]
fn post_r082_a_post_right_of_travel_is_chosen_one_on_the_left_is_not() {
    assert_eq!(
        run(&two_points(good("f:R", 3.0)), &heuristic()).contexts[0].kind,
        DecisionKind::Physical
    );
    assert_eq!(
        run(&two_points(left("f:R", 3.0)), &heuristic()).contexts[0].kind,
        DecisionKind::Centroid
    );
}

#[test]
fn post_r083_a_denied_mode_lowers_the_score_below_physical() {
    let mut denied = point("a", 0.0, 0.0, vec![osm(1)]);
    denied.denied_modes = "BUS".into();
    let points = vec![denied, point("b", 0.0, 30.0, vec![osm(2)])];
    let pack = pack(
        &[(STOP, points)],
        vec![context(
            STOP,
            "L1",
            vec![variant("in", vec![good("f:R", 3.0), left("f:L", 3.0)])],
        )],
    );
    assert_eq!(
        run(&pack, &heuristic()).contexts[0].kind,
        DecisionKind::Centroid
    );
}

#[test]
fn post_r084_r086_a_confident_dissenting_context_vetoes_its_family() {
    let points = || {
        vec![
            point("a", 0.0, 0.0, vec![osm(1)]),
            point("b", 0.0, 30.0, vec![osm(2)]),
        ]
    };
    let first = context(
        STOP,
        "L1",
        vec![variant("in", vec![good("f:R", 1.0), good("g:R", 10.0)])],
    );
    let second = context(
        STOP,
        "L2",
        vec![variant("in", vec![good("f:R", 10.0), good("g:R", 1.0)])],
    );
    let alone = run(
        &pack(&[(STOP, points())], vec![first.clone()]),
        &heuristic(),
    );
    assert_eq!(alone.contexts[0].kind, DecisionKind::Physical);
    let together = run(
        &pack(&[(STOP, points())], vec![first, second]),
        &heuristic(),
    );
    assert_eq!(together.stops[0].families.len(), 1, "{together:#?}");
    assert!(
        together
            .contexts
            .iter()
            .all(|c| c.kind == DecisionKind::Centroid)
    );
}

#[test]
fn post_r085_popularity_in_other_families_lifts_a_post_to_physical() {
    let points = || {
        vec![
            point("a", 0.0, 0.0, vec![osm(1)]),
            point("b", 0.0, 30.0, vec![osm(2)]),
        ]
    };
    let target = context(
        STOP,
        "L0",
        vec![variant("t", vec![good("f:R", 5.6), left("g:L", 3.0)])],
    );
    let alone = run(
        &pack(&[(STOP, points())], vec![target.clone()]),
        &heuristic(),
    );
    assert_eq!(alone.contexts[0].kind, DecisionKind::Centroid);
    let mut contexts = vec![target];
    for n in 1..=5 {
        contexts.push(context(
            STOP,
            &format!("L{n}"),
            vec![variant(
                &format!("o{n}"),
                vec![good("f:R", 1.0), left("g:L", 3.0)],
            )],
        ));
    }
    let popular = run(&pack(&[(STOP, points())], contexts), &heuristic());
    assert_eq!(popular.stops[0].families.len(), 6);
    assert!(
        popular
            .contexts
            .iter()
            .all(|c| c.kind == DecisionKind::Physical)
    );
}

#[test]
fn post_r087_two_equally_good_posts_leave_no_margin() {
    let points = vec![
        point("a", 0.0, 0.0, vec![osm(1)]),
        point("b", 0.0, 30.0, vec![osm(2)]),
    ];
    let pack = pack(
        &[(STOP, points)],
        vec![context(
            STOP,
            "L1",
            vec![variant("in", vec![good("f:R", 1.0), good("g:R", 1.0)])],
        )],
    );
    assert_eq!(
        run(&pack, &heuristic()).contexts[0].kind,
        DecisionKind::Centroid
    );
}

#[test]
fn post_r088_a_physical_family_decision_carries_score_margin_and_face() {
    let decisions = run(&two_points(good("f:R", 3.0)), &heuristic());
    let decision = &decisions.contexts[0];
    let family = decisions.family(decision);
    let choice = family.physical.as_ref().expect("physical");
    assert_eq!(choice.face.as_deref(), Some("f:R"));
    assert_eq!(decision.score, Some(choice.score));
    assert_eq!(decision.margin, Some(choice.margin));
    assert_eq!(
        choice.score, choice.margin,
        "no runner-up: the margin is the score"
    );
    assert!(choice.score >= 0.98);
}

#[test]
fn post_r089_the_plausible_score_is_validated_but_decides_nothing() {
    let too_high = heuristic_with(|v| v["resolution"]["minimum_plausible_score"] = 0.99.into());
    assert!(too_high.is_err());
    let low = heuristic_with(|v| v["resolution"]["minimum_plausible_score"] = 0.1.into()).unwrap();
    let pack = two_points(good("f:R", 3.0));
    assert_eq!(run(&pack, &low).contexts, run(&pack, &heuristic()).contexts);
}

#[test]
fn post_r100_r120_learned_decides_unlabelled_contexts_authored_stay_heuristic() {
    let points = vec![
        point("a", 0.0, 0.0, vec![osm(1)]),
        point("b", 0.0, 30.0, vec![osm(2)]),
    ];
    let unlabelled = context(
        STOP,
        "L1",
        vec![variant("in", vec![good("f:R", 3.0), left("f:L", 3.0)])],
    );
    let mut authored = unlabelled.clone();
    authored.line = "L2".into();
    authored.pattern = "pattern-L2".into();
    authored.authored = Some("id:5".into());
    let pack = pack(&[(STOP, points)], vec![unlabelled, authored]);
    assert_eq!(
        run(&pack, &heuristic()).contexts[0].kind,
        DecisionKind::Physical
    );
    let decisions = run(&pack, &learned(-10.0, 0.9, None));
    let (first, second) = (&decisions.contexts[0], &decisions.contexts[1]);
    // Both hypotheses are candidates of equal utility: 0.5 each, below the threshold.
    assert_eq!(first.kind, DecisionKind::Abstained);
    assert!(first.learned.is_some());
    assert_eq!(second.kind, DecisionKind::Authored);
    assert_eq!(second.hypothesis, None);
    assert!(decisions.family(second).physical.is_some());
}

#[test]
fn post_r101_r102_stop_level_sources_leave_the_choice_set() {
    let mut stops = Vec::new();
    for stop in 1..=20 {
        stops.push((
            stop,
            vec![
                point("a", 0.0, 0.0, vec![osm(stop * 10)]),
                point("b", 0.0, 30.0, vec![osm(stop * 10 + 1)]),
                point("c", 0.0, 60.0, vec![catalogue("cat.csv", stop)]),
            ],
        ));
    }
    stops.push((
        STOP * 10,
        vec![
            point("a", 0.0, 0.0, vec![osm(1)]),
            point("c", 0.0, 30.0, vec![catalogue("cat.csv", 999)]),
        ],
    ));
    let contexts = vec![context(
        STOP * 10,
        "L1",
        vec![variant("in", vec![good("f:R", 3.0), good("g:R", 3.0)])],
    )];
    let decisions = run(&pack(&stops, contexts), &learned(-10.0, 0.9, None));
    assert_eq!(decisions.counts.stop_level_sources, 1);
    assert_eq!(decisions.contexts[0].kind, DecisionKind::Physical);
    assert_eq!(
        observations(&decisions, 0),
        Some(vec!["osm:node:1".to_owned()])
    );
}

#[test]
fn post_r105_a_post_whose_modes_exclude_the_context_leaves_it_unseen() {
    let mut a = point("a", 0.0, 0.0, vec![osm(1)]);
    a.explicit_modes = "ROAD".into();
    let mut b = point("b", 0.0, 30.0, vec![osm(2)]);
    b.explicit_modes = "ROAD".into();
    let mut tram = context(
        STOP,
        "L1",
        vec![variant("in", vec![good("f:R", 3.0), good("g:R", 3.0)])],
    );
    tram.mode = "E".into();
    let decisions = run(
        &pack(&[(STOP, vec![a, b])], vec![tram]),
        &learned(-10.0, 0.9, None),
    );
    assert_eq!(decisions.contexts[0].kind, DecisionKind::Unseen);
}

#[test]
fn post_r106_a_feature_the_evaluator_does_not_compute_is_refused() {
    let mut model = jr_formats::post_policy::parse(
        serde_json::json!({
            "schema_version": 3,
            "policy": serde_json::from_str::<serde_json::Value>(POLICY_V2).unwrap(),
            "scorer": {"kind": "learned", "model": {
                "format": "post-scorer-two-stage-v1",
                "stage1": {"format": "post-scorer-conditional-logit-v1", "features": ["colour"],
                           "mean": [0.0], "scale": [1.0], "weights": [0.0], "null_utility": 0.0},
                "stage2": {"format": "post-scorer-conditional-logit-v1",
                           "features": ["colour", "reverse_mass", "has_reverse", "same_movement_mass",
                                        "has_same_movement", "same_next_mass", "same_previous_mass"],
                           "mean": [0.0,0.0,0.0,0.0,0.0,0.0,0.0], "scale": [1.0,1.0,1.0,1.0,1.0,1.0,1.0],
                           "weights": [0.0,0.0,0.0,0.0,0.0,0.0,0.0], "null_utility": 0.0},
                "thresholds": {"simple": 0.5, "complex": 0.5}
            }}
        })
        .to_string()
        .as_bytes(),
    )
    .expect("the document itself is well-formed");
    assert!(EvaluationPolicy::from_document(&model).is_err());
    model.scorer = jr_formats::post_policy::Scorer::Heuristic;
    assert!(EvaluationPolicy::from_document(&model).is_ok());
}

#[test]
fn post_r109_an_area_decision_publishes_a_post_with_the_area_probability_as_margin() {
    let points = vec![
        point("a", 0.0, 0.0, vec![osm(1)]),
        point("b", 0.0, 30.0, vec![osm(2)]),
    ];
    let pack = pack(
        &[(STOP, points)],
        vec![context(
            STOP,
            "L1",
            vec![variant("in", vec![good("f:R", 3.0), good("g:R", 3.0)])],
        )],
    );
    let decisions = run(&pack, &learned(-10.0, 0.9, Some(0.7)));
    let decision = &decisions.contexts[0];
    assert_eq!(decision.kind, DecisionKind::Area);
    assert!(decision.kind.is_located());
    let scores = decision.learned.expect("learned");
    assert_eq!(decision.score, Some(scores.post_probability));
    assert_eq!(decision.margin, Some(scores.area_probability));
    assert!(scores.post_probability < 0.51 && scores.area_probability > 0.99);
    assert_eq!(decisions.counts.estimated_posts, 1);
}

#[test]
fn post_r121_contexts_choosing_one_hypothesis_publish_one_post() {
    let points = vec![
        point("a", 0.0, 0.0, vec![osm(1)]),
        point("b", 0.0, 30.0, vec![osm(2)]),
    ];
    let contexts = ["L1", "L2"]
        .iter()
        .map(|line| {
            context(
                STOP,
                line,
                vec![variant("in", vec![good("f:R", 3.0), left("g:L", 3.0)])],
            )
        })
        .collect();
    let decisions = run(&pack(&[(STOP, points)], contexts), &heuristic());
    assert_eq!(decisions.counts.physical, 2);
    assert_eq!(decisions.counts.estimated_posts, 1);
}

#[test]
fn post_r122_an_authored_post_takes_a_position_only_where_an_unlabelled_call_published_it() {
    let points = || {
        vec![
            point("a", 0.0, 0.0, vec![osm(1)]),
            point("b", 0.0, 30.0, vec![osm(2)]),
        ]
    };
    let mut authored = context(
        STOP,
        "L2",
        vec![variant("in", vec![good("f:R", 3.0), left("g:L", 3.0)])],
    );
    authored.authored = Some("num:3".into());
    let unlabelled = context(
        STOP,
        "L1",
        vec![variant("in", vec![good("f:R", 3.0), left("g:L", 3.0)])],
    );
    let both = run(
        &pack(&[(STOP, points())], vec![authored.clone(), unlabelled]),
        &heuristic(),
    );
    assert_eq!(both.authored_positions.len(), 1);
    let position = &both.authored_positions[0];
    assert_eq!(
        (position.stop, &*position.authored_post_key),
        (STOP, "num:3")
    );
    let hypothesis = &both.stops[0].hypotheses[position.hypothesis];
    assert_eq!(&*hypothesis.observation_ids[0], "osm:node:1");
    // B11 (verdict Q5): at a stop where every call is authored nothing is positioned.
    let only_authored = run(&pack(&[(STOP, points())], vec![authored]), &heuristic());
    assert!(only_authored.authored_positions.is_empty());
}

#[test]
fn post_r123_stops_with_one_point_count_as_candidate_stops_but_are_not_evaluated() {
    let pack = pack(
        &[
            (
                STOP,
                vec![
                    point("a", 0.0, 0.0, vec![osm(1)]),
                    point("b", 0.0, 30.0, vec![osm(2)]),
                ],
            ),
            (STOP + 1, vec![point("a", 0.0, 0.0, vec![osm(3)])]),
        ],
        vec![context(
            STOP,
            "L1",
            vec![variant("in", vec![good("f:R", 3.0), left("g:L", 3.0)])],
        )],
    );
    let counts = run(&pack, &heuristic()).counts;
    assert_eq!((counts.candidate_stops, counts.evaluated_stops), (2, 1));
    assert_eq!(counts.contexts, 1);
}

fn several_stops() -> Pack {
    let mut stops = Vec::new();
    let mut contexts = Vec::new();
    for stop in 1..=12u64 {
        stops.push((
            stop,
            vec![
                point("a", 0.0, 0.0, vec![osm(stop * 10)]),
                point("b", 0.0, 1.0, vec![osm(stop * 10 + 1)]),
                point("c", 0.0, 30.0, vec![osm(stop * 10 + 2)]),
            ],
        ));
        for line in ["L1", "L2"] {
            let distance = (stop % 5) as f64;
            contexts.push(context(
                stop,
                line,
                vec![variant(
                    "in",
                    vec![good("f:R", distance), good("f:R", 2.0), good("g:R", 4.0)],
                )],
            ));
        }
    }
    let mut block = context(
        3,
        "L9",
        vec![variant(
            "x",
            vec![good("f:R", 1.0), good("f:R", 1.0), left("g:L", 1.0)],
        )],
    );
    block.role = BlockRole::Incoming;
    block.next = None;
    contexts.push(block);
    pack(&stops, contexts)
}

#[test]
fn post_r180_decisions_do_not_depend_on_the_thread_count() {
    let pack = several_stops();
    for policy in [heuristic(), learned(-2.0, 0.6, Some(0.67))] {
        let parallel = run(&pack, &policy);
        let single = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap()
            .install(|| run(&pack, &policy));
        assert_eq!(parallel, single);
    }
}

#[test]
fn a_pack_written_and_read_back_gives_the_same_decisions() {
    let pack = several_stops();
    let files = post_evidence::write(&pack).expect("writes");
    let read = post_evidence::read(&files).expect("reads");
    for policy in [heuristic(), learned(-2.0, 0.6, Some(0.67))] {
        assert_eq!(run(&pack, &policy), run(&read, &policy));
    }
}

#[test]
fn a_pack_that_breaks_an_invariant_is_not_evaluated() {
    let mut pack = two_points(good("f:R", 3.0));
    pack.contexts.movement_family_id[0] = "movement-family:corrupt".into();
    assert!(evaluate::evaluate(&pack, &heuristic()).is_err());
}
