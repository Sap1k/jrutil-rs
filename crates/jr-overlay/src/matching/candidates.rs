//! Candidates of one source trip (OVL-R065–R068).

use jr_model::{PlaceId, Rule};

use super::score::{self, Rank};
use crate::prepare::{
    BaseTripIx, BaseView, PlaceRef, PreparedSource, SourceCall, SourceTrip, Times,
};
use crate::profile::{RouteTier, SourceProfile, TripTier};

/// A source trip's candidates are the published base trips of its route's primary candidate
/// routes, and separately of its relaxed structural routes less the primary ones (R049–R051).
/// Where some base trips of a set have exactly the source's place pattern, only they are tried.
/// With `keep_source_calls`, a view of the source calls with consecutive calls at one place
/// folded (first arrival, last departure) is tried too, and used when its fewest edits are
/// fewer than those of the calls as they are (and those are not zero). Agency, direction and
/// block never count.
///
/// The base usually has one call where the source calls several posts of a stop place.
pub const OVL_R065: Rule = Rule {
    id: "OVL-R065",
    summary: "candidates: base trips of primary, then structural routes; exact patterns first",
};

/// A candidate's tier, the strongest the profile enables: exact pattern with equal `full`,
/// `endpoints` or `first` signatures (R055), exact pattern alone when the profile allows
/// proximity (`pattern_nearest`); else with `keep_source_calls` a superset alignment
/// (`pattern_nearest`, the further calls counting as edits); else an edit alignment
/// (`pattern_edit_nearest`); else no candidate.
pub const OVL_R066: Rule = Rule {
    id: "OVL-R066",
    summary: "a candidate's tier: signatures, exact pattern, superset, then pattern edits",
};

/// An edit alignment needs both patterns non-empty, the same endpoints when the profile says so,
/// a length difference and a quick support bound within the maximum edits; then a Levenshtein
/// distance within the maximum and an agreement `1 − edits / longer` of at least the minimum.
/// Backtracking prefers the diagonal, then a source deletion; a base call is aligned only on an
/// equal diagonal step.
///
/// A source trip may skip or add a stop the base trip has or lacks.
pub const OVL_R067: Rule = Rule {
    id: "OVL-R067",
    summary: "pattern edit alignment within the profile's edits and agreement",
};

/// A superset alignment: the source has 1 to the maximum edits more calls, the same first and
/// last place; each base call but the last takes the next source call at its place (never the
/// last source call), the last base call the last source call.
///
/// "Bind a source trip to a base trip whose every call it also makes … and keep the source's
/// further calls."
pub const OVL_R068: Rule = Rule {
    id: "OVL-R068",
    summary: "superset alignment: every base call is made by the source, between equal ends",
};

pub const RULES: &[Rule] = &[OVL_R065, OVL_R066, OVL_R067, OVL_R068];

/// A source call's place as the tiers compare it: a base place, or its own stop (unplaced or a
/// source-native place, never equal to a base place; distinct per stop as in the reference).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Token {
    Place(PlaceId),
    Stop(u32),
}

/// A base trip a source trip may match (R066), with how.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub base_trip: BaseTripIx,
    pub tier: TripTier,
    /// Per base call, the first source call aligned with it (R066–R068).
    pub alignment: Box<[Option<u32>]>,
    /// Per base call, how many consecutive source calls it stands for (a folded view, R065);
    /// empty when each stands for one.
    pub run_lengths: Box<[u32]>,
    /// R069, R070.
    pub rank: Rank,
}

/// A view of a source trip's calls (R065).
struct View {
    tokens: Vec<Token>,
    arrivals: Vec<Option<i32>>,
    departures: Vec<Option<i32>>,
    /// For a folded view, each view call's first source call and run length.
    runs: Option<(Vec<u32>, Vec<u32>)>,
}

impl View {
    fn times(&self) -> Times<'_> {
        Times {
            arrivals: &self.arrivals,
            departures: &self.departures,
        }
    }
}

fn token(call: &SourceCall) -> Token {
    match call.place {
        Some(PlaceRef::Base(place)) => Token::Place(place),
        _ => Token::Stop(call.stop),
    }
}

fn view(calls: &[SourceCall]) -> View {
    View {
        tokens: calls.iter().map(token).collect(),
        arrivals: calls.iter().map(|call| call.arrival).collect(),
        departures: calls.iter().map(|call| call.departure).collect(),
        runs: None,
    }
}

/// R065: consecutive calls at one place folded; `None` when nothing folds.
fn folded(calls: &[SourceCall]) -> Option<View> {
    let mut result = View {
        tokens: Vec::new(),
        arrivals: Vec::new(),
        departures: Vec::new(),
        runs: Some((Vec::new(), Vec::new())),
    };
    let mut index = 0;
    while index < calls.len() {
        let mut last = index;
        while last + 1 < calls.len() && token(&calls[last + 1]) == token(&calls[index]) {
            last += 1;
        }
        result.tokens.push(token(&calls[index]));
        result.arrivals.push(calls[index].arrival);
        result.departures.push(calls[last].departure);
        if let Some((starts, lengths)) = &mut result.runs {
            starts.push(index as u32);
            lengths.push((last - index + 1) as u32);
        }
        index = last + 1;
    }
    (result.tokens.len() < calls.len()).then_some(result)
}

