//! Plane geometry helpers of the area index: boxes in longitude and latitude, the local
//! tangent plane, segments.

use jr_model::Coord;

/// A longitude/latitude box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Bounds {
    pub(super) min: Coord,
    pub(super) max: Coord,
}

impl Bounds {
    pub(super) const EMPTY: Bounds = Bounds {
        min: Coord::new(f64::INFINITY, f64::INFINITY),
        max: Coord::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
    };

    pub(super) fn include(&mut self, coord: Coord) {
        self.min.lon = self.min.lon.min(coord.lon);
        self.min.lat = self.min.lat.min(coord.lat);
        self.max.lon = self.max.lon.max(coord.lon);
        self.max.lat = self.max.lat.max(coord.lat);
    }

    pub(super) fn union(mut self, other: Bounds) -> Bounds {
        self.include(other.min);
        self.include(other.max);
        self
    }

    pub(super) fn contains(&self, coord: Coord) -> bool {
        (self.min.lon..=self.max.lon).contains(&coord.lon)
            && (self.min.lat..=self.max.lat).contains(&coord.lat)
    }

    pub(super) fn intersects(&self, other: &Bounds) -> bool {
        self.min.lon <= other.max.lon
            && other.min.lon <= self.max.lon
            && self.min.lat <= other.max.lat
            && other.min.lat <= self.max.lat
    }

    /// Grown by `metres` on every side (degrees from `metres_per_degree`, longitude at the
    /// latitude farthest from the equator).
    pub(super) fn grown(&self, metres: f64, metres_per_degree: f64) -> Bounds {
        let dlat = metres / metres_per_degree;
        let widest = self.min.lat.abs().max(self.max.lat.abs()) + dlat;
        let dlon = dlat / widest.min(89.0).to_radians().cos();
        Bounds {
            min: Coord::new(self.min.lon - dlon, self.min.lat - dlat),
            max: Coord::new(self.max.lon + dlon, self.max.lat + dlat),
        }
    }

    pub(super) fn intersection(&self, other: &Bounds) -> Bounds {
        Bounds {
            min: Coord::new(
                self.min.lon.max(other.min.lon),
                self.min.lat.max(other.min.lat),
            ),
            max: Coord::new(
                self.max.lon.min(other.max.lon),
                self.max.lat.min(other.max.lat),
            ),
        }
    }

    pub(super) fn centre(&self) -> Coord {
        Coord::new(
            (self.min.lon + self.max.lon) / 2.0,
            (self.min.lat + self.max.lat) / 2.0,
        )
    }
}

/// The local tangent plane at a point: metres east and north, equirectangular.
pub(super) struct Plane {
    origin: Coord,
    east: f64,
    north: f64,
}

impl Plane {
    pub(super) fn at(origin: Coord, metres_per_degree: f64) -> Plane {
        Plane {
            origin,
            east: metres_per_degree * origin.lat.to_radians().cos(),
            north: metres_per_degree,
        }
    }

    pub(super) fn project(&self, coord: Coord) -> (f64, f64) {
        (
            (coord.lon - self.origin.lon) * self.east,
            (coord.lat - self.origin.lat) * self.north,
        )
    }
}

pub(super) fn lerp(a: Coord, b: Coord, t: f64) -> Coord {
    Coord::new(a.lon + (b.lon - a.lon) * t, a.lat + (b.lat - a.lat) * t)
}

/// The parameter of the point of segment `a`–`b` nearest to `p`, and the squared distance.
pub(super) fn nearest_on_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx * dx + dy * dy;
    let t = if length > 0.0 {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (x, y) = (a.0 + t * dx - p.0, a.1 + t * dy - p.1);
    (t, x * x + y * y)
}

/// Whether two closed segments share a point.
pub(super) fn segments_intersect(
    p1: (f64, f64),
    p2: (f64, f64),
    q1: (f64, f64),
    q2: (f64, f64),
) -> bool {
    let orientation = |a: (f64, f64), b: (f64, f64), c: (f64, f64)| {
        let cross = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
        cross.partial_cmp(&0.0).map_or(0, |order| order as i8)
    };
    let on_segment = |a: (f64, f64), b: (f64, f64), c: (f64, f64)| {
        c.0 >= a.0.min(b.0) && c.0 <= a.0.max(b.0) && c.1 >= a.1.min(b.1) && c.1 <= a.1.max(b.1)
    };
    let (d1, d2) = (orientation(q1, q2, p1), orientation(q1, q2, p2));
    let (d3, d4) = (orientation(p1, p2, q1), orientation(p1, p2, q2));
    if d1 * d2 < 0 && d3 * d4 < 0 {
        return true;
    }
    (d1 == 0 && on_segment(q1, q2, p1))
        || (d2 == 0 && on_segment(q1, q2, p2))
        || (d3 == 0 && on_segment(p1, p2, q1))
        || (d4 == 0 && on_segment(p1, p2, q2))
}
