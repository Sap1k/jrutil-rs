//! A checked [`Pack`] to its six files (POST-R060): Snappy, row groups of 65,536 rows, the
//! footer metadata of section 2.3 in the F# writer's order, no embedded arrow schema, then the
//! manifest with each file's size, hash, rows and fingerprint.

use std::sync::Arc;

use arrow_array::{
    ArrayRef, BooleanArray, Float64Array, Int32Array, Int64Array, RecordBatch, StringArray,
};
use parquet::arrow::ArrowWriter;
use parquet::arrow::arrow_writer::ArrowWriterOptions;
use parquet::basic::Compression;
use parquet::file::metadata::KeyValue;
use parquet::file::properties::WriterProperties;

use super::ids::{sha256_hex, stop_text};
use super::manifest::invariant;
use super::schema::{self, Column};
use super::{
    CAPTURE_MAXIMUM_CORRIDOR_VARIANTS, CAPTURE_ROUTED_EXCESS_METRES, CaptureCeilings,
    EVIDENCE_FORMAT, EVIDENCE_SCHEMA_VERSION, FileEntry, MAXIMUM_SEARCH_DISTANCE_METRES,
    MAXIMUM_SEARCH_STATES, Manifest, Pack, PackError, PackFiles, RELATIONS,
    VARIANT_ENUMERATION_VERSION,
};

const GROUP_ROWS: usize = 65_536;

fn text(values: impl Iterator<Item = String>) -> ArrayRef {
    Arc::new(values.map(Some).collect::<StringArray>())
}

fn optional_text<'a>(values: impl Iterator<Item = Option<&'a str>>) -> ArrayRef {
    Arc::new(values.collect::<StringArray>())
}

fn doubles(values: impl Iterator<Item = f64>) -> ArrayRef {
    Arc::new(values.map(Some).collect::<Float64Array>())
}

fn optional_doubles(values: impl Iterator<Item = Option<f64>>) -> ArrayRef {
    Arc::new(values.collect::<Float64Array>())
}

fn ints(values: impl Iterator<Item = i32>) -> ArrayRef {
    Arc::new(values.map(Some).collect::<Int32Array>())
}

fn optional_ints(values: impl Iterator<Item = Option<i32>>) -> ArrayRef {
    Arc::new(values.collect::<Int32Array>())
}

fn optional_longs(values: impl Iterator<Item = Option<i64>>) -> ArrayRef {
    Arc::new(values.collect::<Int64Array>())
}

fn booleans(values: impl Iterator<Item = bool>) -> ArrayRef {
    Arc::new(values.map(Some).collect::<BooleanArray>())
}

