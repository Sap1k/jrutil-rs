//! Where the feed's members are: at the root, or all in one top directory.

use std::collections::BTreeMap;

use super::GtfsError;

const REQUIRED: [&str; 5] = [
    "agency.txt",
    "routes.txt",
    "trips.txt",
    "stops.txt",
    "stop_times.txt",
];
const CALENDARS: [&str; 2] = ["calendar.txt", "calendar_dates.txt"];

pub(super) struct Layout {
    /// The top directory every member sits in (`feed/`), if they all do.
    pub directory: Option<Box<str>>,
    /// Member names relative to the feed, sorted, with their index in the container.
    pub entries: Vec<(Box<str>, usize)>,
}

/// Finds the feed among container entry names (`/`-separated; names ending in `/` are
/// directories). Checks that member names are unique and the required members exist.
pub(super) fn locate<'a>(names: impl Iterator<Item = &'a str>) -> Result<Layout, GtfsError> {
    let files: Vec<(usize, &str)> = names
        .enumerate()
        .filter(|(_, name)| !name.is_empty() && !name.ends_with('/'))
        .collect();
    let top = |name: &'a str| name.split_once('/').map(|(top, _)| top);
    let directory = match files.first() {
        Some(&(_, first)) => {
            top(first).filter(|dir| files.iter().all(|&(_, name)| top(name) == Some(*dir)))
        }
        None => None,
    };
    let strip = directory.map_or(0, |dir| dir.len() + 1);
    let mut entries: BTreeMap<Box<str>, usize> = BTreeMap::new();
    for &(index, name) in &files {
        let member: Box<str> = name[strip..].into();
        if entries.insert(member, index).is_some() {
            return Err(GtfsError::DuplicateMember(name.into()));
        }
    }
    let prefix = directory.map_or_else(String::new, |dir| format!("{dir}/"));
    // A required member found only below the root names the directory it is in.
    let nested = |member: &str| {
        entries.keys().find_map(|name| {
            let dir = name.strip_suffix(member)?.strip_suffix('/')?;
            Some(GtfsError::Nested {
                directory: format!("{prefix}{dir}/").into(),
            })
        })
    };
    for member in REQUIRED {
        if !entries.contains_key(member) {
            return Err(nested(member).unwrap_or(GtfsError::MissingMember(member)));
        }
    }
    if !CALENDARS.iter().any(|member| entries.contains_key(*member)) {
        return Err(CALENDARS
            .iter()
            .find_map(|member| nested(member))
            .unwrap_or(GtfsError::NoCalendar));
    }
    Ok(Layout {
        directory: directory.map(|dir| format!("{dir}/").into()),
        entries: entries.into_iter().collect(),
    })
}
