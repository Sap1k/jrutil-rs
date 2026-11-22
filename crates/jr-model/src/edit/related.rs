//! What is stated for a trip, found without scanning.
//!
//! Splitting a trip, replacing its calls and removing calls all touch the rows stated for that
//! one trip: notes and features, connection claims, travel restrictions, keys, call keys and
//! transfers. A regional overlay splits a hundred thousand trips in one batch, so the rows are
//! indexed by trip once, when a batch first needs it, and the index is kept in step as the
//! batch adds rows. Rows a batch removes are only marked, and dropped together when the batch
//! ends, so indexes stay valid while it runs. The tables end in the same order as if every edit
//! had been applied to them directly.

use std::collections::{HashMap, HashSet};

use crate::handle::TripId;
use crate::timetable::{
    Assignment, CallKey, ConnectionClaim, Entity, Restriction, RestrictionScope, Scope, SourceKey,
    Timetable, Transfer,
};

type Rows = HashMap<TripId, Vec<u32>>;

#[derive(Default)]
pub(super) struct Related {
    built: bool,
    assignments: Rows,
    claims: Rows,
    /// Restrictions that name the trip, as their call or in their route stop's call list.
    restrictions: Rows,
    keys: Rows,
    call_keys: Rows,
    transfers: Rows,
    dead_assignments: HashSet<u32>,
    dead_claims: HashSet<u32>,
    dead_restrictions: HashSet<u32>,
    dead_call_keys: HashSet<u32>,
}

fn assignment_trip(assignment: &Assignment) -> Option<TripId> {
    match assignment.scope {
        Scope::Trip(trip) | Scope::Call { trip, .. } | Scope::CallRange { trip, .. } => Some(trip),
        Scope::Route(_) | Scope::Location(_) => None,
    }
}

fn restriction_trips(restriction: &Restriction) -> Vec<TripId> {
    match &restriction.scope {
        RestrictionScope::Call { trip, .. } => vec![*trip],
        RestrictionScope::RouteStop { calls, .. } => {
            let mut trips: Vec<TripId> = calls.iter().map(|(trip, _)| *trip).collect();
            trips.sort_unstable();
            trips.dedup();
            trips
        }
    }
}

fn transfer_trips(transfer: &Transfer) -> impl Iterator<Item = TripId> {
    let from = transfer.from_trip;
    let to = transfer.to_trip.filter(|to| Some(*to) != from);
    from.into_iter().chain(to)
}

fn add(rows: &mut Rows, trip: TripId, index: usize) {
    rows.entry(trip).or_default().push(index as u32);
}

impl Related {
    fn ensure(&mut self, t: &Timetable) {
        if self.built {
            return;
        }
        self.built = true;
        for (index, row) in t.assignments.iter().enumerate() {
            if let Some(trip) = assignment_trip(row) {
                add(&mut self.assignments, trip, index);
            }
        }
        for (index, row) in t.connection_claims.iter().enumerate() {
            add(&mut self.claims, row.origin_trip, index);
        }
        for (index, row) in t.restrictions.iter().enumerate() {
            for trip in restriction_trips(row) {
                add(&mut self.restrictions, trip, index);
            }
        }
        for (index, row) in t.keys.iter().enumerate() {
            if let Entity::Trip(trip) = row.entity {
                add(&mut self.keys, trip, index);
            }
        }
        for (index, row) in t.call_keys.iter().enumerate() {
            add(&mut self.call_keys, row.trip, index);
        }
        for (index, row) in t.transfers.iter().enumerate() {
            for trip in transfer_trips(row) {
                add(&mut self.transfers, trip, index);
            }
        }
    }

    // --- Rows a batch adds: appended to their table, and to the index when there is one. ---

    pub fn push_assignment(&mut self, t: &mut Timetable, row: Assignment) {
        if let (true, Some(trip)) = (self.built, assignment_trip(&row)) {
            add(&mut self.assignments, trip, t.assignments.len());
        }
        t.assignments.push(row);
    }

    pub fn push_claim(&mut self, t: &mut Timetable, row: ConnectionClaim) {
        if self.built {
            add(&mut self.claims, row.origin_trip, t.connection_claims.len());
        }
        t.connection_claims.push(row);
    }

    pub fn push_restriction(&mut self, t: &mut Timetable, row: Restriction) {
        if self.built {
            for trip in restriction_trips(&row) {
                add(&mut self.restrictions, trip, t.restrictions.len());
            }
        }
        t.restrictions.push(row);
    }

    pub fn push_key(&mut self, t: &mut Timetable, row: SourceKey) {
        if let (true, Entity::Trip(trip)) = (self.built, row.entity) {
            add(&mut self.keys, trip, t.keys.len());
        }
        t.keys.push(row);
    }

    pub fn push_call_key(&mut self, t: &mut Timetable, row: CallKey) {
        if self.built {
            add(&mut self.call_keys, row.trip, t.call_keys.len());
        }
        t.call_keys.push(row);
    }

    pub fn push_transfer(&mut self, t: &mut Timetable, row: Transfer) {
        if self.built {
            for trip in transfer_trips(&row) {
                add(&mut self.transfers, trip, t.transfers.len());
            }
        }
        t.transfers.push(row);
    }

    fn of(rows: &Rows, trip: TripId) -> Vec<u32> {
        rows.get(&trip).cloned().unwrap_or_default()
    }

