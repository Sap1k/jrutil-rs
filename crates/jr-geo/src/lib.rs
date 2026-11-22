//! Name matching, spatial indexes, administrative areas and the road and tram router. No I/O.
//!
//! See ARCHITECTURE.md.

pub mod areas;
pub mod laea;
pub mod names;
pub mod plane;
pub mod routing;
pub mod stop_index;
pub mod towns;

#[cfg(test)]
mod test_data;
