//! Means of positions in a local tangent plane (verdict Q5 of `docs/rules/jdf-read.md`): the
//! centroid of a group of candidates (JDF-R048, R049, R066, R078) and its radius.
//!
//! The plane is the equirectangular one the area index uses: east and north in metres at the
//! group's latitude. It is affine in longitude and latitude, so the mean in the plane is the mean
//! of the coordinates; distances are decided with `jr_model::geo::distance_m`.

use jr_model::Coord;
use jr_model::geo::distance_m;

/// The mean of the positions; `None` for none.
pub fn centroid(points: &[Coord]) -> Option<Coord> {
    if points.is_empty() {
        return None;
    }
    let count = points.len() as f64;
    let (lon, lat) = points.iter().fold((0.0, 0.0), |(lon, lat), point| {
        (lon + point.lon, lat + point.lat)
    });
    Some(Coord::new(lon / count, lat / count))
}

/// The largest distance in metres from the centroid to a position (NTS `pointsRadius`); 0 for
/// no positions.
pub fn radius_m(points: &[Coord]) -> f64 {
    centroid(points).map_or(0.0, |centre| {
        points
            .iter()
            .map(|&point| distance_m(centre, point))
            .fold(0.0, f64::max)
    })
}

/// The position `metres` north of `coord`, at `metres_per_degree` metres per degree of
/// latitude (the placeholder of CZPTT-R100 and JDF-R077).
pub fn north_of(coord: Coord, metres: f64, metres_per_degree: f64) -> Coord {
    Coord::new(coord.lon, coord.lat + metres / metres_per_degree)
}

/// The point at `ratio` of the way from `from` to `to`, in the plane.
pub fn interpolate(from: Coord, to: Coord, ratio: f64) -> Coord {
    Coord::new(
        from.lon + (to.lon - from.lon) * ratio,
        from.lat + (to.lat - from.lat) * ratio,
    )
}

/// A call with a precise position and a time, `index` its place in the call list.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub index: usize,
    pub coord: Coord,
    pub time: f64,
}

/// How a call between its nearest anchors is estimated (CZPTT-R100, JDF-R076, R077).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AlongRoute {
    /// Interpolated by time between two anchors.
    ByTime {
        left: Anchor,
        right: Anchor,
        coord: Coord,
    },
    /// Beside one anchor: the only one, or the nearer (by calls, the earlier on a tie) when the
    /// two are not reached one after the other in time.
    Beside { anchor: Anchor, calls_apart: usize },
}

/// The estimate for the call at `index`, reached at `time`, from its nearest anchors before and
/// after. Anchors whose times do not increase give no time to interpolate by, so the call is
/// placed beside the nearer one instead of not at all (CZPTT B24, JDF-read B18).
pub fn along_route(
    left: Option<Anchor>,
    right: Option<Anchor>,
    index: usize,
    time: f64,
) -> Option<AlongRoute> {
    let beside = |anchor: Anchor| AlongRoute::Beside {
        anchor,
        calls_apart: anchor.index.abs_diff(index),
    };
    match (left, right) {
        (Some(left), Some(right)) if right.time > left.time => {
            let ratio = ((time - left.time) / (right.time - left.time)).clamp(0.0, 1.0);
            Some(AlongRoute::ByTime {
                left,
                right,
                coord: interpolate(left.coord, right.coord, ratio),
            })
        }
        (Some(left), Some(right)) => Some(if right.index - index < index - left.index {
            beside(right)
        } else {
            beside(left)
        }),
        (Some(anchor), None) | (None, Some(anchor)) => Some(beside(anchor)),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centroid_and_radius_of_a_pair() {
        let a = Coord::new(14.0, 50.0);
        let b = Coord::new(14.0, 50.01);
        let centre = centroid(&[a, b]).unwrap();
        assert!((centre.lat - 50.005).abs() < 1e-12);
        let radius = radius_m(&[a, b]);
        assert!((radius - distance_m(a, b) / 2.0).abs() < 1e-6, "{radius}");
        assert_eq!(centroid(&[]), None);
    }

    #[test]
    fn czptt_r100_jdf_r076_inverted_anchors_place_the_call_beside_the_nearer() {
        let anchor = |index, lat, time| Anchor {
            index,
            coord: Coord::new(14.0, lat),
            time,
        };
        let by_time = along_route(
            Some(anchor(0, 50.0, 0.0)),
            Some(anchor(4, 50.4, 40.0)),
            1,
            10.0,
        );
        assert!(
            matches!(by_time, Some(AlongRoute::ByTime { coord, .. }) if (coord.lat - 50.1).abs() < 1e-12)
        );
        // Equal times: nothing to interpolate by; the later anchor is one call away.
        let beside = along_route(
            Some(anchor(0, 50.0, 30.0)),
            Some(anchor(3, 50.3, 30.0)),
            2,
            30.0,
        );
        assert_eq!(
            beside,
            Some(AlongRoute::Beside {
                anchor: anchor(3, 50.3, 30.0),
                calls_apart: 1
            })
        );
        // A tie goes to the earlier anchor.
        let tie = along_route(
            Some(anchor(0, 50.0, 30.0)),
            Some(anchor(2, 50.2, 20.0)),
            1,
            25.0,
        );
        assert!(
            matches!(tie, Some(AlongRoute::Beside { anchor, calls_apart: 1 }) if anchor.index == 0)
        );
        assert_eq!(along_route(None, None, 0, 0.0), None);
    }
}
