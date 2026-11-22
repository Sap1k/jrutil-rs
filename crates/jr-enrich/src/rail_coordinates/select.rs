//! SR70 authority and OSM selection (CZPTT-R094–R098).

use std::cell::OnceCell;

use jr_geo::names::{Prepared, qualifiers_compatible, similarity_at_least};
use jr_model::Coord;
use jr_model::geo::distance_m;
use rayon::prelude::*;

use super::Policy;
use super::evidence::Evidence;
use super::network::{Network, PathCall};

/// The candidate groups in authority order (CZPTT-R094).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Method {
    Plc,
    ExactName,
    RailwayName,
    FuzzyName,
}

impl Method {
    const ALL: [Method; 4] = [
        Method::Plc,
        Method::ExactName,
        Method::RailwayName,
        Method::FuzzyName,
    ];

    /// The F# `coordinate_match_method`.
    pub fn name(self) -> &'static str {
        match self {
            Method::Plc => "ref_eu_plc",
            Method::ExactName => "normalized_exact_name",
            Method::RailwayName => "normalized_railway_name",
            Method::FuzzyName => "normalized_fuzzy_name",
        }
    }
}

/// A real (not estimated) position.
pub(super) enum Resolved {
    Sr70(Coord),
    Osm {
        coord: Coord,
        method: Method,
        candidate: usize,
        /// Every plausible candidate of the group, when there was more than one (CZPTT-R096).
        ranked: Option<Vec<usize>>,
    },
}

impl Resolved {
    pub fn coord(&self) -> Coord {
        match self {
            Resolved::Sr70(coord) | Resolved::Osm { coord, .. } => *coord,
        }
    }
}

/// What the latest evaluation of a point vetoed (CZPTT-R095).
#[derive(Default)]
pub(super) struct Attempt {
    pub rejected: Vec<(Method, usize)>,
}

pub(super) struct Resolution {
    /// Per point.
    pub resolved: Vec<Option<Resolved>>,
    /// Per point: its final OSM evaluation; `None` for SR70 points (CZPTT-R098, B13).
    pub attempts: Vec<Option<Attempt>>,
}

pub(super) struct Resolver<'a> {
    network: &'a Network<'a>,
    evidence: &'a Evidence,
    policy: &'a Policy,
    /// Per point, its name evidence (CZPTT-R089).
    names: Vec<Prepared>,
    /// Per point, its fuzzy group, computed when first needed.
    fuzzy: Vec<OnceCell<Vec<usize>>>,
}

impl<'a> Resolver<'a> {
    pub fn new(network: &'a Network<'a>, evidence: &'a Evidence, policy: &'a Policy) -> Self {
        Self {
            network,
            evidence,
            policy,
            names: network
                .points
                .iter()
                .map(|point| Prepared::new(point.name, &policy.names))
                .collect(),
            fuzzy: network.points.iter().map(|_| OnceCell::new()).collect(),
        }
    }

    /// CZPTT-R097, then R098.
    pub fn resolve(&self) -> Resolution {
        let points = &self.network.points;
        let mut resolved: Vec<Option<Resolved>> = points
            .iter()
            .map(|point| {
                self.evidence
                    .sr70_coord(point.country, point.code)
                    .map(Resolved::Sr70)
            })
            .collect();
        let mut attempts: Vec<Option<Attempt>> = points.iter().map(|_| None).collect();
        for point in 0..points.len() {
            if resolved[point].is_none() {
                let (pick, attempt) = self.select(point, &resolved);
                resolved[point] = pick;
                attempts[point] = Some(attempt);
            }
        }
        for _ in 0..self.policy.resolution.reevaluation_passes {
            for point in 0..points.len() {
                if matches!(resolved[point], Some(Resolved::Sr70(_))) {
                    continue;
                }
                resolved[point] = None;
                let (pick, attempt) = self.select(point, &resolved);
                resolved[point] = pick;
                attempts[point] = Some(attempt);
            }
        }
        Resolution { resolved, attempts }
    }

    /// CZPTT-R094–R096 for one point, against the positions in `resolved`.
    fn select(&self, point: usize, resolved: &[Option<Resolved>]) -> (Option<Resolved>, Attempt) {
        let mut attempt = Attempt::default();
        for method in Method::ALL {
            let mut plausible = Vec::new();
            for &candidate in self.group(point, method) {
                let coord = self.evidence.candidates[candidate].coord;
                if self.plausible(point, coord, resolved) {
                    plausible.push(candidate);
                } else if !attempt.rejected.iter().any(|&(_, seen)| seen == candidate) {
                    // A node found again by a later group is the same rejection.
                    attempt.rejected.push((method, candidate));
                }
            }
            let (candidate, ranked) = match plausible.len() {
                0 => continue,
                1 => (plausible[0], None),
                _ => (self.rank(point, &plausible, resolved), Some(plausible)),
            };
            let pick = Resolved::Osm {
                coord: self.evidence.candidates[candidate].coord,
                method,
                candidate,
                ranked,
            };
            return (Some(pick), attempt);
        }
        (None, attempt)
    }

