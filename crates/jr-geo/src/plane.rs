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
}
