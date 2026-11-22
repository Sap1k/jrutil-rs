//! The five relations of the pack as typed, column-wise tables (`docs/rules/posts.md` 2.3).
//!
//! One vector per column, all of a table's vectors of equal length. Foreign keys are row
//! indexes, not the stored text: an observation names its route point, a variant its context,
//! an attachment its variant and route point. The text keys the files repeat on every row
//! (`gtfs_stop_place_id` of variants and attachments, the attachment's `context_id`,
//! `variant_rank` and `corridor_id`, a route point's `observation_ids`, a context's
//! `assignment_kind`) are derived when a pack is written and checked when one is read; they are
//! not stored twice. Stops are numbers (verdict Q9).

/// `same_stop_block_role` (POST-R042).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BlockRole {
    /// The only call of its block.
    Through,
    /// The first call of a block of repeated calls at the stop.
    Incoming,
    /// The last call of such a block.
    Outgoing,
    /// A call inside such a block.
    Interior,
}

impl BlockRole {
    pub const ALL: [BlockRole; 4] = [
        BlockRole::Through,
        BlockRole::Incoming,
        BlockRole::Outgoing,
        BlockRole::Interior,
    ];

    /// The stored text, which is also the role's sort key (contexts are ordered by it).
    pub fn as_str(self) -> &'static str {
        match self {
            BlockRole::Through => "through",
            BlockRole::Incoming => "incoming",
            BlockRole::Outgoing => "outgoing",
            BlockRole::Interior => "interior",
        }
    }

    pub fn parse(text: &str) -> Option<BlockRole> {
        BlockRole::ALL
            .into_iter()
            .find(|role| role.as_str() == text)
    }
}

/// `routing_availability` of a variant or an attachment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Availability {
    Available,
    Unavailable,
}

impl Availability {
    pub fn as_str(self) -> &'static str {
        match self {
            Availability::Available => "available",
            Availability::Unavailable => "unavailable",
        }
    }

    pub fn parse(text: &str) -> Option<Availability> {
        match text {
            "available" => Some(Availability::Available),
            "unavailable" => Some(Availability::Unavailable),
            _ => None,
        }
    }
}

/// `observations.parquet`: one row per admitted observation, ordered by (stop, observation id).
/// The stop is the route point's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Observations {
    /// Row of the observation's route point in [`RoutePoints`] (`route_point_id`).
    pub route_point: Vec<u32>,
    pub observation_id: Vec<Box<str>>,
    pub source_kind: Vec<Box<str>>,
    pub source_object_id: Vec<Option<Box<str>>>,
    pub observed_at: Vec<Option<Box<str>>>,
    pub latitude: Vec<f64>,
    pub longitude: Vec<f64>,
    pub support_weight: Vec<f64>,
    /// `key=value` pairs joined `;`, verbatim.
    pub raw_tags: Vec<Box<str>>,
    pub explicit_modes: Vec<Box<str>>,
    pub denied_modes: Vec<Box<str>>,
    pub lifecycle: Vec<Box<str>>,
}

