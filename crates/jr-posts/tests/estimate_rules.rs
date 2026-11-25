//! The estimator as a pass: capture (POST-R030–R057), plan and publication (R130–R140), on a
//! hand-built street.

mod common;
mod world;

use std::collections::BTreeSet;

use jr_formats::geodata::registry::{Post, PostKey, Status};
use jr_formats::post_evidence::{self, BlockRole};
use jr_model::timetable::{BoardingPointKind, CoordSource, Precision};
use jr_model::{Diagnostics, Timetable};
use world::{World, at, crossing, osm};

/// The boarding point ID of every call of every trip, in trip order.
fn boarding(timetable: &Timetable) -> Vec<Vec<String>> {
    timetable
        .trips
        .rows()
        .iter()
        .map(|trip| {
            trip.calls
                .iter()
                .map(|call| {
                    call.boarding_point
                        .map(|point| timetable.boarding_points[point].id.to_string())
                        .unwrap_or_default()
                })
                .collect()
        })
        .collect()
}

fn estimated_posts(timetable: &Timetable) -> BTreeSet<String> {
    timetable
        .boarding_points
        .rows()
        .iter()
        .filter(|point| point.id.as_str().contains(":est:"))
        .map(|point| point.id.to_string())
        .collect()
}

/// The estimated post a call boards at and its position's metres north of the street.
fn post_north(timetable: &Timetable, trip: usize, call: usize) -> f64 {
    let point = timetable.trips.rows()[trip].calls[call]
        .boarding_point
        .unwrap();
    let position = timetable.boarding_points[point].position.unwrap();
    (position.coord.lat - 50.0) * 111_250.0
}

#[test]
fn post_r040_r135_each_direction_boards_at_the_post_right_of_its_travel() {
    let world = crossing();
    let estimated = world.estimate(&common::heuristic(), &[]);
    let pack = estimated.pack.as_ref().unwrap();
    assert_eq!(
        pack.contexts.len(),
        2,
        "one context per direction at stop 2"
    );
    assert_eq!(estimated.summary.single_internal_posts, 2);
    let timetable = world.applied(estimated);
    assert!(
        post_north(&timetable, 0, 1) < 0.0,
        "eastbound: the south post"
    );
    assert!(
        post_north(&timetable, 1, 1) > 0.0,
        "westbound: the north post"
    );
    assert_eq!(
        estimated_posts(&timetable),
        ["jdf:stop:2:est:1", "jdf:stop:2:est:2"]
            .map(String::from)
            .into()
    );
}

#[test]
fn post_r060_the_captured_pack_is_valid_and_survives_a_write_and_a_read() {
    let world = crossing();
    let policy = common::heuristic();
    let estimated = world.estimate(&policy, &[]);
    let pack = estimated.pack.clone().unwrap();
    post_evidence::check(&pack).expect("valid pack");
    let read = post_evidence::read(&post_evidence::write(&pack).unwrap()).unwrap();
    let again = jr_posts::estimate::estimate_from_pack(
        &world.timetable,
        &world.facts,
        read,
        &policy,
        &[],
        &mut Diagnostics::new(),
    )
    .unwrap();
    assert_eq!(again.edits, estimated.edits);
}

#[test]
fn post_r030_candidates_far_from_the_stop_are_not_observed() {
    let mut world = World::new();
    world.stop(2, 0.0, true, vec![osm(21, -4.0, 0.0), osm(22, 4.0, 350.0)]);
    world.stop(1, -500.0, true, vec![]);
    world.trip(1, "A", "1", &[(1, None), (2, None)]);
    let estimated = world.estimate(&common::heuristic(), &[]);
    let pack = estimated.pack.unwrap();
    assert_eq!(pack.observations.len(), 1);
    assert_eq!(pack.contexts.len(), 0, "one point left: R033");
}

#[test]
fn post_r033_a_stop_without_a_precise_position_or_with_one_point_is_not_estimated() {
    let mut world = World::new();
    world.stop(1, -500.0, true, vec![osm(11, -4.0, -500.0)]);
    world.stop(2, 0.0, false, vec![osm(21, -4.0, 0.0), osm(22, 4.0, 0.0)]);
    world.trip(1, "A", "1", &[(1, None), (2, None)]);
    let estimated = world.estimate(&common::heuristic(), &[]);
    let pack = estimated.pack.as_ref().unwrap();
    assert_eq!(pack.route_points.len(), 1);
    assert_eq!(pack.contexts.len(), 0);
    let timetable = world.applied(estimated);
    assert!(estimated_posts(&timetable).is_empty());
}

#[test]
fn post_r041_identical_trips_share_their_contexts() {
    let mut world = crossing();
    world.trip(3, "A", "100001", &[(1, None), (2, None), (3, None)]);
    let estimated = world.estimate(&common::heuristic(), &[]);
    assert_eq!(estimated.summary.capture.patterns, 2);
    assert_eq!(estimated.summary.capture.contexts, 2);
    let timetable = world.applied(estimated);
    assert_eq!(boarding(&timetable)[2], boarding(&timetable)[0]);
}

#[test]
fn post_r042_a_repeated_call_forms_a_block_with_incoming_and_outgoing_roles() {
    let mut world = crossing();
    world.trip(
        5,
        "A",
        "100002",
        &[(1, None), (2, None), (2, None), (3, None)],
    );
    let estimated = world.estimate(&common::heuristic(), &[]);
    let pack = estimated.pack.unwrap();
    let roles: Vec<(BlockRole, Option<u64>, Option<u64>, bool)> = (0..pack.contexts.len())
        .filter(|&i| &*pack.contexts.line_id[i] == "100002")
        .map(|i| {
            (
                pack.contexts.role[i],
                pack.contexts.previous_stop[i],
                pack.contexts.next_stop[i],
                pack.contexts.same_stop_block_id[i].is_some(),
            )
        })
        .collect();
    assert_eq!(
        roles,
        vec![
            (BlockRole::Incoming, Some(1), None, true),
            (BlockRole::Outgoing, None, Some(3), true),
        ]
    );
}

