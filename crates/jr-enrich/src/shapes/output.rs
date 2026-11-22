//! Reading the companion's output (serving 8.3): `pattern_status.csv`, `shapes.csv` and
//! `offsets.csv`, each with a fixed header and plain comma-separated fields (the companion
//! writes no quoting).

use jr_model::Coord;

use super::CompanionOutput;
use super::plan::Plan;

const STATUS_HEADER: &str = "pattern_id,class,status,points,routed_m,beeline_m";
const SHAPES_HEADER: &str = "pattern_id,seq,lat,lon,dist_m";
const OFFSETS_HEADER: &str = "pattern_id,match_seq,point_index,dist_m";

/// What the companion said, per planned pattern (indices of `Plan::patterns`).
#[derive(Debug)]
pub(super) struct Routed {
    pub status: Vec<Option<String>>,
    /// Shape points in `seq` order.
    pub points: Vec<Vec<Coord>>,
    /// In file order.
    pub offsets: Vec<Vec<Offset>>,
    /// The first problem found in a pattern's rows.
    pub malformed: Vec<Option<String>>,
    /// Rows naming a pattern that was not planned.
    pub unknown_rows: usize,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Offset {
    pub match_seq: usize,
    pub point_index: usize,
    pub dist_m: f64,
}

impl Routed {
    fn mark(&mut self, pattern: usize, problem: String) {
        self.malformed[pattern].get_or_insert(problem);
    }
}

/// `Err` when a file as a whole cannot be read (a wrong header); a bad row only marks its
/// pattern malformed.
pub(super) fn read(plan: &Plan, output: &CompanionOutput) -> Result<Routed, String> {
    let count = plan.patterns.len();
    let mut routed = Routed {
        status: vec![None; count],
        points: vec![Vec::new(); count],
        offsets: vec![Vec::new(); count],
        malformed: vec![None; count],
        unknown_rows: 0,
    };

    for (line, fields) in rows("pattern_status.csv", &output.pattern_status, STATUS_HEADER)? {
        let Some(pattern) = routed_pattern(plan, &mut routed, &fields) else {
            continue;
        };
        let problem = if fields.len() != 6 {
            Some(format!("pattern_status.csv:{line}: expected 6 fields"))
        } else if fields[1] != plan.patterns[pattern].class {
            Some(format!("pattern_status.csv:{line}: class {}", fields[1]))
        } else if fields[2].is_empty() {
            Some(format!("pattern_status.csv:{line}: no status"))
        } else if routed.status[pattern].is_some() {
            Some(format!("pattern_status.csv:{line}: a second status row"))
        } else {
            routed.status[pattern] = Some(fields[2].to_owned());
            None
        };
        if let Some(problem) = problem {
            routed.mark(pattern, problem);
        }
    }

    for (line, fields) in rows("shapes.csv", &output.shapes, SHAPES_HEADER)? {
        let Some(pattern) = routed_pattern(plan, &mut routed, &fields) else {
            continue;
        };
        match shape_point(&fields, routed.points[pattern].len()) {
            Some(coord) => routed.points[pattern].push(coord),
            None => routed.mark(pattern, format!("shapes.csv:{line}: bad shape point")),
        }
    }

    for (line, fields) in rows("offsets.csv", &output.offsets, OFFSETS_HEADER)? {
        let Some(pattern) = routed_pattern(plan, &mut routed, &fields) else {
            continue;
        };
        match offset(&fields) {
            Some(offset) => routed.offsets[pattern].push(offset),
            None => routed.mark(pattern, format!("offsets.csv:{line}: bad offset")),
        }
    }
    Ok(routed)
}

/// The planned pattern a row is about; rows about any other are counted and skipped.
fn routed_pattern(plan: &Plan, routed: &mut Routed, fields: &[&str]) -> Option<usize> {
    let pattern = plan.position(fields[0]);
    if pattern.is_none() {
        routed.unknown_rows += 1;
    }
    pattern
}

/// The data rows of a file after its header, with their 1-based line numbers. Blank lines are
/// skipped; a trailing CR is dropped.
fn rows<'a>(
    name: &str,
    text: &'a str,
    header: &str,
) -> Result<impl Iterator<Item = (usize, Vec<&'a str>)>, String> {
    let mut lines = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .enumerate();
    match lines.next() {
        Some((_, first)) if first == header => {}
        _ => return Err(format!("{name}: header is not `{header}`")),
    }
    Ok(lines
        .filter(|(_, line)| !line.is_empty())
        .map(|(index, line)| (index + 1, line.split(',').collect())))
}

/// A `shapes.csv` row: the next point of its pattern (`seq` = `next`), within WGS84 bounds.
fn shape_point(fields: &[&str], next: usize) -> Option<Coord> {
    let [_, seq, lat, lon, metres] = fields else {
        return None;
    };
    let coord = Coord::new(number(lon)?, number(lat)?);
    number(metres)?;
    (seq.parse::<usize>().ok()? == next && coord.is_valid()).then_some(coord)
}

/// An `offsets.csv` row.
fn offset(fields: &[&str]) -> Option<Offset> {
    let [_, match_seq, point_index, metres] = fields else {
        return None;
    };
    Some(Offset {
        match_seq: match_seq.parse().ok()?,
        point_index: point_index.parse().ok()?,
        dist_m: number(metres).filter(|metres| *metres >= 0.0)?,
    })
}

/// A finite decimal number.
fn number(field: &str) -> Option<f64> {
    field.parse::<f64>().ok().filter(|value| value.is_finite())
}
