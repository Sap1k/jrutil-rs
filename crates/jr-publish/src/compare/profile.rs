//! What a comparison treats as generated, how it identifies rows and how close doubles must be.

use std::collections::BTreeMap;

/// The comparison's rules. `Profile::default()` is the built-in profile for the serving
/// contract.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    /// Identifiers the two implementations may generate differently. Never compared literally:
    /// a row keyed by one is compared by content, a reference to one by what it refers to.
    pub generated: Vec<GeneratedId>,
    pub trip_identity: TripIdentity,
    /// Absolute tolerance of a double column not listed in `tolerances`.
    pub default_tolerance: f64,
    /// Absolute tolerance by column name (in whichever relation the column appears).
    pub tolerances: BTreeMap<String, f64>,
    /// Samples kept per category (and ledger category); counts are always exact.
    pub samples: usize,
}

/// A generated identifier and what stands for it when something references it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedId {
    pub relation: String,
    pub field: String,
    pub content: Content,
}

/// The canonical content of a generated identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    /// A service: the dates of its calendar row (`valid_from..=valid_to` on the weekdays of
    /// `weekday_mask`, bit 0 Monday) with the rows of `exceptions` added or removed.
    Dates { exceptions: String },
    /// The row's other columns and, in order, the rows of `relation` that reference it (their
    /// columns other than the reference and the ordering key). A shape and its points.
    Children { relation: String },
    /// The row's other columns. A note.
    Columns,
}

/// How trips (and the rows keyed by trip) are matched between the two packages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TripIdentity {
    /// By `trip_id`, i.e. by primary key like every other relation.
    ///
    /// A second strategy, "journey" (source key + service date), comes later for packages
    /// whose trip IDs themselves differ.
    TripId,
}

impl Default for Profile {
    fn default() -> Profile {
        let generated = |relation: &str, field: &str, content: Content| GeneratedId {
            relation: relation.to_owned(),
            field: field.to_owned(),
            content,
        };
        let tolerances = [
            ("longitude", 1e-7),
            ("latitude", 1e-7),
            ("shape_distance_traveled", 0.01),
            ("distance_traveled", 0.01),
            ("source_kilometre", 1e-6),
        ]
        .into_iter()
        .map(|(column, tolerance)| (column.to_owned(), tolerance))
        .collect();
        Profile {
            generated: vec![
                generated(
                    "service_calendar",
                    "service_id",
                    Content::Dates {
                        exceptions: "service_exception".to_owned(),
                    },
                ),
                generated(
                    "shape",
                    "shape_id",
                    Content::Children {
                        relation: "shape_point".to_owned(),
                    },
                ),
                generated("service_note", "note_id", Content::Columns),
                generated("assignment", "assignment_id", Content::Columns),
                generated("transfer", "transfer_key", Content::Columns),
                generated("connection_claim", "connection_id", Content::Columns),
                generated("travel_restriction", "restriction_id", Content::Columns),
            ],
            trip_identity: TripIdentity::TripId,
            default_tolerance: 1e-9,
            tolerances,
            samples: 20,
        }
    }
}

impl Profile {
    pub fn tolerance(&self, column: &str) -> f64 {
        self.tolerances
            .get(column)
            .copied()
            .unwrap_or(self.default_tolerance)
    }

    pub fn generated(&self, relation: &str, field: &str) -> Option<&GeneratedId> {
        self.generated
            .iter()
            .find(|id| id.relation == relation && id.field == field)
    }

    /// Relations that only define generated content (a calendar and its exceptions, a shape
    /// and its points): compared through the rows that reference them, never row by row.
    pub fn is_definition(&self, relation: &str) -> bool {
        self.generated.iter().any(|id| match &id.content {
            Content::Dates { exceptions } => id.relation == relation || exceptions == relation,
            Content::Children { relation: child } => id.relation == relation || child == relation,
            Content::Columns => false,
        })
    }
}
