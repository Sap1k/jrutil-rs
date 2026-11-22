//! Line facts of each version and the output routes they form (JDF-R405–R408, R426–R430, R478).

use std::collections::{BTreeMap, HashMap};

use jr_formats::geodata::route_rules::{
    TransportModeRule, most_specific, selector_matches, specificity,
};
use jr_formats::jdf::{RouteType, TransportMode};
use jr_model::identity::{self, PublicId};
use jr_model::timetable::Mode;
use jr_model::timetable::{Agency, Route, TimetableKind};
use jr_model::{AgencyId, Diagnostic, Diagnostics, RouteId, Rule, Severity, Timetable};
use sha2::{Digest, Sha256};

use super::calls::Plans;
use super::{AssembleError, Policy};
use crate::merge::{Merged, Version};

/// The reviewed transport-mode rules (`routes/transport-modes.csv`, read by `jr-formats`) are
/// applied before routes are formed, so route type, colours and grouping see the effective mode.
pub const R405: Rule = Rule {
    id: "JDF-R405",
    summary: "Reviewed transport-mode rules correct versions before routes are formed",
};

/// A rule's selector matches a licence exactly, by an equally long range, or by a prefix, and
/// the operator's IČO when it names one; the most specific matching rule wins. The selector and
/// ranking are `jr_formats::geodata::route_rules`' (shared with the presentation rules); two
/// equally specific rules are a broken rules file: an `AssembleError` naming their lines.
pub const R406: Rule = Rule {
    id: "JDF-R406",
    summary: "The most specific matching transport-mode rule wins; a tie is an error",
};

/// A rule applies only while the version still has the rule's expected mode and, when the rule
/// names public lines, a numeric public line among them; a matching rule whose guard fails is
/// diagnosed and changes nothing.
pub const R407: Rule = Rule {
    id: "JDF-R407",
    summary: "A transport-mode rule applies only while its expected mode and line guard hold",
};

/// The public line number: the one preferential `LinExt` designation (digits without leading
/// zeros, anything else verbatim); without one, the last three digits of a six-digit licence
/// without leading zeros; otherwise none, diagnosed with the reason.
pub const R408: Rule = Rule {
    id: "JDF-R408",
    summary: "Public line: the preferential LinExt designation, else the licence's last digits",
};

/// An agency: `jdf:agency:<IČO>:<distinction>`, the name verbatim, the website (with `http://`
/// unless it carries a scheme anywhere, F# parity), the office phone (Q4), the e-mail, the IČO
/// as the registration number; timezone and language from policy.
pub const R426: Rule = Rule {
    id: "JDF-R426",
    summary: "Agencies carry name, website, office phone, e-mail and the IČO",
};

/// The extended GTFS route type of (effective mode, JDF route type): local, regional,
/// international and national coach for buses; one type per other mode. A bus version with
/// regional cross-border service is regional; an unknown bus route type is regional, diagnosed.
pub const R427: Rule = Rule {
    id: "JDF-R427",
    summary: "The GTFS route type follows the mode and the JDF route type",
};

/// Colours by mode, route type and (metro) public line; detours keep the line's colours.
pub const R428: Rule = Rule {
    id: "JDF-R428",
    summary: "Route colours follow the mode, route type and metro line",
};

/// Per licence, regular and detour versions apart, versions with equal semantics (licence,
/// agency, public line, name, route type, colours, detour) form one output route. The group
/// holding the earliest-starting version is `jdf:route:<licence>[:detour]`; any other appends
/// eight hex characters of SHA-256 of the semantics as F# concatenates them.
pub const R429: Rule = Rule {
    id: "JDF-R429",
    summary: "Versions of a line with equal semantics form one output route",
};

/// A route's fields come from its earliest-starting version; a detour route says so in its
/// description and timetable kind. A route without any published trip is not published (the
/// proposed verdict for B31), diagnosed.
pub const R430: Rule = Rule {
    id: "JDF-R430",
    summary: "Routes take their earliest version's fields; routes without trips are dropped",
};

/// A version whose transport mode is empty or not a JDF code (1.10/1.11; F# refused the batch)
/// is published with the policy's fallback mode, diagnosed.
pub const R478: Rule = Rule {
    id: "JDF-R478",
    summary: "A version without a known transport mode is published as the fallback mode",
};

