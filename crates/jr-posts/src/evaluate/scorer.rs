//! The learned two-stage scorer (POST-R103, R104, R107, R108; F# `JdfPostScorer`, Python
//! `post_scorer`). Two conditional-logit stages with a "no correct candidate" option; stage 2
//! adds what related contexts at the stop believe. All reductions are sequential in a fixed
//! order so the result does not depend on anything but the input.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use jr_formats::post_policy;

use super::policy::{LearnedConstants, PolicyError};

/// A stage-1 feature the evaluator computes (POST-R106; frozen names of section 2.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Feature {
    Side,
    Alignment,
    Proximity,
    CorridorDistance,
    RoutedFit,
    RoutedExcessMetresCapped,
    ExcessMinusContextMin,
    Eligible,
    HasLocalRef,
    IsBay,
    PostSupport,
    PostSupportDeficit,
    IsTram,
    IsTrolleybus,
    AnchorPreviousMissing,
    AnchorNextMissing,
    ServiceEdgeCount,
}

impl Feature {
    pub fn parse(name: &str) -> Option<Feature> {
        Some(match name {
            "side" => Feature::Side,
            "alignment" => Feature::Alignment,
            "proximity" => Feature::Proximity,
            "corridor_distance" => Feature::CorridorDistance,
            "routed_fit" => Feature::RoutedFit,
            "routed_excess_metres_capped" => Feature::RoutedExcessMetresCapped,
            "excess_minus_context_min" => Feature::ExcessMinusContextMin,
            "eligible" => Feature::Eligible,
            "has_local_ref" => Feature::HasLocalRef,
            "is_bay" => Feature::IsBay,
            "post_support" => Feature::PostSupport,
            "post_support_deficit" => Feature::PostSupportDeficit,
            "is_tram" => Feature::IsTram,
            "is_trolleybus" => Feature::IsTrolleybus,
            "anchor_previous_missing" => Feature::AnchorPreviousMissing,
            "anchor_next_missing" => Feature::AnchorNextMissing,
            "service_edge_count" => Feature::ServiceEdgeCount,
            _ => return None,
        })
    }
}

/// One stage's parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct Stage {
    pub mean: Vec<f64>,
    pub scale: Vec<f64>,
    pub weights: Vec<f64>,
    pub null_utility: f64,
}

impl Stage {
    fn new(stage: &post_policy::Stage) -> Stage {
        Stage {
            mean: stage.mean.clone(),
            scale: stage.scale.clone(),
            weights: stage.weights.clone(),
            null_utility: stage.null_utility,
        }
    }
}

/// A model ready to score: stage-1 features resolved to what the evaluator computes.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub features: Vec<Feature>,
    pub stage1: Stage,
    pub stage2: Stage,
    pub simple: f64,
    pub complex: f64,
    pub area: Option<f64>,
}

impl Model {
    /// Fails when a stage-1 feature is not one the evaluator computes (F#: "Learned scorer
    /// feature … is not available in JrUtil").
    pub fn new(model: &post_policy::Model) -> Result<Model, PolicyError> {
        let features = model
            .stage1
            .features
            .iter()
            .map(|name| {
                Feature::parse(name).ok_or(PolicyError {
                    field: "scorer.model.stage1.features",
                    message: "names a feature the evaluator does not compute",
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Model::from_parts(model, features))
    }

    /// The model with its stage-1 features taken as given, in order, whatever their names (the
    /// Python parity fixture scores feature vectors directly).
    pub fn from_parts(model: &post_policy::Model, features: Vec<Feature>) -> Model {
        Model {
            features,
            stage1: Stage::new(&model.stage1),
            stage2: Stage::new(&model.stage2),
            simple: model.thresholds.simple,
            complex: model.thresholds.complex,
            area: model.thresholds.area,
        }
    }
}

/// One candidate of one context, with the stage-1 feature values in model order.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub context: String,
    pub hypothesis: String,
    pub post: String,
    pub area: String,
    pub is_platform: bool,
    pub features: Vec<f64>,
}

/// The movement of a context: its neighbour stops (`None` when missing) and the stop's post
/// count.
#[derive(Clone, Debug, PartialEq)]
pub struct ContextKey {
    pub context: String,
    pub previous: Option<String>,
    pub next: Option<String>,
    pub stop_post_count: usize,
}

/// A learned resolution (POST-R108).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    Physical,
    Area,
    Abstain,
}

/// A learned decision for one context.
#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub context: String,
    pub resolution: Resolution,
    pub hypothesis: Option<String>,
    pub post_probability: f64,
    pub area_probability: f64,
}

