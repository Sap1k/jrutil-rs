//! Reviewed route rules (`jrunify-ext-geodata/routes/`, `docs/rules/jdf-convert.md` JDF-R405,
//! R406, R431): `transport-modes.csv` and `presentation.csv`.
//!
//! Both files have an exact header; cells are trimmed. A row that breaks the file grammar (as
//! the F# `RouteRules.fs` and `JdfGtfsRules.loadTransportModeRules` define it) is an error
//! naming its line. What a selector means (which routes it names, which of two rules is the
//! more specific) is part of the file's definition and is here, once, for both files; applying
//! the rules is the compiler's. Mode letters stay the JDF letters, colours stay as written.

pub const TRANSPORT_MODES_HEADER: [&str; 6] = [
    "agency_id",
    "licence",
    "public_line",
    "expected_mode",
    "effective_mode",
    "reason",
];
pub const PRESENTATION_HEADER: [&str; 6] = [
    "agency_id",
    "licence",
    "route_short_name",
    "route_color",
    "route_text_color",
    "reason",
];

/// The JDF transport-mode letters the file grammar allows (`A` bus, `E` tram, `T` trolleybus,
/// `L` cable car, `M` metro, `P` ferry).
pub const MODE_LETTERS: [&str; 6] = ["A", "E", "T", "L", "M", "P"];

/// The parsed `licence` cell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LicencePattern {
    /// All digits: one licence.
    Licence(Box<str>),
    /// `L-H`: both all digits, equally long, `L <= H`.
    Range { low: Box<str>, high: Box<str> },
    /// `N*`: a digit prefix; empty for `*` alone (every licence).
    Prefix(Box<str>),
}

impl LicencePattern {
    /// Whether a licence is one the pattern names (JDF-R406): equal; within an equally long
    /// range, compared as text; or starting with the prefix.
    pub fn matches(&self, licence: &str) -> bool {
        match self {
            LicencePattern::Licence(value) => licence == &**value,
            LicencePattern::Range { low, high } => {
                licence.len() == low.len() && licence >= &**low && licence <= &**high
            }
            LicencePattern::Prefix(prefix) => licence.starts_with(&**prefix),
        }
    }
}

/// Whether a rule's selector names a route (JDF-R406): the licence matches, and a selector with
/// an operator also needs the route's operator to be that one.
pub fn selector_matches(
    rule_agency: Option<&str>,
    pattern: &LicencePattern,
    agency_ico: Option<&str>,
    licence: &str,
) -> bool {
    pattern.matches(licence) && rule_agency.is_none_or(|rule| agency_ico == Some(rule))
}

/// How narrowly a selector names its routes (JDF-R406); larger is more specific. One licence
/// beats a range beats a prefix; a narrower range and a longer prefix beat a wider and a
/// shorter one; then a selector naming an operator beats one that does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Specificity {
    kind: u8,
    narrowness: i64,
    agency: bool,
}

pub fn specificity(rule_agency: Option<&str>, pattern: &LicencePattern) -> Specificity {
    let (kind, narrowness) = match pattern {
        LicencePattern::Licence(_) => (3, 0),
        LicencePattern::Range { low, high } => {
            // Equally long digit strings with `low <= high`; one too long to count is the widest.
            let width = match (low.parse::<i64>(), high.parse::<i64>()) {
                (Ok(low), Ok(high)) => high - low,
                _ => i64::MAX,
            };
            (2, -width)
        }
        LicencePattern::Prefix(prefix) => (1, prefix.len() as i64),
    };
    Specificity {
        kind,
        narrowness,
        agency: rule_agency.is_some(),
    }
}

/// The value of the most specific candidate (JDF-R406). `candidates` are the matching rules
/// that set the field, as (specificity, the rule's line, value). Two equally specific ones are
/// a mistake in the file: `Err` with their lines.
pub fn most_specific<T>(
    candidates: impl IntoIterator<Item = (Specificity, u64, T)>,
) -> Result<Option<T>, Vec<u64>> {
    let mut best: Option<(Specificity, Vec<u64>, T)> = None;
    for (specificity, line, value) in candidates {
        match &mut best {
            Some((top, lines, _)) if *top == specificity => lines.push(line),
            Some((top, _, _)) if *top > specificity => {}
            _ => best = Some((specificity, vec![line], value)),
        }
    }
    match best {
        None => Ok(None),
        Some((_, lines, value)) if lines.len() == 1 => Ok(Some(value)),
        Some((_, lines, _)) => Err(lines),
    }
}

/// An inclusive range of public line numbers (`N` or `N-M`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublicLines {
    pub low: i32,
    pub high: i32,
}

/// One row of `transport-modes.csv`.
#[derive(Clone, Debug, PartialEq)]
pub struct TransportModeRule {
    /// `agency_id`: an IČO (ASCII digits), `None` when blank.
    pub agency_ico: Option<Box<str>>,
    /// `licence` as written (trimmed).
    pub licence: Box<str>,
    pub licence_pattern: LicencePattern,
    pub public_lines: Option<PublicLines>,
    /// One of [`MODE_LETTERS`].
    pub expected_mode: Box<str>,
    /// One of [`MODE_LETTERS`].
    pub effective_mode: Box<str>,
    pub reason: Box<str>,
    pub line: u64,
}

