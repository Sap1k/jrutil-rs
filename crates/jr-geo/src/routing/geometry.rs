//! Where a route point lies relative to the path that passes it (POST-R053).

use jr_model::Rule;

use crate::laea::Laea;

use super::policy::EvidencePolicy;
use super::reader::Reader;
use super::snap::Snap;
use super::thread::{Face, FaceSide};

/// The attachment's snap edge must occur exactly once in the path it is measured along (first
/// the variant's own path, else the path through the attachment); a path that visits the edge
/// twice cannot say which visit the vehicle stops on, so the geometry abstains. The corridor
/// direction runs from the point a heading window before the snap to the point the window after
/// it, along the path (the snap edge's own direction when they coincide). The signed lateral
/// offset is the route point's distance from the snap point across that direction, negative to
/// the right of travel; the headings are azimuths in the projection's grid. The face is the
/// side of the snap edge's thread (in the thread's own direction) the point lies on, the centre
/// within a band. Distances to the corridor and proximity equal the snap distance (the names
/// are the pack's; section 7 of the inventory).
pub const POST_R053: Rule = Rule {
    id: "POST-R053",
    summary: "side, offset, headings and face of a route point along the path through its snap",
};

/// Guards against division by zero lengths (numeric, not thresholds).
const MIN_LENGTH_M: f64 = 1e-6;
const MIN_DIRECTION_SQUARED: f64 = 1e-6;

/// The geometry of an attachment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Geometry {
    pub signed_lateral_offset: f64,
    pub corridor_heading: f64,
    pub attachment_heading: f64,
    pub face: Face,
}

/// The azimuth of a direction in degrees, [0, 360).
fn heading(dx: f64, dy: f64) -> f64 {
    (dx.atan2(dy).to_degrees() + 360.0) % 360.0
}

/// The smaller angle between two headings.
pub(super) fn angle_difference(left: f64, right: f64) -> f64 {
    let difference = (left - right).abs() % 360.0;
    difference.min(360.0 - difference)
}

fn signed_offset(dx: f64, dy: f64, from: Laea, to: Laea) -> f64 {
    (dx * (to.northing - from.northing) - dy * (to.easting - from.easting))
        / dx.hypot(dy).max(MIN_LENGTH_M)
}

impl Reader<'_> {
    /// The point `window` metres from the snap along the path, forwards or backwards; the
    /// path's end when it is shorter.
    fn along(&self, path: &[u32], index: usize, snap: &Snap, window: f64, forward: bool) -> Laea {
        let mut index = index as isize;
        let mut fraction = snap.fraction;
        let mut remaining = window;
        let mut point = snap.point;
        while index >= 0 && (index as usize) < path.len() {
            let edge = path[index as usize];
            let (from, to) = (self.xy(self.edge_from(edge)), self.xy(self.edge_to(edge)));
            let length = self.edge_length(edge).max(MIN_LENGTH_M);
            let available = if forward { 1.0 - fraction } else { fraction } * length;
            if remaining <= available {
                let delta = remaining / length;
                let at = if forward {
                    fraction + delta
                } else {
                    fraction - delta
                };
                return Laea {
                    easting: from.easting + at * (to.easting - from.easting),
                    northing: from.northing + at * (to.northing - from.northing),
                };
            }
            remaining -= available;
            if forward {
                point = to;
                index += 1;
                fraction = 0.0;
            } else {
                point = from;
                index -= 1;
                fraction = 1.0;
            }
        }
        point
    }

    /// R053: the geometry of a route point at `position` attached by `snap`, along `path`;
    /// `None` unless the snap edge occurs exactly once in the path.
    pub(super) fn attachment_geometry(
        &self,
        path: &[u32],
        position: Laea,
        snap: &Snap,
        policy: &EvidencePolicy,
    ) -> Option<Geometry> {
        let mut visits = path
            .iter()
            .enumerate()
            .filter(|&(_, &edge)| edge == snap.edge);
        let (index, _) = visits.next()?;
        if visits.next().is_some() {
            return None;
        }
        let before = self.along(path, index, snap, policy.heading_window_m, false);
        let after = self.along(path, index, snap, policy.heading_window_m, true);
        let (from, to) = (
            self.xy(self.edge_from(snap.edge)),
            self.xy(self.edge_to(snap.edge)),
        );
        let (edge_dx, edge_dy) = (to.easting - from.easting, to.northing - from.northing);
        let (window_dx, window_dy) = (
            after.easting - before.easting,
            after.northing - before.northing,
        );
        let (dx, dy) = if window_dx * window_dx + window_dy * window_dy < MIN_DIRECTION_SQUARED {
            (edge_dx, edge_dy)
        } else {
            (window_dx, window_dy)
        };
        let thread = self.directed_thread(snap.edge);
        let edge_signed = signed_offset(edge_dx, edge_dy, snap.point, position);
        let thread_signed = if thread.reversed {
            -edge_signed
        } else {
            edge_signed
        };
        let side = if thread_signed.abs() <= policy.face_centre_band_m {
            FaceSide::Centre
        } else if thread_signed < 0.0 {
            FaceSide::Right
        } else {
            FaceSide::Left
        };
        Some(Geometry {
            signed_lateral_offset: signed_offset(dx, dy, snap.point, position),
            corridor_heading: heading(dx, dy),
            attachment_heading: heading(edge_dx, edge_dy),
            face: Face {
                thread: thread.thread,
                side,
            },
        })
    }
}