/// `y = lat × m_lat`, `x = lon × m_lon × cos(mean latitude)` and the pairwise distances.
fn distances(latitudes: &[f64], longitudes: &[f64], learned: &LearnedConstants) -> Vec<Vec<f64>> {
    let count = latitudes.len();
    let mean = if count == 0 {
        learned.empty_stop_latitude
    } else {
        latitudes.iter().fold(0.0, |sum, value| sum + value) / count as f64
    };
    let cosine = (mean * std::f64::consts::PI / 180.0).cos();
    let y: Vec<f64> = latitudes.iter().map(|value| value * 110_540.0).collect();
    let x: Vec<f64> = longitudes
        .iter()
        .map(|value| value * 111_320.0 * cosine)
        .collect();
    (0..count)
        .map(|i| {
            (0..count)
                .map(|j| {
                    let dx = x[i] - x[j];
                    let dy = y[i] - y[j];
                    (dx * dx + dy * dy).sqrt()
                })
                .collect()
        })
        .collect()
}

/// Closest-first complete-linkage clustering; a cluster index per input. Each round merges
/// the mergeable pair with the smallest diameter (then, with `member_tie`, the smallest member
/// index of the union), the earlier pair on equality; the union takes the left place.
fn complete_linkage(
    distance: &[Vec<f64>],
    maximum: f64,
    mergeable: impl Fn(&BTreeSet<usize>, &BTreeSet<usize>) -> bool,
    member_tie: bool,
) -> Vec<usize> {
    let mut clusters: Vec<BTreeSet<usize>> = (0..distance.len())
        .map(|index| BTreeSet::from([index]))
        .collect();
    loop {
        let mut best: Option<(f64, usize, usize, usize)> = None;
        for left in 0..clusters.len() {
            for right in left + 1..clusters.len() {
                if !mergeable(&clusters[left], &clusters[right]) {
                    continue;
                }
                let mut diameter = 0.0;
                for &i in &clusters[left] {
                    for &j in &clusters[right] {
                        if distance[i][j] > diameter {
                            diameter = distance[i][j];
                        }
                    }
                }
                if diameter <= maximum {
                    let tie = if member_tie {
                        clusters[left]
                            .first()
                            .copied()
                            .unwrap_or(0)
                            .min(clusters[right].first().copied().unwrap_or(0))
                    } else {
                        0
                    };
                    let better = match best {
                        None => true,
                        Some((best_diameter, best_tie, _, _)) => {
                            diameter < best_diameter
                                || (diameter == best_diameter && tie < best_tie)
                        }
                    };
                    if better {
                        best = Some((diameter, tie, left, right));
                    }
                }
            }
        }
        let Some((_, _, left, right)) = best else {
            break;
        };
        let merged = clusters.remove(right);
        clusters[left].extend(merged);
    }
    let mut assignment = vec![0; distance.len()];
    for (index, cluster) in clusters.iter().enumerate() {
        for &member in cluster {
            assignment[member] = index;
        }
    }
    assignment
}