impl Observations {
    pub fn len(&self) -> usize {
        self.observation_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// `route_points.parquet`: one row per (stop, distinct coordinate), ordered by (stop, route
/// point id). The members (`observation_ids`) are the observations naming the row.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoutePoints {
    pub stop: Vec<u64>,
    pub route_point_id: Vec<Box<str>>,
    /// One of the members; the F# capture writes the ordinal-smallest.
    pub representative_observation_id: Vec<Box<str>>,
    pub latitude: Vec<f64>,
    pub longitude: Vec<f64>,
    pub has_current_lifecycle: Vec<bool>,
    pub has_obsolete_lifecycle: Vec<bool>,
    pub source_support_weight: Vec<f64>,
    /// Upper-cased mode tokens joined `;`, verbatim.
    pub explicit_modes: Vec<Box<str>>,
    pub denied_modes: Vec<Box<str>>,
}

impl RoutePoints {
    pub fn len(&self) -> usize {
        self.route_point_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// `contexts.parquet`: one row per logical context, in the capture's work-key order (stop,
/// mode, line, distinction, direction, pattern, position, role text, authored key or "",
/// context id). `assignment_kind` is `authored` exactly when `authored_post_key` is set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Contexts {
    pub context_id: Vec<Box<str>>,
    pub stop: Vec<u64>,
    /// `A` bus, `E` tram, `T` trolleybus, as stored.
    pub mode: Vec<Box<str>>,
    pub line_id: Vec<Box<str>>,
    pub route_distinction: Vec<i32>,
    pub direction: Vec<i32>,
    pub pattern_hash: Vec<Box<str>>,
    pub pattern_position: Vec<i32>,
    pub role: Vec<BlockRole>,
    pub movement_family_id: Vec<Box<str>>,
    pub previous_stop: Vec<Option<u64>>,
    pub next_stop: Vec<Option<u64>>,
    pub authored_post_key: Vec<Option<Box<str>>>,
    pub same_stop_block_id: Vec<Option<Box<str>>>,
}

impl Contexts {
    pub fn len(&self) -> usize {
        self.context_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `assignment_kind` as stored: `authored` or `unlabelled` (POST-R043).
    pub fn assignment_kind(&self, row: usize) -> &'static str {
        if self.authored_post_key[row].is_some() {
            "authored"
        } else {
            "unlabelled"
        }
    }
}

/// `corridor_variants.parquet`: per context, ranks contiguous from 0 (at most three), in
/// context order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CorridorVariants {
    /// Row of the variant's context in [`Contexts`].
    pub context: Vec<u32>,
    pub variant_rank: Vec<i32>,
    pub corridor_id: Vec<Option<Box<str>>>,
    pub absolute_cost_metres: Vec<Option<f64>>,
    pub relative_cost_metres: Vec<Option<f64>>,
    pub relative_cost_fraction: Vec<Option<f64>>,
    pub path_length_metres: Vec<Option<f64>>,
    pub directed_edge_count: Vec<i32>,
    pub repeated_directed_edge_count: Vec<i32>,
    pub service_edge_count: Vec<i32>,
    pub restricted_access_edge_count: Vec<i32>,
    pub access_penalty_metres: Vec<Option<f64>>,
    pub ingress_thread_id: Vec<Option<Box<str>>>,
    pub egress_thread_id: Vec<Option<Box<str>>>,
    pub routing_availability: Vec<Availability>,
    pub invariant_failure_reason: Vec<Option<Box<str>>>,
}

impl CorridorVariants {
    pub fn len(&self) -> usize {
        self.context.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// `route_point_evidence.parquet`: for every variant, one row per route point of the context's
/// stop in the stop's route-point order, in variant order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoutePointEvidence {
    /// Row of the attachment's variant in [`CorridorVariants`].
    pub variant: Vec<u32>,
    /// Row of the attached route point in [`RoutePoints`].
    pub route_point: Vec<u32>,
    pub routed_excess_metres: Vec<Option<f64>>,
    pub snap_edge_id: Vec<Option<i32>>,
    pub snap_way_id: Vec<Option<i64>>,
    pub snap_fraction: Vec<Option<f64>>,
    pub snap_projected_longitude: Vec<Option<f64>>,
    pub snap_projected_latitude: Vec<Option<f64>>,
    pub snap_distance_metres: Vec<Option<f64>>,
    pub corridor_face_id: Vec<Option<Box<str>>>,
    pub corridor_distance_metres: Vec<Option<f64>>,
    pub signed_lateral_offset_metres: Vec<Option<f64>>,
    pub corridor_heading_degrees: Vec<Option<f64>>,
    pub attachment_heading_degrees: Vec<Option<f64>>,
    pub heading_difference_degrees: Vec<Option<f64>>,
    pub proximity_distance_metres: Vec<Option<f64>>,
    pub routing_availability: Vec<Availability>,
    pub invariant_failure_reason: Vec<Option<Box<str>>>,
}

impl RoutePointEvidence {
    pub fn len(&self) -> usize {
        self.variant.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The eleven nullable numeric facts in the F# store's validation order: routed excess,
    /// snap fraction, snap longitude, snap latitude, snap distance, corridor distance, signed
    /// offset, corridor heading, attachment heading, heading difference, proximity distance.
    pub fn numeric(&self, row: usize) -> [Option<f64>; 11] {
        [
            self.routed_excess_metres[row],
            self.snap_fraction[row],
            self.snap_projected_longitude[row],
            self.snap_projected_latitude[row],
            self.snap_distance_metres[row],
            self.corridor_distance_metres[row],
            self.signed_lateral_offset_metres[row],
            self.corridor_heading_degrees[row],
            self.attachment_heading_degrees[row],
            self.heading_difference_degrees[row],
            self.proximity_distance_metres[row],
        ]
    }
}
