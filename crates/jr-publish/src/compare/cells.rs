//! One column of the reference and the candidate, compared cell by cell without building
//! strings: dictionary codes are translated once per distinct string, doubles compare within
//! a tolerance, references to generated identifiers compare by what they refer to.

use std::cmp::Ordering;
use std::collections::HashMap;

use jiff::civil::Date;

use super::targets::TargetPair;
use crate::table::{Column, StrColumn};
use crate::validate::keys::{self, ValueHashes};

/// Code of an A string that does not occur in B.
const ABSENT: u32 = u32::MAX;

/// The same column in A and B.
pub(crate) struct Pair<'a> {
    columns: [&'a Column; 2],
    cmp: Cmp<'a>,
}

enum Cmp<'a> {
    Str {
        a: &'a StrColumn,
        b: &'a StrColumn,
        /// A pool code -> B pool code (`ABSENT` when B lacks the string), through `renamed`.
        to_b: Vec<u32>,
        renamed: Option<Renamed<'a>>,
    },
    Int16(&'a [Option<i16>], &'a [Option<i16>]),
    Int32(&'a [Option<i32>], &'a [Option<i32>]),
    Bool(&'a [Option<bool>], &'a [Option<bool>]),
    Date(&'a [Option<Date>], &'a [Option<Date>]),
    Double {
        a: &'a [Option<f64>],
        b: &'a [Option<f64>],
        tolerance: f64,
    },
    Ref(RefPair<'a>),
}

/// A reference to a generated identifier on both sides: each side's pool code resolved to
/// the content it stands for.
pub(crate) struct RefPair<'a> {
    pub a: &'a StrColumn,
    pub b: &'a StrColumn,
    /// Pool code -> content index; `None` for an identifier the target does not define.
    pub a_content: Vec<Option<u32>>,
    pub b_content: Vec<Option<u32>>,
    pub target: &'a TargetPair<'a>,
}

/// A's strings that B names differently (trip IDs paired by `journey`), per A pool code: a
/// renamed A value compares, hashes and matches as B's name for it.
struct Renamed<'a> {
    /// B's name for a renamed code.
    names: Vec<Option<&'a str>>,
    /// Hash of each code's value as B names it.
    hashes: Vec<u64>,
}

/// The two columns have different types.
#[derive(Debug)]
pub(crate) struct TypeMismatch;

