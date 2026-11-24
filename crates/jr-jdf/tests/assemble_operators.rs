//! Alternate operators (JDF-R483, R484): `Altdop` rows decide who runs a trip day by day, and
//! the operators pass splits the trip by operator.

mod support;

use jiff::civil::Weekday;
use jr_jdf::batch::AgencyAlternation;
use jr_jdf::fixed::FixedBatch;
use jr_model::edit::{Precedence, apply};
use jr_model::{Diagnostics, Timetable};
use support::merge::*;

const LICENCE: &str = "100001";
const OTHER: &str = "87654321";

/// A line run every day of the year, with a second operator and the given `Altdop` rows as
/// (trip number, day codes).
fn line(alternations: &[(u64, &[&str])]) -> FixedBatch {
    let mut builder = year_route(
        batch("a")
            .stop(at(stop("Alfa", None, None), 50.0, 14.0))
            .stop(at(stop("Beta", None, None), 50.01, 14.0)),
        LICENCE,
    )
    .trip(LICENCE, 1, &[]);
    let rows: Vec<AgencyAlternation> = alternations
        .iter()
        .map(|&(trip_id, codes)| AgencyAlternation {
            route_id: LICENCE.into(),
            trip_id,
            agency_id: OTHER.into(),
            attributes: builder.slots(codes),
            time_type: None,
            reserved1: None,
            date_from: None,
            date_to: None,
            agency_distinction: Some(1),
            route_distinction: Some(1),
        })
        .collect();
    let mut batch = builder.build();
    let mut other = agency("");
    other.id = OTHER.into();
    other.name = "Jiný dopravce".into();
    batch.batch.agencies.push(other);
    batch.batch.agency_alternations = rows;
    batch
}

/// Assembles and applies the operators pass as the pipeline does.
fn split(batch: FixedBatch) -> Timetable {
    let (assembled, _) = assemble(vec![batch]);
    let mut timetable = assembled.timetable;
    let (edits, variants) = jr_jdf::assemble::operators::edits(&timetable, &assembled.operators);
    assert_eq!(
        variants.len(),
        usize::from(!assembled.operators.is_empty()),
        "one variant route per operator of the line"
    );
    let precedence = Precedence {
        position: Vec::new(),
    };
    apply(&mut timetable, edits, &precedence, &mut Diagnostics::new()).unwrap();
    assert_eq!(timetable.check(), Vec::new());
    timetable
}

/// (trip ID, route ID, agency ID, whether its days are all weekend days, day count).
fn trips(t: &Timetable) -> Vec<(String, String, String, bool, usize)> {
    t.trips
        .rows()
        .iter()
        .map(|trip| {
            let route = t.routes.get(trip.route).unwrap();
            let agency = t.agencies.get(route.agency).unwrap();
            let days = t.services.get(trip.service);
            let weekend = days
                .dates(t.horizon)
                .all(|day| matches!(day.weekday(), Weekday::Saturday | Weekday::Sunday));
            (
                trip.id.as_str().to_owned(),
                route.id.as_str().to_owned(),
                agency.id.as_str().to_owned(),
                weekend,
                days.iter().count(),
            )
        })
        .collect()
}

#[test]
fn r483_r484_weekends_by_another_operator_are_a_trip_of_their_own() {
    let t = split(line(&[(1, &["6", "7"])]));
    let found = trips(&t);
    assert_eq!(found.len(), 2);
    let (own, other) = (&found[0], &found[1]);
    assert_eq!(
        (own.0.as_str(), own.1.as_str(), own.2.as_str(), own.3),
        (
            "jdf:trip:100001:251214:1",
            "jdf:route:100001",
            "jdf:agency:12345678:1",
            false
        )
    );
    assert_eq!(
        (
            other.0.as_str(),
            other.1.as_str(),
            other.2.as_str(),
            other.3
        ),
        (
            "jdf:trip:100001:251214:1:operator:87654321:1",
            "jdf:route:100001:operator:87654321:1",
            "jdf:agency:87654321:1",
            true
        )
    );
    // Together they run on the days the trip ran: the year's 364 days.
    assert_eq!(own.4 + other.4, 364);
    assert_eq!(other.4, 104);
}

#[test]
fn r484_a_line_another_operator_runs_throughout_keeps_its_trip_ids() {
    // Trip number 0: every trip of the line; no day codes: every day.
    let t = split(line(&[(0, &[])]));
    let found = trips(&t);
    assert_eq!(found.len(), 1);
    assert_eq!(
        (
            found[0].0.as_str(),
            found[0].1.as_str(),
            found[0].2.as_str()
        ),
        (
            "jdf:trip:100001:251214:1",
            "jdf:route:100001:operator:87654321:1",
            "jdf:agency:87654321:1"
        )
    );
    assert_eq!(found[0].4, 364);
}

#[test]
fn r483_a_row_for_another_trip_changes_nothing() {
    let t = split(line(&[(3, &[])]));
    assert_eq!(trips(&t).len(), 1);
    assert_eq!(t.trips.rows()[0].id.as_str(), "jdf:trip:100001:251214:1");
}
