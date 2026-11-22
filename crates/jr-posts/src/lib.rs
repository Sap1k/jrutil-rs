//! The boarding-post estimator: which post of a stop a road trip uses, where the source does
//! not say. No I/O.
//!
//! A stop with several observed post positions gets, for each way trips pass it, a *context*;
//! the router gives each context its evidence (how each observed position sits relative to
//! the trip's way); the evidence is a pack ([`jr_formats::post_evidence::Pack`]), the boundary
//! at which a policy or a scorer can be evaluated without a build; the evaluator
//! ([`evaluate`]) decides from the pack alone; the decisions become boarding points and the
//! calls' boarding points, as edits ([`estimate`]).
//!
//! See ARCHITECTURE.md and `docs/rules/posts.md`.

pub mod estimate;
pub mod evaluate;

pub use estimate::{EstimateError, EstimateInput, Estimated, estimate};
