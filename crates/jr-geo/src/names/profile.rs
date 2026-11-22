//! The one place a name becomes tokens. Every normalisation (rail point names, road stop
//! matching, road stop identity) is a [`Profile`] of this machinery: characters are mapped
//! (transliteration, combining marks, case), phrases replaced, and the result cut into tokens by
//! one character classification. What the synonym table then does to the tokens is the
//! profile's [`SynonymUse`] (`super::analyzer`).

use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;

/// The case every character is mapped to. The mapping is the simple (one-to-one) one: a
/// character whose case counterpart is several characters stays as it is, so the result does not
/// depend on a culture (jdf-read B22).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Case {
    /// CZPTT-R090.
    Upper,
    /// JDF-R039 (`ToLower`), JDF-R214 (`ToLowerInvariant`).
    Lower,
}

/// Which characters can be part of a token; every other character separates tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WordCharacters {
    /// Letters and numbers (`char::is_alphanumeric`): CZPTT-R090, JDF-R214. (The F# merge used
    /// `Char.IsLetterOrDigit`, which differs only on letter-like numerals such as `²` and on a
    /// few combining signs; neither occurs in stop names.)
    Alphanumeric,
    /// Everything but white space (`char::is_whitespace`, which includes U+00A0): JDF-R039, where
    /// Lucene split on white space only and punctuation other than the listed separators stays
    /// inside a token (`(Hlavní`).
    NonWhitespace,
}

/// What the synonym table does to a profile's tokens (`super::Analyzer`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SynonymUse {
    /// Tokens stay as they are (CZPTT-R090).
    #[default]
    None,
    /// Tokens stay as they are; they compare through their expansions, the union of every line
    /// they appear in (JDF-R040, the Lucene synonym filter with `expand=true`).
    Expand,
    /// Each token is replaced by the canonical form of its line, the last line naming it winning;
    /// a token no line names loses the trailing `trim_unmapped` characters (JDF-R215).
    Canonical,
}

/// A name normalisation, compiled for use when deserialised.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(try_from = "RawProfile")]
pub struct Profile {
    pub(super) characters: CharacterMap,
    replacements: Vec<(Box<str>, Box<str>)>,
    word: WordCharacters,
    separators: Vec<char>,
    terminators: Vec<char>,
    pub(super) synonyms: SynonymUse,
    pub(super) trim_unmapped: Vec<char>,
}

/// The policy form of [`Profile`].
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProfile {
    case: Case,
    /// One character to its replacement, applied before the case mapping.
    #[serde(default)]
    transliteration: BTreeMap<String, String>,
    /// Inclusive ranges of characters that are dropped (combining marks).
    #[serde(default)]
    marks: Vec<(char, char)>,
    /// Substring replacements on the mapped text, in order (JDF-R214 `n.l.` → `nad labem`).
    #[serde(default)]
    replacements: Vec<(String, String)>,
    word_characters: WordCharacters,
    /// Further characters that separate tokens although they are word characters.
    #[serde(default)]
    separators: Vec<char>,
    /// Characters that end the token they are part of, kept as its last character (the `.` of
    /// JDF-R039 and R214: `hl.nádr.` is `hl.` and `nádr.`).
    #[serde(default)]
    terminators: Vec<char>,
    #[serde(default)]
    synonyms: SynonymUse,
    /// With `synonyms = "canonical"`: trailing characters removed from a token no line names.
    #[serde(default)]
    trim_unmapped: Vec<char>,
}

impl TryFrom<RawProfile> for Profile {
    type Error = String;

    fn try_from(raw: RawProfile) -> Result<Self, String> {
        let characters = CharacterMap::new(raw.transliteration, raw.marks, raw.case)?;
        let mut replacements = Vec::with_capacity(raw.replacements.len());
        for (from, to) in raw.replacements {
            if from.is_empty() {
                return Err("a replacement replaces the empty string".to_owned());
            }
            replacements.push((from.into_boxed_str(), to.into_boxed_str()));
        }
        Ok(Profile {
            characters,
            replacements,
            word: raw.word_characters,
            separators: raw.separators,
            terminators: raw.terminators,
            synonyms: raw.synonyms,
            trim_unmapped: raw.trim_unmapped,
        })
    }
}

