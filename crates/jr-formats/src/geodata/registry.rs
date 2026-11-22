//! The stop registry (`jrunify-ext-geodata/registry/`, `docs/rules/jdf-merge.md` 2.2,
//! JDF-R207, R208, R211): `stops.csv`, `posts.csv` and `overlay_places.csv`.
//!
//! The files are a reviewed contract, so the reader is strict about what one row says: the
//! header must be exactly the documented one, every row has its columns, typed cells parse, a
//! reference has both coordinates or neither, `status` is `active` or `retired`, a
//! `merged_into:` note names another number. What needs more than one row is the compiler's:
//! the id range (the provisional base is policy), unique keys, posts of registered stops,
//! merge targets and chains (R208). Cells are trimmed (except `overlay_places.group_key` and the
//! notes, which F# keeps verbatim); nothing is upper-cased or otherwise canonicalised.

use std::fmt;

use jr_model::Coord;

pub const STOPS_HEADER: [&str; 10] = [
    "id",
    "town",
    "district",
    "nearby_place",
    "okres",
    "country",
    "lat",
    "lon",
    "status",
    "note",
];
pub const POSTS_HEADER: [&str; 6] = ["stop_id", "post_key", "lat", "lon", "status", "note"];
pub const OVERLAY_PLACES_HEADER: [&str; 5] =
    ["source_id", "group_key", "place_id", "status", "note"];

/// The note marker naming the number a duplicate row resolves to (R208).
pub const MERGED_INTO_MARKER: &str = "merged_into:";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Active,
    Retired,
}

/// One row of `stops.csv`: a spelling of the merged-JDF stop `id`.
#[derive(Clone, Debug, PartialEq)]
pub struct Stop {
    pub id: i64,
    pub town: Box<str>,
    pub district: Option<Box<str>>,
    pub nearby_place: Option<Box<str>>,
    /// As written (trimmed), not upper-cased.
    pub okres: Option<Box<str>>,
    /// As written (trimmed), not upper-cased.
    pub country: Option<Box<str>>,
    pub reference: Option<Coord>,
    pub status: Status,
    /// The number after the first `merged_into:` in the note.
    pub merged_into: Option<i64>,
    /// The note verbatim, `None` when blank.
    pub note: Option<Box<str>>,
    pub line: u64,
}

/// The suffix of a registered post after `jdf:stop:N:`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PostKey {
    /// `est:<k>`: an inferred post ordinal.
    Estimated(u32),
    /// `post:<x>`: a JDF post designation.
    Post(Box<str>),
}

impl fmt::Display for PostKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Estimated(ordinal) => write!(f, "est:{ordinal}"),
            Self::Post(designation) => write!(f, "post:{designation}"),
        }
    }
}

/// One row of `posts.csv`.
#[derive(Clone, Debug, PartialEq)]
pub struct Post {
    pub stop_id: i64,
    pub post_key: PostKey,
    /// Always present for [`PostKey::Estimated`].
    pub reference: Option<Coord>,
    pub status: Status,
    pub note: Option<Box<str>>,
    pub line: u64,
}

/// One row of `overlay_places.csv`.
#[derive(Clone, Debug, PartialEq)]
pub struct OverlayPlace {
    pub source_id: Box<str>,
    /// Verbatim, not trimmed.
    pub group_key: Box<str>,
    pub place_id: Box<str>,
    pub status: Status,
    pub note: Option<Box<str>>,
    pub line: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("stop registry: {0}")]
    Csv(#[from] csv::Error),
    #[error("stop registry: header must be {expected}, found {found}")]
    Header { expected: String, found: String },
    #[error("stop registry line {line}: {reason}")]
    Row { line: u64, reason: String },
}

/// Parses `stops.csv`.
pub fn parse_stops(csv: &[u8]) -> Result<Vec<Stop>, RegistryError> {
    read_table(csv, &STOPS_HEADER, |record, line| {
        let id = integer(&record[0], "id", line)?;
        let town = super::trimmed(&record[1]).ok_or_else(|| invalid(line, "town is required"))?;
        let note = note(&record[9]);
        let merged_into = note
            .as_deref()
            .map(|note| merged_into(note, id, line))
            .transpose()?
            .flatten();
        Ok(Stop {
            id,
            town,
            district: super::trimmed(&record[2]),
            nearby_place: super::trimmed(&record[3]),
            okres: super::trimmed(&record[4]),
            country: super::trimmed(&record[5]),
            reference: reference(&record[6], &record[7], line)?,
            status: status(&record[8], line)?,
            merged_into,
            note,
            line,
        })
    })
}

