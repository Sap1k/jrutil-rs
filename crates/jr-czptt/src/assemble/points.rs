//! Places and boarding points (R052–R055).

use jr_model::identity::czptt;
use jr_model::timetable::{BoardingPoint, BoardingPointKind, Place, PlaceKind, Tristate};
use jr_model::{BoardingPointId, Diagnostic, Diagnostics, PlaceId, PublicId, Rule, Severity};

use super::{Builder, display_name};
use crate::path::Call;

/// A station per railway point; passengers board at a track the source names, else somewhere
/// at the station. A point where a passenger call of a part happens is a stop place.
pub(crate) const R052: Rule = Rule {
    id: "CZPTT-R052",
    summary: "One place per point; calls board at their track or the place's unspecified point",
};

/// SR70 names Czech points authoritatively; otherwise the source's name, first seen first.
pub(crate) const R053: Rule = Rule {
    id: "CZPTT-R053",
    summary: "A place is named by the catalogue, else by its first source name",
};

/// The source names a track by code and sometimes by a friendlier name; a replacement bus
/// stops somewhere at the station that the source does not say.
pub(crate) const R054: Rule = Rule {
    id: "CZPTT-R054",
    summary: "A track shows its name, else its code; a replacement bus boards at the station's BUS point",
};

/// The BUS point's ID equals that of a track coded `BUS`; none exists, so such a track shares
/// the point and is diagnosed.
pub(crate) const R055: Rule = Rule {
    id: "CZPTT-R055",
    summary: "A track coded like the replacement-bus point shares it and is diagnosed",
};

impl Builder<'_> {
    /// R052, R053.
    pub(super) fn place(&mut self, call: &Call) -> (PlaceId, PublicId) {
        let key = (call.country.clone(), call.code.clone());
        if let Some((handle, station)) = self.places.get(&key) {
            let (handle, station) = (*handle, station.clone());
            if let (Some(Some(first)), Some(name)) =
                (self.source_names.get(&handle), call.source_name.as_ref())
                && first != name
            {
                self.name_conflicts.insert(handle);
            }
            return (handle, station);
        }
        let station = czptt::station(&call.country, &call.code);
        let stop_place = self.stop_places.contains(&key);
        let name = display_name(self.point_names, call).unwrap_or_default();
        let handle = self.timetable.places.push(Place {
            id: station.clone(),
            kind: if stop_place {
                PlaceKind::StopPlace
            } else {
                PlaceKind::OperationalPoint
            },
            name: name.into(),
            source_code: Some(call.code.clone()),
            description: None,
            municipality: None,
            district_name: None,
            district_code: None,
            nearby_place: None,
            country: Some(call.country.clone()),
            position: None,
            url: None,
            timezone: stop_place.then(|| self.policy.places.timezone.clone()),
            wheelchair_boarding: Tristate::Unknown,
        });
        self.source_names.insert(handle, call.source_name.clone());
        self.places.insert(key, (handle, station.clone()));
        (handle, station)
    }

    pub(super) fn point(&mut self, point: BoardingPoint) -> BoardingPointId {
        if let Some(&existing) = self.points.get(&point.id) {
            return existing;
        }
        let id = point.id.clone();
        let handle = self.timetable.boarding_points.push(point);
        self.points.insert(id, handle);
        handle
    }

    pub(super) fn new_point(
        place: PlaceId,
        id: PublicId,
        kind: BoardingPointKind,
    ) -> BoardingPoint {
        BoardingPoint {
            id,
            place,
            kind,
            public_code: None,
            source_code: None,
            source_name: None,
            description: None,
            position: None,
            wheelchair_boarding: Tristate::Unknown,
        }
    }

    /// R054, R055.
    pub(super) fn track(
        &mut self,
        place: PlaceId,
        station: &PublicId,
        call: &Call,
        diagnostics: &mut Diagnostics,
    ) -> Option<BoardingPointId> {
        let subsidiary = call.subsidiary.as_ref()?;
        let id = czptt::track(station, &subsidiary.code);
        let public = subsidiary
            .name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(&subsidiary.code);
        if let Some(&existing) = self.points.get(&id) {
            let point = &mut self.timetable.boarding_points[existing];
            if point.kind != BoardingPointKind::Track {
                point.kind = BoardingPointKind::Track;
                point.source_code = Some(subsidiary.code.clone());
                point.source_name = subsidiary.name.clone();
                self.collision(existing, diagnostics);
            }
            return Some(existing);
        }
        let mut point = Self::new_point(place, id, BoardingPointKind::Track);
        point.public_code = Some(public.into());
        point.source_code = Some(subsidiary.code.clone());
        point.source_name = subsidiary.name.clone();
        Some(self.point(point))
    }

    fn collision(&mut self, point: BoardingPointId, diagnostics: &mut Diagnostics) {
        if self.bus_collisions.insert(point) {
            diagnostics.push(
                Diagnostic::new(
                    "czptt_track_coded_bus",
                    Severity::Warning,
                    self.timetable.boarding_points[point].id.as_str(),
                    "a track has the replacement-bus point's code; they share one point",
                )
                .rule(R055),
            );
        }
    }

    pub(super) fn bus_point(
        &mut self,
        place: PlaceId,
        station: &PublicId,
        diagnostics: &mut Diagnostics,
    ) -> BoardingPointId {
        let id = czptt::replacement_bus_point(station);
        if let Some(&existing) = self.points.get(&id) {
            if self.timetable.boarding_points[existing].kind == BoardingPointKind::Track {
                self.collision(existing, diagnostics);
            }
            return existing;
        }
        let rules = &self.policy.replacement_bus_point;
        let mut point = Self::new_point(place, id, BoardingPointKind::Platform);
        point.public_code = Some(rules.public_code.clone());
        point.description = Some(rules.description.clone());
        self.point(point)
    }
}