/// The line facts of one version.
#[derive(Clone, Debug)]
pub(crate) struct Line {
    pub public_line: Option<Box<str>>,
    pub mode: Mode,
    pub gtfs_route_type: i32,
    pub color: Box<str>,
    pub text_color: Box<str>,
    /// Index into [`Lines::groups`].
    pub group: usize,
}

/// One output route.
#[derive(Clone, Debug)]
pub(crate) struct Group {
    pub id: PublicId,
    pub detour: bool,
    /// Versions, earliest first; the first is the representative.
    pub members: Vec<usize>,
}

pub(crate) struct Lines {
    pub lines: Vec<Line>,
    pub groups: Vec<Group>,
}

/// The agency's IČO of a version.
fn ico<'m>(merged: &'m Merged, version: &Version) -> &'m str {
    &merged.agencies[version.agency].id
}

pub(crate) fn prepare(
    merged: &Merged,
    rules: &[TransportModeRule],
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> Result<Lines, AssembleError> {
    let mut lines = Vec::with_capacity(merged.versions.len());
    for version in &merged.versions {
        let subject = version.subject();
        let public_line = public_line(version, policy, &subject, diagnostics);
        let source_mode = match &version.route.transport_mode {
            Some(mode @ TransportMode::Bus)
            | Some(
                mode @ (TransportMode::Tram
                | TransportMode::CableCar
                | TransportMode::Metro
                | TransportMode::Ferry
                | TransportMode::Trolleybus),
            ) => mode.code().to_owned(),
            other => {
                diagnostics.push(
                    Diagnostic::new(
                        "jdf_transport_mode_missing",
                        Severity::Warning,
                        subject.clone(),
                        format!(
                            "transport mode {:?} is no JDF code: published as {}",
                            other.as_ref().map(TransportMode::code),
                            policy.modes.fallback
                        ),
                    )
                    .rule(R478),
                );
                policy.modes.fallback.to_string()
            }
        };
        let mode_letter = transport_mode_rule(
            merged,
            version,
            &source_mode,
            public_line.as_deref(),
            rules,
            &subject,
            diagnostics,
        )
        .map_err(|lines| AssembleError::RuleTie { lines })?
        .unwrap_or(source_mode);
        let mode = policy
            .modes
            .letters
            .get(mode_letter.as_str())
            .copied()
            .unwrap_or(Mode::Bus);
        let (gtfs_route_type, color, text_color) = route_type_and_colours(
            version,
            &mode_letter,
            public_line.as_deref(),
            policy,
            &subject,
            diagnostics,
        );
        lines.push(Line {
            public_line,
            mode,
            gtfs_route_type,
            color,
            text_color,
            group: 0,
        });
    }
    let groups = group(merged, &mut lines);
    Ok(Lines { lines, groups })
}

/// R408.
fn public_line(
    version: &Version,
    policy: &Policy,
    subject: &str,
    diagnostics: &mut Diagnostics,
) -> Option<Box<str>> {
    let preferential: Vec<&str> = version
        .integrations
        .iter()
        .filter(|row| row.preferential == Some(true))
        .map(|row| row.route_name.trim())
        .collect();
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    let strip = |text: &str| {
        let stripped = text.trim_start_matches('0');
        if stripped.is_empty() {
            "0".to_owned()
        } else {
            stripped.to_owned()
        }
    };
    let licence = &*version.route.id;
    let missing = |reason: &str, diagnostics: &mut Diagnostics| {
        diagnostics.push(
            Diagnostic::new(
                "jdf_public_line_missing",
                Severity::Warning,
                subject.to_owned(),
                format!("no public line number: {reason}"),
            )
            .rule(R408),
        );
        None
    };
    match preferential[..] {
        [""] => missing("the preferential LinExt designation is empty", diagnostics),
        [one] if digits(one) => Some(strip(one).into()),
        [one] => Some(one.into()),
        [] if licence.len() == policy.public_line.licence_digits && digits(licence) => {
            let suffix = &licence[licence.len() - policy.public_line.licence_suffix..];
            Some(strip(suffix).into())
        }
        [] => missing(
            "no preferential LinExt row and no six-digit licence",
            diagnostics,
        ),
        _ => missing("several preferential LinExt rows", diagnostics),
    }
}

/// R405–R407: the effective mode letter when a rule applies; `Err` names the lines of equally
/// specific rules.
fn transport_mode_rule(
    merged: &Merged,
    version: &Version,
    mode: &str,
    public_line: Option<&str>,
    rules: &[TransportModeRule],
    subject: &str,
    diagnostics: &mut Diagnostics,
) -> Result<Option<String>, Vec<u64>> {
    let ico = ico(merged, version);
    let licence = &*version.route.id;
    let candidates: Vec<&TransportModeRule> = rules
        .iter()
        .filter(|rule| {
            selector_matches(
                rule.agency_ico.as_deref(),
                &rule.licence_pattern,
                Some(ico),
                licence,
            )
        })
        .collect();
    if candidates.is_empty() {
        return Ok(None);
    }
    let line: Option<i32> = public_line.and_then(|line| line.parse().ok());
    let guarded = candidates.iter().filter(|rule| {
        &*rule.expected_mode == mode
            && rule
                .public_lines
                .is_none_or(|lines| line.is_some_and(|n| lines.low <= n && n <= lines.high))
    });
    let chosen = most_specific(guarded.map(|rule| {
        (
            specificity(rule.agency_ico.as_deref(), &rule.licence_pattern),
            rule.line,
            *rule,
        )
    }))?;
    let Some(rule) = chosen else {
        diagnostics.push(
            Diagnostic::new(
                "jdf_transport_mode_rule_mismatch",
                Severity::Warning,
                subject.to_owned(),
                "reviewed rule guard mismatch",
            )
            .rule(R407),
        );
        return Ok(None);
    };
    diagnostics.push(
        Diagnostic::new(
            "jdf_transport_mode_corrected",
            Severity::Info,
            subject.to_owned(),
            format!("{}; effective_mode={}", rule.reason, rule.effective_mode),
        )
        .rule(R407),
    );
    Ok(Some(rule.effective_mode.to_string()))
}

/// R427, R428.
fn route_type_and_colours(
    version: &Version,
    mode: &str,
    public_line: Option<&str>,
    policy: &Policy,
    subject: &str,
    diagnostics: &mut Diagnostics,
) -> (i32, Box<str>, Box<str>) {
    let colours = &policy.colours;
    if mode != &*policy.modes.bus {
        let route_type = policy
            .route_types
            .modes
            .get(mode)
            .copied()
            .unwrap_or(policy.route_types.bus_default);
        let [color, text] = if mode == "M" {
            public_line
                .and_then(|line| colours.metro.get(line.to_uppercase().as_str()))
                .unwrap_or(&colours.metro_default)
                .clone()
        } else {
            colours
                .modes
                .get(mode)
                .cloned()
                .unwrap_or([colours.bus_default.clone(), colours.text.clone()])
        };
        return (route_type, color, text);
    }
    if version.regional_cross_border {
        return (
            policy.route_types.bus_cross_border,
            colours.bus_cross_border.clone(),
            colours.text.clone(),
        );
    }
    let code = version
        .route
        .route_type
        .as_ref()
        .map(RouteType::code)
        .unwrap_or("");
    let route_type = match policy.route_types.bus.get(code) {
        Some(&route_type) => route_type,
        None => {
            diagnostics.push(
                Diagnostic::new(
                    "jdf_route_type_unknown",
                    Severity::Warning,
                    subject.to_owned(),
                    format!("route type {code:?} is no JDF code: published as regional"),
                )
                .rule(R427),
            );
            policy.route_types.bus_default
        }
    };
    let color = colours
        .bus_by_route_type
        .get(code)
        .unwrap_or(&colours.bus_default)
        .clone();
    (route_type, color, colours.text.clone())
}

/// R429.
fn group(merged: &Merged, lines: &mut [Line]) -> Vec<Group> {
    let mut by_licence: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, version) in merged.versions.iter().enumerate() {
        by_licence.entry(&version.route.id).or_default().push(index);
    }
    let start = |index: usize| merged.versions[index].won[0].0;
    let mut groups = Vec::new();
    for (licence, versions) in by_licence {
        for detour in [false, true] {
            let mut keyed: BTreeMap<String, Vec<usize>> = BTreeMap::new();
            for &index in versions
                .iter()
                .filter(|&&i| merged.versions[i].route.detour == detour)
            {
                let version = &merged.versions[index];
                let line = &lines[index];
                let agency = &merged.agencies[version.agency];
                let key = [
                    licence,
                    identity::jdf::agency(&agency.id, agency.distinction).as_str(),
                    line.public_line.as_deref().unwrap_or(""),
                    &version.route.name,
                    &line.gtfs_route_type.to_string(),
                    &line.color,
                    &line.text_color,
                    if detour { "True" } else { "False" },
                ]
                .concat();
                keyed.entry(key).or_default().push(index);
            }
            let mut kinds: Vec<(String, Vec<usize>)> = keyed
                .into_iter()
                .map(|(key, mut members)| {
                    members.sort_by_key(|&i| (start(i), i));
                    (key, members)
                })
                .collect();
            kinds.sort_by(|a, b| (start(a.1[0]), &a.0).cmp(&(start(b.1[0]), &b.0)));
            for (rank, (key, members)) in kinds.into_iter().enumerate() {
                let semantics = (rank > 0).then(|| {
                    Sha256::digest(key.as_bytes())[..4]
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>()
                });
                let id = identity::jdf::route(licence, detour, semantics.as_deref());
                for &member in &members {
                    lines[member].group = groups.len();
                }
                groups.push(Group {
                    id,
                    detour,
                    members,
                });
            }
        }
    }
    groups
}