fn union<'a>(members: &BTreeSet<usize>, sets: &'a [BTreeSet<&'a str>]) -> BTreeSet<&'a str> {
    members
        .iter()
        .flat_map(|&index| sets[index].iter().copied())
        .collect()
}

/// POST-R103: physical posts at one stop. A post never holds one source twice, never two
/// distinct `local_ref` values, and is at most 25 m wide. Inputs in hypothesis-id order.
pub fn cluster_posts(
    latitudes: &[f64],
    longitudes: &[f64],
    sources: &[Vec<String>],
    local_refs: &[Option<String>],
    learned: &LearnedConstants,
) -> Vec<usize> {
    let source_sets: Vec<BTreeSet<&str>> = sources
        .iter()
        .map(|sources| sources.iter().map(String::as_str).collect())
        .collect();
    let refs: Vec<BTreeSet<&str>> = local_refs
        .iter()
        .map(|value| value.as_deref().into_iter().collect())
        .collect();
    complete_linkage(
        &distances(latitudes, longitudes, learned),
        learned.maximum_post_diameter_m,
        |left, right| {
            union(left, &source_sets).is_disjoint(&union(right, &source_sets))
                && union(left, &refs).union(&union(right, &refs)).count() <= 1
        },
        true,
    )
}

/// POST-R104: areas of posts within 40 m that never mix tram and road posts (`any` joins
/// either). Inputs are post centres in post-id string order.
pub fn cluster_areas(
    latitudes: &[f64],
    longitudes: &[f64],
    classes: &[&str],
    learned: &LearnedConstants,
) -> Vec<usize> {
    let sets: Vec<BTreeSet<&str>> = classes
        .iter()
        .map(|&class| {
            if class == "any" {
                BTreeSet::new()
            } else {
                BTreeSet::from([class])
            }
        })
        .collect();
    complete_linkage(
        &distances(latitudes, longitudes, learned),
        learned.maximum_area_diameter_m,
        |left, right| union(left, &sets).union(&union(right, &sets)).count() <= 1,
        false,
    )
}

/// Softmax over one context's candidates plus the null option.
fn softmax(stage: &Stage, rows: &[&[f64]]) -> Vec<f64> {
    let utilities: Vec<f64> = rows
        .iter()
        .map(|values| {
            let mut total = 0.0;
            for (index, value) in values.iter().enumerate() {
                total += (value - stage.mean[index]) / stage.scale[index] * stage.weights[index];
            }
            total
        })
        .collect();
    let maximum = utilities
        .iter()
        .fold(stage.null_utility, |maximum, &value| {
            if maximum < value { value } else { maximum }
        });
    let exponentials: Vec<f64> = utilities
        .iter()
        .map(|value| (value - maximum).exp())
        .collect();
    let mut denominator = (stage.null_utility - maximum).exp();
    for value in &exponentials {
        denominator += value;
    }
    exponentials
        .iter()
        .map(|value| value / denominator)
        .collect()
}

type Movement<'a> = (Option<&'a str>, Option<&'a str>);

/// POST-R107: the collective features of every (context, post) from stage-1 post masses, in
/// [`post_policy::COLLECTIVE_FEATURES`] order.
fn collective_features<'a>(
    contexts: &'a [ContextKey],
    masses: &BTreeMap<(&'a str, &'a str), f64>,
) -> HashMap<(&'a str, &'a str), [f64; 6]> {
    let key = |context: &'a ContextKey| -> Movement<'a> {
        (context.previous.as_deref(), context.next.as_deref())
    };
    let by_context: HashMap<&str, &ContextKey> = contexts
        .iter()
        .map(|context| (context.context.as_str(), context))
        .collect();
    let mut by_movement: HashMap<Movement, Vec<&str>> = HashMap::new();
    let mut by_next: HashMap<Option<&str>, Vec<&str>> = HashMap::new();
    let mut by_previous: HashMap<Option<&str>, Vec<&str>> = HashMap::new();
    for context in contexts {
        let id = context.context.as_str();
        by_movement.entry(key(context)).or_default().push(id);
        by_next.entry(context.next.as_deref()).or_default().push(id);
        by_previous
            .entry(context.previous.as_deref())
            .or_default()
            .push(id);
    }
    for members in by_movement
        .values_mut()
        .chain(by_next.values_mut())
        .chain(by_previous.values_mut())
    {
        members.sort_unstable();
    }
    let total = |members: &[&str], post: &str| -> f64 {
        let mut sum = 0.0;
        for &context in members {
            if let Some(mass) = masses.get(&(context, post)) {
                sum += mass;
            }
        }
        sum
    };
    let shared = |groups: &HashMap<Option<&str>, Vec<&str>>,
                  neighbour: Option<&str>,
                  post: &str,
                  own: f64| {
        let Some(members) = neighbour.and_then(|_| groups.get(&neighbour)) else {
            return 0.0;
        };
        if members.len() > 1 {
            (total(members, post) - own) / (members.len() - 1) as f64
        } else {
            0.0
        }
    };
    let mut result = HashMap::new();
    for (&(context, post), &own) in masses {
        let Some(key_context) = by_context.get(context) else {
            continue;
        };
        let (previous, next) = key(key_context);
        let members = &by_movement[&(previous, next)];
        let (same_movement, has_same) = if members.len() > 1 {
            (
                (total(members, post) - own) / (members.len() - 1) as f64,
                1.0,
            )
        } else {
            (0.0, 0.0)
        };
        let (reverse, has_reverse) = if previous == next {
            (0.0, 0.0)
        } else {
            match by_movement.get(&(next, previous)) {
                Some(reverse) => (total(reverse, post) / reverse.len() as f64, 1.0),
                None => (0.0, 0.0),
            }
        };
        result.insert(
            (context, post),
            [
                reverse,
                has_reverse,
                same_movement,
                has_same,
                shared(&by_next, next, post, own),
                shared(&by_previous, previous, post, own),
            ],
        );
    }
    result
}

/// Per-(context, key) sums of probabilities in candidate order.
fn mass<'a>(
    candidates: &'a [Candidate],
    probabilities: &[f64],
    key: impl Fn(&'a Candidate) -> &'a str,
) -> BTreeMap<(&'a str, &'a str), f64> {
    let mut sums = BTreeMap::new();
    for (candidate, probability) in candidates.iter().zip(probabilities) {
        *sums
            .entry((candidate.context.as_str(), key(candidate)))
            .or_insert(0.0) += probability;
    }
    sums
}

/// Ranges of `candidates` (sorted by context) per context.
fn context_ranges(candidates: &[Candidate]) -> Vec<std::ops::Range<usize>> {
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    for (index, candidate) in candidates.iter().enumerate() {
        match ranges.last_mut() {
            Some(range) if candidates[range.start].context == candidate.context => {
                range.end = index + 1;
            }
            _ => ranges.push(index..index + 1),
        }
    }
    ranges
}

