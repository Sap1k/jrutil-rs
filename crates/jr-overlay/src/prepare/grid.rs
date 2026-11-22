//! A point grid in degrees for OVL-R027: candidates near a point, to be decided by exact
//! distance. The cells are small enough that a lookup visits few points and the neighbourhood
//! is widened by the latitude, so every point within the radius is returned.

use std::collections::HashMap;

use jr_model::Coord;

/// Cell edge in degrees (about 220 m of latitude).
const CELL: f64 = 0.002;
/// Metres per degree of latitude, rounded down so the neighbourhood errs wide.
const METRES_PER_DEGREE: f64 = 111_000.0;

#[derive(Debug, Default)]
pub struct Grid {
    cells: HashMap<(i32, i32), Vec<u32>>,
}

fn cell(point: Coord) -> (i32, i32) {
    (
        (point.lat / CELL).floor() as i32,
        (point.lon / CELL).floor() as i32,
    )
}

impl Grid {
    pub fn insert(&mut self, point: Coord, value: u32) {
        self.cells.entry(cell(point)).or_default().push(value);
    }

    /// Appends every value with a point that may lie within `metres` of `point` (a superset).
    pub fn near(&self, point: Coord, metres: f64, out: &mut Vec<u32>) {
        let (row, column) = cell(point);
        let lat_cells = (metres / METRES_PER_DEGREE / CELL).ceil() as i32 + 1;
        let cos = point.lat.to_radians().cos().abs().max(0.01);
        let lon_cells = (metres / (METRES_PER_DEGREE * cos) / CELL).ceil() as i32 + 1;
        for r in row - lat_cells..=row + lat_cells {
            for c in column - lon_cells..=column + lon_cells {
                if let Some(values) = self.cells.get(&(r, c)) {
                    out.extend_from_slice(values);
                }
            }
        }
    }
}
