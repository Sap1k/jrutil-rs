//! POST-R070–R073: route points of a stop merged into hypotheses, and the evidence rows
//! aggregated per hypothesis (F# `consolidateReplayRows`).

use std::collections::HashMap;

use jr_formats::post_evidence::ids::sha256_hex;

use super::geometry::{angle_difference, equirect, evidence_score, routed_fit_of};
use super::policy::{Constants, Settings};
use super::replay::{Point, Row};

/// A consolidated hypothesis of one stop.
#[derive(Clone, Debug)]
pub(crate) struct Hypothesis<'a> {
    /// R072: `sha("{stop}|{observation ids joined |}")`.
    pub id: String,
    /// Member points (indexes into the stop's points), in route-point-id order.
    pub members: Vec<usize>,
    /// The medoid point.
    pub medoid: usize,
    pub current: bool,
    pub obsolete: bool,
    pub support: f64,
    /// Members' explicit modes, distinct, first seen first.
    pub explicit_modes: Vec<&'a str>,
    pub denied_modes: Vec<&'a str>,
    /// Distinct member observation IDs, ordinal order.
    pub observations: Vec<&'a str>,
}

/// The stop's hypotheses (sorted by id) and the aggregated rows (candidate = hypothesis
/// index), plus the number of merges made.
pub(crate) struct Consolidated<'a> {
    pub hypotheses: Vec<Hypothesis<'a>>,
    pub rows: Vec<Row<'a>>,
    pub merges: usize,
}

struct Pairs {
    distance: Vec<f64>,
    dx: Vec<f64>,
    dy: Vec<f64>,
    count: usize,
}

impl Pairs {
    fn at(&self, left: usize, right: usize) -> usize {
        left * self.count + right
    }
}

/// R070: whether `left` and `right` may merge, given the rows of each per (context, rank).
#[allow(clippy::too_many_arguments)]
fn compatible(
    settings: &Settings,
    constants: &Constants,
    points: &[Point],
    pairs: &Pairs,
    by_point: &[HashMap<(u32, i32), usize>],
    rows: &[Row],
    left: usize,
    right: usize,
) -> bool {
    let index = pairs.at(left, right);
    if pairs.distance[index] > settings.maximum_diameter_metres {
        return false;
    }
    let (l, r) = (&points[left], &points[right]);
    if (l.current != r.current || l.obsolete != r.obsolete) && (l.obsolete || r.obsolete) {
        return false;
    }
    let (dx, dy) = (pairs.dx[index], pairs.dy[index]);
    let score = |row: &Row| {
        evidence_score(
            settings,
            constants,
            row.corridor_distance,
            row.offset,
            row.corridor_heading,
            row.attachment_heading,
            routed_fit_of(settings, row.excess),
        )
    };
    let tolerance = settings.opposite_side_tolerance_metres;
    let mut distinguished = 0;
    for (key, &left_row) in &by_point[left] {
        let Some(&right_row) = by_point[right].get(key) else {
            continue;
        };
        let (lr, rr) = (&rows[left_row], &rows[right_row]);
        if let (Some(a), Some(b)) = (lr.face, rr.face)
            && a != b
        {
            return false;
        }
        if let (Some(a), Some(b)) = (lr.offset, rr.offset)
            && a.abs() > tolerance
            && b.abs() > tolerance
            && sign(a) != sign(b)
        {
            return false;
        }
        if let (Some(a), Some(b)) = (lr.corridor_heading, rr.corridor_heading) {
            if angle_difference(a, b) > settings.maximum_heading_difference_degrees {
                return false;
            }
            let radians = a * std::f64::consts::PI / 180.0;
            let chainage = (dx * radians.sin() + dy * radians.cos()).abs();
            if chainage > settings.maximum_chainage_difference_metres {
                return false;
            }
        }
        let (a, b) = (score(lr), score(rr));
        if a.max(b) >= settings.distinguished_context_minimum_score
            && (a - b).abs() >= settings.distinguished_context_minimum_margin
        {
            distinguished += 1;
        }
    }
    distinguished < settings.distinguished_context_minimum_count
}

