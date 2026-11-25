//! POST-R101–R106: the learned scorer's candidate table for one stop, reproducing F#
//! `learnedStopDecisions` (and Python `build_candidate_table`): stop-level sources leave the
//! choice set, hypotheses cluster into posts and areas, post modes gate candidates, and the
//! frozen features are computed by name.

use std::collections::{BTreeMap, HashMap, HashSet};

use jr_formats::post_evidence::Pack;

use super::consolidate::Consolidated;
use super::geometry::{components, hard_gate, mode_incompatible, routed_fit_of};
use super::policy::{Constants, Settings};
use super::replay::{Point, Row};
use super::scorer::{self, Candidate, ContextKey, Decision, Feature, Model};

/// R101: what the learned scorer reads from the observations of the whole pack.
#[derive(Clone, Debug, Default)]
pub(crate) struct ObservationFacts<'a> {
    /// Per observation ID (the last row of that ID wins, as in F#): `local_ref` and whether it
    /// is a `public_transport=platform`.
    pub tags: HashMap<&'a str, (Option<&'a str>, bool)>,
    /// Stop-level source classes.
    pub stop_level: HashSet<String>,
}

/// R101: the source class of an observation: `osm`, `external:<catalogue path>`, or the
/// first `:` field.
pub(crate) fn source_class(observation: &str) -> String {
    if observation.starts_with("osm:") {
        "osm".to_owned()
    } else if observation.starts_with("external:") {
        let second = observation.split(':').nth(1).unwrap_or("");
        format!("external:{second}")
    } else {
        observation.split(':').next().unwrap_or("").to_owned()
    }
}

/// R101 over the whole pack.
pub(crate) fn observation_facts<'a>(pack: &'a Pack, constants: &Constants) -> ObservationFacts<'a> {
    let observations = &pack.observations;
    let points = &pack.route_points;
    let mut tags = HashMap::new();
    let mut points_by: BTreeMap<(u64, String), HashSet<u32>> = BTreeMap::new();
    for row in 0..observations.len() {
        let mut local_ref = None;
        let mut platform = false;
        let raw = &*observations.raw_tags[row];
        if !raw.is_empty() {
            for pair in raw.split(';') {
                if let Some(separator) = pair.find('=')
                    && separator > 0
                {
                    let (key, value) = (&pair[..separator], &pair[separator + 1..]);
                    if key == "local_ref" && local_ref.is_none() {
                        local_ref = Some(value);
                    } else if key == "public_transport" && value == "platform" {
                        platform = true;
                    }
                }
            }
        }
        let id = &*observations.observation_id[row];
        tags.insert(id, (local_ref, platform));
        let point = observations.route_point[row];
        points_by
            .entry((points.stop[point as usize], source_class(id)))
            .or_default()
            .insert(point);
    }
    let multi_post: HashSet<u64> = points_by
        .iter()
        .filter(|((_, source), set)| source == "osm" && set.len() >= 2)
        .map(|((stop, _), _)| *stop)
        .collect();
    let mut per_source: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for ((stop, source), set) in &points_by {
        if multi_post.contains(stop) {
            let entry = per_source.entry(source).or_default();
            entry.0 += 1;
            if set.len() >= 2 {
                entry.1 += 1;
            }
        }
    }
    let learned = &constants.learned;
    let stop_level = per_source
        .into_iter()
        .filter(|&(_, (stops, multiple))| {
            stops >= learned.stop_level_minimum_stops
                && (multiple as f64) / (stops as f64) < learned.stop_level_maximum_share
        })
        .map(|(source, _)| source.to_owned())
        .collect();
    ObservationFacts { tags, stop_level }
}

/// A post-level hypothesis (R102).
struct Choice<'a> {
    hypothesis: usize,
    latitude: f64,
    longitude: f64,
    local_ref: Option<&'a str>,
    platform: bool,
    sources: Vec<String>,
    modes: Vec<&'a str>,
}

