//! The invariants of a pack (POST-R061, the F# `JdfPostEvidenceStore.validateSemantics`):
//! ordering and uniqueness, observation membership, numeric ranges, recomputed context,
//! block and movement-family identities, neighbours consistent with roles, variant sentinels
//! and monotone costs, attachment coverage and sentinels. The reader runs them on every pack it
//! reads; a package that builds a pack in memory runs them before handing it on.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use super::ids::{context_id, is_sha256, movement_family_id, same_stop_block_id};
use super::{Availability, BlockRole, Pack, PackError, Violation};

const OBSERVATIONS: &str = "observations";
const ROUTE_POINTS: &str = "route_points";
const CONTEXTS: &str = "contexts";
const VARIANTS: &str = "corridor_variants";
const ATTACHMENTS: &str = "route_point_evidence";

fn fail(relation: &'static str, row: usize, violation: Violation) -> PackError {
    PackError::invariant(relation, row, violation)
}

/// Rows `start..end` of the route points of each stop, stops ascending.
pub(crate) fn stop_ranges(stops: &[u64]) -> Vec<(u64, usize, usize)> {
    let mut ranges: Vec<(u64, usize, usize)> = Vec::new();
    for (row, &stop) in stops.iter().enumerate() {
        match ranges.last_mut() {
            Some(last) if last.0 == stop => last.2 = row + 1,
            _ => ranges.push((stop, row, row + 1)),
        }
    }
    ranges
}

/// The route-point rows of `stop`, if it has any.
pub(crate) fn stop_points(ranges: &[(u64, usize, usize)], stop: u64) -> Option<(usize, usize)> {
    ranges
        .binary_search_by_key(&stop, |range| range.0)
        .ok()
        .map(|index| (ranges[index].1, ranges[index].2))
}

fn coordinate(latitude: f64, longitude: f64) -> bool {
    latitude.is_finite()
        && (-90.0..=90.0).contains(&latitude)
        && longitude.is_finite()
        && (-180.0..=180.0).contains(&longitude)
}

fn same_lengths(lengths: &[usize]) -> bool {
    lengths.windows(2).all(|pair| pair[0] == pair[1])
}

/// Checks every invariant of the pack; the first violation is the error.
pub fn check(pack: &Pack) -> Result<(), PackError> {
    check_lengths(pack)?;
    check_identity(pack)?;
    let ranges = check_points(pack)?;
    check_contexts(pack, &ranges)
}

