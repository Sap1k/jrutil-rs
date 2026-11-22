//! Headsigns of authority trips (OVL-R089, R092).

use std::collections::{HashMap, HashSet};

use jr_model::{Diagnostic, Diagnostics, PlaceId, Rule, Severity};

use crate::authority::{Authority, trip_subject};
use crate::prepare::{PlaceRef, Prepared};
use crate::profile::Capability;

/// A slice whose selection comes from a projected (authority) source trip, and a trip a source
/// adds, take the source trip's full headsign (R092), and their calls the source calls' stop
/// headsigns, when the profile grants `headsigns` (verdict Q12); other slices keep the base's.
pub const OVL_R089: Rule = Rule {
    id: "OVL-R089",
    summary: "authority slices and added trips take the source's full headsigns",
};

/// A source headsign is expanded to the terminal's full name when it abbreviates it (R035):
/// the terminal is the last call at or after the given one, named by its corrected name, else
/// its base place's published name, else its stop group's. A blank headsign is the terminal's
/// name; one that says something else is kept; without a terminal it is kept and diagnosed
/// (`headsign_unresolved`).
///
/// "Expand a display-shortened terminal to the full canonical place name. Preserve text which
/// is genuinely different (bilingual, via, or an intermediate destination)."
pub const OVL_R092: Rule = Rule {
    id: "OVL-R092",
    summary: "a headsign expands to the terminal's full name when it abbreviates it",
};

pub const RULES: &[Rule] = &[OVL_R089, OVL_R092];

/// One provenance row of `headsigns.csv`.
pub type Row = [String; 8];

/// The headsign resolver and its provenance rows.
#[derive(Debug)]
pub struct Headsigns {
    /// Base place to the name of the one source group correcting it (R046).
    corrections: HashMap<PlaceId, Box<str>>,
    projected: HashSet<(u32, u32)>,
    pub rows: Vec<Row>,
    seen: HashSet<Row>,
}

impl Headsigns {
    pub fn new(prepared: &Prepared<'_>, authority: &Authority) -> Self {
        let mut corrections = HashMap::new();
        for source in &prepared.sources {
            for group in &source.groups {
                if let Some(matched) = group.matched
                    && matched.corrected
                    && let PlaceRef::Base(place) = matched.target
                {
                    corrections.insert(place, group.name.clone());
                }
            }
        }
        Headsigns {
            corrections,
            projected: authority
                .projections
                .iter()
                .map(|p| (p.source, p.trip))
                .collect(),
            rows: Vec::new(),
            seen: HashSet::new(),
        }
    }

    /// Whether the source trip's headsigns apply (a projected trip of a source granting them).
    pub fn applies(&self, prepared: &Prepared<'_>, source: u32, trip: u32) -> bool {
        self.projected.contains(&(source, trip))
            && prepared.sources[source as usize]
                .profile
                .capabilities
                .contains_key(&Capability::Headsigns)
    }

    /// R092: the full headsign from call `sequence` on (`None`: the whole trip).
    pub fn full(
        &mut self,
        prepared: &Prepared<'_>,
        source_index: u32,
        trip_index: u32,
        sequence: Option<i32>,
        raw: &str,
        diagnostics: &mut Diagnostics,
    ) -> String {
        let source = &prepared.sources[source_index as usize];
        let trip = &source.trips[trip_index as usize];
        let calls = source.trip_calls(trip);
        let terminal = calls
            .iter()
            .rev()
            .find(|call| sequence.is_none_or(|from| call.sequence.unwrap_or(i32::MIN) >= from));
        let (value, status, reason, destination) = match terminal {
            None => (
                raw.to_owned(),
                "unresolved",
                "missing_terminal_stop",
                String::new(),
            ),
            Some(call) => {
                let name = self.terminal_name(prepared, source_index, call.stop, call.place);
                let stop = format!(
                    "{}:{}",
                    source.source_id(),
                    source.feed.stops[call.stop as usize].stop_id
                );
                if raw.trim().is_empty() {
                    (name, "full_stop_name", "terminal_default", stop)
                } else if prepared.names.resolve_headsign(raw, &[&name]).0.is_some() {
                    (name, "full_stop_name", "terminal_stop", stop)
                } else {
                    (
                        raw.to_owned(),
                        "preserved",
                        "distinct_source_headsign",
                        String::new(),
                    )
                }
            }
        };
        let subject = trip_subject(source, trip);
        if status == "unresolved" {
            diagnostics.push(
                Diagnostic::new(
                    "headsign_unresolved",
                    Severity::Warning,
                    subject.clone(),
                    format!(
                        "Call {}: destination identity is ambiguous or unsupported: {raw}",
                        sequence.unwrap_or(-1)
                    ),
                )
                .rule(OVL_R092),
            );
        }
        let row: Row = [
            subject,
            sequence.unwrap_or(-1).to_string(),
            raw.to_owned(),
            value.clone(),
            status.to_owned(),
            reason.to_owned(),
            destination,
            String::new(),
        ];
        if self.seen.insert(row.clone()) {
            self.rows.push(row);
        }
        value
    }

    fn terminal_name(
        &self,
        prepared: &Prepared<'_>,
        source: u32,
        stop: u32,
        place: Option<PlaceRef>,
    ) -> String {
        if let Some(PlaceRef::Base(place)) = place {
            if let Some(name) = self.corrections.get(&place) {
                return name.to_string();
            }
            let row = &prepared.base.timetable.places[place];
            let estimated = row
                .position
                .is_some_and(|p| p.precision == jr_model::timetable::Precision::Estimated);
            return if estimated {
                format!(
                    "{}{}",
                    row.name, prepared.base.profile.estimated_name_suffix
                )
            } else {
                row.name.to_string()
            };
        }
        let s = &prepared.sources[source as usize];
        match s.group_of_stop[stop as usize] {
            Some(group) => s.groups[group as usize].name.to_string(),
            None => s.feed.stops[stop as usize]
                .stop_name
                .as_deref()
                .unwrap_or("")
                .to_owned(),
        }
    }
}
