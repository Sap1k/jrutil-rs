//! Agencies (R080) and routes (R056, R058–R061).

use jr_formats::kadr::{Catalog, Line};
use jr_model::identity::czptt;
use jr_model::timetable::{Agency, Mode, Route, TimetableKind};
use jr_model::{AgencyId, Date, Diagnostic, Diagnostics, Rule, Severity};

use crate::path::SegmentKey;
use crate::policy::Policy;

/// The KADR catalogue names operators with legal qualifiers after a spaced dash
/// ("České dráhy, a.s. - peáž DB Netz") and gives URLs without a scheme.
pub(crate) const R080: Rule = Rule {
    id: "CZPTT-R080",
    summary: "An agency is the catalogue company, its name cut at the qualifier, its URL normalized or the default",
};

/// KADR lines are versioned by validity; a path takes the version valid on its first day (Q9).
pub(crate) const R056: Rule = Rule {
    id: "CZPTT-R056",
    summary: "A part's catalogue line is the version valid at the path's first date, latest start first",
};

/// The passenger-facing name of a line is its KADR mark; a train without one is known by its
/// category and number.
pub(crate) const R058: Rule = Rule {
    id: "CZPTT-R058",
    summary: "A route is labelled by the line mark, else category and train number, (NAD) for buses",
};

/// Routes are the catalogue line per operator and mode; trains without a line get one route
/// per operator, category, route type and number.
pub(crate) const R059: Rule = Rule {
    id: "CZPTT-R059",
    summary: "A route is keyed by line, operator and route type, else by operator, category, type and number",
};

/// Several parts produce one route; the first in processing order defines it.
pub(crate) const R060: Rule = Rule {
    id: "CZPTT-R060",
    summary: "A route's attributes come from its first part; a disagreeing later part is diagnosed",
};

/// The palette follows the passenger-facing service class.
pub(crate) const R061: Rule = Rule {
    id: "CZPTT-R061",
    summary: "A route's colour follows its route type",
};

/// One `validity` lookup for catalogue entries (R056, R085): the entries with `code` whose
/// validity contains `date` (an absent bound is open), the one starting latest.
pub(crate) fn valid_at<'a, T>(
    entries: &'a [T],
    code: &str,
    date: Date,
    fields: impl Fn(&T) -> (&str, Option<Date>, Option<Date>),
) -> Option<&'a T> {
    entries
        .iter()
        .filter(|entry| {
            let (entry_code, from, to) = fields(entry);
            entry_code == code
                && from.is_none_or(|from| from <= date)
                && to.is_none_or(|to| date <= to)
        })
        .fold(None, |best: Option<&T>, entry| match best {
            Some(best) if fields(best).1 > fields(entry).1 => Some(best),
            _ => Some(entry),
        })
}

pub(crate) fn line_at<'a>(catalog: &'a Catalog, code: &str, date: Date) -> Option<&'a Line> {
    valid_at(&catalog.lines, code, date, |line| {
        (&line.code, line.valid_from, line.valid_to)
    })
}

fn blank(value: &Option<Box<str>>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

/// R080.
pub(crate) fn agency(
    code: &str,
    catalog: &Catalog,
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> Agency {
    let rules = &policy.agencies;
    let company = catalog
        .companies
        .iter()
        .find(|company| &*company.code == code);
    if company.is_none() {
        diagnostics.push(
            Diagnostic::new(
                "czptt_unknown_operator",
                Severity::Warning,
                czptt::agency(code).as_str(),
                format!("operator {code:?} is not in the catalogue"),
            )
            .rule(R080),
        );
    }
    let name = match company.and_then(|company| blank(&company.name)) {
        Some(name) => match name.find(&*rules.qualifier_separator) {
            Some(at) if at > 0 && !name[..at].trim().is_empty() => name[..at].trim_end().into(),
            _ => name.into(),
        },
        None => format!("{}{code}", rules.unknown_name_prefix).into_boxed_str(),
    };
    let url = company
        .and_then(|company| blank(&company.url))
        .and_then(|url| {
            let url = if has_scheme(url) {
                url.to_owned()
            } else {
                format!("{}://{url}", rules.default_scheme)
            };
            let (scheme, rest) = url.split_once("://")?;
            let usable = rules
                .schemes
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(scheme))
                && !rest.is_empty()
                && !rest.starts_with('/')
                && !rest.chars().any(char::is_whitespace);
            usable.then(|| url.into_boxed_str())
        })
        .unwrap_or_else(|| rules.default_url.clone());
    Agency {
        id: czptt::agency(code),
        name,
        url: Some(url),
        timezone: rules.timezone.clone(),
        language: Some(rules.language.clone()),
        phone: None,
        fare_url: None,
        email: None,
    }
}

/// `^[a-z][a-z0-9+.-]*://`, case-insensitively.
fn has_scheme(url: &str) -> bool {
    let Some((scheme, _)) = url.split_once("://") else {
        return false;
    };
    let mut chars = scheme.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
}

/// A part's route (R056, R058–R061), before deduplication.
pub(crate) fn route(
    key: &SegmentKey,
    train_number: Option<&str>,
    first_date: Date,
    agency: AgencyId,
    catalog: &Catalog,
    policy: &Policy,
) -> Route {
    let line = key
        .line
        .as_deref()
        .and_then(|code| line_at(catalog, code, first_date).map(|line| (code, line)));
    let (id, label, long_name) = match line {
        Some((code, line)) => (
            czptt::line_route(code, &key.responsible_ru, key.route_type),
            blank(&line.mark).unwrap_or(code).to_owned(),
            line.name
                .as_deref()
                .filter(|name| !name.trim().is_empty())
                .map(|name| Box::from(name.trim_end())),
        ),
        None => {
            let designation = train_number.unwrap_or(&key.category);
            let label = match train_number {
                Some(number) => format!("{} {number}", key.category),
                None => key.category.to_string(),
            };
            (
                czptt::fallback_route(
                    &key.responsible_ru,
                    &key.category,
                    key.route_type,
                    designation,
                ),
                label,
                None,
            )
        }
    };
    let label = if key.replacement_bus {
        format!("{label}{}", policy.routes.replacement_bus_suffix)
    } else {
        label
    };
    Route {
        id,
        agency,
        mode: if key.replacement_bus {
            Mode::Bus
        } else {
            Mode::Rail
        },
        gtfs_route_type: key.route_type,
        short_name: Some(label.into_boxed_str()),
        long_name,
        description: None,
        url: None,
        color: Some(policy.route_color(key.route_type).into()),
        text_color: Some(policy.routes.text_color.clone()),
        sort_order: None,
        timetable_kind: TimetableKind::Regular,
    }
}

/// R060: whether a later part's route says something else than the one kept.
pub(crate) fn report_disagreement(kept: &Route, other: &Route, diagnostics: &mut Diagnostics) {
    let differs = kept.short_name != other.short_name
        || kept.long_name != other.long_name
        || kept.agency != other.agency;
    if differs {
        diagnostics.push(
            Diagnostic::new(
                "czptt_route_attributes_differ",
                Severity::Info,
                kept.id.as_str(),
                format!(
                    "a later part names it {:?} / {:?}; the first is kept",
                    other.short_name, other.long_name
                ),
            )
            .rule(R060),
        );
    }
}
