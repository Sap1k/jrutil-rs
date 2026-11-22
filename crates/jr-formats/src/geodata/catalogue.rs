//! External stop catalogues (`jrunify-ext-geodata/other/*.csv`, `docs/rules/jdf-read.md` 2.6,
//! JDF-R005..R008): headerless rows `name,latitude,longitude,okres,country`.
//!
//! The reader decodes one file. Choosing the files, their order, the source label and the row
//! identity derived from the relative path (R005, R007) are the caller's; validating
//! coordinates (R006) and collapsing duplicates (R008) are the compiler's.

/// One catalogue row.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// Column 1 verbatim: the JDF-style `town,district,nearby` name (itself a quoted cell when it
    /// holds commas).
    pub name: Box<str>,
    /// Column 2, when its text is a finite decimal number (no range check).
    pub latitude: Option<f64>,
    /// Column 3, when its text is a finite decimal number (no range check).
    pub longitude: Option<f64>,
    /// Column 4 verbatim, `None` when blank.
    pub okres: Option<Box<str>>,
    /// Column 5 verbatim, `None` when blank.
    pub country: Option<Box<str>>,
    /// The 1-based line the row starts on.
    pub line: u64,
    /// The five columns' text verbatim (unquoted): the row's identity is a hash of them
    /// (JDF-R007).
    pub columns: [Box<str>; 5],
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogueError {
    #[error("stop catalogue: {0}")]
    Csv(#[from] csv::Error),
    #[error("stop catalogue line {line}: {columns} column(s), five expected")]
    Columns { line: u64, columns: usize },
}

/// Parses every row of one catalogue (UTF-8, a leading byte-order mark is ignored). A row
/// without exactly five columns is an error: that is the obsolete town format (R006).
pub fn parse(csv: &[u8]) -> Result<Vec<Row>, CatalogueError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(csv);
    let mut rows = Vec::new();
    let mut record = csv::StringRecord::new();
    while reader.read_record(&mut record)? {
        let line = super::line_of(&record);
        if record.len() != 5 {
            return Err(CatalogueError::Columns {
                line,
                columns: record.len(),
            });
        }
        let verbatim = |index: usize| {
            Some(&record[index])
                .filter(|text| !text.trim().is_empty())
                .map(Box::from)
        };
        rows.push(Row {
            name: record[0].into(),
            latitude: super::finite_number(&record[1]),
            longitude: super::finite_number(&record[2]),
            okres: verbatim(3),
            country: verbatim(4),
            line,
            columns: [0, 1, 2, 3, 4].map(|index| Box::from(&record[index])),
        });
    }
    Ok(rows)
}
