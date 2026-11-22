//! The six files to a [`Pack`]: the manifest's file entries, each relation's footer and
//! schema, the columns, the foreign keys resolved to rows, then [`check`](super::check).

use std::collections::HashMap;

use arrow_array::cast::AsArray;
use arrow_array::types::{Float64Type, Int32Type, Int64Type};
use arrow_array::{Array, RecordBatch};
use bytes::Bytes;
use parquet::arrow::arrow_reader::{ParquetRecordBatchReader, ParquetRecordBatchReaderBuilder};

use super::check::{check, stop_points, stop_ranges};
use super::ids::{parse_stop, sha256_hex};
use super::manifest::invariant;
use super::schema::{self, Column};
use super::{
    Availability, BlockRole, CAPTURE_MAXIMUM_CORRIDOR_VARIANTS, CAPTURE_ROUTED_EXCESS_METRES,
    Contexts, CorridorVariants, EVIDENCE_FORMAT, EVIDENCE_SCHEMA_VERSION,
    MAXIMUM_SEARCH_DISTANCE_METRES, MAXIMUM_SEARCH_STATES, Manifest, Observations, Pack, PackError,
    PackFiles, PackIdentity, RELATIONS, RoutePointEvidence, RoutePoints, Violation,
};

const BATCH_ROWS: usize = 65_536;

/// One relation file, its footer checked, ready to stream.
struct Relation {
    file: &'static str,
    reader: ParquetRecordBatchReader,
    source_id: String,
}

impl Relation {
    fn batches(self) -> impl Iterator<Item = Result<(usize, RecordBatch), PackError>> {
        let file = self.file;
        let mut offset = 0usize;
        self.reader.map(move |batch| {
            let batch = batch.map_err(|source| PackError::Arrow { file, source })?;
            let start = offset;
            offset += batch.num_rows();
            Ok((start, batch))
        })
    }
}

pub(crate) fn decode(manifest: &Manifest, files: &PackFiles) -> Result<Pack, PackError> {
    check_entries(manifest, files)?;
    let open = |index: usize, columns: &'static [Column]| {
        let file = RELATIONS[index];
        open_relation(
            manifest,
            files.relation(file).unwrap_or_default(),
            file,
            columns,
        )
    };
    let observations = open(0, schema::OBSERVATIONS)?;
    let route_points = open(1, schema::ROUTE_POINTS)?;
    let contexts = open(2, schema::CONTEXTS)?;
    let variants = open(3, schema::CORRIDOR_VARIANTS)?;
    let attachments = open(4, schema::ROUTE_POINT_EVIDENCE)?;
    let source_id = observations.source_id.clone();
    for relation in [&route_points, &contexts, &variants, &attachments] {
        if relation.source_id != source_id {
            return Err(PackError::FooterMetadata {
                file: relation.file,
                key: "obehy.source_id",
            });
        }
    }

    let (route_points, member_texts) = decode_route_points(route_points)?;
    let ranges = stop_ranges(&route_points.stop);
    let observations = decode_observations(observations, &route_points, &ranges)?;
    check_membership(&observations, &route_points, &member_texts)?;
    let contexts = decode_contexts(contexts)?;
    let context_rows: HashMap<&str, u32> = contexts
        .context_id
        .iter()
        .enumerate()
        .map(|(row, id)| (&**id, row as u32))
        .collect();
    let variants = decode_variants(variants, &contexts, &context_rows)?;
    let attachments = decode_attachments(
        attachments,
        &contexts,
        &context_rows,
        &variants,
        &route_points,
        &ranges,
    )?;
    drop(context_rows);

    let pack = Pack {
        identity: PackIdentity {
            capture_tool_version: manifest.capture_tool_version.clone(),
            merged_jdf_sha256: manifest.merged_jdf_sha256.clone(),
            routing_pbf_sha256: manifest.routing_pbf_sha256.clone(),
            source_id,
            router: manifest.router(),
        },
        observations,
        route_points,
        contexts,
        corridor_variants: variants,
        route_point_evidence: attachments,
    };
    check(&pack)?;
    let counts = [
        (pack.observations.len(), manifest.observation_count),
        (pack.route_points.len(), manifest.route_point_count),
        (pack.contexts.len(), manifest.context_count),
        (
            pack.corridor_variants.len(),
            manifest.corridor_variant_count,
        ),
        (
            pack.route_point_evidence.len(),
            manifest.route_point_evidence_count,
        ),
    ];
    if counts
        .iter()
        .any(|&(actual, declared)| i64::try_from(actual).ok() != Some(declared))
    {
        return Err(PackError::Counts);
    }
    Ok(pack)
}

