//! MHD naming (JDF-R056–R060): batches that write a city line's stops with the stop name alone
//! in the town column.

use jr_model::{Diagnostic, Diagnostics, Severity};

use super::R059;
use super::evidence::Evidence;
use super::names::{WorkStop, first_part};
use super::policy::Policy;
use super::select::{Counters, exact_matches, selection};
use crate::batch::Batch;

/// R057: the group head of a town, or the town.
fn group_head<'a>(town: &'a str, policy: &'a Policy) -> &'a str {
    policy
        .file
        .mhd
        .town_groups
        .iter()
        .find(|group| group.iter().any(|t| &**t == town))
        .and_then(|group| group.first())
        .map_or(town, |head| head)
}

/// R057: the towns tried for a winning town.
pub(crate) fn town_list(town: &str, policy: &Policy) -> Vec<Box<str>> {
    policy
        .file
        .mhd
        .town_groups
        .iter()
        .find(|group| group.iter().any(|t| &**t == town))
        .cloned()
        .unwrap_or_else(|| vec![town.into()])
}

/// R056 (with R057 and R058): candidate towns with their support, best first.
pub(crate) fn town_candidates(
    stops: &[WorkStop],
    batch: &Batch,
    evidence: &Evidence,
    policy: &Policy,
) -> Vec<(Box<str>, usize)> {
    let analyzer = evidence.analyzer();
    let mut counts: Vec<(Box<str>, usize)> = Vec::new();
    for stop in stops.iter().filter(|stop| stop.town_only()) {
        let candidates = evidence.candidates(&stop.name_string());
        let exact = candidates
            .iter()
            .filter(|c| c.quality.is_exact())
            .map(|c| c.entry);
        let mut ignored = Counters::default();
        let accepted = exact_matches(evidence, policy, stop.locality(), exact, &mut ignored);
        if !accepted.is_empty() && evidence.is_municipality(first_part(&stop.town).trim()) {
            continue;
        }
        let bare = analyzer.tokens(&stop.town);
        let mut towns: Vec<&str> = Vec::new();
        for candidate in candidates.iter() {
            let name = &*evidence.entries[candidate.entry as usize].name;
            let tokens = analyzer.tokens(name);
            let town = first_part(name).trim();
            if towns.contains(&town) {
                continue;
            }
            let prefixed = analyzer.tokens(&format!("{town},{}", stop.town));
            if analyzer.same_sequence(&prefixed, &tokens) && !analyzer.same_sequence(&bare, &tokens)
            {
                towns.push(town);
            }
        }
        for town in towns {
            let head = group_head(town, policy);
            match counts.iter_mut().find(|(t, _)| &**t == head) {
                Some((_, count)) => *count += 1,
                None => counts.push((head.into(), 1)),
            }
        }
    }
    counts.retain(|(_, count)| *count >= policy.file.mhd.min_supporting_stops);
    counts.sort_by_key(|town| std::cmp::Reverse(town.1));
    // R058: a town the route name names gets a bonus.
    if let Some(route_town) = batch
        .routes
        .first()
        .and_then(|route| route_town(&route.name, policy))
        .filter(|town| evidence.is_municipality(town))
    {
        let route_tokens = analyzer.tokens(route_town);
        for (town, count) in &mut counts {
            if analyzer.same_sequence(&analyzer.tokens(town), &route_tokens) {
                *count += policy.file.mhd.route_name_bonus;
            }
        }
        counts.sort_by_key(|town| std::cmp::Reverse(town.1));
    }
    counts
}

/// R058: the town of a route name, by the F# pattern
/// `MHD ([^:]*) linka|MHD ([^ :]*)|([^,:-]*) *[,:-]` (leftmost match, alternatives in order).
pub(crate) fn route_town<'a>(name: &'a str, policy: &Policy) -> Option<&'a str> {
    let mhd = &policy.file.mhd;
    let prefix = &*mhd.route_town_prefix;
    let after_prefix = |at: usize| {
        let rest = &name[at + prefix.len()..];
        let span = &rest[..rest.find(':').unwrap_or(rest.len())];
        match span.rfind(&*mhd.route_town_suffix) {
            Some(end) => &span[..end],
            None => &rest[..rest.find([' ', ':']).unwrap_or(rest.len())],
        }
    };
    if name.starts_with(prefix) {
        return Some(after_prefix(0));
    }
    if let Some(end) = name.find(&mhd.route_town_delimiters[..]) {
        return Some(&name[..end]);
    }
    name.find(prefix).map(after_prefix)
}

/// R059: a raw stop renamed for MHD naming with the towns of R057.
pub(crate) fn rename(
    stop: &WorkStop,
    towns: &[Box<str>],
    evidence: &Evidence,
    policy: &Policy,
    batch_name: &str,
    diagnostics: &mut Diagnostics,
) -> WorkStop {
    if !stop.town_only() {
        return stop.clone();
    }
    let parts: Vec<&str> = stop.town.split(',').collect();
    if parts.len() > policy.file.mhd.max_name_parts {
        diagnostics.push(
            Diagnostic::new(
                "jdf_mhd_name_parts",
                Severity::Warning,
                format!("{batch_name}/stop {}", stop.id),
                format!("MHD-named stop `{}` has more than two commas", stop.town),
            )
            .rule(R059),
        );
    }
    let non_empty = |text: &str| (!text.is_empty()).then(|| Box::<str>::from(text));
    let rest = parts[1..].join(" ");
    let adjusted: Vec<WorkStop> = towns
        .iter()
        .map(|town| WorkStop {
            town: town.clone(),
            district: non_empty(parts[0]),
            nearby: non_empty(rest.trim()),
            ..stop.clone()
        })
        .collect();
    if let Some(found) = adjusted.iter().find(|candidate| {
        selection(
            evidence,
            policy,
            &candidate.name_string(),
            candidate.locality(),
        )
        .matched
        .is_some()
    }) {
        return found.clone();
    }
    if evidence.is_municipality(parts[0]) {
        return WorkStop {
            town: parts[0].into(),
            district: parts.get(1).and_then(|p| non_empty(p)),
            nearby: (parts.len() == 3).then(|| non_empty(parts[2])).flatten(),
            ..stop.clone()
        };
    }
    adjusted.into_iter().next().unwrap_or_else(|| stop.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jdf_r058_route_name_town_follows_the_reference_pattern() {
        let policy = Policy::builtin();
        let town = |name| route_town(name, &policy);
        assert_eq!(town("MHD Kolín linka 3"), Some("Kolín"));
        assert_eq!(town("MHD Kolín"), Some("Kolín"));
        assert_eq!(town("Brno - Vyškov"), Some("Brno "));
        assert_eq!(town("Liberec,Jablonec"), Some("Liberec"));
        assert_eq!(town("Městská linka"), None);
        assert_eq!(town("Linka MHD Tábor"), Some("Tábor"));
    }
}
