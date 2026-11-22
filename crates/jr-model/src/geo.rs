//! Coordinates and the one distance function.

use serde::{Deserialize, Serialize};

/// A WGS84 position in degrees.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Coord {
    pub lon: f64,
    pub lat: f64,
}

impl Coord {
    pub const fn new(lon: f64, lat: f64) -> Self {
        Self { lon, lat }
    }

    pub fn is_valid(self) -> bool {
        self.lon.is_finite()
            && self.lat.is_finite()
            && (-180.0..=180.0).contains(&self.lon)
            && (-90.0..=90.0).contains(&self.lat)
    }
}

/// The mean Earth radius in metres (IUGG).
const EARTH_RADIUS_M: f64 = 6_371_008.8;

/// Great-circle distance in metres. Every distance threshold in policy is measured with this;
/// indexes may pre-filter with something cheaper, but the decision uses this function.
pub fn distance_m(a: Coord, b: Coord) -> f64 {
    let (lat_a, lat_b) = (a.lat.to_radians(), b.lat.to_radians());
    let half_dlat = (lat_b - lat_a) / 2.0;
    let half_dlon = (b.lon - a.lon).to_radians() / 2.0;
    let h = half_dlat.sin().powi(2) + lat_a.cos() * lat_b.cos() * half_dlon.sin().powi(2);
    2.0 * EARTH_RADIUS_M * h.sqrt().min(1.0).asin()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_between_two_prague_stations() {
        // Praha hl.n. to Praha Masarykovo n.: about 620 m.
        let hlavni = Coord::new(14.4359, 50.0830);
        let masarykovo = Coord::new(14.4343, 50.0885);
        let metres = distance_m(hlavni, masarykovo);
        assert!((600.0..640.0).contains(&metres), "{metres}");
        assert_eq!(distance_m(hlavni, hlavni), 0.0);
    }
}