/// The manifest's file entries against the bytes (the F# `validateEvidencePack`).
fn check_entries(manifest: &Manifest, files: &PackFiles) -> Result<(), PackError> {
    let mut entries = HashMap::new();
    for entry in &manifest.files {
        if entries.insert(entry.path.as_str(), entry).is_some() {
            return Err(PackError::DuplicateFile(entry.path.clone()));
        }
    }
    for file in RELATIONS {
        let entry = entries.get(file).ok_or(PackError::MissingFile(file))?;
        let bytes = files.relation(file).unwrap_or_default();
        if i64::try_from(bytes.len()).ok() != Some(entry.bytes) {
            return Err(PackError::FileSize {
                file,
                declared: entry.bytes,
                actual: bytes.len(),
            });
        }
        if !sha256_hex(bytes).eq_ignore_ascii_case(&entry.sha256) {
            return Err(PackError::FileHash { file });
        }
        if entry.rows < 0 || entry.schema_fingerprint.trim().is_empty() {
            return Err(PackError::FileMetadata { file });
        }
        if entry.rows != manifest.declared_rows(file) {
            return Err(PackError::RowCount { file });
        }
    }
    if entries.len() != RELATIONS.len() {
        return Err(PackError::ExtraFile);
    }
    Ok(())
}

/// Opens one relation and checks its schema, fingerprint, footer metadata and row count.
fn open_relation(
    manifest: &Manifest,
    bytes: &[u8],
    file: &'static str,
    columns: &'static [Column],
) -> Result<Relation, PackError> {
    let parquet = |source| PackError::Parquet { file, source };
    let builder =
        ParquetRecordBatchReaderBuilder::try_new(Bytes::copy_from_slice(bytes)).map_err(parquet)?;
    if !schema::matches(builder.schema(), columns) {
        return Err(PackError::Schema { file });
    }
    let entry = manifest
        .files
        .iter()
        .find(|entry| entry.path == file)
        .ok_or(PackError::MissingFile(file))?;
    if schema::fingerprint(columns, manifest.router()) != entry.schema_fingerprint {
        return Err(PackError::Fingerprint { file });
    }
    let metadata = builder.metadata().file_metadata();
    let value = |key: &str| -> Option<&str> {
        metadata
            .key_value_metadata()?
            .iter()
            .find(|entry| entry.key == key)?
            .value
            .as_deref()
    };
    let snapshot = format!("sha256:{}", manifest.merged_jdf_sha256);
    let expected: [(&'static str, &str); 13] = [
        ("obehy.evidence_format", EVIDENCE_FORMAT),
        (
            "obehy.evidence_schema_version",
            &EVIDENCE_SCHEMA_VERSION.to_string(),
        ),
        (
            "obehy.router_evidence_version",
            &manifest.router_evidence_version,
        ),
        (
            "obehy.variant_enumeration_version",
            &manifest.variant_enumeration_version,
        ),
        ("obehy.capture_tool_version", &manifest.capture_tool_version),
        ("obehy.pack_id", &manifest.pack_id),
        ("obehy.relation", file),
        ("obehy.snapshot_id", &snapshot),
        ("obehy.routing_pbf_sha256", &manifest.routing_pbf_sha256),
        (
            "obehy.capture_routed_excess_metres",
            &invariant(CAPTURE_ROUTED_EXCESS_METRES),
        ),
        (
            "obehy.capture_maximum_corridor_variants",
            &CAPTURE_MAXIMUM_CORRIDOR_VARIANTS.to_string(),
        ),
        (
            "obehy.maximum_search_states",
            &MAXIMUM_SEARCH_STATES.to_string(),
        ),
        (
            "obehy.maximum_search_distance_metres",
            &invariant(MAXIMUM_SEARCH_DISTANCE_METRES),
        ),
    ];
    for (key, wanted) in expected {
        if value(key) != Some(wanted) {
            return Err(PackError::FooterMetadata { file, key });
        }
    }
    let source_id = value("obehy.source_id")
        .filter(|value| !value.trim().is_empty())
        .ok_or(PackError::FooterMetadata {
            file,
            key: "obehy.source_id",
        })?
        .to_owned();
    if metadata.num_rows() != entry.rows {
        return Err(PackError::PhysicalRows { file });
    }
    let reader = builder
        .with_batch_size(BATCH_ROWS)
        .build()
        .map_err(parquet)?;
    Ok(Relation {
        file,
        reader,
        source_id,
    })
}

/// Typed access to the columns of one batch.
struct Batch<'a> {
    batch: &'a RecordBatch,
}

