//! Administrative areas in WGS84 (okresy, municipalities): which area contains a point, how far
//! a point is from an area, which areas lie near an area.
//!
//! An area is one or more polygons, each an exterior ring with holes; a point is inside an area
//! when it is inside one of its polygons (inside the exterior, outside every hole). A point on a
//! ring is inside or outside, whichever the crossing count says.
//!
//! Distances are decided with the one distance function (`jr_model::geo::distance_m`, verdict
//! Q5): the nearest boundary point is found in a local tangent plane at the query point
//! (equirectangular: longitude scaled by the cosine of the point's latitude, the scale taken from
//! `distance_m` itself), and the distance reported is `distance_m` from the point to that
//! boundary point. Because the plane is affine in longitude and latitude, the boundary point is
//! exact on the ring; only the choice among nearly equidistant segments carries the plane's
//! error, which is second order at the kilometre scale the rules use. Two areas are compared the
//! same way in a plane at the middle of the region where they come close.

use std::ops::Range;

use jr_model::Coord;
use jr_model::geo::distance_m;
use rayon::prelude::*;

use geometry::{Bounds, Plane, lerp, nearest_on_segment, segments_intersect};

mod geometry;

/// One polygon as given: the exterior ring and the holes, closed or not.
#[derive(Clone, Debug, PartialEq)]
pub struct Polygon {
    pub exterior: Vec<Coord>,
    pub holes: Vec<Vec<Coord>>,
}

/// A set of areas, numbered by their position in the list it was built from, indexed for
/// point lookups.
#[derive(Debug)]
pub struct Areas {
    /// Every ring's vertices, each ring closed (its last vertex equals its first).
    vertices: Vec<Coord>,
    rings: Vec<Ring>,
    polygons: Vec<PolygonEntry>,
    areas: Vec<AreaEntry>,
    /// Per polygon band, the edges (by first vertex) crossing it.
    band_offsets: Vec<u32>,
    band_edges: Vec<u32>,
    grid: Grid,
    /// Metres per degree of latitude, as `distance_m` measures it.
    metres_per_degree: f64,
}

#[derive(Clone, Copy, Debug)]
struct Ring {
    vertices: (u32, u32),
    exterior: bool,
}

#[derive(Clone, Debug)]
struct PolygonEntry {
    area: u32,
    rings: (u32, u32),
    bounds: Bounds,
    /// The polygon's latitude range is cut into `band_count` equal bands.
    band_count: u32,
    band_height: f64,
    /// Into `band_offsets`: `band_count + 1` offsets.
    band_start: u32,
}

#[derive(Clone, Debug)]
struct AreaEntry {
    polygons: (u32, u32),
    bounds: Bounds,
}

/// A uniform grid over all polygon boxes: per cell, the polygons whose box touches it.
#[derive(Debug)]
struct Grid {
    bounds: Bounds,
    columns: usize,
    rows: usize,
    cell_offsets: Vec<u32>,
    cell_polygons: Vec<u32>,
}

impl Grid {
    fn new(polygons: &[PolygonEntry]) -> Grid {
        let bounds = polygons.iter().fold(Bounds::EMPTY, |bounds, polygon| {
            bounds.union(polygon.bounds)
        });
        let side = ((polygons.len() as f64).sqrt().ceil() as usize).clamp(1, 1024);
        let mut grid = Grid {
            bounds,
            columns: side,
            rows: side,
            cell_offsets: Vec::new(),
            cell_polygons: Vec::new(),
        };
        let mut cells: Vec<Vec<u32>> = vec![Vec::new(); side * side];
        for (index, polygon) in polygons.iter().enumerate() {
            let (first_column, first_row) = grid.cell_of(polygon.bounds.min);
            let (last_column, last_row) = grid.cell_of(polygon.bounds.max);
            for row in first_row..=last_row {
                for column in first_column..=last_column {
                    cells[row * side + column].push(index as u32);
                }
            }
        }
        grid.cell_offsets.push(0);
        for cell in cells {
            grid.cell_polygons.extend(cell);
            grid.cell_offsets.push(grid.cell_polygons.len() as u32);
        }
        grid
    }

    fn cell_of(&self, coord: Coord) -> (usize, usize) {
        let along = |value: f64, min: f64, max: f64, count: usize| {
            let span = max - min;
            if span.is_nan() || span <= 0.0 {
                return 0;
            }
            (((value - min) / span * count as f64) as usize).min(count - 1)
        };
        (
            along(
                coord.lon,
                self.bounds.min.lon,
                self.bounds.max.lon,
                self.columns,
            ),
            along(
                coord.lat,
                self.bounds.min.lat,
                self.bounds.max.lat,
                self.rows,
            ),
        )
    }

