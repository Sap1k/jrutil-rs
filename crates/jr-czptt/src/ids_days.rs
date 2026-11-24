//! Trips whose IDS zones differ by day (CZPTT-R120): the assembly records the days on which
//! calendar-limited memberships (R119) do not hold, and this pass splits those days off.

use jr_model::edit::{Edit, EditBuilder, Edits};
use jr_model::timetable::Zone;
use jr_model::{DateSet, PublicId, Rule, Timetable, TripId};

/// A trip some of whose IDS memberships do not hold on some of its days runs those days as a
/// trip of its own with the zones that hold (`SplitTrip` and `SetCallZones`). Each set of
/// memberships that do not hold is one variant, `identity::czptt::ids_variant`, named after
/// those memberships as the source writes them, so the name does not move with the dates. The
/// days on which every membership holds keep the trip's ID; when there are none, the first
/// variant (in the order of the memberships) does.
///
/// A call's zones are per call, not per day; a passenger's fare depends on the day's
/// membership (B17 fixed, `docs/roadmap.md`).
pub const R120: Rule = Rule {
    id: "CZPTT-R120",
    summary: "A trip is split where its IDS zones differ by day",
};

/// What the assembly decided, for [`edits`].
#[derive(Debug, Default)]
pub struct IdsDays {
    pub(crate) trips: Vec<TripDays>,
}

impl IdsDays {
    pub fn is_empty(&self) -> bool {
        self.trips.is_empty()
    }
}

#[derive(Debug)]
pub(crate) struct TripDays {
    pub trip: TripId,
    /// The days every membership holds.
    pub full: DateSet,
    pub variants: Vec<Variant>,
}

#[derive(Debug)]
pub(crate) struct Variant {
    pub id: PublicId,
    pub days: DateSet,
    /// Per published call sequence whose zones differ from the trip's, its zones.
    pub zones: Vec<(i32, Box<[Zone]>)>,
}

/// R120 as edits.
pub fn edits(timetable: &Timetable, days: &IdsDays) -> Edits {
    let mut b = EditBuilder::new(timetable, "czptt-ids-days");
    for trip in &days.trips {
        let keeper = trip.full.is_empty().then_some(0);
        for (index, variant) in trip.variants.iter().enumerate() {
            let service = b.service(variant.days);
            let target = if keeper == Some(index) {
                b.push(
                    R120,
                    Edit::SetTripService {
                        trip: trip.trip,
                        service,
                    },
                );
                trip.trip
            } else {
                b.split_trip(R120, trip.trip, variant.id.clone(), service)
            };
            for (sequence, zones) in &variant.zones {
                b.push(
                    R120,
                    Edit::SetCallZones {
                        trip: target,
                        sequence: *sequence,
                        zones: zones.clone(),
                    },
                );
            }
        }
        if keeper.is_none() {
            let service = b.service(trip.full);
            b.push(
                R120,
                Edit::SetTripService {
                    trip: trip.trip,
                    service,
                },
            );
        }
    }
    b.finish()
}