impl<'a> Batch<'a> {
    fn text(&self, index: usize, row: usize) -> &'a str {
        self.batch.column(index).as_string::<i32>().value(row)
    }

    fn optional_text(&self, index: usize, row: usize) -> Option<&'a str> {
        let column = self.batch.column(index).as_string::<i32>();
        (!column.is_null(row)).then(|| column.value(row))
    }

    fn boxed(&self, index: usize, row: usize) -> Box<str> {
        self.text(index, row).into()
    }

    fn optional_boxed(&self, index: usize, row: usize) -> Option<Box<str>> {
        self.optional_text(index, row).map(Into::into)
    }

    fn double(&self, index: usize, row: usize) -> f64 {
        self.batch
            .column(index)
            .as_primitive::<Float64Type>()
            .value(row)
    }

    fn optional_double(&self, index: usize, row: usize) -> Option<f64> {
        let column = self.batch.column(index).as_primitive::<Float64Type>();
        (!column.is_null(row)).then(|| column.value(row))
    }

    fn int(&self, index: usize, row: usize) -> i32 {
        self.batch
            .column(index)
            .as_primitive::<Int32Type>()
            .value(row)
    }

    fn optional_int(&self, index: usize, row: usize) -> Option<i32> {
        let column = self.batch.column(index).as_primitive::<Int32Type>();
        (!column.is_null(row)).then(|| column.value(row))
    }

    fn optional_long(&self, index: usize, row: usize) -> Option<i64> {
        let column = self.batch.column(index).as_primitive::<Int64Type>();
        (!column.is_null(row)).then(|| column.value(row))
    }

    fn boolean(&self, index: usize, row: usize) -> bool {
        self.batch.column(index).as_boolean().value(row)
    }
}

fn stop(relation: &'static str, row: usize, text: &str) -> Result<u64, PackError> {
    parse_stop(text).ok_or_else(|| {
        PackError::invariant(relation, row, Violation::StopIdentity(text.to_owned()))
    })
}

fn availability(relation: &'static str, row: usize, text: &str) -> Result<Availability, PackError> {
    Availability::parse(text)
        .ok_or_else(|| PackError::invariant(relation, row, Violation::Enumeration(text.to_owned())))
}

/// The route points and each row's stored `observation_ids`.
fn decode_route_points(relation: Relation) -> Result<(RoutePoints, Vec<Box<str>>), PackError> {
    let mut points = RoutePoints::default();
    let mut members = Vec::new();
    for batch in relation.batches() {
        let (start, batch) = batch?;
        let b = Batch { batch: &batch };
        for row in 0..batch.num_rows() {
            points
                .stop
                .push(stop("route_points", start + row, b.text(0, row))?);
            points.route_point_id.push(b.boxed(1, row));
            points.representative_observation_id.push(b.boxed(2, row));
            members.push(b.boxed(3, row));
            points.latitude.push(b.double(4, row));
            points.longitude.push(b.double(5, row));
            points.has_current_lifecycle.push(b.boolean(6, row));
            points.has_obsolete_lifecycle.push(b.boolean(7, row));
            points.source_support_weight.push(b.double(8, row));
            points.explicit_modes.push(b.boxed(9, row));
            points.denied_modes.push(b.boxed(10, row));
        }
    }
    Ok((points, members))
}

/// The row of route point `id` at `stop`.
fn point_row(
    points: &RoutePoints,
    ranges: &[(u64, usize, usize)],
    stop: u64,
    id: &str,
) -> Option<u32> {
    let (start, end) = stop_points(ranges, stop)?;
    points.route_point_id[start..end]
        .binary_search_by(|candidate| (**candidate).cmp(id))
        .ok()
        .map(|offset| (start + offset) as u32)
}

