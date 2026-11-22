//! The filtered road feed's rules (`jrunify-ext-geodata/filtered-jdf/rules-v1.json`,
//! `docs/rules/jdf-convert.md` section 6): which lines regional and operators' own feeds
//! publish, so the filtered feed leaves them out.

use std::collections::BTreeMap;

use serde::Deserialize;

/// The rules file. `source` and `ids_names` are documentation carried by the file.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilterRules {
    pub schema_version: u32,
    #[serde(default)]
    pub source: Option<Box<str>>,
    /// Operator names, compared exactly with `Dopravci` names.
    pub operators: Vec<Box<str>>,
    /// Integrated transport system codes (`LinExt`).
    pub ids_codes: Vec<u32>,
    #[serde(default)]
    pub ids_names: BTreeMap<Box<str>, Box<str>>,
    /// Licence prefixes, digits only.
    pub line_prefixes: Vec<Box<str>>,
}

#[derive(Debug, thiserror::Error)]
pub enum FilterRulesError {
    #[error("filter rules: {0}")]
    Json(#[from] serde_json::Error),
    #[error("filter rules: schema version {0}, 1 expected")]
    Version(u32),
    #[error("filter rules: line prefix {0:?} is not digits")]
    Prefix(Box<str>),
}

pub fn parse(json: &[u8]) -> Result<FilterRules, FilterRulesError> {
    let rules: FilterRules = serde_json::from_slice(json)?;
    if rules.schema_version != 1 {
        return Err(FilterRulesError::Version(rules.schema_version));
    }
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    if let Some(prefix) = rules.line_prefixes.iter().find(|prefix| !digits(prefix)) {
        return Err(FilterRulesError::Prefix(prefix.clone()));
    }
    Ok(rules)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rules_file_is_read_and_its_prefixes_must_be_digits() {
        let text = |prefix: &str| {
            format!(
                r#"{{"schema_version": 1, "source": "x", "operators": ["FlixBus CZ s.r.o."],
                   "ids_codes": [30001], "ids_names": {{"30001": "PID"}},
                   "line_prefixes": ["{prefix}"]}}"#
            )
        };
        let rules = parse(text("199").as_bytes()).unwrap();
        assert_eq!(rules.ids_codes, [30001]);
        assert_eq!(&*rules.operators[0], "FlixBus CZ s.r.o.");
        assert!(matches!(
            parse(text("19x").as_bytes()),
            Err(FilterRulesError::Prefix(_))
        ));
    }
}
