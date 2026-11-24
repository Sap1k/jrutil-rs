//! Preparation (OVL-R013–R056): each source against its profile and the base horizon, the
//! base's indexes, source stop groups matched to base stop places, candidate base routes of
//! source routes, call patterns. Nothing here changes the timetable: what a source will add
//! (stop places) is described, with its ID, for projection to emit.
//!
//! Sources are prepared in parallel, and within a source the stop groups and trips are; the
//! result does not depend on the thread count. The one step across sources is R046.

mod base;
mod calendar;
mod columns;
mod context;
mod grid;
mod groups;
mod hooks;
mod names;
mod native;
mod report;
mod routes;
mod signature;
mod source;
mod stops;

use jr_formats::geodata::registry::OverlayPlace;
use jr_model::{Diagnostics, Rule};
use rayon::prelude::*;

pub use base::{
    BasePlace, BaseRoute, BaseTrip, BaseTripIx, BaseView, PatternId, point_set_distance,
};
pub use calendar::{DAY, services};
pub use context::OVL_R046;
pub use hooks::{CrosswalkRow, crosswalk};
pub use names::Names;
pub use native::OVL_R047;
pub use routes::labels_compatible;
pub(crate) use signature::present as signature_present;
pub use signature::{Times, rounded};
pub use source::{
    ContextInference, MatchMethod, NativePlace, PlaceRef, PreparedSource, SourceCall, SourceRoute,
    SourceTrip, StopGroup, StopMatch,
};

use crate::OverlayInput;
use crate::report::ReportTable;

/// The rules of each module.
const MODULE_RULES: &[&[Rule]] = &[
    source::RULES,
    hooks::RULES,
    groups::RULES,
    calendar::RULES,
    base::RULES,
    names::RULES,
    routes::RULES,
    stops::RULES,
    context::RULES,
    native::RULES,
    signature::RULES,
];

const fn count(parts: &[&[Rule]]) -> usize {
    let mut total = 0;
    let mut part = 0;
    while part < parts.len() {
        total += parts[part].len();
        part += 1;
    }
    total
}

const fn join<const N: usize>(parts: &[&[Rule]]) -> [Rule; N] {
    let mut out = [Rule {
        id: "",
        summary: "",
    }; N];
    let mut at = 0;
    let mut part = 0;
    while part < parts.len() {
        let mut index = 0;
        while index < parts[part].len() {
            out[at] = parts[part][index];
            at += 1;
            index += 1;
        }
        part += 1;
    }
    out
}

const JOINED: [Rule; count(MODULE_RULES)] = join(MODULE_RULES);

/// Every rule of preparation, module by module.
pub const RULES: &[Rule] = &JOINED;

#[derive(Debug, thiserror::Error)]
pub enum PrepareError {}

/// What authority, matching and projection read.
#[derive(Debug)]
pub struct Prepared<'a> {
    /// The base's indexes (R025–R027, R053).
    pub base: BaseView<'a>,
    /// Per source, in source ID order.
    pub sources: Vec<PreparedSource<'a>>,
    /// The name rules (R032–R035), for headsigns (R092) and later name comparisons.
    pub names: Names,
    /// The registry's pinned stop places (R047, R048).
    pub places: &'a [OverlayPlace],
}

impl Prepared<'_> {
    pub fn reports(&self) -> Vec<ReportTable> {
        vec![
            report::stop_group_matches(self),
            report::stop_context_inference(self),
            report::overlay_place_candidates(self),
        ]
    }
}

pub fn prepare<'a>(
    input: OverlayInput<'a>,
    diagnostics: &mut Diagnostics,
) -> Result<Prepared<'a>, PrepareError> {
    let names = Names::new(input.synonyms);
    let base = BaseView::new(input.base, input.base_profile, &names);
    let horizon_days = input.base.horizon.days();

    // Per source: reading, grouping, matching, routes, the fold, context inference.
    let prepared: Vec<(PreparedSource<'a>, Diagnostics)> = input
        .sources
        .into_par_iter()
        .map(|source| {
            let mut found = Diagnostics::new();
            let mut prepared = source::read(
                source.profile,
                source.feed,
                source.overrides,
                &base,
                &mut found,
            );
            let scheduled = source::scheduled_stops(&prepared);
            groups::group_stops(&mut prepared, &scheduled, &names);
            stops::match_groups(&mut prepared, &base, &names, &mut found);
            routes::candidates(&mut prepared, &base, &mut found);
            if base.profile.dated_by_departure {
                source::fold_after_midnight(&mut prepared, horizon_days, &mut found);
            }
            context::infer(&mut prepared, &base, &names, &mut found);
            (prepared, found)
        })
        .collect();
    let mut sources = Vec::with_capacity(prepared.len());
    for (source, found) in prepared {
        diagnostics.append(found);
        sources.push(source);
    }

    // R046, across sources.
    if base.profile.estimated_places_correctable {
        context::correct_approximate(&mut sources, &base, diagnostics);
    }

    // R047, R054.
    let pins = native::pins(input.places);
    sources.par_iter_mut().for_each(|source| {
        native::native_places(source, &pins);
        context::place_calls(source);
    });

    Ok(Prepared {
        base,
        sources,
        names,
        places: input.places,
    })
}