    /// Everything stated for `trip` is stated for its copy `new` too.
    pub fn split(&mut self, t: &mut Timetable, trip: TripId, new: TripId) {
        self.ensure(t);
        for index in Self::of(&self.assignments, trip) {
            if self.dead_assignments.contains(&index) {
                continue;
            }
            let mut copy = t.assignments[index as usize].clone();
            copy.scope = match copy.scope {
                Scope::Trip(_) => Scope::Trip(new),
                Scope::Call { sequence, .. } => Scope::Call {
                    trip: new,
                    sequence,
                },
                Scope::CallRange { from, to, .. } => Scope::CallRange {
                    trip: new,
                    from,
                    to,
                },
                other => other,
            };
            self.push_assignment(t, copy);
        }
        for index in Self::of(&self.claims, trip) {
            if self.dead_claims.contains(&index) {
                continue;
            }
            let mut copy = t.connection_claims[index as usize].clone();
            copy.origin_trip = new;
            self.push_claim(t, copy);
        }
        for index in Self::of(&self.restrictions, trip) {
            if self.dead_restrictions.contains(&index) {
                continue;
            }
            let on_call = match &t.restrictions[index as usize].scope {
                RestrictionScope::Call { sequence, .. } => Some(*sequence),
                RestrictionScope::RouteStop { .. } => None,
            };
            if let Some(sequence) = on_call {
                let mut copy = t.restrictions[index as usize].clone();
                copy.scope = RestrictionScope::Call {
                    trip: new,
                    sequence,
                };
                self.push_restriction(t, copy);
            } else if let RestrictionScope::RouteStop { calls, .. } =
                &mut t.restrictions[index as usize].scope
            {
                let mut all = calls.to_vec();
                let copies: Vec<_> = all
                    .iter()
                    .filter(|(on, _)| *on == trip)
                    .map(|&(_, sequence)| (new, sequence))
                    .collect();
                all.extend(copies);
                *calls = all.into_boxed_slice();
                add(&mut self.restrictions, new, index as usize);
            }
        }
        for index in Self::of(&self.keys, trip) {
            let mut copy = t.keys[index as usize].clone();
            copy.entity = Entity::Trip(new);
            self.push_key(t, copy);
        }
        for index in Self::of(&self.call_keys, trip) {
            if self.dead_call_keys.contains(&index) {
                continue;
            }
            let mut copy = t.call_keys[index as usize].clone();
            copy.trip = new;
            self.push_call_key(t, copy);
        }
        // A transfer between two trips that are both split ends up between every pair of their
        // parts: the copy is indexed under its other end too.
        for index in Self::of(&self.transfers, trip) {
            let mut copy = t.transfers[index as usize].clone();
            let moved = |end: Option<TripId>| end.map(|on| if on == trip { new } else { on });
            copy.from_trip = moved(copy.from_trip);
            copy.to_trip = moved(copy.to_trip);
            self.push_transfer(t, copy);
        }
    }

    /// The calls of `trip` changed their sequences: what was stated for a call follows it to
    /// `moved(sequence)`, and goes when that is `None`.
    pub fn resequence(
        &mut self,
        t: &mut Timetable,
        trip: TripId,
        moved: impl Fn(i32) -> Option<i32>,
    ) {
        self.ensure(t);
        for index in Self::of(&self.claims, trip) {
            let row = &mut t.connection_claims[index as usize];
            match moved(row.origin_sequence) {
                Some(to) => row.origin_sequence = to,
                None => {
                    self.dead_claims.insert(index);
                }
            }
        }
        for index in Self::of(&self.restrictions, trip) {
            let row = &mut t.restrictions[index as usize];
            let kept = match &mut row.scope {
                RestrictionScope::Call { trip: on, sequence } => {
                    *on != trip
                        || moved(*sequence).is_some_and(|to| {
                            *sequence = to;
                            true
                        })
                }
                RestrictionScope::RouteStop { calls, .. } => {
                    let all: Vec<_> = calls
                        .iter()
                        .filter_map(|&(on, sequence)| {
                            if on == trip {
                                Some((on, moved(sequence)?))
                            } else {
                                Some((on, sequence))
                            }
                        })
                        .collect();
                    *calls = all.into_boxed_slice();
                    !calls.is_empty()
                }
            };
            if !kept {
                self.dead_restrictions.insert(index);
            }
        }
        for index in Self::of(&self.call_keys, trip) {
            let row = &mut t.call_keys[index as usize];
            match moved(row.sequence) {
                Some(to) => row.sequence = to,
                None => {
                    self.dead_call_keys.insert(index);
                }
            }
        }
        for index in Self::of(&self.assignments, trip) {
            let row = &mut t.assignments[index as usize];
            let kept = match &mut row.scope {
                Scope::Call { sequence, .. } => moved(*sequence).is_some_and(|to| {
                    *sequence = to;
                    true
                }),
                Scope::CallRange { from, to, .. } => match (moved(*from), moved(*to)) {
                    (Some(start), Some(end)) => {
                        (*from, *to) = (start, end);
                        true
                    }
                    _ => false,
                },
                _ => true,
            };
            if !kept {
                self.dead_assignments.insert(index);
            }
        }
    }

    /// Drops the rows the batch removed.
    pub fn finish(self, t: &mut Timetable) {
        fn drop_rows<T>(rows: &mut Vec<T>, dead: &HashSet<u32>) {
            if dead.is_empty() {
                return;
            }
            let mut index = 0u32;
            rows.retain(|_| {
                let keep = !dead.contains(&index);
                index += 1;
                keep
            });
        }
        drop_rows(&mut t.assignments, &self.dead_assignments);
        drop_rows(&mut t.connection_claims, &self.dead_claims);
        drop_rows(&mut t.restrictions, &self.dead_restrictions);
        drop_rows(&mut t.call_keys, &self.dead_call_keys);
    }
}