fn check_lengths(pack: &Pack) -> Result<(), PackError> {
    let o = &pack.observations;
    let p = &pack.route_points;
    let c = &pack.contexts;
    let v = &pack.corridor_variants;
    let a = &pack.route_point_evidence;
    let tables: [(&'static str, Vec<usize>); 5] = [
        (
            OBSERVATIONS,
            vec![
                o.route_point.len(),
                o.observation_id.len(),
                o.source_kind.len(),
                o.source_object_id.len(),
                o.observed_at.len(),
                o.latitude.len(),
                o.longitude.len(),
                o.support_weight.len(),
                o.raw_tags.len(),
                o.explicit_modes.len(),
                o.denied_modes.len(),
                o.lifecycle.len(),
            ],
        ),
        (
            ROUTE_POINTS,
            vec![
                p.stop.len(),
                p.route_point_id.len(),
                p.representative_observation_id.len(),
                p.latitude.len(),
                p.longitude.len(),
                p.has_current_lifecycle.len(),
                p.has_obsolete_lifecycle.len(),
                p.source_support_weight.len(),
                p.explicit_modes.len(),
                p.denied_modes.len(),
            ],
        ),
        (
            CONTEXTS,
            vec![
                c.context_id.len(),
                c.stop.len(),
                c.mode.len(),
                c.line_id.len(),
                c.route_distinction.len(),
                c.direction.len(),
                c.pattern_hash.len(),
                c.pattern_position.len(),
                c.role.len(),
                c.movement_family_id.len(),
                c.previous_stop.len(),
                c.next_stop.len(),
                c.authored_post_key.len(),
                c.same_stop_block_id.len(),
            ],
        ),
        (
            VARIANTS,
            vec![
                v.context.len(),
                v.variant_rank.len(),
                v.corridor_id.len(),
                v.absolute_cost_metres.len(),
                v.relative_cost_metres.len(),
                v.relative_cost_fraction.len(),
                v.path_length_metres.len(),
                v.directed_edge_count.len(),
                v.repeated_directed_edge_count.len(),
                v.service_edge_count.len(),
                v.restricted_access_edge_count.len(),
                v.access_penalty_metres.len(),
                v.ingress_thread_id.len(),
                v.egress_thread_id.len(),
                v.routing_availability.len(),
                v.invariant_failure_reason.len(),
            ],
        ),
        (
            ATTACHMENTS,
            vec![
                a.variant.len(),
                a.route_point.len(),
                a.routed_excess_metres.len(),
                a.snap_edge_id.len(),
                a.snap_way_id.len(),
                a.snap_fraction.len(),
                a.snap_projected_longitude.len(),
                a.snap_projected_latitude.len(),
                a.snap_distance_metres.len(),
                a.corridor_face_id.len(),
                a.corridor_distance_metres.len(),
                a.signed_lateral_offset_metres.len(),
                a.corridor_heading_degrees.len(),
                a.attachment_heading_degrees.len(),
                a.heading_difference_degrees.len(),
                a.proximity_distance_metres.len(),
                a.routing_availability.len(),
                a.invariant_failure_reason.len(),
            ],
        ),
    ];
    for (relation, lengths) in tables {
        if !same_lengths(&lengths) {
            return Err(fail(relation, 0, Violation::ColumnLengths));
        }
    }
    let rows = [o.len(), p.len(), c.len(), v.len(), a.len()];
    if rows.iter().any(|&rows| u32::try_from(rows).is_err()) {
        return Err(fail(ATTACHMENTS, 0, Violation::ColumnLengths));
    }
    Ok(())
}

fn check_identity(pack: &Pack) -> Result<(), PackError> {
    let identity = &pack.identity;
    let checks = [
        (
            "capture_tool_version",
            !identity.capture_tool_version.trim().is_empty(),
        ),
        ("merged_jdf_sha256", is_sha256(&identity.merged_jdf_sha256)),
        (
            "routing_pbf_sha256",
            is_sha256(&identity.routing_pbf_sha256),
        ),
        ("source_id", !identity.source_id.trim().is_empty()),
    ];
    for (field, ok) in checks {
        if !ok {
            return Err(fail(OBSERVATIONS, 0, Violation::Identity(field)));
        }
    }
    Ok(())
}

/// Route points and observations; returns the route-point rows of each stop.
fn check_points(pack: &Pack) -> Result<Vec<(u64, usize, usize)>, PackError> {
    let points = &pack.route_points;
    for row in 0..points.len() {
        if row > 0 {
            let previous = (points.stop[row - 1], &*points.route_point_id[row - 1]);
            if previous >= (points.stop[row], &*points.route_point_id[row]) {
                return Err(fail(ROUTE_POINTS, row, Violation::RoutePointOrder));
            }
        }
        let support = points.source_support_weight[row];
        if !coordinate(points.latitude[row], points.longitude[row])
            || !support.is_finite()
            || support < 0.0
        {
            return Err(fail(ROUTE_POINTS, row, Violation::RoutePointNumeric));
        }
    }
    let observations = &pack.observations;
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); points.len()];
    for row in 0..observations.len() {
        let point = observations.route_point[row] as usize;
        if point >= points.len() {
            return Err(fail(OBSERVATIONS, row, Violation::ObservationRoutePoint));
        }
        if row > 0 {
            let previous_point = observations.route_point[row - 1] as usize;
            let previous = (
                points.stop.get(previous_point).copied().unwrap_or(0),
                &*observations.observation_id[row - 1],
            );
            if previous >= (points.stop[point], &*observations.observation_id[row]) {
                return Err(fail(OBSERVATIONS, row, Violation::ObservationOrder));
            }
        }
        let support = observations.support_weight[row];
        if !coordinate(observations.latitude[row], observations.longitude[row])
            || !support.is_finite()
            || support < 0.0
        {
            return Err(fail(OBSERVATIONS, row, Violation::ObservationNumeric));
        }
        members[point].push(row);
    }
    for (row, rows) in members.iter().enumerate() {
        let representative = &points.representative_observation_id[row];
        if rows.is_empty()
            || !rows
                .iter()
                .any(|&observation| observations.observation_id[observation] == *representative)
        {
            return Err(fail(ROUTE_POINTS, row, Violation::RoutePointMembership));
        }
    }
    Ok(stop_ranges(&points.stop))
}

/// The members of a same-stop block within one stop.
struct Block {
    line: Box<str>,
    distinction: i32,
    pattern: Box<str>,
    first_row: usize,
    minimum: i32,
    maximum: i32,
}

fn check_blocks(
    blocks: &mut BTreeMap<Box<str>, Block>,
    seen: &mut BTreeSet<Box<str>>,
) -> Result<(), PackError> {
    for (id, block) in std::mem::take(blocks) {
        let expected = same_stop_block_id(
            &block.line,
            block.distinction,
            &block.pattern,
            block.minimum,
            block.maximum.saturating_add(1),
        );
        if *id != expected || !seen.insert(id) {
            return Err(fail(CONTEXTS, block.first_row, Violation::BlockIdentity));
        }
    }
    Ok(())
}