    fn polygons_at(&self, coord: Coord) -> &[u32] {
        if !self.bounds.contains(coord) {
            return &[];
        }
        let (column, row) = self.cell_of(coord);
        let cell = row * self.columns + column;
        &self.cell_polygons[self.cell_offsets[cell] as usize..self.cell_offsets[cell + 1] as usize]
    }
}

impl Areas {
    /// Indexes the areas; area `i` is `areas[i]`. Rings with fewer than three distinct
    /// vertices are kept but contain nothing.
    pub fn new(areas: &[Vec<Polygon>]) -> Areas {
        let mut vertices = Vec::new();
        let mut rings = Vec::new();
        let mut polygons = Vec::new();
        let mut area_entries = Vec::with_capacity(areas.len());
        for (area_index, area) in areas.iter().enumerate() {
            let first_polygon = polygons.len() as u32;
            let mut area_bounds = Bounds::EMPTY;
            for polygon in area {
                let first_ring = rings.len() as u32;
                let mut bounds = Bounds::EMPTY;
                let all_rings = std::iter::once((&polygon.exterior, true))
                    .chain(polygon.holes.iter().map(|hole| (hole, false)));
                for (ring, exterior) in all_rings {
                    let start = vertices.len() as u32;
                    vertices.extend_from_slice(ring);
                    if let (Some(&first), Some(&last)) = (ring.first(), ring.last())
                        && first != last
                    {
                        vertices.push(first);
                    }
                    for &vertex in ring {
                        bounds.include(vertex);
                    }
                    rings.push(Ring {
                        vertices: (start, vertices.len() as u32),
                        exterior,
                    });
                }
                area_bounds = area_bounds.union(bounds);
                polygons.push(PolygonEntry {
                    area: area_index as u32,
                    rings: (first_ring, rings.len() as u32),
                    bounds,
                    band_count: 0,
                    band_height: 0.0,
                    band_start: 0,
                });
            }
            area_entries.push(AreaEntry {
                polygons: (first_polygon, polygons.len() as u32),
                bounds: area_bounds,
            });
        }
        // Per polygon, its edges by latitude band, built in parallel and joined in order.
        let bands: Vec<(u32, f64, Vec<u32>, Vec<u32>)> = polygons
            .par_iter()
            .map(|polygon| polygon_bands(polygon, &rings, &vertices))
            .collect();
        let mut band_offsets = Vec::new();
        let mut band_edges = Vec::new();
        for (polygon, (count, height, offsets, edges)) in polygons.iter_mut().zip(bands) {
            polygon.band_count = count;
            polygon.band_height = height;
            polygon.band_start = band_offsets.len() as u32;
            let base = band_edges.len() as u32;
            band_offsets.extend(offsets.into_iter().map(|offset| base + offset));
            band_edges.extend(edges);
        }
        let grid = Grid::new(&polygons);
        Areas {
            vertices,
            rings,
            polygons,
            areas: area_entries,
            band_offsets,
            band_edges,
            grid,
            metres_per_degree: distance_m(Coord::new(0.0, 0.0), Coord::new(0.0, 1.0)),
        }
    }

    pub fn len(&self) -> usize {
        self.areas.len()
    }

    pub fn is_empty(&self) -> bool {
        self.areas.is_empty()
    }

    /// The areas containing the point, ascending. More than one means overlapping areas (the
    /// caller decides: the F# `czechTownByPoint` took none and logged an error).
    pub fn containing(&self, point: Coord) -> Vec<usize> {
        let mut found: Vec<usize> = self
            .grid
            .polygons_at(point)
            .iter()
            .filter(|&&polygon| self.polygon_contains(polygon as usize, point))
            .map(|&polygon| self.polygons[polygon as usize].area as usize)
            .collect();
        found.dedup();
        found
    }

    /// Whether the area contains the point.
    pub fn contains(&self, area: usize, point: Coord) -> bool {
        self.areas[area].bounds.contains(point)
            && self
                .polygon_range(area)
                .any(|polygon| self.polygon_contains(polygon, point))
    }

    /// Metres from the point to the nearest point on any ring (exterior or hole) of the area,
    /// inside or outside (NTS `polygon.Boundary.Distance`, JDF-R046). Infinite for an area
    /// without rings.
    pub fn boundary_distance_m(&self, area: usize, point: Coord) -> f64 {
        self.nearest_ring_distance(area, point, |_| true)
    }