/// The candidates of one source trip.
#[derive(Debug)]
pub struct TripCandidates {
    pub primary_tier: RouteTier,
    pub primary: Vec<Candidate>,
    pub structural: Vec<Candidate>,
    /// Base trips on any candidate route, agreeing or not (R073).
    pub route_trips: usize,
}

/// R065.
pub fn candidates(
    base: &BaseView<'_>,
    source: &PreparedSource<'_>,
    trip: &SourceTrip,
) -> TripCandidates {
    let profile = source.profile;
    let calls = source.trip_calls(trip);
    let route = &source.routes[trip.route as usize];
    let structural_routes: Vec<_> = route
        .structural
        .iter()
        .copied()
        .filter(|candidate| !route.primary.contains(candidate))
        .collect();
    let trips_of = |routes: &[jr_model::RouteId]| -> Vec<BaseTripIx> {
        let mut trips: Vec<BaseTripIx> = Vec::new();
        for &route in routes {
            for &trip in base.trips_of_route(route) {
                trips.push(trip);
            }
        }
        distinct(trips)
    };
    let primary_trips = trips_of(&route.primary);
    let structural_trips = trips_of(&structural_routes);
    let as_is = view(calls);
    let folded_view = if profile.trips.keep_source_calls {
        folded(calls)
    } else {
        None
    };
    let matching = |routes: &[jr_model::RouteId], trips: &[BaseTripIx]| {
        let try_view = |view: &View| -> Vec<Candidate> {
            let exact = exact_trips(base, view, routes);
            let tried = if exact.is_empty() { trips } else { &exact };
            tried
                .iter()
                .filter_map(|&trip| candidate_for(base, profile, view, trip))
                .collect()
        };
        let fewest = |values: &[Candidate]| {
            values
                .iter()
                .map(|c| c.rank.edits)
                .min()
                .unwrap_or(u32::MAX)
        };
        let plain = try_view(&as_is);
        match &folded_view {
            Some(folded_view) if fewest(&plain) > 0 => {
                let folded = try_view(folded_view);
                if fewest(&folded) < fewest(&plain) {
                    folded
                } else {
                    plain
                }
            }
            _ => plain,
        }
    };
    let mut all = primary_trips.clone();
    all.extend(&structural_trips);
    TripCandidates {
        primary_tier: route.primary_tier,
        primary: matching(&route.primary, &primary_trips),
        structural: matching(&structural_routes, &structural_trips),
        route_trips: distinct(all).len(),
    }
}

fn distinct(values: Vec<BaseTripIx>) -> Vec<BaseTripIx> {
    let mut seen = std::collections::HashSet::with_capacity(values.len());
    values
        .into_iter()
        .filter(|value| seen.insert(*value))
        .collect()
}

/// Base trips of the routes with exactly the view's pattern.
fn exact_trips(base: &BaseView<'_>, view: &View, routes: &[jr_model::RouteId]) -> Vec<BaseTripIx> {
    let places: Option<Vec<PlaceId>> = view
        .tokens
        .iter()
        .map(|token| match token {
            Token::Place(place) => Some(*place),
            Token::Stop(_) => None,
        })
        .collect();
    let Some(pattern) = places.and_then(|places| base.find_pattern(&places)) else {
        return Vec::new();
    };
    let mut trips = Vec::new();
    for &route in routes {
        trips.extend_from_slice(base.trips_with_pattern(route, pattern));
    }
    distinct(trips)
}

