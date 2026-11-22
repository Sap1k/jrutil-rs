//! The road compiler: JDF batches to a Timetable. No I/O.
//!
//! A batch as read is admitted and given its format version's defaults ([`source`], [`batch`]);
//! its stops are matched, one batch at a time ([`fix`], handing over a [`fixed::FixedBatch`]);
//! the fixed batches are merged into stops, operators and line versions, each trip with its
//! running days ([`merge`], [`calendar`], [`holidays`]), and the international filter is applied
//! ([`international`]); the merged facts are assembled into the timetable ([`assemble`]). Every
//! decision is a named rule of `docs/rules/jdf-read.md`, `jdf-merge.md` or `jdf-convert.md`;
//! [`rules`] lists them.
//!
//! See ARCHITECTURE.md.

pub mod assemble;
pub mod batch;
pub mod calendar;
pub mod filtered;
pub mod fix;
pub mod fixed;
pub mod holidays;
pub mod international;
pub mod merge;
pub mod policy;
pub mod source;

pub use policy::Policy;

/// The rules of the batch foundation: version defaults, calendars and the international filter.
const FOUNDATION: &[jr_model::Rule] = &[
    batch::R025,
    batch::R026,
    batch::R028,
    batch::R030,
    calendar::R255,
    calendar::R256,
    calendar::R257,
    calendar::R258,
    calendar::R259,
    calendar::R260,
    calendar::R261,
    calendar::R262,
    calendar::R263,
    calendar::R264,
    holidays::R265,
    holidays::R266,
    calendar::R267,
    international::R269,
    international::R270,
    international::R271,
    international::R272,
    international::R273,
    international::R274,
    international::R275,
    international::R276,
    international::R277,
    international::R278,
    international::R281,
    batch::R283,
];

/// Every rule this crate implements, stage by stage.
pub fn rules() -> impl Iterator<Item = jr_model::Rule> {
    [
        source::RULES,
        FOUNDATION,
        fix::RULES,
        merge::RULES,
        assemble::RULES,
        filtered::RULES,
    ]
    .into_iter()
    .flatten()
    .copied()
}
