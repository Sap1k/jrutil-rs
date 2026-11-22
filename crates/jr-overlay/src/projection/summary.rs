//! The counts of what the batch does, for the build's diagnostics summary.

use serde::Serialize;

/// What projection changed (R118: what the package's diagnostics carry of the overlay).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    /// Parts of base trips that take a source selection.
    pub trips_bound: usize,
    /// Base trips split into date slices, and the slices.
    pub trips_sliced: usize,
    pub slices: usize,
    /// Base trips keeping fewer days without being split.
    pub trips_shortened: usize,
    /// Base trips without any day left.
    pub trips_retired: usize,
    pub trips_added: usize,
    pub places_added: usize,
    pub places_renamed: usize,
    pub places_moved: usize,
    pub posts_added: usize,
    pub routes_added: usize,
    pub agencies_added: usize,
    pub calls_zoned: usize,
    pub transfers_added: usize,
    pub keys_added: usize,
    pub call_keys_added: usize,
    /// R082: conflicts quarantined, by kind, and claims coalesced or decided by revision.
    pub fact_conflicts: usize,
    pub cross_source_conflicts: usize,
    pub cross_source_coalesced: usize,
    pub newer_revision_selected: usize,
    /// R084.
    pub baseline_instances_removed: usize,
}

impl Summary {
    /// Counts the decisions diagnostics record.
    pub fn count(&mut self, diagnostics: &jr_model::Diagnostics) {
        let counts = diagnostics.counts_by_code();
        let get = |code: &str| counts.get(code).copied().unwrap_or(0);
        self.fact_conflicts = get("overlay_fact_conflict");
        self.cross_source_conflicts = get("cross_source_fact_conflict");
        self.cross_source_coalesced = get("cross_source_fact_coalesced");
        self.newer_revision_selected = get("overlay_newer_source_selected");
        self.baseline_instances_removed = get("equivalent_baseline_instance_removed");
    }
}
