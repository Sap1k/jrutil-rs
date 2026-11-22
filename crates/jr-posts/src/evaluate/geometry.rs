//! The scoring primitives both scorers share (POST-R081–R083; F# `JdfPostInference`): the
//! local metric, geometry components and score, routed fit, the hard gates, mode
//! compatibility and the supporting-score combination. Arithmetic is written in F#'s operation
//! order (degrees to radians as `x × π / 180`, sums left to right) so results are bit-equal.
//!
//! The local metric is not `jr_model::geo::distance_m`: the evaluator must decide exactly as
//! the F# evaluator did on F#-captured packs, and the shipped model was trained with this
//! metric. Its constants are in `policy/posts.toml` (`[metric]`).

use std::f64::consts::PI;

use super::policy::{Constants, Settings};

/// `equirect(a, b)` and its components `dx`, `dy` (a − b, metres).
pub(crate) fn equirect(
    constants: &Constants,
    latitude_a: f64,
    longitude_a: f64,
    latitude_b: f64,
    longitude_b: f64,
) -> (f64, f64, f64) {
    let metric = &constants.metric;
    let latitude = (latitude_a + latitude_b) * 0.5 * PI / 180.0;
    let dx = (longitude_a - longitude_b) * metric.metres_per_degree_longitude * latitude.cos();
    let dy = (latitude_a - latitude_b) * metric.metres_per_degree_latitude;
    ((dx * dx + dy * dy).sqrt(), dx, dy)
}

/// The isolation adjustment's distance (POST-R085, B18: latitude measured with the longitude
/// factor).
pub(crate) fn isolation_distance(
    constants: &Constants,
    latitude_a: f64,
    longitude_a: f64,
    latitude_b: f64,
    longitude_b: f64,
) -> f64 {
    let metric = &constants.metric;
    let dlat = (latitude_a - latitude_b) * metric.isolation_metres_per_degree_latitude;
    let dlon = (longitude_a - longitude_b)
        * metric.metres_per_degree_longitude
        * ((latitude_a + latitude_b) * PI / 360.0).cos();
    (dlat * dlat + dlon * dlon).sqrt()
}

/// The smaller angle between two headings, degrees.
pub(crate) fn angle_difference(left: f64, right: f64) -> f64 {
    let difference = (left - right).abs() % 360.0;
    difference.min(360.0 - difference)
}

/// POST-R082: alignment, side and proximity.
pub(crate) fn components(
    settings: &Settings,
    constants: &Constants,
    corridor_distance: Option<f64>,
    offset: Option<f64>,
    corridor_heading: Option<f64>,
    attachment_heading: Option<f64>,
) -> (f64, f64, f64) {
    let alignment = match (corridor_heading, attachment_heading) {
        (Some(corridor), Some(attachment)) => (1.0
            - angle_difference(corridor, attachment) / constants.geometry.alignment_range_deg)
            .max(0.0),
        _ => 0.0,
    };
    let side = match offset {
        Some(offset) if offset.abs() <= settings.centreline_neutral_tolerance_metres => 0.5,
        Some(offset) if offset < 0.0 => 1.0,
        _ => 0.0,
    };
    let proximity = corridor_distance
        .map(|distance| (1.0 - distance / constants.geometry.proximity_range_m).max(0.0))
        .unwrap_or(0.0);
    (alignment, side, proximity)
}

/// POST-R082: the weighted geometry score, clamped to [0, 1].
pub(crate) fn geometry_score(
    settings: &Settings,
    alignment: f64,
    side: f64,
    proximity: f64,
    routed_fit: f64,
) -> f64 {
    (settings.weight_alignment * alignment
        + settings.weight_side * side
        + settings.weight_proximity * proximity
        + settings.weight_routed_excess * routed_fit)
        .clamp(0.0, 1.0)
}

/// POST-R082: `max(0, 1 − excess / maximum routed excess)`.
pub(crate) fn routed_fit(settings: &Settings, excess: f64) -> f64 {
    (1.0 - excess / settings.maximum_routed_excess_metres).max(0.0)
}

/// Routed fit of an optional excess, 0 when missing.
pub(crate) fn routed_fit_of(settings: &Settings, excess: Option<f64>) -> f64 {
    excess
        .map(|excess| routed_fit(settings, excess))
        .unwrap_or(0.0)
}

/// POST-R082: the geometry score of a row's facts.
pub(crate) fn evidence_score(
    settings: &Settings,
    constants: &Constants,
    corridor_distance: Option<f64>,
    offset: Option<f64>,
    corridor_heading: Option<f64>,
    attachment_heading: Option<f64>,
    routed_fit: f64,
) -> f64 {
    let (alignment, side, proximity) = components(
        settings,
        constants,
        corridor_distance,
        offset,
        corridor_heading,
        attachment_heading,
    );
    geometry_score(settings, alignment, side, proximity, routed_fit)
}

