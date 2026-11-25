//! The shipped profiles and the strictness of profile validation (OVL-R005 as ported, B18).

mod support;

use jr_overlay::profile::{
    BaseProfile, Capability, ProfileError, RouteTier, SourceProfile, TripTier,
};

#[test]
fn ovl_shipped_profiles_parse_with_their_effective_values() {
    let pid = SourceProfile::builtin("pid").unwrap().unwrap();
    let jmk = SourceProfile::builtin("ids-jmk").unwrap().unwrap();
    assert!(SourceProfile::builtin("unknown").is_none());
    assert_eq!(&*pid.source_id, "pid-gtfs");
    assert_eq!(&*jmk.source_id, "ids-jmk-gtfs");
    // B2, verdict Q2: each source has its own settings. PID states no coordinate-identity
    // tier and no stop zones; IDS JMK states its 25 m / 10 m tier and no companion route tier.
    assert!(pid.stops.coordinate_identity.is_none());
    assert!(!pid.capabilities.contains_key(&Capability::StopZones));
    let identity = jmk.stops.coordinate_identity.unwrap();
    assert_eq!(
        (identity.maximum_metres, identity.minimum_margin_metres),
        (25.0, 10.0)
    );
    assert_eq!(jmk.routes.tiers, vec![RouteTier::StructuralTripEvidence]);
    assert_eq!(pid.trips.maximum_first_departure_delta_seconds, 420);
    assert_eq!(jmk.trips.maximum_first_departure_delta_seconds, 420);
    assert!(pid.grants(Capability::Schedules, TripTier::PatternNearest));
    assert!(!pid.grants(Capability::Schedules, TripTier::PatternEditNearest));
    assert!(jmk.hooks.operational_crosswalk.is_some() && jmk.hooks.zones.is_some());
    assert!(pid.hooks.revision.is_some() && pid.hooks.cis_line_claim.is_some());
    let request = pid.reader();
    assert_eq!(request.tables, vec![Box::from("route_sub_agencies.txt")]);
    assert!(
        request
            .extra_columns
            .iter()
            .any(|(file, columns)| &**file == "stops.txt"
                && columns.iter().any(|c| &**c == "asw_node_id"))
    );
    assert_eq!(jmk.reader().raw, vec![Box::from("api.txt")]);
    let base = BaseProfile::road().unwrap();
    assert!(base.dated_by_departure);
}

fn pid_with(from: &str, to: &str) -> Result<SourceProfile, ProfileError> {
    let text = include_str!("../../../policy/overlay/pid.toml");
    assert!(text.contains(from), "{from}");
    SourceProfile::parse(&text.replacen(from, to, 1))
}

#[test]
fn ovl_profile_refuses_unknown_keys_tiers_and_capabilities() {
    assert!(matches!(
        pid_with("maximum_snapshot_skew_days", "maximum_snapshot_skew"),
        Err(ProfileError::Toml(_))
    ));
    assert!(matches!(
        pid_with("[hooks.revision]", "[hooks.revision]\ncolumnn = \"x\""),
        Err(ProfileError::Toml(_))
    ));
    assert!(matches!(
        pid_with("\"pattern_first\",", "\"pattern_firstt\","),
        Err(ProfileError::Toml(_))
    ));
    assert!(matches!(
        pid_with("route_long_name = {}", "route_long_nam = {}"),
        Err(ProfileError::Toml(_))
    ));
    // B18: a minimum tier the profile does not use.
    assert!(matches!(
        pid_with("\"pattern_edit_nearest\"]", "]"),
        Err(ProfileError::Invalid { .. })
    ));
    // A revision pattern without its group.
    assert!(matches!(
        pid_with("(?<revision>\\d{6})", "(\\d{6})"),
        Err(ProfileError::Invalid { .. })
    ));
    assert!(matches!(
        pid_with(
            "source_native_modes = [\"bus\",",
            "source_native_modes = [\"rail\", \"bus\","
        ),
        Err(ProfileError::Invalid { .. })
    ));
}

#[test]
fn ovl_r136_a_profile_lists_its_tiers_strongest_first() {
    assert!(matches!(
        pid_with(
            "tiers = [\"full_signature\", \"pattern_endpoints\"",
            "tiers = [\"pattern_endpoints\", \"full_signature\""
        ),
        Err(ProfileError::Invalid { .. })
    ));
    assert!(matches!(
        pid_with(
            "tiers = [\"companion_assertion\", \"structural_trip_evidence\"]",
            "tiers = [\"structural_trip_evidence\", \"companion_assertion\"]"
        ),
        Err(ProfileError::Invalid { .. })
    ));
    assert!(matches!(
        pid_with(
            "\"pattern_first\", \"pattern_nearest\"",
            "\"pattern_first\", \"pattern_first\", \"pattern_nearest\""
        ),
        Err(ProfileError::Invalid { .. })
    ));
}
