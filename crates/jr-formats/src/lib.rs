//! Readers and writers of file formats: bytes to typed source facts, no policy.
//!
//! A reader says what the source says (AGENTS.md rule 2): a missing value is `None`, unknown
//! codes stay text, nothing is repaired or defaulted. The crate never opens a file; it decodes
//! the bytes and readers it is handed.
//!
//! See ARCHITECTURE.md.

pub mod archive;
pub mod czptt;
pub mod geodata;
pub mod gtfs;
pub mod jdf;
pub mod kadr;
pub mod osm;
pub mod snapshot;
pub mod sr70;
