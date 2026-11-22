//! Reviewed stop overrides of a regional source (`jrunify-ext-geodata/overlay/*.csv`,
//! `docs/rules/overlay.md` OVL-R040): which base stop place a source's stop group is.
//!
//! The header is exact; cells are trimmed. What a namespace means, whether a row is in force
//! and what it wins over are the overlay's rules.

use jr_model::Date;

pub const HEADER: [&str; 7] = [
    "source_namespace",
    "source_id",
    "target_namespace",
    "target_id",
    "valid_from",
    "valid_to",
    "review_note",
];

/// One row, as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub source_namespace: Box<str>,
    pub source_id: Box<str>,
    pub target_namespace: Box<str>,
    pub target_id: Box<str>,
    /// `YYYYMMDD`; `None` when the cell is empty.
    pub valid_from: Option<Date>,
    pub valid_to: Option<Date>,
    pub review_note: Box<str>,
    pub line: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum StopOverridesError {
    #[error("stop overrides: {0}")]
    Csv(#[from] csv::Error),
    #[error("stop overrides: header must be {expected}, found {found}")]
    Header { expected: String, found: String },
    #[error("stop overrides line {line}: {column} is not a date (YYYYMMDD): {value}")]
    Date {
        line: u64,
        column: &'static str,
        value: String,
    },
}

pub fn parse(csv: &[u8]) -> Result<Vec<Row>, StopOverridesError> {
    let mut reader = csv::ReaderBuilder::new().has_headers(true).from_reader(csv);
    let found = reader.headers()?;
    if found.iter().map(str::trim).ne(HEADER.iter().copied()) {
        return Err(StopOverridesError::Header {
            expected: HEADER.join(","),
            found: found.iter().collect::<Vec<_>>().join(","),
        });
    }
    let mut rows = Vec::new();
    let mut record = csv::StringRecord::new();
    while reader.read_record(&mut record)? {
        let line = super::line_of(&record);
        let cell = |index: usize| record.get(index).unwrap_or("").trim();
        let date = |index: usize, column: &'static str| {
            let value = cell(index);
            if value.is_empty() {
                return Ok(None);
            }
            Date::strptime("%Y%m%d", value)
                .map(Some)
                .map_err(|_| StopOverridesError::Date {
                    line,
                    column,
                    value: value.to_owned(),
                })
        };
        rows.push(Row {
            source_namespace: cell(0).into(),
            source_id: cell(1).into(),
            target_namespace: cell(2).into(),
            target_id: cell(3).into(),
            valid_from: date(4, "valid_from")?,
            valid_to: date(5, "valid_to")?,
            review_note: cell(6).into(),
            line,
        });
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_read_as_written_and_a_bad_date_is_an_error() {
        let header = HEADER.join(",");
        let text = format!(
            "{header}\npid-stop-group,U1000S1,jdf-stop-place-name,\"Praha,Letňany\",20251214,,note\n"
        );
        let rows = parse(text.as_bytes()).unwrap();
        assert_eq!(&*rows[0].target_id, "Praha,Letňany");
        assert_eq!(rows[0].valid_from, Some(jiff::civil::date(2025, 12, 14)));
        assert_eq!((rows[0].valid_to, rows[0].line), (None, 2));
        let bad = format!("{header}\na,b,c,d,2025-12-14,,n\n");
        assert!(matches!(
            parse(bad.as_bytes()),
            Err(StopOverridesError::Date { line: 2, .. })
        ));
        assert!(parse(b"a,b\n").is_err());
    }
}
