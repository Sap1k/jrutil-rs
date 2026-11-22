//! The evidence pack reader and writer (POST-R060, R061, R182).

use jr_formats::post_evidence::ids::{context_id, movement_family_id, same_stop_block_id};
use jr_formats::post_evidence::{
    self, Availability, BlockRole, Contexts, CorridorVariants, Observations, Pack, PackError,
    PackFiles, PackIdentity, RoutePointEvidence, RoutePoints, RouterEvidence, Violation,
    parse_manifest,
};

/// One stop (7) with two route points and two contexts: an unlabelled through context with two
/// variants, and an authored incoming context whose corridor is unavailable.
fn pack(router: RouterEvidence) -> Pack {
    let mut points = RoutePoints::default();
    for (id, observation, lat) in [
        ("route-point:a", "osm:node:1", 50.0),
        ("route-point:b", "osm:node:2", 50.0001),
    ] {
        points.stop.push(7);
        points.route_point_id.push(id.into());
        points
            .representative_observation_id
            .push(observation.into());
        points.latitude.push(lat);
        points.longitude.push(14.0);
        points.has_current_lifecycle.push(true);
        points.has_obsolete_lifecycle.push(false);
        points.source_support_weight.push(1.0);
        points.explicit_modes.push("ROAD".into());
        points.denied_modes.push("".into());
    }
    let mut observations = Observations::default();
    for (row, id) in ["osm:node:1", "osm:node:2"].iter().enumerate() {
        observations.route_point.push(row as u32);
        observations.observation_id.push((*id).into());
        observations.source_kind.push("osm".into());
        observations.source_object_id.push(Some((*id).into()));
        observations.observed_at.push(None);
        observations.latitude.push(points.latitude[row]);
        observations.longitude.push(14.0);
        observations.support_weight.push(1.0);
        observations
            .raw_tags
            .push("highway=bus_stop;local_ref=1".into());
        observations.explicit_modes.push("road".into());
        observations.denied_modes.push("".into());
        observations.lifecycle.push("active".into());
    }
    let mut contexts = Contexts::default();
    let specs = [
        ("1", BlockRole::Through, None, Some(8), Some(9)),
        ("2", BlockRole::Incoming, Some("id:4"), Some(8), None),
    ];
    for (line, role, authored, previous, next) in specs {
        contexts
            .context_id
            .push(context_id(7, "A", line, 0, 1, "p", 3, role.as_str(), authored).into());
        contexts.stop.push(7);
        contexts.mode.push("A".into());
        contexts.line_id.push(line.into());
        contexts.route_distinction.push(0);
        contexts.direction.push(1);
        contexts.pattern_hash.push("p".into());
        contexts.pattern_position.push(3);
        contexts.role.push(role);
        let (ingress, egress) = if authored.is_none() {
            (Some("t:+"), Some("u:+"))
        } else {
            (None, None)
        };
        contexts
            .movement_family_id
            .push(movement_family_id(7, "A", ingress, egress, role.as_str()).into());
        contexts.previous_stop.push(previous);
        contexts.next_stop.push(next);
        contexts.authored_post_key.push(authored.map(Into::into));
        contexts.same_stop_block_id.push(
            (role != BlockRole::Through).then(|| same_stop_block_id(line, 0, "p", 3, 4).into()),
        );
    }
    let mut variants = CorridorVariants::default();
    let mut attachments = RoutePointEvidence::default();
    for (context, rank, available) in [(0u32, 0, true), (0, 1, true), (1, 0, false)] {
        let row = variants.len() as u32;
        let set = |value: f64| available.then_some(value);
        variants.context.push(context);
        variants.variant_rank.push(rank);
        variants
            .corridor_id
            .push(available.then(|| format!("c{rank}").into()));
        variants
            .absolute_cost_metres
            .push(set(100.0 + f64::from(rank)));
        variants.relative_cost_metres.push(set(f64::from(rank)));
        variants
            .relative_cost_fraction
            .push(set(f64::from(rank) / 100.0));
        variants.path_length_metres.push(set(90.0));
        variants
            .directed_edge_count
            .push(if available { 4 } else { 0 });
        variants.repeated_directed_edge_count.push(0);
        variants
            .service_edge_count
            .push(if available { 1 } else { 0 });
        variants.restricted_access_edge_count.push(0);
        variants.access_penalty_metres.push(set(10.0));
        variants
            .ingress_thread_id
            .push(available.then(|| "t:+".into()));
        variants
            .egress_thread_id
            .push(available.then(|| "u:+".into()));
        variants.routing_availability.push(if available {
            Availability::Available
        } else {
            Availability::Unavailable
        });
        variants
            .invariant_failure_reason
            .push((!available).then(|| "disconnected".into()));
        for point in 0..2u32 {
            let value = |x: f64| available.then_some(x);
            attachments.variant.push(row);
            attachments.route_point.push(point);
            attachments.routed_excess_metres.push(value(5.0));
            attachments.snap_edge_id.push(available.then_some(3));
            attachments.snap_way_id.push(available.then_some(33));
            attachments.snap_fraction.push(value(0.25));
            attachments.snap_projected_longitude.push(value(14.0));
            attachments.snap_projected_latitude.push(value(50.0));
            attachments.snap_distance_metres.push(value(2.5));
            attachments
                .corridor_face_id
                .push(available.then(|| "t:R".into()));
            attachments.corridor_distance_metres.push(value(2.5));
            attachments.signed_lateral_offset_metres.push(value(-2.5));
            attachments.corridor_heading_degrees.push(value(90.0));
            attachments.attachment_heading_degrees.push(value(91.0));
            attachments.heading_difference_degrees.push(value(1.0));
            attachments.proximity_distance_metres.push(value(2.5));
            attachments.routing_availability.push(if available {
                Availability::Available
            } else {
                Availability::Unavailable
            });
            attachments
                .invariant_failure_reason
                .push((!available).then(|| "corridor-unavailable".into()));
        }
    }
    Pack {
        identity: PackIdentity {
            capture_tool_version: "test-tool".into(),
            merged_jdf_sha256: "0".repeat(64),
            routing_pbf_sha256: "1".repeat(64),
            source_id: "source".into(),
            router,
        },
        observations,
        route_points: points,
        contexts,
        corridor_variants: variants,
        route_point_evidence: attachments,
    }
}