fn context_order(pack: &Pack, left: usize, right: usize) -> Ordering {
    let c = &pack.contexts;
    let key = |row: usize| {
        (
            c.stop[row],
            &*c.mode[row],
            &*c.line_id[row],
            c.route_distinction[row],
            c.direction[row],
            &*c.pattern_hash[row],
            c.pattern_position[row],
            c.role[row].as_str(),
            c.authored_post_key[row].as_deref().unwrap_or(""),
            &*c.context_id[row],
        )
    };
    key(left).cmp(&key(right))
}

fn check_contexts(pack: &Pack, ranges: &[(u64, usize, usize)]) -> Result<(), PackError> {
    let contexts = &pack.contexts;
    let variants = &pack.corridor_variants;
    let attachments = &pack.route_point_evidence;
    let mut blocks: BTreeMap<Box<str>, Block> = BTreeMap::new();
    let mut seen_blocks: BTreeSet<Box<str>> = BTreeSet::new();
    let mut current_stop: Option<u64> = None;
    let mut variant = 0usize;
    let mut attachment = 0usize;
    for row in 0..contexts.len() {
        let stop = contexts.stop[row];
        if current_stop != Some(stop) {
            check_blocks(&mut blocks, &mut seen_blocks)?;
            current_stop = Some(stop);
        }
        if row > 0 && context_order(pack, row - 1, row) != Ordering::Less {
            return Err(fail(CONTEXTS, row, Violation::ContextOrder));
        }
        let (first_point, end_point) =
            stop_points(ranges, stop).ok_or_else(|| fail(CONTEXTS, row, Violation::ContextStop))?;
        let role = contexts.role[row];
        let block = contexts.same_stop_block_id[row].as_deref();
        if (role == BlockRole::Through) != block.is_none() {
            return Err(fail(CONTEXTS, row, Violation::ContextFacts));
        }
        let previous = contexts.previous_stop[row];
        let next = contexts.next_stop[row];
        if (role == BlockRole::Incoming && next.is_some())
            || (role == BlockRole::Outgoing && previous.is_some())
            || (role == BlockRole::Interior && (previous.is_some() || next.is_some()))
        {
            return Err(fail(CONTEXTS, row, Violation::ContextNeighbours));
        }
        let expected = context_id(
            stop,
            &contexts.mode[row],
            &contexts.line_id[row],
            contexts.route_distinction[row],
            contexts.direction[row],
            &contexts.pattern_hash[row],
            contexts.pattern_position[row],
            role.as_str(),
            contexts.authored_post_key[row].as_deref(),
        );
        if *contexts.context_id[row] != *expected {
            return Err(fail(CONTEXTS, row, Violation::ContextIdentity));
        }
        if let Some(block) = block {
            let position = contexts.pattern_position[row];
            let entry = blocks.entry(block.into()).or_insert_with(|| Block {
                line: contexts.line_id[row].clone(),
                distinction: contexts.route_distinction[row],
                pattern: contexts.pattern_hash[row].clone(),
                first_row: row,
                minimum: position,
                maximum: position,
            });
            if entry.line != contexts.line_id[row]
                || entry.distinction != contexts.route_distinction[row]
                || entry.pattern != contexts.pattern_hash[row]
            {
                return Err(fail(CONTEXTS, row, Violation::BlockIdentity));
            }
            entry.minimum = entry.minimum.min(position);
            entry.maximum = entry.maximum.max(position);
        }

        // Variants of this context, ranks contiguous from zero.
        if variant >= variants.len() || variants.context[variant] as usize != row {
            return Err(fail(CONTEXTS, row, Violation::MissingVariants));
        }
        let mut expected_rank = 0;
        let mut last_cost: Option<f64> = None;
        let mut baseline: Option<usize> = None;
        while variant < variants.len() && variants.context[variant] as usize == row {
            check_variant(pack, variant, expected_rank, &mut last_cost)?;
            if expected_rank == 0 {
                baseline = Some(variant);
            }
            // Attachments: exactly the stop's route points, in order.
            let unavailable = variants.routing_availability[variant] == Availability::Unavailable;
            for point in first_point..end_point {
                if attachment >= attachments.len()
                    || attachments.variant[attachment] as usize != variant
                {
                    return Err(fail(ATTACHMENTS, attachment, Violation::AttachmentCoverage));
                }
                if attachments.route_point[attachment] as usize != point {
                    return Err(fail(ATTACHMENTS, attachment, Violation::AttachmentOrder));
                }
                check_attachment(pack, attachment, unavailable)?;
                attachment += 1;
            }
            if attachment < attachments.len() && attachments.variant[attachment] as usize == variant
            {
                return Err(fail(ATTACHMENTS, attachment, Violation::AttachmentOrder));
            }
            expected_rank += 1;
            variant += 1;
        }
        let Some(baseline) = baseline else {
            return Err(fail(CONTEXTS, row, Violation::MissingVariants));
        };
        let family = movement_family_id(
            stop,
            &contexts.mode[row],
            variants.ingress_thread_id[baseline].as_deref(),
            variants.egress_thread_id[baseline].as_deref(),
            role.as_str(),
        );
        if *contexts.movement_family_id[row] != *family {
            return Err(fail(CONTEXTS, row, Violation::MovementFamily));
        }
    }
    check_blocks(&mut blocks, &mut seen_blocks)?;
    if variant < variants.len() {
        return Err(fail(VARIANTS, variant, Violation::VariantContext));
    }
    if attachment < attachments.len() {
        return Err(fail(ATTACHMENTS, attachment, Violation::AttachmentVariant));
    }
    Ok(())
}