fn decode_observations(
    relation: Relation,
    points: &RoutePoints,
    ranges: &[(u64, usize, usize)],
) -> Result<Observations, PackError> {
    let mut observations = Observations::default();
    for batch in relation.batches() {
        let (start, batch) = batch?;
        let b = Batch { batch: &batch };
        for row in 0..batch.num_rows() {
            let stop = stop("observations", start + row, b.text(0, row))?;
            let point = point_row(points, ranges, stop, b.text(1, row)).ok_or_else(|| {
                PackError::invariant(
                    "observations",
                    start + row,
                    Violation::ObservationRoutePoint,
                )
            })?;
            observations.route_point.push(point);
            observations.observation_id.push(b.boxed(2, row));
            observations.source_kind.push(b.boxed(3, row));
            observations.source_object_id.push(b.optional_boxed(4, row));
            observations.observed_at.push(b.optional_boxed(5, row));
            observations.latitude.push(b.double(6, row));
            observations.longitude.push(b.double(7, row));
            observations.support_weight.push(b.double(8, row));
            observations.raw_tags.push(b.boxed(9, row));
            observations.explicit_modes.push(b.boxed(10, row));
            observations.denied_modes.push(b.boxed(11, row));
            observations.lifecycle.push(b.boxed(12, row));
        }
    }
    Ok(observations)
}

/// A route point's stored `observation_ids` (split on `;`, empty entries dropped) must be its
/// members in ordinal order, each once.
fn check_membership(
    observations: &Observations,
    points: &RoutePoints,
    stored: &[Box<str>],
) -> Result<(), PackError> {
    let mut members: Vec<Vec<&str>> = vec![Vec::new(); points.len()];
    for row in 0..observations.len() {
        members[observations.route_point[row] as usize].push(&observations.observation_id[row]);
    }
    for (row, members) in members.iter_mut().enumerate() {
        members.sort_unstable();
        let listed: Vec<&str> = stored[row].split(';').filter(|id| !id.is_empty()).collect();
        if listed != *members {
            return Err(PackError::invariant(
                "route_points",
                row,
                Violation::RoutePointMembership,
            ));
        }
    }
    Ok(())
}

fn decode_contexts(relation: Relation) -> Result<Contexts, PackError> {
    let mut contexts = Contexts::default();
    for batch in relation.batches() {
        let (start, batch) = batch?;
        let b = Batch { batch: &batch };
        for row in 0..batch.num_rows() {
            let at = start + row;
            let invalid = || PackError::invariant("contexts", at, Violation::ContextFacts);
            contexts.context_id.push(b.boxed(0, row));
            contexts.stop.push(stop("contexts", at, b.text(1, row))?);
            contexts.mode.push(b.boxed(2, row));
            contexts.line_id.push(b.boxed(3, row));
            contexts.route_distinction.push(b.int(4, row));
            contexts.direction.push(b.int(5, row));
            contexts.pattern_hash.push(b.boxed(6, row));
            contexts.pattern_position.push(b.int(7, row));
            contexts
                .role
                .push(BlockRole::parse(b.text(8, row)).ok_or_else(invalid)?);
            contexts.movement_family_id.push(b.boxed(9, row));
            let neighbour = |index| {
                b.optional_text(index, row)
                    .map(|text| stop("contexts", at, text))
                    .transpose()
            };
            contexts.previous_stop.push(neighbour(10)?);
            contexts.next_stop.push(neighbour(11)?);
            let authored = b.optional_boxed(13, row);
            let consistent = match b.text(12, row) {
                "authored" => authored.is_some(),
                "unlabelled" => authored.is_none(),
                _ => false,
            };
            if !consistent {
                return Err(invalid());
            }
            contexts.authored_post_key.push(authored);
            contexts.same_stop_block_id.push(b.optional_boxed(14, row));
        }
    }
    Ok(contexts)
}