fn files() -> PackFiles {
    post_evidence::write(&pack(RouterEvidence::Rust)).expect("writes")
}

/// Replaces a relation's bytes and updates its manifest entry so only the content differs.
fn replace(files: &mut PackFiles, name: &str, bytes: Vec<u8>) {
    let mut manifest = parse_manifest(&files.manifest).unwrap();
    let entry = manifest
        .files
        .iter_mut()
        .find(|entry| entry.path == name)
        .unwrap();
    entry.sha256 = post_evidence::ids::sha256_hex(&bytes);
    entry.bytes = bytes.len() as i64;
    files.manifest = manifest.to_bytes();
    match name {
        "observations.parquet" => files.observations = bytes,
        "contexts.parquet" => files.contexts = bytes,
        _ => unreachable!(),
    }
}

fn manifest_edit(files: &mut PackFiles, edit: impl FnOnce(&mut serde_json::Value)) {
    let mut value: serde_json::Value = serde_json::from_slice(&files.manifest).unwrap();
    edit(&mut value);
    files.manifest = serde_json::to_vec_pretty(&value).unwrap();
}

#[test]
fn post_r060_a_written_pack_reads_back_unchanged_and_writing_is_deterministic() {
    let original = pack(RouterEvidence::Rust);
    let first = post_evidence::write(&original).unwrap();
    assert_eq!(first, post_evidence::write(&original).unwrap());
    assert_eq!(post_evidence::read(&first).unwrap(), original);
    let text = String::from_utf8(first.manifest.clone()).unwrap();
    assert!(text.ends_with("}\n"));
    assert!(text.contains("\"routed_excess_metres\": 1000,"));
}