#[test]
fn post_r043_an_authored_call_keeps_its_post() {
    let mut world = crossing();
    let post = world.authored(2, 7);
    world.trip(5, "A", "100002", &[(1, None), (2, Some(post)), (3, None)]);
    let estimated = world.estimate(&common::heuristic(), &[]);
    let pack = estimated.pack.as_ref().unwrap();
    assert!(
        pack.contexts
            .authored_post_key
            .iter()
            .any(|key| key.as_deref() == Some("id:7"))
    );
    let timetable = world.applied(estimated);
    assert_eq!(boarding(&timetable)[2][1], "jdf:stop:2:post:id:7");
}

#[test]
fn post_r045_the_anchor_skips_a_neighbour_without_one() {
    let mut world = crossing();
    world.stop(4, -250.0, false, vec![]);
    world.trip(
        5,
        "A",
        "100002",
        &[(1, None), (4, None), (2, None), (3, None)],
    );
    let estimated = world.estimate(&common::heuristic(), &[]);
    let pack = estimated.pack.unwrap();
    let previous: Vec<Option<u64>> = (0..pack.contexts.len())
        .filter(|&i| &*pack.contexts.line_id[i] == "100002")
        .map(|i| pack.contexts.previous_stop[i])
        .collect();
    assert_eq!(previous, vec![Some(1)]);
}

#[test]
fn post_r132_registered_ordinals_are_reused_and_retired_ones_never_issued_again() {
    let world = crossing();
    let registered = |ordinal, north, status| Post {
        stop_id: 2,
        post_key: PostKey::Estimated(ordinal),
        reference: Some(at(north, 0.0)),
        status,
        note: None,
        line: 2,
    };
    let registry = [
        registered(7, -5.0, Status::Active),
        registered(9, 4.0, Status::Retired),
    ];
    let estimated = world.estimate(&common::heuristic(), &registry);
    assert_eq!(estimated.summary.registry_reused_ordinals, 1);
    let candidates = estimated
        .reports
        .iter()
        .find(|report| report.name == "post_candidates")
        .unwrap()
        .clone();
    let timetable = world.applied(estimated);
    let south = timetable.trips.rows()[0].calls[1].boarding_point.unwrap();
    let north = timetable.trips.rows()[1].calls[1].boarding_point.unwrap();
    assert_eq!(
        timetable.boarding_points[south].id.as_str(),
        "jdf:stop:2:est:7"
    );
    assert_eq!(
        timetable.boarding_points[north].id.as_str(),
        "jdf:stop:2:est:10"
    );
    // R133: only the fresh ordinal is listed, in F#'s format.
    assert_eq!(
        candidates.header,
        ["stop_id", "post_key", "lat", "lon", "reason"]
    );
    assert_eq!(candidates.rows.len(), 1);
    assert_eq!(candidates.rows[0][..2], ["2", "est:10"]);
    assert_eq!(
        candidates.rows[0][2].split('.').nth(1).map(str::len),
        Some(6)
    );
    assert_eq!(candidates.rows[0][4], "new");
}

#[test]
fn post_r130_r134_an_estimated_post_is_the_estimators_position() {
    // B13 fixed (verdict Q15 revisited, serving 5.4): no longer the stop's provenance.
    let world = crossing();
    let timetable = world.applied(world.estimate(&common::heuristic(), &[]));
    let mut estimated = 0;
    for point in timetable.boarding_points.rows() {
        if point.id.as_str().contains(":est:") {
            estimated += 1;
            assert_eq!(point.kind, BoardingPointKind::Post);
            assert_eq!(point.public_code, None);
            let position = point.position.unwrap();
            assert_eq!(
                (position.precision, position.source),
                (Precision::Estimated, Some(CoordSource::PostEstimate))
            );
        }
    }
    assert!(estimated > 0);
}

#[test]
fn post_r136_calls_of_other_modes_and_undecided_contexts_keep_their_point() {
    let mut world = crossing();
    world.trip(5, "V", "100003", &[(1, None), (2, None), (3, None)]);
    let timetable = world.applied(world.estimate(&common::heuristic(), &[]));
    assert_eq!(boarding(&timetable)[2][1], "jdf:stop:2:unspecified");
    assert_eq!(boarding(&timetable)[0][0], "jdf:stop:1:unspecified");
}

#[test]
fn post_r140_the_decisions_table_has_a_row_per_context() {
    let world = crossing();
    let estimated = world.estimate(&common::heuristic(), &[]);
    let table = estimated
        .reports
        .iter()
        .find(|report| report.name == "post_decisions")
        .unwrap();
    assert_eq!(
        table.rows.len(),
        estimated.pack.as_ref().unwrap().contexts.len()
    );
    let kind = table.header.iter().position(|h| *h == "kind").unwrap();
    assert!(table.rows.iter().all(|row| row[kind] == "physical"));
}

#[test]
fn post_r057_the_estimate_does_not_depend_on_the_thread_count() {
    let world = crossing();
    let policy = common::heuristic();
    let parallel = world.estimate(&policy, &[]);
    let single = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(|| world.estimate(&policy, &[]));
    assert_eq!(parallel.edits, single.edits);
    assert_eq!(parallel.pack, single.pack);
}