impl<'a> Pair<'a> {
    pub fn new(a: &'a Column, b: &'a Column, tolerance: f64) -> Result<Pair<'a>, TypeMismatch> {
        let columns = [a, b];
        let cmp = match (a, b) {
            (Column::Str(a), Column::Str(b)) => strings(a, b, None),
            (Column::Int16(a), Column::Int16(b)) => Cmp::Int16(a, b),
            (Column::Int32(a), Column::Int32(b)) => Cmp::Int32(a, b),
            (Column::Bool(a), Column::Bool(b)) => Cmp::Bool(a, b),
            (Column::Date(a), Column::Date(b)) => Cmp::Date(a, b),
            (Column::Double(a), Column::Double(b)) => Cmp::Double { a, b, tolerance },
            _ => return Err(TypeMismatch),
        };
        Ok(Pair { columns, cmp })
    }

    /// A reference to a generated identifier: compared by what it refers to.
    pub fn reference(
        a: &'a Column,
        b: &'a Column,
        target: &'a TargetPair<'a>,
    ) -> Result<Pair<'a>, TypeMismatch> {
        let (Column::Str(sa), Column::Str(sb)) = (a, b) else {
            return Err(TypeMismatch);
        };
        let resolve = |side: usize, column: &StrColumn| {
            column
                .pool()
                .iter()
                .map(|id| target.resolve(side, id))
                .collect()
        };
        let cmp = Cmp::Ref(RefPair {
            a: sa,
            b: sb,
            a_content: resolve(0, sa),
            b_content: resolve(1, sb),
            target,
        });
        Ok(Pair {
            columns: [a, b],
            cmp,
        })
    }

    /// Strings naming trips: an A value `renames` maps stands for B's name for it.
    pub fn renamed(
        a: &'a Column,
        b: &'a Column,
        renames: &'a HashMap<Box<str>, Box<str>>,
    ) -> Result<Pair<'a>, TypeMismatch> {
        let (Column::Str(sa), Column::Str(sb)) = (a, b) else {
            return Err(TypeMismatch);
        };
        Ok(Pair {
            columns: [a, b],
            cmp: strings(sa, sb, Some(renames)),
        })
    }

    /// One side's column.
    pub fn column(&self, side: usize) -> &'a Column {
        self.columns[side]
    }

    /// The dates of the service a reference names, as `first..last (n days)`.
    pub fn date_span(&self, side: usize, row: usize) -> Option<String> {
        match &self.cmp {
            Cmp::Ref(pair) => match pair.content(side, row)? {
                Ok(index) => pair.target.date_span(side, index),
                Err(_) => None,
            },
            _ => None,
        }
    }

    /// The sorted day numbers of the service a reference names.
    pub fn days(&self, side: usize, row: usize) -> Option<&'a [i32]> {
        match &self.cmp {
            Cmp::Ref(pair) => pair.target.days(side, pair.content(side, row)?.ok()?),
            _ => None,
        }
    }

    /// One side's value as text, `None` for null.
    pub fn text(&self, side: usize, row: usize) -> Option<String> {
        if side == 0 {
            self.text_a(row)
        } else {
            self.text_b(row, None)
        }
    }

    /// The content indexes one side's references resolve to (for marking them used).
    pub fn referenced(&self, side: usize) -> impl Iterator<Item = u32> + '_ {
        let contents = match &self.cmp {
            Cmp::Ref(pair) if side == 0 => pair.a_content.as_slice(),
            Cmp::Ref(pair) => pair.b_content.as_slice(),
            _ => &[],
        };
        contents.iter().flatten().copied()
    }

    /// Whether A's row `ra` and B's row `rb` hold the same value.
    pub fn eq(&self, ra: usize, rb: usize) -> bool {
        match &self.cmp {
            Cmp::Str { a, b, to_b, .. } => match (a.code(ra), b.code(rb)) {
                (None, None) => true,
                (Some(code_a), Some(code_b)) => to_b[code_a as usize] == code_b,
                _ => false,
            },
            Cmp::Int16(a, b) => a[ra] == b[rb],
            Cmp::Int32(a, b) => a[ra] == b[rb],
            Cmp::Bool(a, b) => a[ra] == b[rb],
            Cmp::Date(a, b) => a[ra] == b[rb],
            Cmp::Double { a, b, tolerance } => match (a[ra], b[rb]) {
                (None, None) => true,
                (Some(x), Some(y)) => x.to_bits() == y.to_bits() || (x - y).abs() <= *tolerance,
                _ => false,
            },
            Cmp::Ref(pair) => pair.eq(ra, rb),
        }
    }

    /// A's value as text, `None` for null; a renamed value as `<A's> → <B's>`.
    pub fn text_a(&self, ra: usize) -> Option<String> {
        match &self.cmp {
            Cmp::Str { a, renamed, .. } => {
                let code = a.code(ra)? as usize;
                let value = &*a.pool()[code];
                Some(match renamed.as_ref().and_then(|r| r.names[code]) {
                    Some(name) => format!("{value} → {name}"),
                    None => value.to_owned(),
                })
            }
            Cmp::Int16(a, _) => a[ra].map(|v| v.to_string()),
            Cmp::Int32(a, _) => a[ra].map(|v| v.to_string()),
            Cmp::Bool(a, _) => a[ra].map(|v| v.to_string()),
            Cmp::Date(a, _) => a[ra].map(|v| v.to_string()),
            Cmp::Double { a, .. } => a[ra].map(|v| v.to_string()),
            Cmp::Ref(pair) => pair.text(0, ra, None),
        }
    }

    /// B's value as text, `None` for null; a reference also says how it differs from A's.
    pub fn text_b(&self, rb: usize, ra: Option<usize>) -> Option<String> {
        match &self.cmp {
            Cmp::Str { b, .. } => b.get(rb).map(str::to_owned),
            Cmp::Int16(_, b) => b[rb].map(|v| v.to_string()),
            Cmp::Int32(_, b) => b[rb].map(|v| v.to_string()),
            Cmp::Bool(_, b) => b[rb].map(|v| v.to_string()),
            Cmp::Date(_, b) => b[rb].map(|v| v.to_string()),
            Cmp::Double { b, .. } => b[rb].map(|v| v.to_string()),
            Cmp::Ref(pair) => pair.text(1, rb, ra),
        }
    }

    /// One side's value in a content key: a reference by its content, not its identifier.
    pub fn canonical_text(&self, side: usize, row: usize) -> String {
        match &self.cmp {
            Cmp::Ref(pair) => pair.canonical_text(side, row),
            _ if side == 0 => self.text_a(row).unwrap_or_else(|| "null".to_owned()),
            _ => self.text_b(row, None).unwrap_or_else(|| "null".to_owned()),
        }
    }

    /// Hashes of one side's cells, equal for equal values (doubles exactly).
    pub fn hasher(&self, side: usize) -> CellHasher<'_> {
        match &self.cmp {
            Cmp::Ref(pair) => CellHasher::Ref(pair, side),
            Cmp::Str {
                a,
                renamed: Some(renamed),
                ..
            } if side == 0 => CellHasher::Renamed(a, &renamed.hashes),
            _ => CellHasher::Plain(ValueHashes::new(self.columns[side])),
        }
    }
}