/// Writes one relation; `group` builds the arrays of rows `range`.
fn relation(
    pack: &Pack,
    file: &'static str,
    columns: &'static [Column],
    rows: usize,
    group: impl Fn(std::ops::Range<usize>) -> Vec<ArrayRef>,
) -> Result<(Vec<u8>, FileEntry), PackError> {
    let identity = &pack.identity;
    let pack_id = identity.pack_id();
    let metadata = [
        ("obehy.evidence_format", EVIDENCE_FORMAT.to_owned()),
        (
            "obehy.evidence_schema_version",
            EVIDENCE_SCHEMA_VERSION.to_string(),
        ),
        (
            "obehy.router_evidence_version",
            identity.router.as_str().to_owned(),
        ),
        (
            "obehy.variant_enumeration_version",
            VARIANT_ENUMERATION_VERSION.to_owned(),
        ),
        (
            "obehy.capture_tool_version",
            identity.capture_tool_version.clone(),
        ),
        ("obehy.pack_id", pack_id),
        ("obehy.relation", file.to_owned()),
        ("obehy.source_id", identity.source_id.clone()),
        (
            "obehy.snapshot_id",
            format!("sha256:{}", identity.merged_jdf_sha256),
        ),
        (
            "obehy.routing_pbf_sha256",
            identity.routing_pbf_sha256.clone(),
        ),
        (
            "obehy.capture_routed_excess_metres",
            invariant(CAPTURE_ROUTED_EXCESS_METRES),
        ),
        (
            "obehy.capture_maximum_corridor_variants",
            CAPTURE_MAXIMUM_CORRIDOR_VARIANTS.to_string(),
        ),
        (
            "obehy.maximum_search_states",
            MAXIMUM_SEARCH_STATES.to_string(),
        ),
        (
            "obehy.maximum_search_distance_metres",
            invariant(MAXIMUM_SEARCH_DISTANCE_METRES),
        ),
    ]
    .into_iter()
    .map(|(key, value)| KeyValue::new(key.to_owned(), value))
    .collect();
    let properties = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .set_max_row_group_row_count(Some(GROUP_ROWS))
        .set_key_value_metadata(Some(metadata))
        .build();
    let schema = Arc::new(schema::arrow_schema(columns));
    let parquet = |source| PackError::Parquet { file, source };
    let mut bytes = Vec::new();
    let mut writer = ArrowWriter::try_new_with_options(
        &mut bytes,
        schema.clone(),
        ArrowWriterOptions::new()
            .with_properties(properties)
            .with_skip_arrow_metadata(true),
    )
    .map_err(parquet)?;
    for start in (0..rows).step_by(GROUP_ROWS) {
        let range = start..(start + GROUP_ROWS).min(rows);
        let batch = RecordBatch::try_new(schema.clone(), group(range))
            .map_err(|source| PackError::Arrow { file, source })?;
        writer.write(&batch).map_err(parquet)?;
        writer.flush().map_err(parquet)?;
    }
    writer.close().map_err(parquet)?;
    let entry = FileEntry {
        path: file.to_owned(),
        sha256: sha256_hex(&bytes),
        bytes: bytes.len() as i64,
        rows: rows as i64,
        schema_fingerprint: schema::fingerprint(columns, identity.router),
    };
    Ok((bytes, entry))
}

