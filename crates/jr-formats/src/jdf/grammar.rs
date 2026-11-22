//! JDF-R020–R023: text decoding, the record grammar and typed field access.
//!
//! A record is `"f1","f2",…,"fn";` followed by CR LF. Fields are split on `","` with no
//! unescaping, so a quote inside a field stays as written. Tolerated and reported: a record ended
//! by LF alone, a final record without its terminator, whitespace between records, a byte-order
//! mark. Skipped and reported: an empty record, one that does not start with a quote, one with too
//! few fields, one whose required field does not decode.

use std::borrow::Cow;

use jr_model::Date;

use super::Problem;
use super::codes::{Km, TimeCell};

/// Decodes a file: CP1250, unless a byte-order mark says otherwise (honoured and reported).
pub(super) fn decode<'b>(
    bytes: &'b [u8],
    file: &'static str,
    problems: &mut Vec<Problem>,
) -> Cow<'b, str> {
    match encoding_rs::Encoding::for_bom(bytes) {
        Some((encoding, bom)) => {
            problems.push(problem(
                file,
                0,
                None,
                format!("byte-order mark: decoded as {}", encoding.name()),
            ));
            encoding.decode_without_bom_handling(&bytes[bom..]).0
        }
        None => {
            encoding_rs::WINDOWS_1250
                .decode_without_bom_handling(bytes)
                .0
        }
    }
}

pub(super) fn problem(
    file: &str,
    record: u32,
    field: Option<&'static str>,
    message: impl Into<Box<str>>,
) -> Problem {
    Problem {
        file: file.into(),
        record,
        field,
        message: message.into(),
    }
}

/// Calls `each` with the number (1-based) and fields of every record that splits.
pub(super) fn records<'t>(
    text: &'t str,
    file: &'static str,
    problems: &mut Vec<Problem>,
    mut each: impl FnMut(u32, &[&'t str], &mut Vec<Problem>),
) {
    let bytes = text.as_bytes();
    let mut fields: Vec<&'t str> = Vec::with_capacity(16);
    let mut position = 0;
    let mut number = 0u32;
    let mut lf_reported = false;
    while position < bytes.len() {
        let (raw, next) = match terminator(text, position) {
            Some((end, next, ending)) => {
                number += 1;
                match ending {
                    Ending::CrLf => {}
                    Ending::Lf if !lf_reported => {
                        lf_reported = true;
                        problems.push(problem(
                            file,
                            number,
                            None,
                            "record ended by LF without CR (reported once per file)",
                        ));
                    }
                    Ending::Lf => {}
                    Ending::None => problems.push(problem(
                        file,
                        number,
                        None,
                        "record without line end after its terminator",
                    )),
                }
                (&text[position..end], next)
            }
            None => {
                let tail = text[position..].trim_end();
                if tail.trim_start().is_empty() {
                    break;
                }
                number += 1;
                problems.push(problem(file, number, None, "record without terminator"));
                let tail = tail.strip_suffix(';').unwrap_or(tail);
                (tail.strip_suffix('"').unwrap_or(tail), bytes.len())
            }
        };
        position = next;
        let trimmed = raw.trim_start();
        if trimmed.len() != raw.len() {
            problems.push(problem(file, number, None, "whitespace before the record"));
        }
        let Some(body) = trimmed.strip_prefix('"') else {
            let message = if trimmed.is_empty() {
                "empty record (skipped)"
            } else {
                "record does not start with a quote (skipped)"
            };
            problems.push(problem(file, number, None, message));
            continue;
        };
        fields.clear();
        fields.extend(body.split("\",\""));
        each(number, &fields, problems);
    }
}

enum Ending {
    CrLf,
    Lf,
    None,
}

/// The next `";` at or after `from` that ends a record (followed by CR LF, LF or the end of the
/// text): the record's end (before the closing quote) and where the next record starts.
fn terminator(text: &str, from: usize) -> Option<(usize, usize, Ending)> {
    let bytes = text.as_bytes();
    let mut search = from;
    loop {
        let at = search + text[search..].find("\";")?;
        let after = at + 2;
        let rest = &bytes[after..];
        if rest.starts_with(b"\r\n") {
            return Some((at, after + 2, Ending::CrLf));
        }
        if rest.starts_with(b"\n") {
            return Some((at, after + 1, Ending::Lf));
        }
        if rest.is_empty() {
            return Some((at, after, Ending::None));
        }
        search = at + 1;
    }
}

/// A field's 1-based position in the three layouts (1.8/1.9, 1.10, 1.11); 0 = not in that layout.
pub(super) type Col = [u8; 3];

/// A field at position `n` in every layout.
pub(super) const fn all(n: u8) -> Col {
    [n, n, n]
}

/// A field at different positions per layout.
pub(super) const fn at(v1_9: u8, v1_10: u8, v1_11: u8) -> Col {
    [v1_9, v1_10, v1_11]
}

/// A record was skipped; the problem has been reported.
pub(super) struct Skip;

/// A typed scalar a field decodes to.
pub(super) trait Scalar: Sized {
    /// What the field should hold, for problem messages.
    const WHAT: &'static str;
    fn parse(text: &str) -> Option<Self>;
}

impl Scalar for u64 {
    const WHAT: &'static str = "not a number";
    fn parse(text: &str) -> Option<u64> {
        if text.is_empty() {
            return None;
        }
        text.bytes().try_fold(0u64, |value, byte| {
            byte.is_ascii_digit()
                .then_some(())
                .and_then(|()| value.checked_mul(10)?.checked_add(u64::from(byte - b'0')))
        })
    }
}

impl Scalar for u32 {
    const WHAT: &'static str = "not a number";
    fn parse(text: &str) -> Option<u32> {
        <u64 as Scalar>::parse(text).and_then(|value| u32::try_from(value).ok())
    }
}

impl Scalar for bool {
    const WHAT: &'static str = "not a flag (0 or 1)";
    fn parse(text: &str) -> Option<bool> {
        match text {
            "0" => Some(false),
            "1" => Some(true),
            _ => None,
        }
    }
}

impl Scalar for Date {
    const WHAT: &'static str = "not a date (ddMMyyyy)";
    fn parse(text: &str) -> Option<Date> {
        let bytes = text.as_bytes();
        if bytes.len() != 8 || !bytes.iter().all(u8::is_ascii_digit) {
            return None;
        }
        let number = |range: std::ops::Range<usize>| text[range].parse::<i16>().ok();
        let day = i8::try_from(number(0..2)?).ok()?;
        let month = i8::try_from(number(2..4)?).ok()?;
        Date::new(number(4..8)?, month, day).ok()
    }
}

impl Scalar for Km {
    const WHAT: &'static str = "not a kilometre value";
    fn parse(text: &str) -> Option<Km> {
        Km::parse(text)
    }
}

impl Scalar for TimeCell {
    const WHAT: &'static str = "not a time (HHMM, | or <)";
    fn parse(text: &str) -> Option<TimeCell> {
        TimeCell::parse(text)
    }
}

/// One split record of a relation, read in the batch's layout.
pub(super) struct Row<'r, 't> {
    pub(super) fields: &'r [&'t str],
    /// 0 = 1.8/1.9, 1 = 1.10, 2 = 1.11.
    pub(super) layout: usize,
    pub(super) file: &'static str,
    pub(super) record: u32,
    pub(super) problems: &'r mut Vec<Problem>,
}