impl Profile {
    /// The rail profile of CZPTT-R090: the tables' character map, upper case, letters and
    /// numbers only, no replacements, no synonyms.
    pub(super) fn rail(characters: CharacterMap) -> Self {
        Profile {
            characters,
            replacements: Vec::new(),
            word: WordCharacters::Alphanumeric,
            separators: Vec::new(),
            terminators: Vec::new(),
            synonyms: SynonymUse::None,
            trim_unmapped: Vec::new(),
        }
    }

    /// The name's tokens before any synonym is applied: characters mapped, replacements made,
    /// cut by the character classification. No token is empty.
    pub fn raw_tokens(&self, name: &str) -> Vec<String> {
        let mut mapped = String::with_capacity(name.len());
        self.characters.map_into(name, &mut mapped);
        for (from, to) in &self.replacements {
            if mapped.contains(&**from) {
                mapped = mapped.replace(&**from, to);
            }
        }
        let mut tokens = Vec::new();
        let mut token = String::new();
        for character in mapped.chars() {
            if self.terminators.contains(&character) {
                token.push(character);
                tokens.push(std::mem::take(&mut token));
            } else if self.separators.contains(&character) || !self.is_word(character) {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
            } else {
                token.push(character);
            }
        }
        if !token.is_empty() {
            tokens.push(token);
        }
        tokens
    }

    fn is_word(&self, character: char) -> bool {
        match self.word {
            WordCharacters::Alphanumeric => character.is_alphanumeric(),
            WordCharacters::NonWhitespace => !character.is_whitespace(),
        }
    }
}

/// Transliteration, mark removal and case mapping, one character at a time.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CharacterMap {
    transliteration: HashMap<char, Box<str>>,
    marks: Vec<(char, char)>,
    case: Case,
}

impl CharacterMap {
    pub(crate) fn new(
        transliteration: BTreeMap<String, String>,
        marks: Vec<(char, char)>,
        case: Case,
    ) -> Result<Self, String> {
        let mut map = HashMap::with_capacity(transliteration.len());
        for (from, to) in transliteration {
            let mut chars = from.chars();
            let (Some(character), None) = (chars.next(), chars.next()) else {
                return Err(format!("transliteration key `{from}` is not one character"));
            };
            map.insert(character, to.into_boxed_str());
        }
        Ok(CharacterMap {
            transliteration: map,
            marks,
            case,
        })
    }

    fn is_mark(&self, character: char) -> bool {
        self.marks
            .iter()
            .any(|&(first, last)| (first..=last).contains(&character))
    }

    /// Appends the mapped text: each character transliterated, the marks of the result dropped,
    /// the rest case-mapped.
    pub(crate) fn map_into(&self, text: &str, out: &mut String) {
        for character in text.chars() {
            match self.transliteration.get(&character) {
                Some(replacement) => {
                    for replaced in replacement.chars() {
                        self.push_mapped(replaced, out);
                    }
                }
                None => self.push_mapped(character, out),
            }
        }
    }

    fn push_mapped(&self, character: char, out: &mut String) {
        if self.is_mark(character) {
            return;
        }
        out.push(match self.case {
            Case::Upper => single_or(character.to_uppercase(), character),
            Case::Lower => single_or(character.to_lowercase(), character),
        });
    }
}

/// The only character of `mapped`, or `fallback` when it has several.
fn single_or(mut mapped: impl Iterator<Item = char>, fallback: char) -> char {
    match (mapped.next(), mapped.next()) {
        (Some(single), None) => single,
        _ => fallback,
    }
}
