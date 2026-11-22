//! EPSG:3035, ETRS89-extended / LAEA Europe: the Lambert azimuthal equal-area projection on the
//! GRS 1980 ellipsoid (EPSG method 9820, IOGP guidance note 7-2), to and from WGS84.
//!
//! The municipality polygons are stored in it (`docs/rules/jdf-read.md` 2.8); they are brought
//! to WGS84 once when loaded, and every decision is then made on WGS84 with the one distance
//! function (verdict Q5). ETRS89 and WGS84 are taken as equal (the EPSG transformation between
//! them is the null transformation at this accuracy). The inverse uses the EPSG series for the
//! latitude, accurate to well under a millimetre.

use std::sync::LazyLock;

use jr_model::Coord;

/// The EPSG:3035 definition.
const SEMI_MAJOR_AXIS_M: f64 = 6_378_137.0;
const INVERSE_FLATTENING: f64 = 298.257_222_101;
const ORIGIN_LATITUDE_DEG: f64 = 52.0;
const ORIGIN_LONGITUDE_DEG: f64 = 10.0;
const FALSE_EASTING_M: f64 = 4_321_000.0;
const FALSE_NORTHING_M: f64 = 3_210_000.0;

/// A position in EPSG:3035, in metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Laea {
    pub easting: f64,
    pub northing: f64,
}

struct Constants {
    e: f64,
    e2: f64,
    q_p: f64,
    r_q: f64,
    d: f64,
    sin_beta0: f64,
    cos_beta0: f64,
    lambda0: f64,
}

static CONSTANTS: LazyLock<Constants> = LazyLock::new(|| {
    let f = 1.0 / INVERSE_FLATTENING;
    let e2 = 2.0 * f - f * f;
    let e = e2.sqrt();
    let phi0 = ORIGIN_LATITUDE_DEG.to_radians();
    let q_p = q(e, e2, 1.0);
    let q0 = q(e, e2, phi0.sin());
    let beta0 = (q0 / q_p).asin();
    let r_q = SEMI_MAJOR_AXIS_M * (q_p / 2.0).sqrt();
    let d = SEMI_MAJOR_AXIS_M * phi0.cos()
        / (1.0 - e2 * phi0.sin().powi(2)).sqrt()
        / (r_q * beta0.cos());
    Constants {
        e,
        e2,
        q_p,
        r_q,
        d,
        sin_beta0: beta0.sin(),
        cos_beta0: beta0.cos(),
        lambda0: ORIGIN_LONGITUDE_DEG.to_radians(),
    }
});

/// The `q` function of the authalic latitude.
fn q(e: f64, e2: f64, sin_phi: f64) -> f64 {
    (1.0 - e2)
        * (sin_phi / (1.0 - e2 * sin_phi * sin_phi)
            - (1.0 / (2.0 * e)) * ((1.0 - e * sin_phi) / (1.0 + e * sin_phi)).ln())
}

/// WGS84 to EPSG:3035.
pub fn from_wgs84(coord: Coord) -> Laea {
    let c = &*CONSTANTS;
    let phi = coord.lat.to_radians();
    let dlambda = coord.lon.to_radians() - c.lambda0;
    let beta = (q(c.e, c.e2, phi.sin()) / c.q_p).clamp(-1.0, 1.0).asin();
    let (sin_beta, cos_beta) = beta.sin_cos();
    let b = c.r_q
        * (2.0 / (1.0 + c.sin_beta0 * sin_beta + c.cos_beta0 * cos_beta * dlambda.cos())).sqrt();
    Laea {
        easting: FALSE_EASTING_M + b * c.d * cos_beta * dlambda.sin(),
        northing: FALSE_NORTHING_M
            + (b / c.d) * (c.cos_beta0 * sin_beta - c.sin_beta0 * cos_beta * dlambda.cos()),
    }
}

/// EPSG:3035 to WGS84.
pub fn to_wgs84(position: Laea) -> Coord {
    let c = &*CONSTANTS;
    let x = position.easting - FALSE_EASTING_M;
    let y = position.northing - FALSE_NORTHING_M;
    let rho = ((x / c.d).powi(2) + (c.d * y).powi(2)).sqrt();
    if rho == 0.0 {
        return Coord::new(ORIGIN_LONGITUDE_DEG, ORIGIN_LATITUDE_DEG);
    }
    let big_c = 2.0 * (rho / (2.0 * c.r_q)).clamp(-1.0, 1.0).asin();
    let (sin_c, cos_c) = big_c.sin_cos();
    let beta = (cos_c * c.sin_beta0 + c.d * y * sin_c * c.cos_beta0 / rho)
        .clamp(-1.0, 1.0)
        .asin();
    let lambda = c.lambda0
        + (x * sin_c).atan2(c.d * rho * c.cos_beta0 * cos_c - c.d * c.d * y * c.sin_beta0 * sin_c);
    let (e2, e4, e6) = (c.e2, c.e2 * c.e2, c.e2 * c.e2 * c.e2);
    let phi = beta
        + (e2 / 3.0 + 31.0 * e4 / 180.0 + 517.0 * e6 / 5040.0) * (2.0 * beta).sin()
        + (23.0 * e4 / 360.0 + 251.0 * e6 / 3780.0) * (4.0 * beta).sin()
        + (761.0 * e6 / 45360.0) * (6.0 * beta).sin();
    Coord::new(lambda.to_degrees(), phi.to_degrees())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metres_apart(a: Laea, b: Laea) -> f64 {
        (a.easting - b.easting).hypot(a.northing - b.northing)
    }

    #[test]
    fn epsg_guidance_example_and_origin() {
        // IOGP guidance note 7-2, method 9820, ETRS89 / LAEA Europe: 50°N 5°E.
        let example = Coord::new(5.0, 50.0);
        let published = Laea {
            easting: 3_962_799.45,
            northing: 2_999_718.85,
        };
        assert!(metres_apart(from_wgs84(example), published) < 0.01);
        let back = to_wgs84(published);
        assert!(
            (back.lon - 5.0).abs() < 1e-7 && (back.lat - 50.0).abs() < 1e-7,
            "{back:?}"
        );
        // The projection origin is the false origin.
        let origin = from_wgs84(Coord::new(10.0, 52.0));
        assert!(
            metres_apart(
                origin,
                Laea {
                    easting: 4_321_000.0,
                    northing: 3_210_000.0
                }
            ) < 1e-6
        );
    }

    #[test]
    fn round_trip_across_czechia_within_a_centimetre() {
        for lat in [48.5, 49.3, 50.1, 51.0] {
            for lon in [12.0, 13.9, 15.7, 18.9] {
                let coord = Coord::new(lon, lat);
                let projected = from_wgs84(coord);
                let back = from_wgs84(to_wgs84(projected));
                assert!(metres_apart(projected, back) < 0.01, "{coord:?}");
            }
        }
    }
}