/// A string pair; with `renames`, A's values are taken as B names them.
fn strings<'a>(
    a: &'a StrColumn,
    b: &'a StrColumn,
    renames: Option<&'a HashMap<Box<str>, Box<str>>>,
) -> Cmp<'a> {
    let index: HashMap<&str, u32> = b
        .pool()
        .iter()
        .enumerate()
        .map(|(code, value)| (&**value, code as u32))
        .collect();
    let names: Vec<Option<&'a str>> = match renames {
        Some(renames) => a
            .pool()
            .iter()
            .map(|value| renames.get(value).map(|name| &**name))
            .collect(),
        None => Vec::new(),
    };
    let value = |code: usize| -> &str {
        names
            .get(code)
            .copied()
            .flatten()
            .unwrap_or(&a.pool()[code])
    };
    let codes = 0..a.pool().len();
    let to_b = codes
        .clone()
        .map(|code| index.get(value(code)).copied().unwrap_or(ABSENT))
        .collect();
    let hashes: Option<Vec<u64>> = names.iter().any(Option::is_some).then(|| {
        codes
            .map(|code| keys::text(value(code).as_bytes()))
            .collect()
    });
    let renamed = hashes.map(|hashes| Renamed { names, hashes });
    Cmp::Str {
        a,
        b,
        to_b,
        renamed,
    }
}

impl RefPair<'_> {
    fn content(&self, side: usize, row: usize) -> Option<Result<u32, &str>> {
        let (column, contents) = if side == 0 {
            (self.a, &self.a_content)
        } else {
            (self.b, &self.b_content)
        };
        let code = column.code(row)?;
        Some(contents[code as usize].ok_or_else(|| &*column.pool()[code as usize]))
    }

    fn eq(&self, ra: usize, rb: usize) -> bool {
        match (self.content(0, ra), self.content(1, rb)) {
            (None, None) => true,
            (Some(Ok(x)), Some(Ok(y))) => self.target.eq(x, y),
            (Some(Err(x)), Some(Err(y))) => x == y,
            _ => false,
        }
    }

    fn text(&self, side: usize, row: usize, other: Option<usize>) -> Option<String> {
        Some(match self.content(side, row)? {
            Err(id) => format!("{id} {{undefined}}"),
            Ok(index) => {
                let mut text = format!(
                    "{} {}",
                    self.target.id(side, index),
                    self.target.text(side, index)
                );
                if let Some(Some(Ok(reference))) = other.map(|ra| self.content(0, ra)) {
                    text.push_str(&self.target.difference(reference, index));
                }
                text
            }
        })
    }

    fn canonical_text(&self, side: usize, row: usize) -> String {
        match self.content(side, row) {
            None => "null".to_owned(),
            Some(Err(id)) => format!("{id} {{undefined}}"),
            Some(Ok(index)) => self.target.canonical_text(side, index),
        }
    }

    fn hash(&self, side: usize, row: usize) -> u64 {
        match self.content(side, row) {
            None => keys::null(),
            Some(Err(id)) => keys::combine([keys::null(), keys::text(id.as_bytes())]),
            Some(Ok(index)) => self.target.hash(side, index),
        }
    }
}

/// Cell hashes of one side.
pub(crate) enum CellHasher<'a> {
    Plain(ValueHashes<'a>),
    Ref(&'a RefPair<'a>, usize),
    /// A's strings through `Renamed`: hashes per pool code.
    Renamed(&'a StrColumn, &'a [u64]),
}

impl CellHasher<'_> {
    pub fn get(&self, row: usize) -> u64 {
        match self {
            CellHasher::Plain(values) => values.get_or_null(row),
            CellHasher::Ref(pair, side) => pair.hash(*side, row),
            CellHasher::Renamed(column, hashes) => column
                .code(row)
                .map_or_else(keys::null, |code| hashes[code as usize]),
        }
    }
}

/// Order of two rows of one column (nulls first, doubles by total order).
pub(crate) fn cmp_rows(column: &Column, x: usize, y: usize) -> Ordering {
    match column {
        Column::Str(values) => values.get(x).cmp(&values.get(y)),
        Column::Int16(values) => values[x].cmp(&values[y]),
        Column::Int32(values) => values[x].cmp(&values[y]),
        Column::Bool(values) => values[x].cmp(&values[y]),
        Column::Date(values) => values[x].cmp(&values[y]),
        Column::Double(values) => match (values[x], values[y]) {
            (Some(p), Some(q)) => p.total_cmp(&q),
            (p, q) => p.is_some().cmp(&q.is_some()),
        },
    }
}