    /// The point's candidates of one group, ascending and distinct (CZPTT-R094).
    fn group(&self, point: usize, method: Method) -> &[usize] {
        let row = &self.network.points[point];
        let name = &self.names[point];
        let indexed = |index: &'a std::collections::HashMap<Box<str>, Vec<usize>>, key: &str| {
            if key.is_empty() {
                &[][..]
            } else {
                index.get(key).map_or(&[][..], Vec::as_slice)
            }
        };
        match method {
            Method::Plc => self.evidence.plc_candidates(row.country, row.code),
            Method::ExactName => indexed(&self.evidence.by_name, name.full()),
            Method::RailwayName => indexed(&self.evidence.by_core, name.core()),
            Method::FuzzyName => self.fuzzy[point].get_or_init(|| self.fuzzy_group(point)),
        }
    }

    /// Candidates with a qualifier-compatible name at least the minimum similar, best first
    /// (then by node ID), at most the policy's number.
    fn fuzzy_group(&self, point: usize) -> Vec<usize> {
        let expected = &self.names[point];
        if expected.full().is_empty() {
            return Vec::new();
        }
        let tables = &self.policy.names;
        let minimum = self.policy.matching.fuzzy_min_similarity;
        let mut scored: Vec<(f64, usize)> = self
            .evidence
            .candidates
            .par_iter()
            .enumerate()
            .filter_map(|(index, candidate)| {
                candidate
                    .names
                    .iter()
                    .filter(|name| qualifiers_compatible(expected, name, tables))
                    .filter_map(|name| similarity_at_least(expected, name, minimum))
                    .reduce(f64::max)
                    .map(|score| (score, index))
            })
            .collect();
        scored.sort_by(|(left_score, left), (right_score, right)| {
            right_score.total_cmp(left_score).then(left.cmp(right))
        });
        scored.truncate(self.policy.matching.fuzzy_max_candidates);
        scored.into_iter().map(|(_, index)| index).collect()
    }

    /// CZPTT-R095.
    fn plausible(&self, point: usize, coord: Coord, resolved: &[Option<Resolved>]) -> bool {
        let mut evidence = false;
        for &(path, position) in &self.network.points[point].occurrences {
            let calls = &self.network.paths[path].calls;
            let current = calls[position];
            let mut edges = 0;
            let mut fits = true;
            if position > 0 {
                let previous = calls[position - 1];
                if let Some(at) = placed(previous, resolved)
                    && let Some(fit) = self.edge(at, previous, coord, current)
                {
                    edges += 1;
                    fits &= fit;
                }
            }
            if let Some(&next) = calls.get(position + 1)
                && let Some(at) = placed(next, resolved)
                && let Some(fit) = self.edge(coord, current, at, next)
            {
                edges += 1;
                fits &= fit;
            }
            if edges > 0 {
                if fits {
                    return true;
                }
                evidence = true;
            }
        }
        !evidence
    }

    /// Whether the train can get from `from` to `to` in the scheduled time; `None` when the
    /// times do not make the edge evidence.
    fn edge(&self, from: Coord, from_call: PathCall, to: Coord, to_call: PathCall) -> Option<bool> {
        let (Some(left), Some(right)) = (from_call.leaves(), to_call.reaches()) else {
            return None;
        };
        if right <= left {
            return None;
        }
        let rules = &self.policy.matching;
        let maximum = rules.corridor_slack_m
            + f64::from(right - left) / 3600.0 * rules.corridor_speed_kmh * 1000.0;
        Some(distance_m(from, to) <= maximum)
    }

    /// CZPTT-R096: the best of several plausible candidates.
    fn rank(&self, point: usize, plausible: &[usize], resolved: &[Option<Resolved>]) -> usize {
        let country = self.network.points[point].country;
        let keyed: Vec<(usize, u8, f64, usize)> = plausible
            .iter()
            .map(|&candidate| {
                let row = &self.evidence.candidates[candidate];
                let (neighbours, distance) = self.corridor_fit(point, row.coord, resolved);
                let hint = match row.country.as_deref() {
                    Some(value) if value == country => 0,
                    None => 1,
                    Some(_) => 2,
                };
                (neighbours, hint, distance, candidate)
            })
            .collect();
        keyed
            .into_iter()
            .min_by(|left, right| {
                right
                    .0
                    .cmp(&left.0)
                    .then(left.1.cmp(&right.1))
                    .then(left.2.total_cmp(&right.2))
                    .then(left.3.cmp(&right.3))
            })
            .map(|(_, _, _, candidate)| candidate)
            .expect("several plausible candidates")
    }

    /// Placed neighbour calls over all occurrences, and the total distance to them.
    fn corridor_fit(
        &self,
        point: usize,
        coord: Coord,
        resolved: &[Option<Resolved>],
    ) -> (usize, f64) {
        let mut count = 0;
        let mut total = 0.0;
        for &(path, position) in &self.network.points[point].occurrences {
            let calls = &self.network.paths[path].calls;
            let neighbours = position
                .checked_sub(1)
                .map(|previous| calls[previous])
                .into_iter()
                .chain(calls.get(position + 1).copied());
            for neighbour in neighbours {
                if let Some(at) = placed(neighbour, resolved) {
                    count += 1;
                    total += distance_m(coord, at);
                }
            }
        }
        (count, total)
    }
}

/// The real position of a call's point, if it has one yet.
fn placed(call: PathCall, resolved: &[Option<Resolved>]) -> Option<Coord> {
    call.point
        .and_then(|point| resolved[point].as_ref())
        .map(Resolved::coord)
}