    /// Metres from the point to the nearest point on an exterior ring of the area (NTS
    /// `polygon.Shell.Distance`, JDF-R062).
    pub fn exterior_distance_m(&self, area: usize, point: Coord) -> f64 {
        self.nearest_ring_distance(area, point, |ring| ring.exterior)
    }

    /// Metres from the point to the area: 0 inside, else to the nearest ring (NTS
    /// `polygon.Distance(point)`, JDF-R063).
    pub fn distance_m(&self, area: usize, point: Coord) -> f64 {
        if self.contains(area, point) {
            0.0
        } else {
            self.boundary_distance_m(area, point)
        }
    }

    /// Metres between two areas: 0 when they overlap or touch, else between their nearest
    /// boundary points (NTS `polygon.Distance(polygon)`).
    pub fn area_distance_m(&self, left: usize, right: usize) -> f64 {
        self.area_distance_within(left, right, f64::INFINITY)
    }

    /// The other areas within `metres` of the area (area distance ≤ `metres`), ascending: okres
    /// adjacency (JDF-R046).
    pub fn within_m(&self, area: usize, metres: f64) -> Vec<usize> {
        let reach = self.areas[area]
            .bounds
            .grown(metres, self.metres_per_degree);
        (0..self.areas.len())
            .filter(|&other| other != area && self.areas[other].bounds.intersects(&reach))
            .filter(|&other| self.area_distance_within(area, other, metres) <= metres)
            .collect()
    }

    fn polygon_range(&self, area: usize) -> Range<usize> {
        let (first, last) = self.areas[area].polygons;
        first as usize..last as usize
    }

    fn ring_range(&self, polygon: usize) -> Range<usize> {
        let (first, last) = self.polygons[polygon].rings;
        first as usize..last as usize
    }

    /// The edges of a ring, by first vertex.
    fn ring_edges(&self, ring: usize) -> Range<usize> {
        let (start, end) = self.rings[ring].vertices;
        start as usize..(end as usize).saturating_sub(1).max(start as usize)
    }

    fn edge(&self, first_vertex: usize) -> (Coord, Coord) {
        (self.vertices[first_vertex], self.vertices[first_vertex + 1])
    }

    /// Even-odd crossing count over the edges of the point's latitude band.
    fn polygon_contains(&self, polygon: usize, point: Coord) -> bool {
        let entry = &self.polygons[polygon];
        if !entry.bounds.contains(point) || entry.band_count == 0 {
            return false;
        }
        let band = (((point.lat - entry.bounds.min.lat) / entry.band_height) as usize)
            .min(entry.band_count as usize - 1);
        let offsets = entry.band_start as usize + band;
        let edges = &self.band_edges
            [self.band_offsets[offsets] as usize..self.band_offsets[offsets + 1] as usize];
        let mut inside = false;
        for &edge in edges {
            let (a, b) = self.edge(edge as usize);
            if (a.lat > point.lat) != (b.lat > point.lat) {
                let crossing = a.lon + (point.lat - a.lat) * (b.lon - a.lon) / (b.lat - a.lat);
                if point.lon < crossing {
                    inside = !inside;
                }
            }
        }
        inside
    }

    fn nearest_ring_distance(
        &self,
        area: usize,
        point: Coord,
        rings: impl Fn(&Ring) -> bool,
    ) -> f64 {
        let plane = Plane::at(point, self.metres_per_degree);
        let origin = plane.project(point);
        let mut best: Option<(f64, Coord)> = None;
        for polygon in self.polygon_range(area) {
            for ring in self.ring_range(polygon) {
                if !rings(&self.rings[ring]) {
                    continue;
                }
                for edge in self.ring_edges(ring) {
                    let (a, b) = self.edge(edge);
                    let (t, squared) =
                        nearest_on_segment(origin, plane.project(a), plane.project(b));
                    if best.is_none_or(|(best, _)| squared < best) {
                        best = Some((squared, lerp(a, b, t)));
                    }
                }
            }
        }
        best.map_or(f64::INFINITY, |(_, nearest)| distance_m(point, nearest))
    }

    /// The area distance, computed exactly when it is at most `limit` (edges farther than
    /// `limit` from the other area's box are skipped, so a larger distance comes out as some
    /// value above `limit`, possibly infinite).
    fn area_distance_within(&self, left: usize, right: usize, limit: f64) -> f64 {
        let mut best = f64::INFINITY;
        for a in self.polygon_range(left) {
            for b in self.polygon_range(right) {
                best = best.min(self.polygon_distance_within(a, b, limit));
                if best == 0.0 {
                    return 0.0;
                }
            }
        }
        best
    }

