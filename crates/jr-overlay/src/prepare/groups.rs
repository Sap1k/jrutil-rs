//! Source stop groups (OVL-R016, R039).

use std::collections::HashMap;

use jr_model::{Coord, Rule};

use super::columns::StopColumns;
use super::names::Names;
use super::source::{PreparedSource, StopGroup};

/// A stop without a parent station is grouped by the profile's group key column (a blank cell
/// falling back to its own stop ID); without the hook, by its own stop ID.
///
/// PID's `asw_node_id` names the stop place its posts belong to (BASE_PLAN 14.1: "a documented
/// stop-place key such as PID `asw_node_id`").
pub const OVL_R016: Rule = Rule {
    id: "OVL-R016",
    summary: "stops without a parent are grouped by the profile's key column, else their own ID",
};

/// Scheduled stops (R030) are grouped: by their parent station, else by their group key (R016),
/// split by the key words of their names (R032, `-`-joined) when the profile says so. A group's
/// name is its parent station's, else its first member's; its position the mean of its
/// members'; its points their distinct positions, else that mean. Groups are in order of their
/// first member in `stops.txt`, members in file order.
///
/// BASE_PLAN 14.1 grouping order. The group's key, in the namespaced form of section 4.2, is
/// what the reviewed registry pins source-native places by.
pub const OVL_R039: Rule = Rule {
    id: "OVL-R039",
    summary: "scheduled source stops grouped by parent station, else key column and name",
};

pub const RULES: &[Rule] = &[OVL_R016, OVL_R039];

/// R039: fills `groups` and `group_of_stop`.
pub fn group_stops(source: &mut PreparedSource<'_>, scheduled: &[bool], names: &Names) {
    let profile = source.profile;
    let feed = &source.feed;
    let columns = StopColumns::new(profile);
    let mut by_id: HashMap<&str, u32> = HashMap::new();
    for (index, stop) in feed.stops.iter().enumerate().rev() {
        by_id.insert(&stop.stop_id, index as u32);
    }
    let mut keys: HashMap<(bool, String), u32> = HashMap::new();
    let mut groups: Vec<StopGroup> = Vec::new();
    let mut group_of_stop = vec![None; feed.stops.len()];
    for (index, stop) in feed.stops.iter().enumerate() {
        if !scheduled[index] {
            continue;
        }
        let parent = stop
            .parent_station
            .as_deref()
            .filter(|parent| !parent.trim().is_empty());
        let key = match parent {
            Some(parent) => (true, parent.to_owned()),
            None => {
                // R016.
                let value = profile
                    .hooks
                    .stop_group
                    .as_ref()
                    .and_then(|hook| columns.get(stop, &hook.key_column))
                    .unwrap_or(&stop.stop_id);
                let local = if profile.stops.split_flat_groups_by_name {
                    let words = names.key_words(stop.stop_name.as_deref().unwrap_or(""));
                    format!("group:{value}:{}", words.join("-"))
                } else {
                    format!("group:{value}")
                };
                (false, local)
            }
        };
        let group = *keys.entry(key.clone()).or_insert_with(|| {
            groups.push(StopGroup {
                local_id: key.1.clone().into(),
                parent: key.0,
                members: Vec::new(),
                name: "".into(),
                position: None,
                points: Vec::new(),
                zones: Vec::new(),
                matched: None,
            });
            (groups.len() - 1) as u32
        });
        groups[group as usize].members.push(index as u32);
        group_of_stop[index] = Some(group);
    }
    for group in &mut groups {
        let representative = if group.parent {
            by_id
                .get(&*group.local_id)
                .copied()
                .unwrap_or(group.members[0])
        } else {
            group.members[0]
        };
        group.name = feed.stops[representative as usize]
            .stop_name
            .clone()
            .unwrap_or_default();
        let members = group.members.iter().map(|&m| &feed.stops[m as usize]);
        let lats: Vec<f64> = members.clone().filter_map(|stop| stop.stop_lat).collect();
        let lons: Vec<f64> = members.clone().filter_map(|stop| stop.stop_lon).collect();
        group.position = match (mean(&lats), mean(&lons)) {
            (Some(lat), Some(lon)) => Some(Coord::new(lon, lat)),
            _ => None,
        };
        for stop in members.clone() {
            if let (Some(lat), Some(lon)) = (stop.stop_lat, stop.stop_lon) {
                let point = Coord::new(lon, lat);
                if !group.points.contains(&point) {
                    group.points.push(point);
                }
            }
        }
        if group.points.is_empty() {
            group.points.extend(group.position);
        }
        if profile.hooks.zones.is_some() {
            let mut zones: Vec<Box<str>> = members
                .filter_map(|stop| stop.zone_id.clone())
                .filter(|zone| !zone.trim().is_empty())
                .collect();
            zones.sort();
            zones.dedup();
            group.zones = zones;
        }
    }
    source.groups = groups;
    source.group_of_stop = group_of_stop;
}

fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}
