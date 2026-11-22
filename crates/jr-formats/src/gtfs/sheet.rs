//! One CSV member: header, rows with their lines, cells and their parsers.

use csv::StringRecord;
use jr_model::Date;

use super::{GtfsError, Problem, parse_date, parse_number, parse_time};

/// A CSV member read as text: the header and every row with as many fields as the header, cells
/// trimmed of surrounding whitespace, in file order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Table {
    pub header: Vec<Box<str>>,
    pub rows: Vec<Box<[Box<str>]>>,
    /// Each row's 1-based line.
    pub lines: Vec<u64>,
}

impl Table {
    /// The position of the first column of this name.
    pub fn column(&self, name: &str) -> Option<usize> {
        self.header.iter().position(|column| &**column == name)
    }
}

pub(super) struct Sheet<'a> {
    file: &'a str,
    reader: csv::Reader<&'a [u8]>,
    header: Vec<Box<str>>,
}

impl<'a> Sheet<'a> {
    /// Reads the header. A member without a header line has an empty header.
    pub fn open(
        file: &'a str,
        bytes: &'a [u8],
        problems: &mut Vec<Problem>,
    ) -> Result<Sheet<'a>, GtfsError> {
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .from_reader(bytes);
        let mut record = StringRecord::new();
        let mut header: Vec<Box<str>> = Vec::new();
        if reader
            .read_record(&mut record)
            .map_err(|error| csv_error(file, error))?
        {
            for name in &record {
                let name = name.trim();
                if header.iter().any(|seen| &**seen == name) {
                    problems.push(Problem {
                        file: file.into(),
                        line: line_of(&record),
                        column: Some(name.into()),
                        message: "duplicate column; the first is read".into(),
                    });
                }
                header.push(name.into());
            }
        }
        Ok(Sheet {
            file,
            reader,
            header,
        })
    }

    pub fn has_header(&self) -> bool {
        !self.header.is_empty()
    }

    pub fn require(&self, columns: &[&'static str]) -> Result<(), GtfsError> {
        match columns.iter().find(|name| self.column(name).is_none()) {
            Some(column) => Err(GtfsError::MissingColumn {
                file: self.file.into(),
                column,
            }),
            None => Ok(()),
        }
    }

    /// An optional member without a header line is empty: a problem, and `false`.
    pub fn present(&self, problems: &mut Vec<Problem>) -> bool {
        if !self.has_header() {
            problems.push(Problem {
                file: self.file.into(),
                line: 0,
                column: None,
                message: "empty file; read as no rows".into(),
            });
        }
        self.has_header()
    }

    pub fn column(&self, name: &str) -> Option<usize> {
        self.header.iter().position(|column| &**column == name)
    }

    pub fn columns<const N: usize>(&self, names: [&str; N]) -> [Option<usize>; N] {
        names.map(|name| self.column(name))
    }

    pub fn columns_of(&self, names: &[&str]) -> Vec<Option<usize>> {
        names.iter().map(|name| self.column(name)).collect()
    }

    /// Calls `each` for every row with as many fields as the header; a row with another number
    /// of fields is a problem and skipped.
    pub fn rows(
        mut self,
        problems: &mut Vec<Problem>,
        mut each: impl FnMut(&mut Row<'_>),
    ) -> Result<(), GtfsError> {
        let mut record = StringRecord::new();
        while self
            .reader
            .read_record(&mut record)
            .map_err(|error| csv_error(self.file, error))?
        {
            let line = line_of(&record);
            if record.len() != self.header.len() {
                problems.push(Problem {
                    file: self.file.into(),
                    line,
                    column: None,
                    message: format!(
                        "{} fields, the header has {}; row skipped",
                        record.len(),
                        self.header.len()
                    )
                    .into(),
                });
                continue;
            }
            each(&mut Row {
                file: self.file,
                header: &self.header,
                record: &record,
                line,
                problems,
            });
        }
        Ok(())
    }

    pub fn table(self, problems: &mut Vec<Problem>) -> Result<Table, GtfsError> {
        let mut table = Table {
            header: self.header.clone(),
            ..Table::default()
        };
        self.rows(problems, |row| {
            table
                .rows
                .push(row.record.iter().map(|cell| cell.trim().into()).collect());
            table.lines.push(row.line);
        })?;
        Ok(table)
    }
}

pub(super) struct Row<'r> {
    file: &'r str,
    header: &'r [Box<str>],
    record: &'r StringRecord,
    pub line: u64,
    problems: &'r mut Vec<Problem>,
}

impl<'r> Row<'r> {
    /// The trimmed cell; `None` when the column is absent or the cell empty.
    pub fn cell(&self, column: Option<usize>) -> Option<&'r str> {
        let record: &'r StringRecord = self.record;
        record
            .get(column?)
            .map(str::trim)
            .filter(|cell| !cell.is_empty())
    }

    pub fn text(&self, column: Option<usize>) -> Option<Box<str>> {
        self.cell(column).map(Into::into)
    }

    /// A required key: an empty cell is a problem and the empty string.
    pub fn key(&mut self, column: Option<usize>) -> Box<str> {
        match self.cell(column) {
            Some(cell) => cell.into(),
            None => {
                self.problem(column, "empty required value".into());
                "".into()
            }
        }
    }

    pub fn int(&mut self, column: Option<usize>) -> Option<i32> {
        self.parse(column, "an integer", |cell| cell.parse().ok())
    }

    pub fn number(&mut self, column: Option<usize>) -> Option<f64> {
        self.parse(column, "a number", parse_number)
    }

    pub fn date(&mut self, column: Option<usize>) -> Option<Date> {
        self.parse(column, "a YYYYMMDD date", parse_date)
    }

    pub fn time(&mut self, column: Option<usize>) -> Option<i32> {
        self.parse(column, "an H:MM:SS time", parse_time)
    }

    /// The requested extra cells, in request order.
    pub fn extras(&self, columns: &[Option<usize>]) -> Box<[Option<Box<str>>]> {
        columns.iter().map(|&column| self.text(column)).collect()
    }

    /// Parses a non-empty cell; one that does not parse is a problem and `None`.
    pub fn parse<T>(
        &mut self,
        column: Option<usize>,
        expected: &str,
        parse: impl FnOnce(&str) -> Option<T>,
    ) -> Option<T> {
        let cell = self.cell(column)?;
        let value = parse(cell);
        if value.is_none() {
            self.problem(column, format!("not {expected}: \"{cell}\"").into());
        }
        value
    }

    pub fn problem(&mut self, column: Option<usize>, message: Box<str>) {
        self.problems.push(Problem {
            file: self.file.into(),
            line: self.line,
            column: column.map(|column| self.header[column].clone()),
            message,
        });
    }
}

fn line_of(record: &StringRecord) -> u64 {
    record.position().map_or(0, csv::Position::line)
}

fn csv_error(file: &str, error: csv::Error) -> GtfsError {
    match error.kind() {
        csv::ErrorKind::Utf8 { pos, .. } => GtfsError::Utf8 {
            file: file.into(),
            line: pos.as_ref().map_or(0, csv::Position::line),
        },
        _ => GtfsError::Csv {
            file: file.into(),
            source: error,
        },
    }
}