#[test]
fn post_r061_packs_of_the_f_sharp_and_the_rust_router_are_both_accepted() {
    for router in [RouterEvidence::FSharp, RouterEvidence::Rust] {
        let original = pack(router);
        let files = post_evidence::write(&original).unwrap();
        let manifest = parse_manifest(&files.manifest).unwrap();
        assert_eq!(manifest.router_evidence_version, router.as_str());
        assert_eq!(post_evidence::read(&files).unwrap(), original);
    }
    let fs = parse_manifest(
        &post_evidence::write(&pack(RouterEvidence::FSharp))
            .unwrap()
            .manifest,
    )
    .unwrap();
    let rust = parse_manifest(&files().manifest).unwrap();
    assert_ne!(fs.pack_id, rust.pack_id);
    assert_ne!(
        fs.files[0].schema_fingerprint,
        rust.files[0].schema_fingerprint
    );
}

#[test]
fn post_r061_evidence_v1_is_rejected_before_any_relation() {
    let files = PackFiles {
        manifest: br#"{"evidence_format":"post-inference-evidence-v1","schema_version":1}"#
            .to_vec(),
        ..PackFiles::default()
    };
    assert!(matches!(
        post_evidence::read(&files),
        Err(PackError::Format { .. })
    ));
}

#[test]
fn post_r061_an_extra_manifest_key_is_rejected() {
    let mut files = files();
    manifest_edit(&mut files, |v| v["unexpected"] = true.into());
    assert!(matches!(
        post_evidence::read(&files),
        Err(PackError::Shape(_))
    ));
}

#[test]
fn post_r061_an_unknown_router_contract_is_rejected() {
    let mut files = files();
    manifest_edit(&mut files, |v| {
        v["router_evidence_version"] = "packed-directed-v2".into()
    });
    assert!(matches!(
        post_evidence::read(&files),
        Err(PackError::RouterContract { .. })
    ));
}

#[test]
fn post_r061_the_capture_tool_version_takes_part_in_the_pack_id() {
    let mut files = files();
    manifest_edit(&mut files, |v| {
        v["capture_tool_version"] = "other-tool".into()
    });
    assert!(matches!(
        post_evidence::read(&files),
        Err(PackError::PackId { .. })
    ));
}

#[test]
fn post_r061_other_capture_ceilings_are_rejected() {
    let mut files = files();
    manifest_edit(&mut files, |v| v["maximum_search_states"] = 5.into());
    assert!(matches!(
        post_evidence::read(&files),
        Err(PackError::Ceilings)
    ));
}

#[test]
fn post_r061_a_missing_file_entry_is_rejected() {
    let mut files = files();
    manifest_edit(&mut files, |v| {
        v["files"].as_array_mut().unwrap().remove(0);
    });
    assert!(matches!(
        post_evidence::read(&files),
        Err(PackError::MissingFile(_))
    ));
}

#[test]
fn post_r061_a_tampered_file_is_rejected_by_its_hash() {
    let mut files = files();
    let last = files.contexts.len() - 10;
    files.contexts[last] ^= 1;
    assert!(matches!(
        post_evidence::read(&files),
        Err(PackError::FileHash { .. })
    ));
}

#[test]
fn post_r061_a_relation_with_another_schema_is_rejected() {
    let mut files = files();
    let contexts = files.contexts.clone();
    replace(&mut files, "observations.parquet", contexts);
    assert!(matches!(
        post_evidence::read(&files),
        Err(PackError::Schema { .. })
    ));
}

#[test]
fn post_r061_a_relation_of_another_pack_is_rejected_by_its_footer() {
    let mut other = pack(RouterEvidence::Rust);
    other.identity.merged_jdf_sha256 = "2".repeat(64);
    let foreign = post_evidence::write(&other).unwrap().contexts;
    let mut files = files();
    replace(&mut files, "contexts.parquet", foreign);
    assert!(matches!(
        post_evidence::read(&files),
        Err(PackError::FooterMetadata { .. })
    ));
}

#[test]
fn post_r061_a_wrong_schema_fingerprint_is_rejected() {
    let mut files = files();
    let fingerprint = parse_manifest(
        &post_evidence::write(&pack(RouterEvidence::FSharp))
            .unwrap()
            .manifest,
    )
    .unwrap()
    .files[0]
        .schema_fingerprint
        .clone();
    let mut manifest = parse_manifest(&files.manifest).unwrap();
    manifest.files[0].schema_fingerprint = fingerprint;
    files.manifest = manifest.to_bytes();
    assert!(matches!(
        post_evidence::read(&files),
        Err(PackError::Fingerprint { .. })
    ));
}

