//! Accepted source stops and their output boarding points (OVL-R078, R079, R100).

use std::collections::{BTreeMap, HashMap};

use jr_model::identity::overlay;
use jr_model::{PlaceId, PublicId, Rule};

use super::capability;
use crate::authority::Authority;
use crate::matching::Matches;
use crate::prepare::{PlaceRef, Prepared};
use crate::profile::Capability;

/// The source stops whose posts may be published: the stops of source calls aligned with a base
/// call in bindings eligible for `call_boarding_points` (with `keep_source_calls` and a fully
/// aligned binding, every placed call of it), and every call of every authority projection.
pub const OVL_R078: Rule = Rule {
    id: "OVL-R078",
    summary: "accepted source stops: aligned calls of eligible bindings and projected calls",
};

/// An accepted, placed source stop is published as its place itself when it is a station
/// (`location_type 1`) or the source lacks `boarding_points` or `call_boarding_points`;
/// otherwise as a post: one post shared by every source whose stops have one place and one
/// platform code (`identity::overlay::shared_post`) when more than one source does, else the
/// source's own post (`identity::overlay::post`).
///
/// BASE_PLAN 14: the overlay "resolves or creates the post, binds the source post ID".
pub const OVL_R079: Rule = Rule {
    id: "OVL-R079",
    summary: "an accepted stop becomes a post of its place, shared between sources by platform",
};

/// A post a source adds stands at its place under the place's name, with the source stop's
/// platform code, accessibility, post key (R017) as its source code and, with
/// `boarding_points`, the stop's position (exact, `overlay`); a shared post takes these from
/// its first stop by namespaced ID. Only posts an output call uses are added.
pub const OVL_R100: Rule = Rule {
    id: "OVL-R100",
    summary: "added posts: place name, platform, accessibility, position of the source stop",
};

pub const RULES: &[Rule] = &[OVL_R078, OVL_R079, OVL_R100];

/// A place a call or post is at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OutPlace {
    Base(PlaceId),
    /// Source index, index into its `native_places`.
    Native {
        source: u32,
        place: u32,
    },
}

/// Where an accepted source stop is published (R079).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OutStop {
    /// An index into [`Stops::posts`].
    Post(u32),
    /// The place itself.
    Place(OutPlace),
}

/// A post the sources add (R079, R100).
#[derive(Clone, Debug)]
pub struct Post {
    pub id: PublicId,
    pub place: OutPlace,
    /// The stop it takes its fields from: (source, feed stop index).
    pub stop: (u32, u32),
    /// Every accepted stop published as it, in namespaced ID order.
    pub members: Vec<(u32, u32)>,
}

/// R078, R079.
#[derive(Debug)]
pub struct Stops {
    /// Per source, per feed stop: accepted (R078).
    pub accepted: Vec<Vec<bool>>,
    /// Per source, per feed stop: where it is published (R079).
    pub out: Vec<Vec<Option<OutStop>>>,
    pub posts: Vec<Post>,
}

impl Stops {
    pub fn out(&self, source: u32, stop: u32) -> Option<OutStop> {
        self.out[source as usize][stop as usize]
    }

    pub fn place_of(&self, stop: OutStop) -> OutPlace {
        match stop {
            OutStop::Post(post) => self.posts[post as usize].place,
            OutStop::Place(place) => place,
        }
    }
}

/// The place a source stop's group is at.
pub fn mapped(prepared: &Prepared<'_>, source: u32, stop: u32) -> Option<OutPlace> {
    let s = &prepared.sources[source as usize];
    let group = s.group_of_stop[stop as usize]?;
    Some(match s.groups[group as usize].matched?.target {
        PlaceRef::Base(place) => OutPlace::Base(place),
        PlaceRef::Native(place) => OutPlace::Native { source, place },
    })
}

/// The public ID of an output place.
pub fn place_id(prepared: &Prepared<'_>, place: OutPlace) -> String {
    place_public_id(prepared, place).to_string()
}

/// The public ID of an output place: a base place's, or a native place's pinned or built ID.
pub fn place_public_id(prepared: &Prepared<'_>, place: OutPlace) -> PublicId {
    match place {
        OutPlace::Base(place) => prepared.base.timetable.places[place].id.clone(),
        OutPlace::Native { source, place } => {
            let native = &prepared.sources[source as usize].native_places[place as usize];
            match &native.pinned {
                Some(pinned) => overlay::registered_place(pinned),
                None => native.id.clone(),
            }
        }
    }
}

