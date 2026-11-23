//! Places, boarding points and location features (JDF-R415–R423, R425).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use jr_model::identity::{self, escape};
use jr_model::timetable::{
    Assignment, AssignmentKind, BoardingPoint, BoardingPointKind, CoordSource, LocationRef, Place,
    PlaceKind, Position, Scope, Tristate,
};
use jr_model::{BoardingPointId, Diagnostic, Diagnostics, PlaceId, Rule, Severity, Timetable};

use super::Policy;
use super::calls::{Plans, Point};
use crate::fixed::Origin;
use crate::merge::{Merged, Spelling};

/// One stop place per merged stop, `jdf:stop:<number>`, named as IDOS shows it: `town`,
/// `town,district`, `town,,nearby place` or `town,district,nearby place`; the name parts,
/// okres and country as published stay separate fields.
pub const R415: Rule = Rule {
    id: "JDF-R415",
    summary: "Stop places are named town[,district][,,nearby place] with the parts kept",
};

/// A stop's position is the merge's: catalogue, gap-fill and OSM positions are exact, route
/// estimates estimated; a stop without one has no position, diagnosed (no 0/0, no ` [?]`: the
/// publisher marks estimates).
pub const R416: Rule = Rule {
    id: "JDF-R416",
    summary: "Stop positions are exact or estimated by their origin; missing ones are diagnosed",
};

/// `@` on a stop makes it, and each of its boarding points, wheelchair accessible; JDF has no
/// code for the opposite.
pub const R417: Rule = Rule {
    id: "JDF-R417",
    summary: "A stop coded @ is wheelchair accessible, otherwise unknown",
};

/// Calls without a post board at the stop's unspecified point.
pub const R418: Rule = Rule {
    id: "JDF-R418",
    summary: "Calls without a post board at the stop's unspecified point",
};

/// An `Oznacniky` post a call uses: `…:post:id:<number>`, shown as its post name, else the one
/// textual number calls give it, else its number.
pub const R419: Rule = Rule {
    id: "JDF-R419",
    summary: "Authored posts show their name, the calls' single number, or their number",
};

/// A call's stanoviště text without a post number: a textual post `…:post:<text>` showing it.
pub const R420: Rule = Rule {
    id: "JDF-R420",
    summary: "A call's stanoviště text is a textual post",
};

/// A call boards at its authored post, else its textual post, else the unspecified point; a post
/// number the stop's `Oznacniky` lacks (F# crashed, B14) is diagnosed and boards at the stop.
pub const R421: Rule = Rule {
    id: "JDF-R421",
    summary: "A call boards at its post, its text, or the stop; unknown posts are diagnosed",
};

/// Calls giving one authored post several textual numbers: none is shown, diagnosed.
pub const R422: Rule = Rule {
    id: "JDF-R422",
    summary: "An authored post with conflicting numbers shows none",
};

/// Only stops and boarding points a published call uses are in the timetable.
pub const R423: Rule = Rule {
    id: "JDF-R423",
    summary: "Only stops and boarding points published calls use are kept",
};

/// A stop's fixed codes with a location meaning (policy table) are location features.
pub const R425: Rule = Rule {
    id: "JDF-R425",
    summary: "A stop's facility codes are location features",
};

/// Handles of what calls use.
pub(crate) struct Points {
    pub places: HashMap<u32, PlaceId>,
    pub points: HashMap<(u32, Point), BoardingPointId>,
}

/// A name part as a fact of its own: a part the source wrote as blanks says nothing.
fn stated(part: &Option<Box<str>>) -> Option<Box<str>> {
    part.clone().filter(|text| !text.trim().is_empty())
}

/// R415's name.
pub(crate) fn stop_name(name: &Spelling) -> String {
    match (name.district.as_deref(), name.nearby_place.as_deref()) {
        (None, None) => name.town.to_string(),
        (Some(d), None) => format!("{},{d}", name.town),
        (None, Some(n)) => format!("{},,{n}", name.town),
        (Some(d), Some(n)) => format!("{},{d},{n}", name.town),
    }
}