/// Stage-1 and stage-2 probabilities of every candidate of one stop, with the candidates
/// sorted by (context id, hypothesis id).
pub fn stop_probabilities(
    model: &Model,
    contexts: &[ContextKey],
    mut candidates: Vec<Candidate>,
) -> (Vec<Candidate>, Vec<f64>, Vec<f64>) {
    candidates.sort_by(|left, right| {
        (&left.context, &left.hypothesis).cmp(&(&right.context, &right.hypothesis))
    });
    let ranges = context_ranges(&candidates);
    let mut stage1 = Vec::with_capacity(candidates.len());
    for range in &ranges {
        let rows: Vec<&[f64]> = candidates[range.clone()]
            .iter()
            .map(|candidate| candidate.features.as_slice())
            .collect();
        stage1.extend(softmax(&model.stage1, &rows));
    }
    let first = mass(&candidates, &stage1, |candidate| &candidate.post);
    let collective = collective_features(contexts, &first);
    let inputs: Vec<Vec<f64>> = candidates
        .iter()
        .map(|candidate| {
            let mut values = candidate.features.clone();
            let extra = collective
                .get(&(candidate.context.as_str(), candidate.post.as_str()))
                .copied()
                .unwrap_or_default();
            values.extend(extra);
            values
        })
        .collect();
    let mut stage2 = Vec::with_capacity(candidates.len());
    for range in &ranges {
        let rows: Vec<&[f64]> = inputs[range.clone()].iter().map(Vec::as_slice).collect();
        stage2.extend(softmax(&model.stage2, &rows));
    }
    (candidates, stage1, stage2)
}

/// POST-R108: scores and decides every context of one stop, in context-id order.
pub fn score_stop(
    model: &Model,
    contexts: &[ContextKey],
    candidates: Vec<Candidate>,
    learned: &LearnedConstants,
) -> Vec<Decision> {
    let (candidates, _, stage2) = stop_probabilities(model, contexts, candidates);
    let posts = mass(&candidates, &stage2, |candidate| &candidate.post);
    let areas = mass(&candidates, &stage2, |candidate| &candidate.area);
    let post_counts: HashMap<&str, usize> = contexts
        .iter()
        .map(|context| (context.context.as_str(), context.stop_post_count))
        .collect();
    let post = |candidate: &Candidate| {
        posts[&(candidate.context.as_str(), candidate.post.as_str())].min(1.0)
    };
    let area =
        |candidate: &Candidate| areas[&(candidate.context.as_str(), candidate.area.as_str())];
    let mut decisions = Vec::new();
    for range in context_ranges(&candidates) {
        let rows: Vec<(&Candidate, f64)> = candidates[range.clone()]
            .iter()
            .zip(&stage2[range])
            .map(|(candidate, &probability)| (candidate, probability))
            .collect();
        let best = |key: &dyn Fn(&Candidate) -> (f64, f64)| {
            rows.iter()
                .copied()
                .min_by(|&(left, left_p), &(right, right_p)| {
                    key(right)
                        .partial_cmp(&key(left))
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then(right.is_platform.cmp(&left.is_platform))
                        .then(
                            right_p
                                .partial_cmp(&left_p)
                                .unwrap_or(std::cmp::Ordering::Equal),
                        )
                        .then(left.hypothesis.cmp(&right.hypothesis))
                })
                .map(|(candidate, _)| candidate)
        };
        let (Some(top), Some(area_top)) = (
            best(&|candidate| (post(candidate), 0.0)),
            best(&|candidate| (area(candidate), post(candidate))),
        ) else {
            continue;
        };
        let complex = post_counts
            .get(top.context.as_str())
            .is_some_and(|&count| count >= learned.complex_stop_posts);
        let threshold = if complex { model.complex } else { model.simple };
        let decision = if post(top) >= threshold {
            Decision {
                context: top.context.clone(),
                resolution: Resolution::Physical,
                hypothesis: Some(top.hypothesis.clone()),
                post_probability: post(top),
                area_probability: area(top),
            }
        } else if model
            .area
            .is_some_and(|threshold| area(area_top) >= threshold)
        {
            Decision {
                context: area_top.context.clone(),
                resolution: Resolution::Area,
                hypothesis: Some(area_top.hypothesis.clone()),
                post_probability: post(area_top),
                area_probability: area(area_top),
            }
        } else {
            Decision {
                context: top.context.clone(),
                resolution: Resolution::Abstain,
                hypothesis: None,
                post_probability: post(top),
                area_probability: area(top),
            }
        };
        decisions.push(decision);
    }
    decisions
}
