//! `SR70.csv`: the SŽ list of railway points, headerless, `code6,name,lat,lon`.
//!
//! Every row is returned as written: the code untouched (six digits in real data; shortening it
//! is a compiler rule), the name verbatim (`None` only when empty; placeholders such as `-` stay
//! text), the coordinates as numbers without any range check. A coordinate whose text is not a
//! finite decimal number (empty, `NaN`, `inf`, anything else) is `None`, and that text is kept
//! nowhere. Columns after the fourth are kept in [`Row::extra`].

/// One row of `SR70.csv`.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// Column 1.
    pub code: Box<str>,
    /// Column 2.
    pub name: Option<Box<str>>,
    /// Column 3, if present and a finite number.
    pub latitude: Option<f64>,
    /// Column 4, if present and a finite number.
    pub longitude: Option<f64>,
    /// Columns 5 and later, verbatim (none in real data).
    pub extra: Vec<Box<str>>,
    /// The 1-based line the row starts on.
    pub line: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum Sr70Error {
    #[error("SR70.csv: {0}")]
    Csv(#[from] csv::Error),
    #[error("SR70.csv line {line}: {columns} column(s), at least code and name expected")]
    TooFewColumns { line: u64, columns: usize },
}

/// Parses every row of `SR70.csv` (UTF-8, a leading byte-order mark is ignored).
pub fn parse(csv: &[u8]) -> Result<Vec<Row>, Sr70Error> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(csv);
    let mut rows = Vec::new();
    let mut record = csv::StringRecord::new();
    while reader.read_record(&mut record)? {
        let line = record.position().map_or(0, csv::Position::line);
        if record.len() < 2 {
            return Err(Sr70Error::TooFewColumns {
                line,
                columns: record.len(),
            });
        }
        let number = |index: usize| {
            record
                .get(index)
                .and_then(|text| text.trim().parse::<f64>().ok())
                .filter(|value| value.is_finite())
        };
        rows.push(Row {
            code: record[0].into(),
            name: Some(&record[1])
                .filter(|name| !name.is_empty())
                .map(Box::from),
            latitude: number(2),
            longitude: number(3),
            extra: record.iter().skip(4).map(Box::from).collect(),
            line,
        });
    }
    Ok(rows)
}