pub(crate) fn build(
    merged: &Merged,
    plans: &Plans,
    policy: &Policy,
    timetable: &mut Timetable,
    diagnostics: &mut Diagnostics,
) -> Points {
    // R423: what the published calls use, with the numbers calls give authored posts.
    let mut used: BTreeMap<u32, BTreeSet<Point>> = BTreeMap::new();
    let mut numbers: BTreeMap<(u32, u64), BTreeSet<&str>> = BTreeMap::new();
    for trips in &plans.trips {
        for trip in trips {
            for call in &trip.calls {
                used.entry(call.stop)
                    .or_default()
                    .insert(call.point.clone());
                if let (Point::Authored(post), Some(number)) = (&call.point, &call.authored_number)
                {
                    numbers
                        .entry((call.stop, *post))
                        .or_default()
                        .insert(number);
                }
            }
        }
    }
    let mut places = HashMap::new();
    let mut points = HashMap::new();
    for (&stop_index, stop_points) in &used {
        let stop = &merged.stops[stop_index as usize];
        let id = identity::jdf::stop(stop.number);
        let codes = merged.code_sets.get(stop.codes);
        let position = stop.location.as_ref().map(|location| {
            let (source, exact) = match location.origin {
                Origin::Gapfill => (CoordSource::Gapfill, true),
                Origin::Catalogue => (CoordSource::Catalogue, true),
                Origin::Osm => (CoordSource::Osm, true),
                Origin::RouteTime => (CoordSource::RouteTime, false),
                Origin::RouteEnd => (CoordSource::RouteEnd, false),
            };
            if exact {
                Position::exact(location.coord, source)
            } else {
                Position::estimated(location.coord, source)
            }
        });
        if position.is_none() {
            diagnostics.push(
                Diagnostic::new(
                    "jdf_stop_without_position",
                    Severity::Warning,
                    id.as_str(),
                    "a published stop has no position",
                )
                .rule(R416),
            );
        }
        let place = timetable.places.push(Place {
            id: id.clone(),
            kind: PlaceKind::StopPlace,
            name: stop_name(&stop.name).into(),
            source_code: None,
            description: None,
            municipality: Some(stop.name.town.clone()),
            district_name: stated(&stop.name.district),
            district_code: stop.name.okres.clone(),
            nearby_place: stated(&stop.name.nearby_place),
            country: stop.name.country.clone(),
            position,
            url: None,
            // Every road stop, the few foreign ones included (a recorded limitation).
            timezone: Some(policy.agencies.timezone.clone()),
            wheelchair_boarding: if codes.contains(&policy.codes.wheelchair_stop) {
                Tristate::Yes
            } else {
                Tristate::Unknown
            },
        });
        places.insert(stop_index, place);

        // R425.
        let mut seen = BTreeSet::new();
        for code in codes {
            if let Some(&feature) = policy.location_features.get(code)
                && seen.insert(code)
            {
                timetable.assignments.push(Assignment {
                    scope: Scope::Location(LocationRef::Place(place)),
                    kind: AssignmentKind::Feature(feature),
                    source_code: Some(code.clone()),
                    note: None,
                    source_object: Some(format!("{id}:attribute:{}", escape(code)).into()),
                });
            }
        }

        for point in stop_points {
            let (point_id, kind, public_code, source_code, source_name) = match point {
                Point::Unspecified => (
                    identity::jdf::unspecified_point(&id),
                    BoardingPointKind::Unspecified,
                    None,
                    None,
                    None,
                ),
                Point::Authored(post) => {
                    let row = stop.posts.iter().find(|p| p.post_id == *post);
                    let point_id = identity::jdf::authored_post(&id, *post);
                    let given = numbers.get(&(stop_index, *post));
                    if given.is_some_and(|n| n.len() > 1) {
                        diagnostics.push(
                            Diagnostic::new(
                                "jdf_post_numbers_conflict",
                                Severity::Warning,
                                point_id.as_str(),
                                format!(
                                    "calls give the post conflicting numbers: {}",
                                    given
                                        .into_iter()
                                        .flatten()
                                        .copied()
                                        .collect::<Vec<_>>()
                                        .join(",")
                                ),
                            )
                            .rule(R422),
                        );
                    }
                    let named = row
                        .and_then(|r| r.post_name.as_deref())
                        .map(str::trim)
                        .filter(|n| !n.is_empty());
                    let single = given
                        .filter(|n| n.len() == 1)
                        .and_then(|n| n.iter().next().copied());
                    let public: Box<str> = named
                        .or(single)
                        .map(Box::from)
                        .unwrap_or_else(|| post.to_string().into());
                    (
                        point_id,
                        BoardingPointKind::Post,
                        Some(public),
                        Some(post.to_string().into()),
                        row.and_then(|r| r.name.clone()),
                    )
                }
                Point::Textual(text) => (
                    identity::jdf::textual_post(&id, text),
                    BoardingPointKind::Post,
                    Some(text.clone()),
                    None,
                    None,
                ),
            };
            let handle = timetable.boarding_points.push(BoardingPoint {
                id: point_id,
                place,
                kind,
                public_code,
                source_code,
                source_name,
                description: None,
                position: None,
                // R417: the stop's code speaks for each of its boarding points.
                wheelchair_boarding: timetable.places[place].wheelchair_boarding,
            });
            points.insert((stop_index, point.clone()), handle);
        }
    }
    Points { places, points }
}