/// .NET `Math.Sign`.
fn sign(value: f64) -> i32 {
    if value > 0.0 {
        1
    } else if value < 0.0 {
        -1
    } else {
        0
    }
}

/// R070–R073 over one stop's points (route-point-id order) and rows.
pub(crate) fn consolidate<'a>(
    settings: &Settings,
    constants: &Constants,
    stop: u64,
    points: &[Point<'a>],
    rows: &[Row<'a>],
) -> Consolidated<'a> {
    let count = points.len();
    let mut pairs = Pairs {
        distance: vec![0.0; count * count],
        dx: vec![0.0; count * count],
        dy: vec![0.0; count * count],
        count,
    };
    for left in 0..count {
        for right in 0..count {
            let (distance, dx, dy) = equirect(
                constants,
                points[left].latitude,
                points[left].longitude,
                points[right].latitude,
                points[right].longitude,
            );
            let index = pairs.at(left, right);
            pairs.distance[index] = distance;
            pairs.dx[index] = dx;
            pairs.dy[index] = dy;
        }
    }
    let mut by_point: Vec<HashMap<(u32, i32), usize>> = vec![HashMap::new(); count];
    for (index, row) in rows.iter().enumerate() {
        by_point[row.candidate].insert((row.context, row.rank), index);
    }
    // Compatibility of an ordered pair, computed once.
    let mut memo: Vec<Option<bool>> = vec![None; count * count];
    let mut is_compatible = |left: usize, right: usize| -> bool {
        let index = left * count + right;
        *memo[index].get_or_insert_with(|| {
            compatible(
                settings, constants, points, &pairs, &by_point, rows, left, right,
            )
        })
    };

    // R071: closest pair first, ties by the joined route point ids.
    let mut clusters: Vec<Vec<usize>> = (0..count).map(|index| vec![index]).collect();
    let mut merges = 0;
    loop {
        let mut best: Option<(f64, String, usize, usize)> = None;
        for left in 0..clusters.len() {
            for right in left + 1..clusters.len() {
                let all = clusters[left]
                    .iter()
                    .all(|&l| clusters[right].iter().all(|&r| is_compatible(l, r)));
                if !all {
                    continue;
                }
                let mut diameter = f64::NEG_INFINITY;
                for &l in &clusters[left] {
                    for &r in &clusters[right] {
                        let distance = pairs.distance[pairs.at(l, r)];
                        if distance > diameter {
                            diameter = distance;
                        }
                    }
                }
                let mut ids: Vec<&str> = clusters[left]
                    .iter()
                    .chain(&clusters[right])
                    .map(|&index| points[index].id)
                    .collect();
                ids.sort_unstable();
                let identity = ids.join("|");
                let better = match &best {
                    None => true,
                    Some((best_diameter, best_identity, _, _)) => {
                        diameter < *best_diameter
                            || (diameter == *best_diameter && identity < *best_identity)
                    }
                };
                if better {
                    best = Some((diameter, identity, left, right));
                }
            }
        }
        let Some((_, _, left, right)) = best else {
            break;
        };
        let merged = clusters.remove(right);
        clusters[left].extend(merged);
        clusters[left].sort_by(|&a, &b| points[a].id.cmp(points[b].id));
        merges += 1;
    }

    // R072: identities and facts.
    let mut hypotheses: Vec<Hypothesis<'a>> = clusters
        .iter()
        .map(|cluster| {
            let mut observations: Vec<&str> = cluster
                .iter()
                .flat_map(|&point| points[point].observations.iter().copied())
                .collect();
            observations.sort_unstable();
            observations.dedup();
            let identities: Vec<&str> = if observations.is_empty() {
                cluster.iter().map(|&point| points[point].id).collect()
            } else {
                observations.clone()
            };
            let id = sha256_hex(format!("{stop}|{}", identities.join("|")).as_bytes());
            let mut medoid = cluster[0];
            let mut medoid_sum = f64::INFINITY;
            for &candidate in cluster {
                let mut sum = 0.0;
                for &other in cluster {
                    sum += pairs.distance[pairs.at(candidate, other)];
                }
                if sum < medoid_sum
                    || (sum == medoid_sum && points[candidate].id < points[medoid].id)
                {
                    medoid = candidate;
                    medoid_sum = sum;
                }
            }
            let mut support = 0.0;
            for &point in cluster {
                support += points[point].support;
            }
            let distinct = |select: for<'p> fn(&'p Point<'a>) -> &'p Vec<&'a str>| {
                let mut values: Vec<&'a str> = Vec::new();
                for &point in cluster {
                    for &value in select(&points[point]) {
                        if !values.contains(&value) {
                            values.push(value);
                        }
                    }
                }
                values
            };
            Hypothesis {
                id,
                members: cluster.clone(),
                medoid,
                current: cluster.iter().any(|&point| points[point].current),
                obsolete: cluster.iter().any(|&point| points[point].obsolete),
                support,
                explicit_modes: distinct(|point| &point.explicit_modes),
                denied_modes: distinct(|point| &point.denied_modes),
                observations,
            }
        })
        .collect();
    hypotheses.sort_by(|a, b| a.id.cmp(&b.id));
    let mut hypothesis_of = vec![0usize; count];
    for (index, hypothesis) in hypotheses.iter().enumerate() {
        for &member in &hypothesis.members {
            hypothesis_of[member] = index;
        }
    }

    // R073: rows per (context, rank, hypothesis), first occurrence first.
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut group_of: HashMap<(u32, i32, usize), usize> = HashMap::new();
    for (index, row) in rows.iter().enumerate() {
        let key = (row.context, row.rank, hypothesis_of[row.candidate]);
        let group = *group_of.entry(key).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[group].push(index);
    }
    let aggregated = groups
        .iter()
        .map(|members| {
            let first = &rows[members[0]];
            let values = || members.iter().map(|&index| &rows[index]);
            let median_of = |select: fn(&Row) -> Option<f64>| {
                let present: Vec<f64> = values().filter_map(select).collect();
                (!present.is_empty()).then(|| super::geometry::median(present))
            };
            let unique_f64 = |select: fn(&Row) -> Option<f64>| {
                let mut distinct: Vec<f64> = Vec::new();
                for value in values().filter_map(select) {
                    if !distinct.contains(&value) {
                        distinct.push(value);
                    }
                }
                (distinct.len() == 1).then(|| distinct[0])
            };
            let mut faces: Vec<&str> = Vec::new();
            for face in values().filter_map(|row| row.face) {
                if !faces.contains(&face) {
                    faces.push(face);
                }
            }
            let failure = if values().any(|row| row.failure.is_none()) {
                None
            } else {
                values().filter_map(|row| row.failure).min()
            };
            Row {
                candidate: hypothesis_of[first.candidate],
                excess: median_of(|row| row.excess),
                face: (faces.len() == 1).then(|| faces[0]),
                corridor_distance: median_of(|row| row.corridor_distance),
                offset: median_of(|row| row.offset),
                corridor_heading: unique_f64(|row| row.corridor_heading),
                attachment_heading: unique_f64(|row| row.attachment_heading),
                alternative_count: values().map(|row| row.alternative_count).max().unwrap_or(0),
                cost_gap: values()
                    .filter_map(|row| row.cost_gap)
                    .min_by(|a, b| a.total_cmp(b)),
                tied_agree: values().all(|row| row.tied_agree),
                failure,
                ..first.clone()
            }
        })
        .collect();
    Consolidated {
        hypotheses,
        rows: aggregated,
        merges,
    }
}
