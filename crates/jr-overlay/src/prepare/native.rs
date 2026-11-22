//! Source-native stop places (OVL-R047) and the registry's review rows for them (R048).

use std::collections::HashMap;

use jr_formats::geodata::registry::OverlayPlace;
use jr_model::Rule;
use jr_model::identity::overlay;

use super::source::{MatchMethod, NativePlace, PlaceRef, PreparedSource, StopMatch};

/// For every eligible trip in one of the profile's source-native modes running in the horizon,
/// each call whose stop group is still unplaced places the group at a stop place of the source's
/// own: the registry's pinned place for the group's key (section 4.2) if there is one, else
/// `identity::overlay::stop_place` of the key. This happens whether or not the trip is later
/// projected; a place no output call uses is not published.
///
/// Stop places only the source knows, for trips it may run on its own.
pub const OVL_R047: Rule = Rule {
    id: "OVL-R047",
    summary: "unplaced stops of source-native trips become stop places of the source",
};

/// Every native place the registry does not pin is a review row: the group's key, the built ID,
/// name and position, and for a group split by name the pinned places of the same source group
/// (key prefix up to its last `:`), which is how a renamed stop keeps its ID.
pub const OVL_R048: Rule = Rule {
    id: "OVL-R048",
    summary: "unpinned native places are listed for the registry with alias suggestions",
};

pub const RULES: &[Rule] = &[OVL_R047, OVL_R048];

/// The registry's pins: `(source ID, group key)` to place ID.
pub type Pins<'p> = HashMap<(&'p str, &'p str), &'p str>;

pub fn pins(places: &[OverlayPlace]) -> Pins<'_> {
    places
        .iter()
        .map(|place| ((&*place.source_id, &*place.group_key), &*place.place_id))
        .collect()
}

/// R047 for one source.
pub fn native_places(source: &mut PreparedSource<'_>, pins: &Pins<'_>) {
    let source_id = source.profile.source_id.to_string();
    let native_modes = &source.profile.authority.source_native_modes;
    for trip_index in 0..source.trips.len() {
        let trip = &source.trips[trip_index];
        let mode = source.routes[trip.route as usize].mode;
        if trip.dates.is_empty() || !mode.is_some_and(|mode| native_modes.contains(&mode)) {
            continue;
        }
        let range = trip.calls.start as usize..trip.calls.end as usize;
        for call_index in range {
            let stop = source.calls[call_index].stop;
            let Some(group) = source.group_of_stop[stop as usize] else {
                continue;
            };
            if source.groups[group as usize].matched.is_some() {
                continue;
            }
            let key = source.groups[group as usize].registry_key(&source_id);
            let pinned = pins.get(&(&*source_id, &*key)).map(|&id| Box::from(id));
            let index = source.native_places.len() as u32;
            source.native_places.push(NativePlace {
                group,
                id: overlay::stop_place(&source_id, &key),
                pinned,
            });
            source.groups[group as usize].matched = Some(StopMatch {
                target: PlaceRef::Native(index),
                method: MatchMethod::SourceNative,
                distance: None,
                corrected: false,
            });
        }
    }
}

/// R048: rows of `source_id, group_key, place_id, stop_name, lat, lon, reason,
/// alias_of_suggestion`, sorted by source and key.
pub fn registry_candidates(source: &PreparedSource<'_>, pins: &Pins<'_>) -> Vec<Vec<String>> {
    let source_id = source.source_id();
    let mut rows: Vec<Vec<String>> = source
        .native_places
        .iter()
        .filter(|place| place.pinned.is_none())
        .map(|place| {
            let group = &source.groups[place.group as usize];
            let key = group.registry_key(source_id);
            let suggestions = match key.strip_prefix("group:").and(key.rfind(':')) {
                Some(last) if last > "group:".len() => {
                    let prefix = &key[..=last];
                    let mut ids: Vec<&str> = pins
                        .iter()
                        .filter(|((source, pinned), _)| {
                            *source == source_id && pinned.starts_with(prefix)
                        })
                        .map(|(_, id)| *id)
                        .collect();
                    ids.sort_unstable();
                    ids.dedup();
                    ids.join(";")
                }
                _ => String::new(),
            };
            let coordinate =
                |value: Option<f64>| value.map_or_else(String::new, |v| format!("{v:.6}"));
            vec![
                source_id.to_owned(),
                key,
                place.id.to_string(),
                group.name.to_string(),
                coordinate(group.position.map(|p| p.lat)),
                coordinate(group.position.map(|p| p.lon)),
                "new".to_owned(),
                suggestions,
            ]
        })
        .collect();
    rows.sort_by(|a, b| (&a[0], &a[1]).cmp(&(&b[0], &b[1])));
    rows
}
