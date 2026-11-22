use super::*;

/// A closed lon/lat rectangle.
fn rectangle(west: f64, south: f64, east: f64, north: f64) -> Vec<Coord> {
    vec![
        Coord::new(west, south),
        Coord::new(east, south),
        Coord::new(east, north),
        Coord::new(west, north),
        Coord::new(west, south),
    ]
}

fn square(west: f64, south: f64, east: f64, north: f64) -> Polygon {
    Polygon {
        exterior: rectangle(west, south, east, north),
        holes: Vec::new(),
    }
}

/// Degrees of latitude for `metres`, by the one distance function.
fn degrees(metres: f64) -> f64 {
    metres / distance_m(Coord::new(14.0, 50.0), Coord::new(14.0, 51.0))
}

#[test]
fn point_in_a_polygon_with_a_hole_and_in_an_area_of_two_polygons() {
    let with_hole = Polygon {
        exterior: rectangle(14.0, 50.0, 15.0, 51.0),
        holes: vec![rectangle(14.4, 50.4, 14.6, 50.6)],
    };
    // Area 1 has two polygons (the two `LT` features, B2); the second is given open.
    let mut open = rectangle(17.0, 50.0, 17.5, 50.5);
    open.pop();
    let two = vec![
        square(16.0, 50.0, 16.5, 50.5),
        Polygon {
            exterior: open,
            holes: Vec::new(),
        },
    ];
    let areas = Areas::new(&[vec![with_hole], two]);
    assert_eq!(areas.containing(Coord::new(14.2, 50.2)), [0]);
    assert!(
        areas.containing(Coord::new(14.5, 50.5)).is_empty(),
        "in the hole"
    );
    assert_eq!(areas.containing(Coord::new(16.2, 50.2)), [1]);
    assert_eq!(areas.containing(Coord::new(17.2, 50.2)), [1]);
    assert!(
        areas.containing(Coord::new(16.7, 50.2)).is_empty(),
        "between the polygons"
    );
    assert!(areas.contains(1, Coord::new(17.4, 50.4)));
    assert!(!areas.contains(0, Coord::new(17.4, 50.4)));
}

#[test]
fn boundary_distance_counts_holes_and_agrees_with_distance_m() {
    let with_hole = Polygon {
        exterior: rectangle(14.0, 50.0, 15.0, 51.0),
        holes: vec![rectangle(14.4, 50.4, 14.6, 50.6)],
    };
    let areas = Areas::new(&[vec![with_hole]]);
    // Just inside the hole's southern edge: 300 m from the hole, kilometres from the exterior.
    let point = Coord::new(14.5, 50.4 + degrees(300.0));
    let foot = Coord::new(14.5, 50.4);
    assert!((areas.boundary_distance_m(0, point) - distance_m(point, foot)).abs() < 1e-6);
    assert!(areas.exterior_distance_m(0, point) > 30_000.0);
    assert!((areas.distance_m(0, point) - distance_m(point, foot)).abs() < 1e-6);
    // Inside the polygon proper the area distance is 0, the boundary distance is not.
    let inside = Coord::new(14.5, 50.0 + degrees(800.0));
    assert_eq!(areas.distance_m(0, inside), 0.0);
    let boundary = areas.boundary_distance_m(0, inside);
    assert!((boundary - 800.0).abs() < 0.01, "{boundary}");
    // Off a corner, the nearest boundary point is the corner.
    let outside = Coord::new(13.99, 49.99);
    let corner = distance_m(outside, Coord::new(14.0, 50.0));
    assert!((areas.boundary_distance_m(0, outside) - corner).abs() < 1e-6);
}

#[test]
fn areas_within_a_distance_touching_near_and_far() {
    let gap = |metres: f64| 50.1 + degrees(metres);
    let areas = Areas::new(&[
        vec![square(14.0, 50.0, 14.1, 50.1)],
        // Shares the eastern edge of area 0.
        vec![square(14.1, 50.0, 14.2, 50.1)],
        // 60 m north of area 0.
        vec![square(14.0, gap(60.0), 14.1, 50.2)],
        // 150 m north of area 0.
        vec![square(14.02, gap(150.0), 14.08, 50.2)],
        // Inside area 0 entirely.
        vec![square(14.04, 50.04, 14.06, 50.06)],
    ]);
    assert_eq!(areas.within_m(0, 100.0), [1, 2, 4]);
    assert_eq!(areas.within_m(0, 200.0), [1, 2, 3, 4]);
    assert_eq!(areas.area_distance_m(0, 1), 0.0);
    assert_eq!(areas.area_distance_m(0, 4), 0.0);
    let near = areas.area_distance_m(0, 2);
    assert!((near - 60.0).abs() < 0.01, "{near}");
}