/// One scored candidate as the scorer saw it (POST-R141): what its training table holds.
#[derive(Clone, Debug, PartialEq)]
pub struct TrainingRow {
    pub context_id: String,
    pub stop: u64,
    pub hypothesis_id: String,
    pub post_id: String,
    pub area_id: String,
    pub is_platform: bool,
    pub previous_stop: Option<String>,
    pub next_stop: Option<String>,
    pub stop_post_count: usize,
    pub mode: String,
    pub latitude: f64,
    pub longitude: f64,
    /// The stage-1 features in the model's order.
    pub features: Vec<f64>,
}

/// The stop's learned decisions by context row (R102–R108).
#[allow(clippy::too_many_arguments)]
pub(crate) fn decide(
    settings: &Settings,
    constants: &Constants,
    model: &Model,
    facts: &ObservationFacts,
    pack: &Pack,
    stop: u64,
    points: &[Point],
    consolidated: &Consolidated,
    training: Option<&mut Vec<TrainingRow>>,
) -> HashMap<u32, Decision> {
    let contexts = &pack.contexts;
    let hypotheses = &consolidated.hypotheses;
    let rows = &consolidated.rows;
    if rows.is_empty() {
        return HashMap::new();
    }
    let learned = &constants.learned;
    // R102: hypotheses some post-level source knows about (already in id order).
    let choices: Vec<Choice> = hypotheses
        .iter()
        .enumerate()
        .filter_map(|(index, hypothesis)| {
            let observations: Vec<&str> = hypothesis
                .observations
                .iter()
                .copied()
                .filter(|id| !facts.stop_level.contains(&source_class(id)))
                .collect();
            if observations.is_empty() {
                return None;
            }
            let tag = |id: &str| facts.tags.get(id).copied().unwrap_or((None, false));
            let mut sources: Vec<String> = observations.iter().map(|id| source_class(id)).collect();
            sources.sort_unstable();
            sources.dedup();
            let mut modes: Vec<&str> = Vec::new();
            for &member in &hypothesis.members {
                for &mode in &points[member].explicit_modes {
                    if !mode.is_empty() && !modes.contains(&mode) {
                        modes.push(mode);
                    }
                }
            }
            let medoid = &points[hypothesis.medoid];
            Some(Choice {
                hypothesis: index,
                latitude: medoid.latitude,
                longitude: medoid.longitude,
                local_ref: observations.iter().find_map(|id| tag(id).0),
                platform: observations.iter().any(|id| tag(id).1),
                sources,
                modes,
            })
        })
        .collect();
    if choices.is_empty() {
        return HashMap::new();
    }
    // R103: physical posts.
    let latitudes: Vec<f64> = choices.iter().map(|choice| choice.latitude).collect();
    let longitudes: Vec<f64> = choices.iter().map(|choice| choice.longitude).collect();
    let post_index = scorer::cluster_posts(
        &latitudes,
        &longitudes,
        &choices
            .iter()
            .map(|choice| choice.sources.clone())
            .collect::<Vec<_>>(),
        &choices
            .iter()
            .map(|choice| choice.local_ref.map(str::to_owned))
            .collect::<Vec<_>>(),
        learned,
    );
    let post_ids: Vec<String> = post_index
        .iter()
        .map(|index| format!("{stop}:{index}"))
        .collect();
    let mut post_members: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (choice, post) in post_ids.iter().enumerate() {
        post_members.entry(post).or_default().push(choice);
    }
    let post_support: HashMap<&str, usize> = post_members
        .iter()
        .map(|(&post, members)| {
            let mut sources: Vec<&str> = members
                .iter()
                .flat_map(|&member| choices[member].sources.iter().map(String::as_str))
                .collect();
            sources.sort_unstable();
            sources.dedup();
            (post, sources.len())
        })
        .collect();
    let maximum_support = post_support.values().copied().max().unwrap_or(0);
    let post_modes: HashMap<&str, Vec<&str>> = post_members
        .iter()
        .map(|(&post, members)| {
            let mut modes: Vec<&str> = Vec::new();
            for &member in members {
                for &mode in &choices[member].modes {
                    if !modes.contains(&mode) {
                        modes.push(mode);
                    }
                }
            }
            (post, modes)
        })
        .collect();
    let numbered = |choice: &Choice| choice.local_ref.is_some() && choice.platform;
    let numbered_count = choices.iter().filter(|choice| numbered(choice)).count();
    let post_is_bay: HashMap<&str, bool> = post_members
        .iter()
        .map(|(&post, members)| {
            (
                post,
                numbered_count >= learned.minimum_numbered_bays
                    && members.iter().any(|&member| numbered(&choices[member])),
            )
        })
        .collect();
    // R104: areas over post centres in post-id string order (the BTreeMap's order).
    let ordered: Vec<&str> = post_members.keys().copied().collect();
    let centre = |post: &str| {
        let members = &post_members[post];
        let mut latitude = 0.0;
        let mut longitude = 0.0;
        for &member in members {
            latitude += choices[member].latitude;
        }
        for &member in members {
            longitude += choices[member].longitude;
        }
        (
            latitude / members.len() as f64,
            longitude / members.len() as f64,
        )
    };
    let class = |post: &str| {
        let modes = &post_modes[post];
        if modes.is_empty() {
            "any"
        } else if modes
            .iter()
            .any(|mode| matches!(*mode, "ROAD" | "BUS" | "TROLLEYBUS" | "SHARED"))
        {
            "road"
        } else {
            "tram"
        }
    };
    let centres: Vec<(f64, f64)> = ordered.iter().map(|post| centre(post)).collect();
    let area_index = scorer::cluster_areas(
        &centres.iter().map(|c| c.0).collect::<Vec<_>>(),
        &centres.iter().map(|c| c.1).collect::<Vec<_>>(),
        &ordered.iter().map(|post| class(post)).collect::<Vec<_>>(),
        learned,
    );
    let area_of: HashMap<&str, String> = ordered
        .iter()
        .zip(&area_index)
        .map(|(&post, index)| (post, format!("{stop}:a{index}")))
        .collect();
    let stop_post_count = ordered.len();
    let choice_of: HashMap<usize, usize> = choices
        .iter()
        .enumerate()
        .map(|(index, choice)| (choice.hypothesis, index))
        .collect();

    // R105: rank-0 rows of post-level hypotheses whose post modes the context can use.
    let base: Vec<&Row> = rows
        .iter()
        .filter(|row| {
            row.rank == 0
                && choice_of.get(&row.candidate).is_some_and(|&choice| {
                    let modes = &post_modes[post_ids[choice].as_str()];
                    modes.is_empty()
                        || !mode_incompatible(&contexts.mode[row.context as usize], modes)
                })
        })
        .collect();
    let capped = |row: &Row| {
        row.excess
            .map(|excess| excess.clamp(0.0, learned.excess_cap_m))
            .unwrap_or(learned.excess_cap_m)
    };
    let mut by_context: Vec<(u32, Vec<&Row>)> = Vec::new();
    let mut group_of: HashMap<u32, usize> = HashMap::new();
    for row in base {
        let group = *group_of.entry(row.context).or_insert_with(|| {
            by_context.push((row.context, Vec::new()));
            by_context.len() - 1
        });
        by_context[group].1.push(row);
    }
    let flag = |value: bool| if value { 1.0 } else { 0.0 };
    let mut candidates = Vec::new();
    let mut keys = Vec::new();
    for (context, context_rows) in &by_context {
        let c = *context as usize;
        let minimum = context_rows
            .iter()
            .map(|row| capped(row))
            .fold(f64::INFINITY, f64::min);
        for row in context_rows {
            let choice = &choices[choice_of[&row.candidate]];
            let post = post_ids[choice_of[&row.candidate]].as_str();
            let hypothesis = &hypotheses[row.candidate];
            let (alignment, side, proximity) = components(
                settings,
                constants,
                row.corridor_distance,
                row.offset,
                row.corridor_heading,
                row.attachment_heading,
            );
            let lifecycle =
                (hypothesis.obsolete && !hypothesis.current).then_some("obsolete-lifecycle");
            let eligible = hard_gate(
                settings,
                lifecycle.or(row.failure),
                row.corridor_distance,
                row.offset,
                row.excess,
            )
            .is_none();
            let mode = &*contexts.mode[c];
            let features = model
                .features
                .iter()
                .map(|feature| match feature {
                    Feature::Side => side,
                    Feature::Alignment => alignment,
                    Feature::Proximity => proximity,
                    Feature::CorridorDistance => row.corridor_distance.unwrap_or(0.0),
                    Feature::RoutedFit => routed_fit_of(settings, row.excess),
                    Feature::RoutedExcessMetresCapped => capped(row),
                    Feature::ExcessMinusContextMin => capped(row) - minimum,
                    Feature::Eligible => flag(eligible),
                    Feature::HasLocalRef => flag(choice.local_ref.is_some()),
                    Feature::IsBay => flag(post_is_bay[post]),
                    Feature::PostSupport => post_support[post] as f64,
                    Feature::PostSupportDeficit => (maximum_support - post_support[post]) as f64,
                    Feature::IsTram => flag(mode == "E"),
                    Feature::IsTrolleybus => flag(mode == "T"),
                    Feature::AnchorPreviousMissing => flag(contexts.previous_stop[c].is_none()),
                    Feature::AnchorNextMissing => flag(contexts.next_stop[c].is_none()),
                    Feature::ServiceEdgeCount => f64::from(row.service_edges),
                })
                .collect();
            candidates.push(Candidate {
                context: contexts.context_id[c].to_string(),
                hypothesis: hypothesis.id.clone(),
                post: post.to_owned(),
                area: area_of[post].clone(),
                is_platform: choice.platform,
                features,
            });
        }
        keys.push(ContextKey {
            context: contexts.context_id[c].to_string(),
            previous: contexts.previous_stop[c].map(|stop| format!("jdf:stop:{stop}")),
            next: contexts.next_stop[c].map(|stop| format!("jdf:stop:{stop}")),
            stop_post_count,
        });
    }
    let rows_by_id: HashMap<String, u32> = by_context
        .iter()
        .map(|(context, _)| (contexts.context_id[*context as usize].to_string(), *context))
        .collect();
    // POST-R141: the rows the scorer scores, for its training table.
    if let Some(training) = training {
        let position: HashMap<&str, (f64, f64)> = choices
            .iter()
            .map(|choice| {
                (
                    hypotheses[choice.hypothesis].id.as_str(),
                    (choice.latitude, choice.longitude),
                )
            })
            .collect();
        let key_of: HashMap<&str, &ContextKey> =
            keys.iter().map(|key| (key.context.as_str(), key)).collect();
        for candidate in &candidates {
            let key = key_of[candidate.context.as_str()];
            let (latitude, longitude) = position[candidate.hypothesis.as_str()];
            let mode = &*contexts.mode[rows_by_id[&candidate.context] as usize];
            training.push(TrainingRow {
                context_id: candidate.context.clone(),
                stop,
                hypothesis_id: candidate.hypothesis.clone(),
                post_id: candidate.post.clone(),
                area_id: candidate.area.clone(),
                is_platform: candidate.is_platform,
                previous_stop: key.previous.clone(),
                next_stop: key.next.clone(),
                stop_post_count: key.stop_post_count,
                mode: mode.to_owned(),
                latitude,
                longitude,
                features: candidate.features.clone(),
            });
        }
    }
    scorer::score_stop(model, &keys, candidates, learned)
        .into_iter()
        .filter_map(|decision| {
            rows_by_id
                .get(&decision.context)
                .map(|&context| (context, decision))
        })
        .collect()
}
