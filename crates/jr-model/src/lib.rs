//! The canonical timetable model (ARCHITECTURE.md, "The model").
//!
//! Everything between the readers (`jr-formats`) and the projections (`jr-publish`) is a
//! [`Timetable`] or an [`Edits`] batch against one. This crate has no I/O and depends on no other
//! workspace crate.
//!
//! - [`handle`]: dense typed handles and the tables they index.
//! - [`text`]: the string interner for hot tables.
//! - [`time`]: [`ServiceTime`], the GVD [`Horizon`].
//! - [`dateset`]: [`DateSet`], the only calendar type.
//! - [`geo`]: [`Coord`] and the one distance function.
//! - [`identity`]: the only place a public ID is built.
//! - [`timetable`]: the tables.
//! - [`edit`]: what a pass may ask for, and [`apply`], the only thing that changes a timetable.
//! - [`diag`]: rules and diagnostics.

pub mod dateset;
pub mod diag;
pub mod edit;
pub mod geo;
pub mod handle;
pub mod identity;
pub mod text;
pub mod time;
pub mod timetable;

pub use dateset::{DateSet, Services};
pub use diag::{Diagnostic, Diagnostics, Rule, Severity};
pub use edit::{ApplyError, Edit, EditBuilder, Edits, Precedence, apply};
pub use geo::Coord;
pub use handle::{
    AgencyId, BoardingPointId, NoteId, PlaceId, RouteId, RouteStopId, ServiceId, ShapeId, Sym,
    Table, TripId, ZoneSetId,
};
pub use identity::{Feed, PublicId};
pub use text::Strings;
pub use time::{Date, Horizon, OptTime, ServiceTime};
pub use timetable::Timetable;
