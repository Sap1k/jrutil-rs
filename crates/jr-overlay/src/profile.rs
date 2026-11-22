//! Source and base profiles. (Skeleton: the preparation stage's package fills this in.)

/// How to read one source and what it may state.
#[derive(Clone, Debug)]
pub struct SourceProfile {
    /// The source's ID (`pid-gtfs`).
    pub source_id: Box<str>,
}

/// What the reader must keep of a feed beyond the standard columns.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReaderRequest {
    /// Per standard file, the non-standard columns to keep.
    pub extra_columns: Vec<(Box<str>, Vec<Box<str>>)>,
    /// Other CSV members to read as text tables.
    pub tables: Vec<Box<str>>,
    /// Members to return verbatim.
    pub raw: Vec<Box<str>>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("overlay profile: {0}")]
    Toml(#[from] toml::de::Error),
}

impl SourceProfile {
    /// A profile shipped with the compiler, by name (`pid`, `ids-jmk`).
    pub fn builtin(_name: &str) -> Option<Result<SourceProfile, ProfileError>> {
        None
    }

    pub fn reader(&self) -> ReaderRequest {
        ReaderRequest::default()
    }
}

/// The facts of the base timetable the engine relies on.
#[derive(Clone, Debug)]
pub struct BaseProfile {}

impl BaseProfile {
    /// The national road timetable compiled from JDF.
    pub fn road() -> Result<BaseProfile, ProfileError> {
        Ok(BaseProfile {})
    }
}