    fn polygon_distance_within(&self, a: usize, b: usize, limit: f64) -> f64 {
        let (bounds_a, bounds_b) = (self.polygons[a].bounds, self.polygons[b].bounds);
        let (reach_a, reach_b) = if limit.is_finite() {
            (
                bounds_a.grown(limit, self.metres_per_degree),
                bounds_b.grown(limit, self.metres_per_degree),
            )
        } else {
            (bounds_a.union(bounds_b), bounds_a.union(bounds_b))
        };
        if !reach_a.intersects(&bounds_b) {
            return f64::INFINITY;
        }
        // One inside the other (not in a hole): a vertex of one is inside the other.
        let first_vertex = |polygon: usize| {
            let ring = self.polygons[polygon].rings.0 as usize;
            self.vertices
                .get(self.rings[ring].vertices.0 as usize)
                .copied()
        };
        if first_vertex(b).is_some_and(|vertex| self.polygon_contains(a, vertex))
            || first_vertex(a).is_some_and(|vertex| self.polygon_contains(b, vertex))
        {
            return 0.0;
        }
        let near_edges = |polygon: usize, reach: &Bounds| -> Vec<usize> {
            self.ring_range(polygon)
                .flat_map(|ring| self.ring_edges(ring))
                .filter(|&edge| {
                    let (p, q) = self.edge(edge);
                    let mut bounds = Bounds::EMPTY;
                    bounds.include(p);
                    bounds.include(q);
                    bounds.intersects(reach)
                })
                .collect()
        };
        let edges_a = near_edges(a, &reach_b);
        let edges_b = near_edges(b, &reach_a);
        let plane = Plane::at(
            reach_a.intersection(&reach_b).centre(),
            self.metres_per_degree,
        );
        let mut best: Option<(f64, Coord, Coord)> = None;
        for &edge_a in &edges_a {
            let (p1, p2) = self.edge(edge_a);
            let (x1, x2) = (plane.project(p1), plane.project(p2));
            for &edge_b in &edges_b {
                let (q1, q2) = self.edge(edge_b);
                let (y1, y2) = (plane.project(q1), plane.project(q2));
                if segments_intersect(x1, x2, y1, y2) {
                    return 0.0;
                }
                let candidates = [
                    (nearest_on_segment(x1, y1, y2), p1, (q1, q2), false),
                    (nearest_on_segment(x2, y1, y2), p2, (q1, q2), false),
                    (nearest_on_segment(y1, x1, x2), q1, (p1, p2), true),
                    (nearest_on_segment(y2, x1, x2), q2, (p1, p2), true),
                ];
                for ((t, squared), vertex, (s1, s2), swapped) in candidates {
                    if best.is_none_or(|(best, _, _)| squared < best) {
                        let on_segment = lerp(s1, s2, t);
                        best = Some(if swapped {
                            (squared, on_segment, vertex)
                        } else {
                            (squared, vertex, on_segment)
                        });
                    }
                }
            }
        }
        best.map_or(f64::INFINITY, |(_, p, q)| distance_m(p, q))
    }
}

/// A polygon's edges by latitude band: (band count, band height, band offsets, edges).
fn polygon_bands(
    polygon: &PolygonEntry,
    rings: &[Ring],
    vertices: &[Coord],
) -> (u32, f64, Vec<u32>, Vec<u32>) {
    let edges: Vec<u32> = rings[polygon.rings.0 as usize..polygon.rings.1 as usize]
        .iter()
        .flat_map(|ring| ring.vertices.0..ring.vertices.1.saturating_sub(1).max(ring.vertices.0))
        .collect();
    let height = polygon.bounds.max.lat - polygon.bounds.min.lat;
    if edges.len() < 3 || height.is_nan() || height <= 0.0 {
        return (0, 0.0, Vec::new(), Vec::new());
    }
    let count = (edges.len() / 4).clamp(1, 4096);
    let band_height = height / count as f64;
    let band_of =
        |lat: f64| (((lat - polygon.bounds.min.lat) / band_height) as usize).min(count - 1);
    let mut per_band: Vec<Vec<u32>> = vec![Vec::new(); count];
    for edge in edges {
        let (a, b) = (vertices[edge as usize], vertices[edge as usize + 1]);
        let (first, last) = (band_of(a.lat.min(b.lat)), band_of(a.lat.max(b.lat)));
        for band in &mut per_band[first..=last] {
            band.push(edge);
        }
    }
    let mut offsets = Vec::with_capacity(count + 1);
    let mut flat = Vec::new();
    offsets.push(0);
    for band in per_band {
        flat.extend(band);
        offsets.push(flat.len() as u32);
    }
    (count as u32, band_height, offsets, flat)
}

#[cfg(test)]
mod tests;