/// R426, R430: agencies and the routes of groups with published trips; per version, its route.
pub(crate) fn build(
    merged: &Merged,
    lines: &Lines,
    plans: &Plans,
    policy: &Policy,
    timetable: &mut Timetable,
    diagnostics: &mut Diagnostics,
) -> Vec<Option<RouteId>> {
    let mut agencies: HashMap<usize, AgencyId> = HashMap::new();
    let mut routes = vec![None; merged.versions.len()];
    for group in &lines.groups {
        let has_trips = group.members.iter().any(|&v| !plans.trips[v].is_empty());
        if !has_trips {
            diagnostics.push(
                Diagnostic::new(
                    "jdf_route_without_trips",
                    Severity::Warning,
                    group.id.as_str(),
                    "no trip of the route is published: the route is not published",
                )
                .rule(R430),
            );
            continue;
        }
        let representative = group.members[0];
        let version = &merged.versions[representative];
        let line = &lines.lines[representative];
        let agency = *agencies.entry(version.agency).or_insert_with(|| {
            timetable
                .agencies
                .push(agency_row(&merged.agencies[version.agency], policy))
        });
        let route = timetable.routes.push(Route {
            id: group.id.clone(),
            agency,
            mode: line.mode,
            gtfs_route_type: line.gtfs_route_type,
            short_name: line.public_line.clone(),
            long_name: (!version.route.name.is_empty()).then(|| version.route.name.clone()),
            description: group
                .detour
                .then(|| policy.routes.detour_description.clone()),
            url: None,
            color: Some(line.color.clone()),
            text_color: Some(line.text_color.clone()),
            sort_order: None,
            timetable_kind: if group.detour {
                TimetableKind::Detour
            } else {
                TimetableKind::Regular
            },
        });
        for &member in &group.members {
            routes[member] = Some(route);
        }
    }
    routes
}

/// R426.
fn agency_row(agency: &crate::batch::Agency, policy: &Policy) -> Agency {
    let blank = |text: &Option<Box<str>>| text.clone().filter(|t| !t.trim().is_empty());
    let p = &policy.agencies;
    let url = blank(&agency.website).map(|site| {
        if p.url_schemes.iter().any(|scheme| site.contains(&**scheme)) {
            site
        } else {
            format!("{}{site}", p.url_prefix).into()
        }
    });
    Agency {
        id: identity::jdf::agency(&agency.id, agency.distinction),
        name: agency.name.clone(),
        url,
        timezone: p.timezone.clone(),
        language: Some(p.language.clone()),
        phone: (!agency.office_phone.trim().is_empty()).then(|| agency.office_phone.clone()),
        fare_url: None,
        email: blank(&agency.email),
        registration: Some(agency.id.clone()),
    }
}