impl<'t> Row<'_, 't> {
    fn raw(&self, col: Col) -> Option<&'t str> {
        let position = usize::from(col[self.layout]);
        position
            .checked_sub(1)
            .and_then(|index| self.fields.get(index).copied())
    }

    fn report(&mut self, field: &'static str, message: String) {
        self.problems
            .push(problem(self.file, self.record, Some(field), message));
    }

    /// The text of a field the layout must have; `Skip` if it does not (never after the field
    /// count check).
    fn present(&mut self, col: Col, field: &'static str) -> Result<&'t str, Skip> {
        match self.raw(col) {
            Some(text) => Ok(text),
            None => {
                self.report(field, "field missing (record skipped)".into());
                Err(Skip)
            }
        }
    }

    /// A required text field, as written (possibly empty).
    pub(super) fn text(&mut self, col: Col, field: &'static str) -> Result<Box<str>, Skip> {
        self.present(col, field).map(Box::from)
    }

    /// An optional text field: `None` when empty or not in the layout.
    pub(super) fn opt_text(&self, col: Col) -> Option<Box<str>> {
        self.raw(col).filter(|text| !text.is_empty()).map(Box::from)
    }

    /// An optional code field: `None` when empty or not in the layout.
    pub(super) fn code<T>(&self, col: Col, from_code: fn(&str) -> T) -> Option<T> {
        self.raw(col).filter(|text| !text.is_empty()).map(from_code)
    }

    /// A field every layout has and that must decode.
    pub(super) fn req<T: Scalar>(&mut self, col: Col, field: &'static str) -> Result<T, Skip> {
        let text = self.present(col, field)?;
        T::parse(text).ok_or_else(|| {
            self.report(field, format!("{} (record skipped): \"{text}\"", T::WHAT));
            Skip
        })
    }

    /// A field that must decode where the layout has it: `None` when it does not.
    pub(super) fn req_where<T: Scalar>(
        &mut self,
        col: Col,
        field: &'static str,
    ) -> Result<Option<T>, Skip> {
        if col[self.layout] == 0 {
            return Ok(None);
        }
        self.req(col, field).map(Some)
    }

    /// An optional field: `None` when empty, not in the layout, or not decodable (reported).
    pub(super) fn opt<T: Scalar>(&mut self, col: Col, field: &'static str) -> Option<T> {
        let text = self.raw(col).filter(|text| !text.is_empty())?;
        let value = T::parse(text);
        if value.is_none() {
            self.report(field, format!("{}: \"{text}\"", T::WHAT));
        }
        value
    }

    /// A call time cell: `None` when not in the layout or not decodable (reported); an empty
    /// cell is [`TimeCell::Empty`].
    pub(super) fn cell(&mut self, col: Col, field: &'static str) -> Option<TimeCell> {
        let text = self.raw(col)?;
        let value = TimeCell::parse(text);
        if value.is_none() {
            self.report(field, format!("{}: \"{text}\"", TimeCell::WHAT));
        }
        value
    }

    /// Attribute slots: the referenced `Pevnykod.txt` numbers.
    pub(super) fn slots<const N: usize>(&mut self, cols: [Col; N]) -> [Option<u32>; N] {
        cols.map(|col| self.opt(col, "attributes"))
    }
}