/// Parses `posts.csv`.
pub fn parse_posts(csv: &[u8]) -> Result<Vec<Post>, RegistryError> {
    read_table(csv, &POSTS_HEADER, |record, line| {
        let stop_id = integer(&record[0], "stop_id", line)?;
        let post_key = post_key(record[1].trim())
            .ok_or_else(|| invalid(line, "post_key must be post:<x> or est:<k>"))?;
        let reference = reference(&record[2], &record[3], line)?;
        if matches!(post_key, PostKey::Estimated(_)) && reference.is_none() {
            return Err(invalid(line, "inferred posts need reference coordinates"));
        }
        Ok(Post {
            stop_id,
            post_key,
            reference,
            status: status(&record[4], line)?,
            note: note(&record[5]),
            line,
        })
    })
}

/// Parses `overlay_places.csv`.
pub fn parse_overlay_places(csv: &[u8]) -> Result<Vec<OverlayPlace>, RegistryError> {
    read_table(csv, &OVERLAY_PLACES_HEADER, |record, line| {
        let required = || invalid(line, "source_id, group_key and place_id are required");
        let source_id = super::trimmed(&record[0]).ok_or_else(required)?;
        let group_key = Some(&record[1])
            .filter(|key| !key.is_empty())
            .map(Box::from)
            .ok_or_else(required)?;
        let place_id = super::trimmed(&record[2]).ok_or_else(required)?;
        Ok(OverlayPlace {
            source_id,
            group_key,
            place_id,
            status: status(&record[3], line)?,
            note: note(&record[4]),
            line,
        })
    })
}

fn read_table<T>(
    csv: &[u8],
    header: &[&str],
    mut row: impl FnMut(&csv::StringRecord, u64) -> Result<T, RegistryError>,
) -> Result<Vec<T>, RegistryError> {
    // Not flexible: a row with another number of columns than the header is a CSV error.
    let mut reader = csv::ReaderBuilder::new().has_headers(true).from_reader(csv);
    let found = reader.headers()?;
    if found.iter().ne(header.iter().copied()) {
        return Err(RegistryError::Header {
            expected: header.join(","),
            found: found.iter().collect::<Vec<_>>().join(","),
        });
    }
    let mut rows = Vec::new();
    let mut record = csv::StringRecord::new();
    while reader.read_record(&mut record)? {
        rows.push(row(&record, super::line_of(&record))?);
    }
    Ok(rows)
}

fn invalid(line: u64, reason: impl Into<String>) -> RegistryError {
    RegistryError::Row {
        line,
        reason: reason.into(),
    }
}

fn integer(text: &str, column: &str, line: u64) -> Result<i64, RegistryError> {
    text.trim()
        .parse()
        .map_err(|_| invalid(line, format!("{column} must be an integer, not '{text}'")))
}

fn note(text: &str) -> Option<Box<str>> {
    Some(text)
        .filter(|text| !text.trim().is_empty())
        .map(Box::from)
}

fn status(text: &str, line: u64) -> Result<Status, RegistryError> {
    match text.trim() {
        "active" => Ok(Status::Active),
        "retired" => Ok(Status::Retired),
        other => Err(invalid(
            line,
            format!("status must be active or retired, not '{other}'"),
        )),
    }
}

/// Both coordinates blank: no reference. Both set: finite decimals with `|lat| <= 90` and
/// `|lon| <= 180`. Anything else is an error.
fn reference(lat: &str, lon: &str, line: u64) -> Result<Option<Coord>, RegistryError> {
    match (lat.trim(), lon.trim()) {
        ("", "") => Ok(None),
        ("", _) | (_, "") => Err(invalid(
            line,
            "lat and lon must both be set or both be empty",
        )),
        (lat, lon) => match (super::finite_number(lat), super::finite_number(lon)) {
            (Some(lat), Some(lon)) if lat.abs() <= 90.0 && lon.abs() <= 180.0 => {
                Ok(Some(Coord::new(lon, lat)))
            }
            _ => Err(invalid(line, "lat/lon must be valid coordinates")),
        },
    }
}

/// The number after the first `merged_into:` (the longest run of digits). A marker without a
/// number, or naming the row's own id, is an error.
fn merged_into(note: &str, id: i64, line: u64) -> Result<Option<i64>, RegistryError> {
    let Some(start) = note.find(MERGED_INTO_MARKER) else {
        return Ok(None);
    };
    let rest = &note[start + MERGED_INTO_MARKER.len()..];
    let digits = &rest[..rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len())];
    match digits.parse::<i64>() {
        Ok(target) if target != id => Ok(Some(target)),
        _ => Err(invalid(line, "merged_into must name another id")),
    }
}

/// `est:<k>` with `k` a positive integer without leading zeros, or `post:<x>` with `x`
/// non-empty and free of `:` and whitespace.
fn post_key(text: &str) -> Option<PostKey> {
    if let Some(ordinal) = text.strip_prefix("est:") {
        let value = ordinal.parse::<u32>().ok()?;
        (value > 0 && value.to_string() == ordinal).then_some(PostKey::Estimated(value))
    } else {
        let designation = text.strip_prefix("post:")?;
        let clean =
            !designation.is_empty() && !designation.chars().any(|c| c == ':' || c.is_whitespace());
        clean.then(|| PostKey::Post(designation.into()))
    }
}
