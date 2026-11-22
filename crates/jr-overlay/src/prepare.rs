//! Preparation (OVL-R013–R056). (Skeleton: the preparation stage's package fills this in.)

use jr_model::{Diagnostics, Rule};

use crate::OverlayInput;
use crate::report::ReportTable;

pub const RULES: &[Rule] = &[];

#[derive(Debug, thiserror::Error)]
pub enum PrepareError {}

/// What the later stages read.
#[derive(Debug)]
pub struct Prepared {}

impl Prepared {
    pub fn reports(&self) -> Vec<ReportTable> {
        Vec::new()
    }
}

pub fn prepare(
    _input: OverlayInput<'_>,
    _diagnostics: &mut Diagnostics,
) -> Result<Prepared, PrepareError> {
    Ok(Prepared {})
}
