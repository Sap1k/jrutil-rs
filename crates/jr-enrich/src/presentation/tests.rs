use jr_formats::geodata::route_rules::parse_presentation;
use jr_model::identity::jdf;
use jr_model::timetable::{Agency, BindingMethod, Mode, Route, SourceKey, TimetableKind};
use jr_model::{Feed, Horizon, Precedence, apply};

use super::*;

/// One operator (IČO 27229131) with the routes of the given licences, each named after its
/// licence's last three digits and white on red.
fn timetable(licences: &[&str]) -> Timetable {
    let mut t = Timetable::new(Feed::Jdf, Horizon::gvd(2026).unwrap());
    let agency = t.agencies.push(Agency {
        id: jdf::agency("27229131", 1),
        name: "Arriva".into(),
        url: None,
        timezone: "Europe/Prague".into(),
        language: None,
        phone: None,
        fare_url: None,
        email: None,
        registration: Some("27229131".into()),
    });
    for licence in licences {
        let route = t.routes.push(Route {
            id: jdf::route(licence, false, None),
            agency,
            mode: Mode::Bus,
            gtfs_route_type: 701,
            short_name: Some(licence[3..].into()),
            long_name: None,
            description: None,
            url: None,
            color: Some("ff0000".into()),
            text_color: Some("ffffff".into()),
            sort_order: None,
            timetable_kind: TimetableKind::Regular,
        });
        t.keys.push(SourceKey {
            namespace: Namespace::CIS_LINE,
            identifier: (*licence).into(),
            entity: Entity::Route(route),
            valid_from: t.horizon.start(),
            valid_to: t.horizon.end(),
            binding: BindingMethod::Identity,
        });
    }
    t
}

fn rules(rows: &str) -> Vec<PresentationRule> {
    let header = "agency_id,licence,route_short_name,route_color,route_text_color,reason\n";
    parse_presentation(format!("{header}{rows}").as_bytes()).unwrap()
}

fn marking(t: &Timetable, licence: &str) -> String {
    let id = jdf::route(licence, false, None);
    let route = t.routes.rows().iter().find(|route| route.id == id).unwrap();
    [&route.short_name, &route.color, &route.text_color]
        .map(|value| value.as_deref().unwrap_or("-"))
        .join(" ")
}

#[test]
fn jdf_r431_each_field_takes_the_most_specific_rule_that_sets_it() {
    let mut t = timetable(&["721341", "721342", "580916"]);
    let rules = rules(
        "27229131,721341,AEx,,,brand\n\
         ,7213*,,1C1745,,livery of the family\n\
         27229131,*,X,00843d,03EAFC,operator default\n\
         11111111,580916,No,,,another operator\n",
    );
    let mut diagnostics = Diagnostics::new();
    let (edits, summary) = route_presentation(&t, &rules, &mut diagnostics).unwrap();
    apply(
        &mut t,
        edits,
        &Precedence { position: vec![] },
        &mut diagnostics,
    )
    .unwrap();

    // The licence rule names the line; the prefix rule beats the operator's `*`; the text
    // colour comes from the only rule that sets it. Colours are lower-cased.
    assert_eq!(marking(&t, "721341"), "AEx 1c1745 03eafc");
    assert_eq!(marking(&t, "721342"), "X 1c1745 03eafc");
    // Another operator's rule does not apply.
    assert_eq!(marking(&t, "580916"), "X 00843d 03eafc");
    assert_eq!(summary.overridden_routes, 3);
    assert_eq!(summary.by_field["route_short_name"], 3);
    assert_eq!(
        diagnostics.counts_by_code()["route_presentation_override"],
        9
    );
    // IDs are untouched.
    assert!(
        t.routes
            .rows()
            .iter()
            .any(|route| route.id == jdf::route("721341", false, None))
    );
}

#[test]
fn jdf_r431_a_value_equal_to_the_compiled_one_is_no_override() {
    let t = timetable(&["721341"]);
    let rules = rules(",721341,341,FF0000,,same as compiled\n");
    let mut diagnostics = Diagnostics::new();
    let (edits, summary) = route_presentation(&t, &rules, &mut diagnostics).unwrap();
    assert!(edits.is_empty());
    assert_eq!(summary.overridden_routes, 0);
    assert!(diagnostics.is_empty());
}

#[test]
fn jdf_r431_equally_specific_rules_setting_one_field_are_an_error() {
    let t = timetable(&["721341"]);
    let rules = rules(
        ",721341,AEx,,,one\n\
         ,721341,AE,,,two\n",
    );
    let error = route_presentation(&t, &rules, &mut Diagnostics::new()).unwrap_err();
    assert_eq!((error.field, error.lines), ("route_short_name", vec![2, 3]));
}