pub(crate) fn encode(pack: &Pack) -> Result<PackFiles, PackError> {
    let o = &pack.observations;
    let p = &pack.route_points;
    let c = &pack.contexts;
    let v = &pack.corridor_variants;
    let a = &pack.route_point_evidence;
    let point_stop = |point: u32| p.stop[point as usize];
    let variant_context = |variant: u32| v.context[variant as usize] as usize;

    let mut members: Vec<Vec<&str>> = vec![Vec::new(); p.len()];
    for row in 0..o.len() {
        members[o.route_point[row] as usize].push(&o.observation_id[row]);
    }
    for list in &mut members {
        list.sort_unstable();
    }

    let observations = relation(pack, RELATIONS[0], schema::OBSERVATIONS, o.len(), |r| {
        vec![
            text(r.clone().map(|i| stop_text(point_stop(o.route_point[i])))),
            text(
                r.clone()
                    .map(|i| p.route_point_id[o.route_point[i] as usize].to_string()),
            ),
            text(r.clone().map(|i| o.observation_id[i].to_string())),
            text(r.clone().map(|i| o.source_kind[i].to_string())),
            optional_text(r.clone().map(|i| o.source_object_id[i].as_deref())),
            optional_text(r.clone().map(|i| o.observed_at[i].as_deref())),
            doubles(r.clone().map(|i| o.latitude[i])),
            doubles(r.clone().map(|i| o.longitude[i])),
            doubles(r.clone().map(|i| o.support_weight[i])),
            text(r.clone().map(|i| o.raw_tags[i].to_string())),
            text(r.clone().map(|i| o.explicit_modes[i].to_string())),
            text(r.clone().map(|i| o.denied_modes[i].to_string())),
            text(r.map(|i| o.lifecycle[i].to_string())),
        ]
    })?;
    let route_points = relation(pack, RELATIONS[1], schema::ROUTE_POINTS, p.len(), |r| {
        vec![
            text(r.clone().map(|i| stop_text(p.stop[i]))),
            text(r.clone().map(|i| p.route_point_id[i].to_string())),
            text(
                r.clone()
                    .map(|i| p.representative_observation_id[i].to_string()),
            ),
            text(r.clone().map(|i| members[i].join(";"))),
            doubles(r.clone().map(|i| p.latitude[i])),
            doubles(r.clone().map(|i| p.longitude[i])),
            booleans(r.clone().map(|i| p.has_current_lifecycle[i])),
            booleans(r.clone().map(|i| p.has_obsolete_lifecycle[i])),
            doubles(r.clone().map(|i| p.source_support_weight[i])),
            text(r.clone().map(|i| p.explicit_modes[i].to_string())),
            text(r.map(|i| p.denied_modes[i].to_string())),
        ]
    })?;
    let contexts = relation(pack, RELATIONS[2], schema::CONTEXTS, c.len(), |r| {
        vec![
            text(r.clone().map(|i| c.context_id[i].to_string())),
            text(r.clone().map(|i| stop_text(c.stop[i]))),
            text(r.clone().map(|i| c.mode[i].to_string())),
            text(r.clone().map(|i| c.line_id[i].to_string())),
            ints(r.clone().map(|i| c.route_distinction[i])),
            ints(r.clone().map(|i| c.direction[i])),
            text(r.clone().map(|i| c.pattern_hash[i].to_string())),
            ints(r.clone().map(|i| c.pattern_position[i])),
            text(r.clone().map(|i| c.role[i].as_str().to_owned())),
            text(r.clone().map(|i| c.movement_family_id[i].to_string())),
            Arc::new(
                r.clone()
                    .map(|i| c.previous_stop[i].map(stop_text))
                    .collect::<StringArray>(),
            ),
            Arc::new(
                r.clone()
                    .map(|i| c.next_stop[i].map(stop_text))
                    .collect::<StringArray>(),
            ),
            text(r.clone().map(|i| c.assignment_kind(i).to_owned())),
            optional_text(r.clone().map(|i| c.authored_post_key[i].as_deref())),
            optional_text(r.map(|i| c.same_stop_block_id[i].as_deref())),
        ]
    })?;
    let variants = relation(
        pack,
        RELATIONS[3],
        schema::CORRIDOR_VARIANTS,
        v.len(),
        |r| {
            vec![
                text(
                    r.clone()
                        .map(|i| c.context_id[v.context[i] as usize].to_string()),
                ),
                text(r.clone().map(|i| stop_text(c.stop[v.context[i] as usize]))),
                ints(r.clone().map(|i| v.variant_rank[i])),
                optional_text(r.clone().map(|i| v.corridor_id[i].as_deref())),
                optional_doubles(r.clone().map(|i| v.absolute_cost_metres[i])),
                optional_doubles(r.clone().map(|i| v.relative_cost_metres[i])),
                optional_doubles(r.clone().map(|i| v.relative_cost_fraction[i])),
                optional_doubles(r.clone().map(|i| v.path_length_metres[i])),
                ints(r.clone().map(|i| v.directed_edge_count[i])),
                ints(r.clone().map(|i| v.repeated_directed_edge_count[i])),
                ints(r.clone().map(|i| v.service_edge_count[i])),
                ints(r.clone().map(|i| v.restricted_access_edge_count[i])),
                optional_doubles(r.clone().map(|i| v.access_penalty_metres[i])),
                optional_text(r.clone().map(|i| v.ingress_thread_id[i].as_deref())),
                optional_text(r.clone().map(|i| v.egress_thread_id[i].as_deref())),
                text(
                    r.clone()
                        .map(|i| v.routing_availability[i].as_str().to_owned()),
                ),
                optional_text(r.map(|i| v.invariant_failure_reason[i].as_deref())),
            ]
        },
    )?;
    let attachments = relation(
        pack,
        RELATIONS[4],
        schema::ROUTE_POINT_EVIDENCE,
        a.len(),
        |r| {
            vec![
                text(
                    r.clone()
                        .map(|i| c.context_id[variant_context(a.variant[i])].to_string()),
                ),
                text(
                    r.clone()
                        .map(|i| stop_text(c.stop[variant_context(a.variant[i])])),
                ),
                ints(r.clone().map(|i| v.variant_rank[a.variant[i] as usize])),
                optional_text(
                    r.clone()
                        .map(|i| v.corridor_id[a.variant[i] as usize].as_deref()),
                ),
                text(
                    r.clone()
                        .map(|i| p.route_point_id[a.route_point[i] as usize].to_string()),
                ),
                optional_doubles(r.clone().map(|i| a.routed_excess_metres[i])),
                optional_ints(r.clone().map(|i| a.snap_edge_id[i])),
                optional_longs(r.clone().map(|i| a.snap_way_id[i])),
                optional_doubles(r.clone().map(|i| a.snap_fraction[i])),
                optional_doubles(r.clone().map(|i| a.snap_projected_longitude[i])),
                optional_doubles(r.clone().map(|i| a.snap_projected_latitude[i])),
                optional_doubles(r.clone().map(|i| a.snap_distance_metres[i])),
                optional_text(r.clone().map(|i| a.corridor_face_id[i].as_deref())),
                optional_doubles(r.clone().map(|i| a.corridor_distance_metres[i])),
                optional_doubles(r.clone().map(|i| a.signed_lateral_offset_metres[i])),
                optional_doubles(r.clone().map(|i| a.corridor_heading_degrees[i])),
                optional_doubles(r.clone().map(|i| a.attachment_heading_degrees[i])),
                optional_doubles(r.clone().map(|i| a.heading_difference_degrees[i])),
                optional_doubles(r.clone().map(|i| a.proximity_distance_metres[i])),
                text(
                    r.clone()
                        .map(|i| a.routing_availability[i].as_str().to_owned()),
                ),
                optional_text(r.map(|i| a.invariant_failure_reason[i].as_deref())),
            ]
        },
    )?;

    let identity = &pack.identity;
    let manifest = Manifest {
        evidence_format: EVIDENCE_FORMAT.to_owned(),
        schema_version: EVIDENCE_SCHEMA_VERSION,
        pack_id: identity.pack_id(),
        capture_tool_version: identity.capture_tool_version.clone(),
        merged_jdf_sha256: identity.merged_jdf_sha256.clone(),
        routing_pbf_sha256: identity.routing_pbf_sha256.clone(),
        osm_snapshot: None,
        router_evidence_version: identity.router.as_str().to_owned(),
        variant_enumeration_version: VARIANT_ENUMERATION_VERSION.to_owned(),
        capture_ceilings: CaptureCeilings {
            routed_excess_metres: CAPTURE_ROUTED_EXCESS_METRES,
            maximum_corridor_variants: CAPTURE_MAXIMUM_CORRIDOR_VARIANTS,
        },
        maximum_search_states: MAXIMUM_SEARCH_STATES,
        maximum_search_distance_metres: MAXIMUM_SEARCH_DISTANCE_METRES,
        context_count: c.len() as i64,
        route_point_count: p.len() as i64,
        observation_count: o.len() as i64,
        corridor_variant_count: v.len() as i64,
        route_point_evidence_count: a.len() as i64,
        files: vec![
            observations.1,
            route_points.1,
            contexts.1,
            variants.1,
            attachments.1,
        ],
    };
    Ok(PackFiles {
        manifest: manifest.to_bytes(),
        observations: observations.0,
        route_points: route_points.0,
        contexts: contexts.0,
        corridor_variants: variants.0,
        route_point_evidence: attachments.0,
    })
}