/// One row of `presentation.csv`. At least one of the three overrides is set.
#[derive(Clone, Debug, PartialEq)]
pub struct PresentationRule {
    pub agency_ico: Option<Box<str>>,
    pub licence: Box<str>,
    pub licence_pattern: LicencePattern,
    pub route_short_name: Option<Box<str>>,
    /// Six hex digits, as written.
    pub route_color: Option<Box<str>>,
    /// Six hex digits, as written.
    pub route_text_color: Option<Box<str>>,
    pub reason: Box<str>,
    pub line: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum RouteRulesError {
    #[error("route rules: {0}")]
    Csv(#[from] csv::Error),
    #[error("route rules: header must be {expected}, found {found}")]
    Header { expected: String, found: String },
    #[error("route rules line {line}: {reason}")]
    Row { line: u64, reason: String },
}

/// Parses `transport-modes.csv`.
pub fn parse_transport_modes(csv: &[u8]) -> Result<Vec<TransportModeRule>, RouteRulesError> {
    read_rules(csv, &TRANSPORT_MODES_HEADER, |cells, line| {
        let (agency_ico, licence_pattern) = selector(cells[0], cells[1], line)?;
        Ok(TransportModeRule {
            agency_ico,
            licence: cells[1].into(),
            licence_pattern,
            public_lines: public_lines(cells[2], line)?,
            expected_mode: mode(cells[3], line)?,
            effective_mode: mode(cells[4], line)?,
            reason: reason(cells[5], line)?,
            line,
        })
    })
}

/// Parses `presentation.csv`.
pub fn parse_presentation(csv: &[u8]) -> Result<Vec<PresentationRule>, RouteRulesError> {
    read_rules(csv, &PRESENTATION_HEADER, |cells, line| {
        let (agency_ico, licence_pattern) = selector(cells[0], cells[1], line)?;
        let rule = PresentationRule {
            agency_ico,
            licence: cells[1].into(),
            licence_pattern,
            route_short_name: optional(cells[2]),
            route_color: colour(cells[3], "route_color", line)?,
            route_text_color: colour(cells[4], "route_text_color", line)?,
            reason: reason(cells[5], line)?,
            line,
        };
        if rule.route_short_name.is_none()
            && rule.route_color.is_none()
            && rule.route_text_color.is_none()
        {
            return Err(invalid(line, "rule overrides nothing"));
        }
        Ok(rule)
    })
}

fn read_rules<T>(
    csv: &[u8],
    header: &[&str],
    mut row: impl FnMut(&[&str], u64) -> Result<T, RouteRulesError>,
) -> Result<Vec<T>, RouteRulesError> {
    // Not flexible: a row with another number of columns than the header is a CSV error.
    let mut reader = csv::ReaderBuilder::new().has_headers(true).from_reader(csv);
    let found = reader.headers()?;
    if found.iter().ne(header.iter().copied()) {
        return Err(RouteRulesError::Header {
            expected: header.join(","),
            found: found.iter().collect::<Vec<_>>().join(","),
        });
    }
    let mut rules = Vec::new();
    let mut record = csv::StringRecord::new();
    while reader.read_record(&mut record)? {
        let cells: Vec<&str> = record.iter().map(str::trim).collect();
        rules.push(row(&cells, super::line_of(&record))?);
    }
    Ok(rules)
}

fn invalid(line: u64, reason: impl Into<String>) -> RouteRulesError {
    RouteRulesError::Row {
        line,
        reason: reason.into(),
    }
}

fn optional(cell: &str) -> Option<Box<str>> {
    (!cell.is_empty()).then(|| cell.into())
}

fn all_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

fn selector(
    agency: &str,
    licence: &str,
    line: u64,
) -> Result<(Option<Box<str>>, LicencePattern), RouteRulesError> {
    if !agency.is_empty() && !all_digits(agency) {
        return Err(invalid(line, format!("agency_id must be an IČO: {agency}")));
    }
    let pattern = licence_pattern(licence)
        .ok_or_else(|| invalid(line, format!("invalid licence pattern: {licence}")))?;
    Ok((optional(agency), pattern))
}

/// `N*` (prefix, `*` alone allowed), `L-H` (equal-length digit strings, `L <= H`), or digits.
fn licence_pattern(text: &str) -> Option<LicencePattern> {
    if let Some(prefix) = text.strip_suffix('*') {
        (prefix.is_empty() || all_digits(prefix)).then(|| LicencePattern::Prefix(prefix.into()))
    } else if text.contains('-') {
        let (low, high) = text.split_once('-')?;
        let valid = all_digits(low) && all_digits(high) && low.len() == high.len() && low <= high;
        valid.then(|| LicencePattern::Range {
            low: low.into(),
            high: high.into(),
        })
    } else {
        all_digits(text).then(|| LicencePattern::Licence(text.into()))
    }
}

fn public_lines(cell: &str, line: u64) -> Result<Option<PublicLines>, RouteRulesError> {
    if cell.is_empty() {
        return Ok(None);
    }
    let error = || invalid(line, format!("invalid public_line: {cell}"));
    let number = |text: &str| text.trim().parse::<i32>().map_err(|_| error());
    let (low, high) = match cell.split_once('-') {
        None => (number(cell)?, number(cell)?),
        Some((low, high)) => (number(low)?, number(high)?),
    };
    if low > high {
        return Err(error());
    }
    Ok(Some(PublicLines { low, high }))
}

fn mode(cell: &str, line: u64) -> Result<Box<str>, RouteRulesError> {
    if MODE_LETTERS.contains(&cell) {
        Ok(cell.into())
    } else {
        Err(invalid(line, format!("unknown JDF transport mode: {cell}")))
    }
}

fn colour(cell: &str, column: &str, line: u64) -> Result<Option<Box<str>>, RouteRulesError> {
    if cell.is_empty() {
        Ok(None)
    } else if cell.len() == 6 && cell.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(Some(cell.into()))
    } else {
        Err(invalid(
            line,
            format!("{column} must be six hex digits: {cell}"),
        ))
    }
}

fn reason(cell: &str, line: u64) -> Result<Box<str>, RouteRulesError> {
    optional(cell).ok_or_else(|| invalid(line, "reason is required"))
}
