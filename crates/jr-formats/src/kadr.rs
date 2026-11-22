//! The KADR catalogue snapshot (`catalog.json`), as the file holds it.
//!
//! Every field except `code` is optional and is `None` when the key is absent or `null`; text is
//! kept verbatim (an empty string stays `Some("")`). Dates are ISO `yyyy-mm-dd`. An entry without
//! a `code`, or with a date that does not parse, is not returned in its list but in
//! [`Catalog::rejected`], so that nothing is dropped silently and a bad date never reads as an
//! open bound. A missing array reads as an empty list (older snapshots lack `central_notes`).
//! A field holding something other than a string or `null` is a [`KadrError`].

use jr_model::Date;
use serde_json::{Map, Value};

/// `catalog.json`. Lists are in file order; duplicate codes are kept.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    pub lines: Vec<Line>,
    pub companies: Vec<Company>,
    pub ids: Vec<IdsSystem>,
    pub train_types: Vec<CodeLabel>,
    pub commercial_train_types: Vec<CodeLabel>,
    pub central_notes: Vec<CentralNote>,
    /// Entries left out of the lists above, in file order per array.
    pub rejected: Vec<Rejected>,
}

/// `lines[]`: a KADR passenger line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub code: Box<str>,
    pub abbreviation: Option<Box<str>>,
    pub mark: Option<Box<str>>,
    pub name: Option<Box<str>>,
    pub valid_from: Option<Date>,
    pub valid_to: Option<Date>,
}

/// `companies[]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Company {
    pub code: Box<str>,
    pub name: Option<Box<str>>,
    pub url: Option<Box<str>>,
}

/// `ids[]`: an integrated transport system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdsSystem {
    pub code: Box<str>,
    pub abbreviation: Option<Box<str>>,
    pub name: Option<Box<str>>,
    pub note: Option<Box<str>>,
    pub valid_from: Option<Date>,
    pub valid_to: Option<Date>,
}

/// `train_types[]`, `commercial_train_types[]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeLabel {
    pub code: Box<str>,
    pub abbreviation: Option<Box<str>>,
}

/// `central_notes[]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CentralNote {
    pub code: Box<str>,
    pub name: Option<Box<str>>,
    pub text: Option<Box<str>>,
    pub valid_from: Option<Date>,
    pub valid_to: Option<Date>,
}

/// An entry that could not be returned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejected {
    /// The array, e.g. `"lines"`.
    pub array: &'static str,
    /// 0-based position in the array.
    pub index: usize,
    pub problem: Problem,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// No `code`, or `code` is `null`.
    MissingCode,
    /// A date field that is not `yyyy-mm-dd`.
    BadDate {
        field: &'static str,
        value: Box<str>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum KadrError {
    #[error("catalog.json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("catalog.json: the document is not an object")]
    NotObject,
    #[error("catalog.json: {0} is not an array")]
    NotArray(&'static str),
    #[error("catalog.json: {array}[{index}] is not an object")]
    EntryNotObject { array: &'static str, index: usize },
    #[error("catalog.json: {array}[{index}].{field} is neither a string nor null")]
    FieldType {
        array: &'static str,
        index: usize,
        field: &'static str,
    },
}

impl Catalog {
    pub fn parse(json: &str) -> Result<Catalog, KadrError> {
        let root: Value = serde_json::from_str(json)?;
        let root = root.as_object().ok_or(KadrError::NotObject)?;
        let mut catalog = Catalog::default();
        let rejected = &mut catalog.rejected;
        catalog.lines = entries(root, "lines", rejected, |e| {
            Ok(Line {
                code: e.code.clone(),
                abbreviation: e.text("abbreviation")?,
                mark: e.text("mark")?,
                name: e.text("name")?,
                valid_from: e.date("valid_from")?,
                valid_to: e.date("valid_to")?,
            })
        })?;
        catalog.companies = entries(root, "companies", rejected, |e| {
            Ok(Company {
                code: e.code.clone(),
                name: e.text("name")?,
                url: e.text("url")?,
            })
        })?;
        catalog.ids = entries(root, "ids", rejected, |e| {
            Ok(IdsSystem {
                code: e.code.clone(),
                abbreviation: e.text("abbreviation")?,
                name: e.text("name")?,
                note: e.text("note")?,
                valid_from: e.date("valid_from")?,
                valid_to: e.date("valid_to")?,
            })
        })?;
        catalog.train_types = entries(root, "train_types", rejected, code_label)?;
        catalog.commercial_train_types =
            entries(root, "commercial_train_types", rejected, code_label)?;
        catalog.central_notes = entries(root, "central_notes", rejected, |e| {
            Ok(CentralNote {
                code: e.code.clone(),
                name: e.text("name")?,
                text: e.text("text")?,
                valid_from: e.date("valid_from")?,
                valid_to: e.date("valid_to")?,
            })
        })?;
        Ok(catalog)
    }
}

fn code_label(e: &Entry<'_>) -> Result<CodeLabel, Fault> {
    Ok(CodeLabel {
        code: e.code.clone(),
        abbreviation: e.text("abbreviation")?,
    })
}

/// One array entry with its `code` present.
struct Entry<'a> {
    object: &'a Map<String, Value>,
    code: Box<str>,
}

/// Why an entry could not be read: rejected, or the file is broken.
enum Fault {
    Rejected(Problem),
    Type(&'static str),
}

impl Entry<'_> {
    fn text(&self, field: &'static str) -> Result<Option<Box<str>>, Fault> {
        text(self.object, field).map_err(|()| Fault::Type(field))
    }

    fn date(&self, field: &'static str) -> Result<Option<Date>, Fault> {
        let Some(value) = self.text(field)? else {
            return Ok(None);
        };
        match value.parse::<Date>() {
            Ok(date) if is_iso_date(&value) => Ok(Some(date)),
            _ => Err(Fault::Rejected(Problem::BadDate { field, value })),
        }
    }
}

fn is_iso_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
}

fn text(object: &Map<String, Value>, field: &str) -> Result<Option<Box<str>>, ()> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.as_str().into())),
        Some(_) => Err(()),
    }
}

fn entries<T>(
    root: &Map<String, Value>,
    array: &'static str,
    rejected: &mut Vec<Rejected>,
    read: impl Fn(&Entry<'_>) -> Result<T, Fault>,
) -> Result<Vec<T>, KadrError> {
    let items = match root.get(array) {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(items)) => items,
        Some(_) => return Err(KadrError::NotArray(array)),
    };
    let mut out = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let object = item
            .as_object()
            .ok_or(KadrError::EntryNotObject { array, index })?;
        let code = text(object, "code").map_err(|()| KadrError::FieldType {
            array,
            index,
            field: "code",
        })?;
        let Some(code) = code else {
            rejected.push(Rejected {
                array,
                index,
                problem: Problem::MissingCode,
            });
            continue;
        };
        match read(&Entry { object, code }) {
            Ok(value) => out.push(value),
            Err(Fault::Rejected(problem)) => rejected.push(Rejected {
                array,
                index,
                problem,
            }),
            Err(Fault::Type(field)) => {
                return Err(KadrError::FieldType {
                    array,
                    index,
                    field,
                });
            }
        }
    }
    Ok(out)
}
