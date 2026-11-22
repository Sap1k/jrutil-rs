//! The road compiler: JDF batches to a Timetable. No I/O.
//!
//! This is the foundation: a batch with its format version's defaults applied ([`batch`]), a
//! trip's running days ([`calendar`], [`holidays`]) and the international route filter
//! ([`international`]). Every decision is a named rule of `docs/rules/jdf-read.md` or
//! `docs/rules/jdf-merge.md`; [`RULES`] lists them.
//!
//! See ARCHITECTURE.md.

pub mod batch;
pub mod calendar;
pub mod fix;
pub mod fixed;
pub mod holidays;
pub mod international;
pub mod merge;
pub mod policy;

pub use policy::Policy;

/// Every rule this crate implements.
pub const RULES: &[jr_model::Rule] = &[
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