#[test]
fn post_r061_semantic_corruption_is_rejected() {
    type Corrupt = Box<dyn Fn(&mut Pack)>;
    let cases: Vec<(&str, Corrupt, Violation)> = vec![
        (
            "context id",
            Box::new(|p| p.contexts.context_id[0] = "context:corrupt".into()),
            Violation::ContextIdentity,
        ),
        (
            "family",
            Box::new(|p| p.contexts.movement_family_id[0] = "movement-family:x".into()),
            Violation::MovementFamily,
        ),
        (
            "block",
            Box::new(|p| p.contexts.same_stop_block_id[1] = None),
            Violation::ContextFacts,
        ),
        (
            "neighbour",
            Box::new(|p| p.contexts.next_stop[1] = Some(9)),
            Violation::ContextNeighbours,
        ),
        (
            "rank",
            Box::new(|p| p.corridor_variants.variant_rank[1] = 2),
            Violation::VariantRank,
        ),
        (
            "relative cost",
            Box::new(|p| p.corridor_variants.relative_cost_metres[1] = Some(-1.0)),
            Violation::AvailableVariant,
        ),
        (
            "unavailable sentinel",
            Box::new(|p| p.corridor_variants.absolute_cost_metres[2] = Some(1.0)),
            Violation::UnavailableVariant,
        ),
        (
            "cost order",
            Box::new(|p| p.corridor_variants.absolute_cost_metres[1] = Some(50.0)),
            Violation::VariantCostOrder,
        ),
        (
            "snap range",
            Box::new(|p| p.route_point_evidence.snap_fraction[0] = Some(2.0)),
            Violation::AttachmentRange,
        ),
        (
            "attachment order",
            Box::new(|p| p.route_point_evidence.route_point.swap(0, 1)),
            Violation::AttachmentOrder,
        ),
        (
            "coverage",
            Box::new(|p| p.route_point_evidence.variant[1] = 1),
            Violation::AttachmentCoverage,
        ),
        (
            "observation range",
            Box::new(|p| p.observations.latitude[0] = 100.0),
            Violation::ObservationNumeric,
        ),
        (
            "representative",
            Box::new(|p| p.route_points.representative_observation_id[0] = "osm:node:9".into()),
            Violation::RoutePointMembership,
        ),
        (
            "point order",
            Box::new(|p| p.route_points.route_point_id.swap(0, 1)),
            Violation::RoutePointOrder,
        ),
    ];
    for (name, corrupt, expected) in cases {
        let mut broken = pack(RouterEvidence::Rust);
        corrupt(&mut broken);
        match post_evidence::check(&broken) {
            Err(PackError::Invariant { violation, .. }) => {
                assert_eq!(violation, expected, "{name}")
            }
            other => panic!("{name}: {other:?}"),
        }
        assert!(
            post_evidence::write(&broken).is_err(),
            "{name}: not written"
        );
    }
}

#[test]
fn post_r061_stop_identities_are_read_only_in_full() {
    assert_eq!(post_evidence::ids::parse_stop("jdf:stop:42"), Some(42));
    assert_eq!(post_evidence::ids::parse_stop("cis:stop:7"), Some(7));
    assert_eq!(post_evidence::ids::parse_stop("jdf:stop:42:est:1"), None);
    assert_eq!(post_evidence::ids::parse_stop("jdf:stop:"), None);
}

#[test]
fn post_r182_relations_of_one_pack_name_one_source() {
    let mut other = pack(RouterEvidence::Rust);
    other.identity.source_id = "another".into();
    let foreign = post_evidence::write(&other).unwrap().contexts;
    let mut files = files();
    replace(&mut files, "contexts.parquet", foreign);
    assert!(matches!(
        post_evidence::read(&files),
        Err(PackError::FooterMetadata {
            key: "obehy.source_id",
            ..
        })
    ));
}
