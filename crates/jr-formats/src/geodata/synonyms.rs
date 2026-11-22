//! Stop-name synonym lines (`StopMatcherSynonyms.txt`, `docs/rules/jdf-merge.md` JDF-R215):
//! per line, the text before `#` holds comma-separated alternatives.
//!
//! Lines are returned as written: alternatives trimmed and in file order, not lower-cased.
//! Choosing a canonical form and resolving lines that share an alternative are the compiler's.

/// One synonym line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// The non-blank alternatives, trimmed, in file order.
    pub alternatives: Vec<Box<str>>,
    /// The 1-based line number.
    pub line: u32,
}

/// Parses the synonym file. Comment-only and blank lines, and lines without any non-blank
/// alternative, are skipped.
pub fn parse(text: &str) -> Vec<Line> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    text.lines()
        .zip(1..)
        .filter_map(|(content, line)| {
            let content = content.split('#').next().unwrap_or_default();
            let alternatives: Vec<Box<str>> = content
                .split(',')
                .map(str::trim)
                .filter(|alternative| !alternative.is_empty())
                .map(Box::from)
                .collect();
            (!alternatives.is_empty()).then_some(Line { alternatives, line })
        })
        .collect()
}