pub fn accepted(prepared: &Prepared<'_>, authority: &Authority, matches: &Matches) -> Stops {
    let mut accepted: Vec<Vec<bool>> = prepared
        .sources
        .iter()
        .map(|s| vec![false; s.feed.stops.len()])
        .collect();
    for binding in &matches.bindings {
        if !capability::eligible(prepared, binding, Capability::CallBoardingPoints) {
            continue;
        }
        let source = &prepared.sources[binding.source as usize];
        let calls = source.trip_calls(&source.trips[binding.trip as usize]);
        let alignment = &binding.candidate.alignment;
        if source.profile.trips.keep_source_calls && alignment.iter().all(Option::is_some) {
            for call in calls.iter().filter(|call| call.place.is_some()) {
                accepted[binding.source as usize][call.stop as usize] = true;
            }
        } else {
            for ordinal in alignment.iter().flatten() {
                accepted[binding.source as usize][calls[*ordinal as usize].stop as usize] = true;
            }
        }
    }
    for projection in &authority.projections {
        let source = &prepared.sources[projection.source as usize];
        for call in source.trip_calls(&source.trips[projection.trip as usize]) {
            accepted[projection.source as usize][call.stop as usize] = true;
        }
    }
    // R079: the place and platform of each accepted stop that may be a post.
    let mut out: Vec<Vec<Option<OutStop>>> = prepared
        .sources
        .iter()
        .map(|s| vec![None; s.feed.stops.len()])
        .collect();
    let mut candidates: Vec<(String, u32, u32, OutPlace)> = Vec::new();
    for (index, source) in prepared.sources.iter().enumerate() {
        let index = index as u32;
        let as_posts = capability::enabled(prepared, index, Capability::BoardingPoints)
            && capability::enabled(prepared, index, Capability::CallBoardingPoints);
        for (stop, row) in source.feed.stops.iter().enumerate() {
            if !accepted[index as usize][stop] {
                continue;
            }
            let Some(place) = mapped(prepared, index, stop as u32) else {
                continue;
            };
            if row.location_type == Some(1) || !as_posts {
                out[index as usize][stop] = Some(OutStop::Place(place));
            } else {
                candidates.push((
                    format!("{}:{}", source.source_id(), row.stop_id),
                    index,
                    stop as u32,
                    place,
                ));
            }
        }
    }
    candidates.sort_by(|a, b| a.0.cmp(&b.0));
    // Shared posts: one place and platform code, more than one source.
    let mut by_platform: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
    for (at, (_, source, stop, place)) in candidates.iter().enumerate() {
        let row = &prepared.sources[*source as usize].feed.stops[*stop as usize];
        if let Some(platform) = row
            .platform_code
            .as_deref()
            .filter(|p| !p.trim().is_empty())
        {
            by_platform
                .entry((place_id(prepared, *place), platform.trim().to_uppercase()))
                .or_default()
                .push(at);
        }
    }
    let mut shared_of: HashMap<usize, (String, String)> = HashMap::new();
    for (key, members) in &by_platform {
        let mut sources: Vec<u32> = members.iter().map(|&at| candidates[at].1).collect();
        sources.dedup();
        if sources.len() > 1 {
            for &at in members {
                shared_of.insert(at, key.clone());
            }
        }
    }
    let mut posts: Vec<Post> = Vec::new();
    let mut shared_post: HashMap<(String, String), u32> = HashMap::new();
    for (at, (_, source, stop, place)) in candidates.iter().enumerate() {
        let post = match shared_of.get(&at) {
            Some(key) => *shared_post.entry(key.clone()).or_insert_with(|| {
                posts.push(Post {
                    id: overlay::shared_post(&place_public_id(prepared, *place), &key.1),
                    place: *place,
                    stop: (*source, *stop),
                    members: Vec::new(),
                });
                (posts.len() - 1) as u32
            }),
            None => {
                let s = &prepared.sources[*source as usize];
                posts.push(Post {
                    id: overlay::post(s.source_id(), &s.feed.stops[*stop as usize].stop_id),
                    place: *place,
                    stop: (*source, *stop),
                    members: Vec::new(),
                });
                (posts.len() - 1) as u32
            }
        };
        posts[post as usize].members.push((*source, *stop));
        out[*source as usize][*stop as usize] = Some(OutStop::Post(post));
    }
    Stops {
        accepted,
        out,
        posts,
    }
}
