//! Review reports: what a stage decided, row by row, for a person or a script to read. They
//! are diagnostics, not an interface: nothing in the build reads them back.

/// A named table of text cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportTable {
    /// The file stem it is written under (`stop_group_matches`).
    pub name: &'static str,
    pub header: Vec<&'static str>,
    /// Rows in a deterministic order the producing stage states.
    pub rows: Vec<Vec<String>>,
}
