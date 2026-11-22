//! Reviewed overrides of a route's public marking (`docs/rules/jdf-convert.md`, JDF-R431).
//!
//! The rules name routes by the operator's registration number and the CIS licence. Both are
//! facts of the timetable (`Agency::registration`, the route's `cis:line` key), so no ID is read
//! apart. A route without a `cis:line` key (rail, a route a regional feed brought) is not
//! touched. The pass changes what a rider sees and nothing else: IDs and keys stay, so whatever
//! matches routes by their source marking does so before this pass runs.

use std::collections::BTreeMap;

use jr_formats::geodata::route_rules::{
    PresentationRule, most_specific, selector_matches, specificity,
};
use jr_model::timetable::{Entity, Namespace};
use jr_model::{
    Diagnostic, Diagnostics, Edit, EditBuilder, Edits, RouteId, Rule, Severity, Timetable,
};

/// The pass name recorded in the timetable's history.
pub const PASS: &str = "route_presentation";

/// Each of a route's short name, colour and text colour takes the value of the most specific
/// reviewed rule that names the route and sets that field (JDF-R406), independently of the
/// other two; a field no rule sets keeps the compiled value. Colours are published in lower
/// case.
///
/// Operators run lines under a brand the licence register does not know ("AEx" for Arriva
/// Express lines licensed as 157710, 580916, 721341) and in liveries riders recognise (Teplice,
/// DÚK); the reviewed file `routes/presentation.csv` of the geodata repository records them.
pub const JDF_R431: Rule = Rule {
    id: "JDF-R431",
    summary: "reviewed presentation rules override a route's short name and colours by specificity",
};

pub const RULES: &[Rule] = &[JDF_R431];

/// The counts published in diagnostics.json under `route_presentation`.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Summary {
    pub rules: usize,
    pub overridden_routes: usize,
    pub by_field: BTreeMap<&'static str, usize>,
}

/// Two equally specific rules set the same field of one route: the rules file is wrong.
#[derive(Debug, thiserror::Error)]
#[error(
    "presentation rules on lines {lines:?} are equally specific and both set {field} of {route}"
)]
pub struct Tie {
    pub field: &'static str,
    pub route: String,
    pub lines: Vec<u64>,
}

pub fn route_presentation(
    timetable: &Timetable,
    rules: &[PresentationRule],
    diagnostics: &mut Diagnostics,
) -> Result<(Edits, Summary), Tie> {
    let mut builder = EditBuilder::new(timetable, PASS);
    let mut summary = Summary {
        rules: rules.len(),
        ..Summary::default()
    };
    // A route's licence is its `cis:line` key; the first in key order when it has several.
    let mut licences: BTreeMap<RouteId, &str> = BTreeMap::new();
    for key in &timetable.keys {
        if let (Entity::Route(route), true) = (key.entity, key.namespace == Namespace::CIS_LINE) {
            let licence = licences.entry(route).or_insert(&key.identifier);
            if *key.identifier < **licence {
                *licence = &key.identifier;
            }
        }
    }
    for (&route, &licence) in &licences {
        let Some(row) = timetable.routes.get(route) else {
            continue;
        };
        let agency = timetable
            .agencies
            .get(row.agency)
            .and_then(|agency| agency.registration.as_deref());
        let matching: Vec<&PresentationRule> = rules
            .iter()
            .filter(|rule| {
                selector_matches(
                    rule.agency_ico.as_deref(),
                    &rule.licence_pattern,
                    agency,
                    licence,
                )
            })
            .collect();
        if matching.is_empty() {
            continue;
        }
        let field = |name: &'static str, value: fn(&PresentationRule) -> Option<&str>| {
            most_specific(matching.iter().filter_map(|rule| {
                let rank = specificity(rule.agency_ico.as_deref(), &rule.licence_pattern);
                Some((rank, rule.line, value(rule)?))
            }))
            .map_err(|lines| Tie {
                field: name,
                route: row.id.to_string(),
                lines,
            })
        };
        let short_name = field("route_short_name", |rule| rule.route_short_name.as_deref())?;
        let color = field("route_color", |rule| rule.route_color.as_deref())?;
        let text_color = field("route_text_color", |rule| rule.route_text_color.as_deref())?;

        let mut changed = false;
        let mut change = |name: &'static str, before: Option<&str>, after: Option<Box<str>>| {
            let after = after.filter(|after| Some(&**after) != before)?;
            diagnostics.push(
                Diagnostic::new(
                    "route_presentation_override",
                    Severity::Info,
                    row.id.to_string(),
                    format!("{name}: {} -> {after}", before.unwrap_or("-")),
                )
                .rule(JDF_R431),
            );
            *summary.by_field.entry(name).or_default() += 1;
            changed = true;
            Some(after)
        };
        let lower = |value: Option<&str>| value.map(|value| value.to_ascii_lowercase().into());
        let short_name = change(
            "route_short_name",
            row.short_name.as_deref(),
            short_name.map(Box::from),
        );
        let color = change("route_color", row.color.as_deref(), lower(color));
        let text_color = change(
            "route_text_color",
            row.text_color.as_deref(),
            lower(text_color),
        );
        if changed {
            summary.overridden_routes += 1;
            builder.push(
                JDF_R431,
                Edit::SetRoutePresentation {
                    route,
                    short_name,
                    color,
                    text_color,
                },
            );
        }
    }
    Ok((builder.finish(), summary))
}

#[cfg(test)]
mod tests;