fn decode_variants(
    relation: Relation,
    contexts: &Contexts,
    context_rows: &HashMap<&str, u32>,
) -> Result<CorridorVariants, PackError> {
    let mut variants = CorridorVariants::default();
    for batch in relation.batches() {
        let (start, batch) = batch?;
        let b = Batch { batch: &batch };
        for row in 0..batch.num_rows() {
            let at = start + row;
            let fail = |violation| PackError::invariant("corridor_variants", at, violation);
            let context = *context_rows
                .get(b.text(0, row))
                .ok_or_else(|| fail(Violation::VariantContext))?;
            if stop("corridor_variants", at, b.text(1, row))? != contexts.stop[context as usize] {
                return Err(fail(Violation::VariantCounts));
            }
            variants.context.push(context);
            variants.variant_rank.push(b.int(2, row));
            variants.corridor_id.push(b.optional_boxed(3, row));
            variants
                .absolute_cost_metres
                .push(b.optional_double(4, row));
            variants
                .relative_cost_metres
                .push(b.optional_double(5, row));
            variants
                .relative_cost_fraction
                .push(b.optional_double(6, row));
            variants.path_length_metres.push(b.optional_double(7, row));
            variants.directed_edge_count.push(b.int(8, row));
            variants.repeated_directed_edge_count.push(b.int(9, row));
            variants.service_edge_count.push(b.int(10, row));
            variants.restricted_access_edge_count.push(b.int(11, row));
            variants
                .access_penalty_metres
                .push(b.optional_double(12, row));
            variants.ingress_thread_id.push(b.optional_boxed(13, row));
            variants.egress_thread_id.push(b.optional_boxed(14, row));
            variants.routing_availability.push(availability(
                "corridor_variants",
                at,
                b.text(15, row),
            )?);
            variants
                .invariant_failure_reason
                .push(b.optional_boxed(16, row));
        }
    }
    Ok(variants)
}

fn decode_attachments(
    relation: Relation,
    contexts: &Contexts,
    context_rows: &HashMap<&str, u32>,
    variants: &CorridorVariants,
    points: &RoutePoints,
    ranges: &[(u64, usize, usize)],
) -> Result<RoutePointEvidence, PackError> {
    let variant_rows: HashMap<(u32, i32), u32> = (0..variants.len())
        .map(|row| {
            (
                (variants.context[row], variants.variant_rank[row]),
                row as u32,
            )
        })
        .collect();
    let mut attachments = RoutePointEvidence::default();
    for batch in relation.batches() {
        let (start, batch) = batch?;
        let b = Batch { batch: &batch };
        for row in 0..batch.num_rows() {
            let at = start + row;
            let fail = |violation| PackError::invariant("route_point_evidence", at, violation);
            let context = *context_rows
                .get(b.text(0, row))
                .ok_or_else(|| fail(Violation::AttachmentForeignKey))?;
            let stop = stop("route_point_evidence", at, b.text(1, row))?;
            if stop != contexts.stop[context as usize] {
                return Err(fail(Violation::AttachmentForeignKey));
            }
            let variant = *variant_rows
                .get(&(context, b.int(2, row)))
                .ok_or_else(|| fail(Violation::AttachmentVariant))?;
            if variants.corridor_id[variant as usize].as_deref() != b.optional_text(3, row) {
                return Err(fail(Violation::AttachmentForeignKey));
            }
            let point = point_row(points, ranges, stop, b.text(4, row))
                .ok_or_else(|| fail(Violation::AttachmentForeignKey))?;
            attachments.variant.push(variant);
            attachments.route_point.push(point);
            attachments
                .routed_excess_metres
                .push(b.optional_double(5, row));
            attachments.snap_edge_id.push(b.optional_int(6, row));
            attachments.snap_way_id.push(b.optional_long(7, row));
            attachments.snap_fraction.push(b.optional_double(8, row));
            attachments
                .snap_projected_longitude
                .push(b.optional_double(9, row));
            attachments
                .snap_projected_latitude
                .push(b.optional_double(10, row));
            attachments
                .snap_distance_metres
                .push(b.optional_double(11, row));
            attachments.corridor_face_id.push(b.optional_boxed(12, row));
            attachments
                .corridor_distance_metres
                .push(b.optional_double(13, row));
            attachments
                .signed_lateral_offset_metres
                .push(b.optional_double(14, row));
            attachments
                .corridor_heading_degrees
                .push(b.optional_double(15, row));
            attachments
                .attachment_heading_degrees
                .push(b.optional_double(16, row));
            attachments
                .heading_difference_degrees
                .push(b.optional_double(17, row));
            attachments
                .proximity_distance_metres
                .push(b.optional_double(18, row));
            attachments.routing_availability.push(availability(
                "route_point_evidence",
                at,
                b.text(19, row),
            )?);
            attachments
                .invariant_failure_reason
                .push(b.optional_boxed(20, row));
        }
    }
    Ok(attachments)
}