fn check_variant(
    pack: &Pack,
    row: usize,
    expected_rank: i32,
    last_cost: &mut Option<f64>,
) -> Result<(), PackError> {
    let v = &pack.corridor_variants;
    let rank = v.variant_rank[row];
    if rank != expected_rank || rank >= 3 {
        return Err(fail(VARIANTS, row, Violation::VariantRank));
    }
    let counts = [
        v.directed_edge_count[row],
        v.repeated_directed_edge_count[row],
        v.service_edge_count[row],
        v.restricted_access_edge_count[row],
    ];
    if counts.iter().any(|&count| count < 0) || counts[1..].iter().any(|&count| count > counts[0]) {
        return Err(fail(VARIANTS, row, Violation::VariantCounts));
    }
    let optional = [
        v.corridor_id[row].is_some(),
        v.ingress_thread_id[row].is_some(),
        v.egress_thread_id[row].is_some(),
    ];
    let numeric = [
        v.absolute_cost_metres[row],
        v.relative_cost_metres[row],
        v.relative_cost_fraction[row],
        v.path_length_metres[row],
        v.access_penalty_metres[row],
    ];
    let failure = v.invariant_failure_reason[row].is_some();
    match v.routing_availability[row] {
        Availability::Unavailable => {
            if rank != 0
                || optional.iter().any(|&set| set)
                || numeric.iter().any(Option::is_some)
                || counts.iter().any(|&count| count != 0)
                || !failure
            {
                return Err(fail(VARIANTS, row, Violation::UnavailableVariant));
            }
        }
        Availability::Available => {
            if !optional.iter().all(|&set| set)
                || failure
                || numeric
                    .iter()
                    .any(|value| !value.is_some_and(|value| value.is_finite() && value >= 0.0))
            {
                return Err(fail(VARIANTS, row, Violation::AvailableVariant));
            }
            let cost = numeric[0].unwrap_or(0.0);
            if last_cost.is_some_and(|previous| cost < previous) {
                return Err(fail(VARIANTS, row, Violation::VariantCostOrder));
            }
            *last_cost = Some(cost);
            if rank == 0 && (numeric[1] != Some(0.0) || numeric[2] != Some(0.0)) {
                return Err(fail(VARIANTS, row, Violation::BaselineCost));
            }
        }
    }
    Ok(())
}

fn check_attachment(pack: &Pack, row: usize, variant_unavailable: bool) -> Result<(), PackError> {
    let a = &pack.route_point_evidence;
    let numeric = a.numeric(row);
    let availability = a.routing_availability[row];
    if variant_unavailable && availability != Availability::Unavailable {
        return Err(fail(ATTACHMENTS, row, Violation::AttachmentOfUnavailable));
    }
    match availability {
        Availability::Unavailable => {
            if numeric.iter().any(Option::is_some)
                || a.snap_edge_id[row].is_some()
                || a.snap_way_id[row].is_some()
                || a.corridor_face_id[row].is_some()
                || a.invariant_failure_reason[row].is_none()
            {
                return Err(fail(ATTACHMENTS, row, Violation::UnavailableAttachment));
            }
        }
        Availability::Available => {
            let all_finite = numeric
                .iter()
                .all(|value| value.is_some_and(f64::is_finite));
            if !all_finite
                || !a.snap_edge_id[row].is_some_and(|edge| edge >= 0)
                || !a.snap_way_id[row].is_some_and(|way| way >= 0)
                || a.corridor_face_id[row].is_none()
                || a.invariant_failure_reason[row].is_some()
            {
                return Err(fail(ATTACHMENTS, row, Violation::AvailableAttachment));
            }
            let value = |index: usize| numeric[index].unwrap_or(0.0);
            if value(0) < 0.0
                || value(4) < 0.0
                || value(5) < 0.0
                || value(10) < 0.0
                || !(0.0..=1.0).contains(&value(1))
                || !(-180.0..=180.0).contains(&value(2))
                || !(-90.0..=90.0).contains(&value(3))
            {
                return Err(fail(ATTACHMENTS, row, Violation::AttachmentRange));
            }
        }
    }
    Ok(())
}