/// R066.
fn candidate_for(
    base: &BaseView<'_>,
    profile: &SourceProfile,
    view: &View,
    trip: BaseTripIx,
) -> Option<Candidate> {
    let target = &base.trips[trip as usize];
    let pattern = base.pattern(target.pattern);
    let policy = &profile.trips;
    let enabled = |tier: TripTier| policy.tiers.contains(&tier);
    let resolution = policy.time_resolution_seconds;
    let target_times = Times {
        arrivals: &target.arrivals,
        departures: &target.departures,
    };
    let source_times = view.times();
    let exact = view.tokens.len() == pattern.len()
        && view
            .tokens
            .iter()
            .zip(pattern)
            .all(|(token, place)| *token == Token::Place(*place));
    let identity = || -> Vec<Option<usize>> { (0..pattern.len()).map(Some).collect() };
    let found: Option<(TripTier, u32, Vec<Option<usize>>)> = if exact
        && enabled(TripTier::FullSignature)
        && source_times.full_equal(&target_times, resolution)
    {
        Some((TripTier::FullSignature, 0, identity()))
    } else if exact
        && enabled(TripTier::PatternEndpoints)
        && source_times.endpoints_equal(&target_times, resolution)
    {
        Some((TripTier::PatternEndpoints, 0, identity()))
    } else if exact
        && enabled(TripTier::PatternFirst)
        && source_times.first_equal(&target_times, resolution)
    {
        Some((TripTier::PatternFirst, 0, identity()))
    } else if exact && policy.exact_pattern_proximity && enabled(TripTier::PatternNearest) {
        Some((TripTier::PatternNearest, 0, identity()))
    } else {
        let contained = if policy.keep_source_calls
            && policy.exact_pattern_proximity
            && enabled(TripTier::PatternNearest)
        {
            superset(policy.pattern_edit.maximum_edits, &view.tokens, pattern)
                .map(|(extra, alignment)| (TripTier::PatternNearest, extra, alignment))
        } else {
            None
        };
        match contained {
            Some(found) => Some(found),
            None if enabled(TripTier::PatternEditNearest) => {
                pattern_edit(&policy.pattern_edit, &view.tokens, pattern)
                    .map(|(edits, alignment)| (TripTier::PatternEditNearest, edits, alignment))
            }
            None => None,
        }
    };
    let (tier, edits, alignment) = found?;
    let rank = score::rank(edits, source_times, target_times, &alignment);
    let (alignment, run_lengths): (Box<[Option<u32>]>, Box<[u32]>) = match &view.runs {
        None => (
            alignment.iter().map(|a| a.map(|o| o as u32)).collect(),
            Box::new([]),
        ),
        Some((starts, lengths)) => (
            alignment.iter().map(|a| a.map(|o| starts[o])).collect(),
            alignment
                .iter()
                .map(|a| a.map_or(0, |o| lengths[o]))
                .collect(),
        ),
    };
    Some(Candidate {
        base_trip: trip,
        tier,
        alignment,
        run_lengths,
        rank,
    })
}

/// R067.
pub fn pattern_edit(
    policy: &crate::profile::PatternEdit,
    source: &[Token],
    target: &[PlaceId],
) -> Option<(u32, Vec<Option<usize>>)> {
    let eq = |i: usize, j: usize| source[i] == Token::Place(target[j]);
    let (n, m) = (source.len(), target.len());
    let maximum = policy.maximum_edits as usize;
    if !policy.enabled || n == 0 || m == 0 {
        return None;
    }
    if policy.require_same_endpoints && (!eq(0, 0) || !eq(n - 1, m - 1)) {
        return None;
    }
    if n.abs_diff(m) > maximum {
        return None;
    }
    let shorter = n.min(m);
    let positional = (0..shorter).filter(|&i| eq(i, i)).count();
    let mut prefix = 0;
    while prefix < shorter && eq(prefix, prefix) {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < shorter - prefix && eq(n - suffix - 1, m - suffix - 1) {
        suffix += 1;
    }
    if (positional.max(prefix + suffix) as i64) < shorter as i64 - maximum as i64 {
        return None;
    }
    let columns = m + 1;
    let mut costs = vec![0usize; (n + 1) * columns];
    for i in 0..=n {
        costs[i * columns] = i;
    }
    for (j, cost) in costs.iter_mut().enumerate().take(m + 1) {
        *cost = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let substitution = costs[(i - 1) * columns + j - 1] + usize::from(!eq(i - 1, j - 1));
            costs[i * columns + j] = substitution
                .min(costs[(i - 1) * columns + j] + 1)
                .min(costs[i * columns + j - 1] + 1);
        }
    }
    let edits = costs[n * columns + m];
    let agreement = 1.0 - edits as f64 / n.max(m) as f64;
    if edits > maximum || agreement < policy.minimum_agreement {
        return None;
    }
    let mut alignment = vec![None; m];
    let (mut i, mut j) = (n, m);
    while i > 0 || j > 0 {
        if i > 0
            && j > 0
            && costs[i * columns + j]
                == costs[(i - 1) * columns + j - 1] + usize::from(!eq(i - 1, j - 1))
        {
            if eq(i - 1, j - 1) {
                alignment[j - 1] = Some(i - 1);
            }
            i -= 1;
            j -= 1;
        } else if i > 0 && costs[i * columns + j] == costs[(i - 1) * columns + j] + 1 {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    Some((edits as u32, alignment))
}

/// R068.
pub fn superset(
    maximum_extra: u32,
    source: &[Token],
    target: &[PlaceId],
) -> Option<(u32, Vec<Option<usize>>)> {
    let extra = source.len() as i64 - target.len() as i64;
    if target.is_empty() || extra <= 0 || extra > i64::from(maximum_extra) {
        return None;
    }
    if source[0] != Token::Place(target[0])
        || source[source.len() - 1] != Token::Place(target[target.len() - 1])
    {
        return None;
    }
    let mut alignment = vec![None; target.len()];
    let mut at = 0;
    for (index, place) in target.iter().enumerate() {
        if index == target.len() - 1 {
            alignment[index] = Some(source.len() - 1);
        } else {
            while at < source.len() - 1 && source[at] != Token::Place(*place) {
                at += 1;
            }
            if at >= source.len() - 1 {
                return None;
            }
            alignment[index] = Some(at);
            at += 1;
        }
    }
    Some((extra as u32, alignment))
}