/// POST-R083: `clamp(g + s(1 − g))` for a supporting `s ≥ 0`, `clamp(g + s·g)` otherwise.
pub(crate) fn supporting_score(geometry: f64, supporting: f64) -> f64 {
    let value = if supporting >= 0.0 {
        geometry + supporting * (1.0 - geometry)
    } else {
        geometry + supporting * geometry
    };
    value.clamp(0.0, 1.0)
}

/// POST-R081: the first hard-gate rejection: a topology (or lifecycle/mode) reason, else
/// `corridor-distance`, `routed-excess`, `confident-left-side`.
pub(crate) fn hard_gate(
    settings: &Settings,
    topology: Option<&str>,
    corridor_distance: Option<f64>,
    offset: Option<f64>,
    excess: Option<f64>,
) -> Option<Box<str>> {
    if let Some(reason) = topology {
        return Some(reason.into());
    }
    if corridor_distance.is_some_and(|value| value > settings.maximum_corridor_distance_metres) {
        return Some("corridor-distance".into());
    }
    if excess.is_some_and(|value| value > settings.maximum_routed_excess_metres) {
        return Some("routed-excess".into());
    }
    if offset.is_some_and(|value| value > settings.centreline_neutral_tolerance_metres) {
        return Some("confident-left-side".into());
    }
    None
}

/// POST-R081: whether an observed mode (ROAD, BUS, TRAM, TROLLEYBUS, SHARED) can serve a
/// context mode (A bus, E tram, T trolleybus).
pub(crate) fn mode_compatible(mode: &str, value: &str) -> bool {
    value == "SHARED"
        || value == mode
        || (mode == "A" && (value == "BUS" || value == "ROAD"))
        || (mode == "E" && value == "TRAM")
        || (mode == "T" && (value == "TROLLEYBUS" || value == "BUS" || value == "ROAD"))
}

/// POST-R081: known modes all exclude the context mode; no known modes never excludes.
pub(crate) fn mode_incompatible<S: AsRef<str>>(mode: &str, known: &[S]) -> bool {
    !known.is_empty()
        && !known
            .iter()
            .any(|value| mode_compatible(mode, value.as_ref()))
}

/// The median of `values` (sorted copy; mean of the middle two for an even count; 0 for none).
pub(crate) fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(|left, right| left.total_cmp(right));
    let count = values.len();
    if count == 0 {
        0.0
    } else if count % 2 == 1 {
        values[count / 2]
    } else {
        (values[count / 2 - 1] + values[count / 2]) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evaluate::policy::Constants;

    fn settings() -> Settings {
        let document =
            jr_formats::post_policy::parse(br#"{"schema_version": 2, "policy_id": "p",
            "consolidation": {"maximum_diameter_metres": 2, "distinguished_context_minimum_count": 2},
            "geometry_weights": {"alignment": 0.4, "side": 0.25, "proximity": 0.2, "routed_excess": 0.15},
            "hard_gates": {"centreline_neutral_tolerance_metres": 1.0, "maximum_corridor_distance_metres": 10.0,
                           "maximum_routed_excess_metres": 20.0},
            "consensus": {"minimum_contexts": 1}, "established_post": {"minimum_supporting_contexts": 1,
                           "minimum_runner_up_ratio": 1.0},
            "alternative_corridors": {"maximum_variants": 3}}"#)
            .unwrap();
        Settings::new(&document.policy).unwrap()
    }

    #[test]
    fn post_r081_the_first_hard_gate_that_applies_is_the_reason() {
        let s = settings();
        assert_eq!(
            hard_gate(&s, None, Some(11.0), None, None).as_deref(),
            Some("corridor-distance")
        );
        assert_eq!(
            hard_gate(&s, None, Some(1.0), None, Some(21.0)).as_deref(),
            Some("routed-excess")
        );
        assert_eq!(
            hard_gate(&s, None, Some(1.0), Some(2.0), Some(1.0)).as_deref(),
            Some("confident-left-side")
        );
        assert_eq!(
            hard_gate(&s, Some("topology"), None, None, None).as_deref(),
            Some("topology")
        );
        assert_eq!(hard_gate(&s, None, Some(1.0), Some(-2.0), Some(1.0)), None);
    }

    #[test]
    fn post_r081_known_modes_that_all_exclude_the_context_mode_reject() {
        assert!(mode_incompatible("A", &["TRAM"]));
        assert!(!mode_incompatible("E", &["TRAM"]));
        assert!(!mode_incompatible("A", &["ROAD", "TRAM"]));
        assert!(!mode_incompatible::<&str>("A", &[]));
        assert!(!mode_incompatible("T", &["BUS"]));
    }

    #[test]
    fn post_r082_geometry_components_and_the_supporting_score() {
        let s = settings();
        let c = Constants::builtin();
        assert_eq!(
            components(&s, &c, Some(25.0), Some(-0.5), Some(350.0), Some(10.0)),
            (1.0 - 20.0 / 90.0, 0.5, 0.5)
        );
        assert_eq!(
            components(&s, &c, None, Some(3.0), None, Some(10.0)),
            (0.0, 0.0, 0.0)
        );
        assert_eq!(supporting_score(0.5, 0.1), 0.55);
        assert_eq!(supporting_score(0.5, -0.1), 0.45);
    }
}
