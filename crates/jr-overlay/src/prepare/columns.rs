//! Reading a named column of a typed GTFS row: the standard columns from their fields, any
//! other from the extras the profile asked the reader for (`SourceProfile::reader`).

use jr_formats::gtfs::{Stop, Trip};

use crate::profile::SourceProfile;

/// Resolves column names of `trips.txt` rows.
#[derive(Debug)]
pub struct TripColumns {
    extras: Vec<Box<str>>,
}

impl TripColumns {
    pub fn new(profile: &SourceProfile) -> Self {
        TripColumns {
            extras: profile.extra_trip_columns(),
        }
    }

    /// The cell, `None` when empty or absent.
    pub fn get<'t>(&self, trip: &'t Trip, column: &str) -> Option<&'t str> {
        let text = match column {
            "route_id" => Some(&*trip.route_id),
            "service_id" => Some(&*trip.service_id),
            "trip_id" => Some(&*trip.trip_id),
            "trip_headsign" => trip.trip_headsign.as_deref(),
            "trip_short_name" => trip.trip_short_name.as_deref(),
            "block_id" => trip.block_id.as_deref(),
            "shape_id" => trip.shape_id.as_deref(),
            _ => {
                let position = self.extras.iter().position(|c| &**c == column)?;
                trip.extra.get(position)?.as_deref()
            }
        };
        text.filter(|text| !text.is_empty())
    }
}

/// Resolves column names of `stops.txt` rows.
#[derive(Debug)]
pub struct StopColumns {
    extras: Vec<Box<str>>,
}

impl StopColumns {
    pub fn new(profile: &SourceProfile) -> Self {
        StopColumns {
            extras: profile.extra_stop_columns(),
        }
    }

    pub fn get<'t>(&self, stop: &'t Stop, column: &str) -> Option<&'t str> {
        let text = match column {
            "stop_id" => Some(&*stop.stop_id),
            "stop_code" => stop.stop_code.as_deref(),
            "stop_name" => stop.stop_name.as_deref(),
            "zone_id" => stop.zone_id.as_deref(),
            "parent_station" => stop.parent_station.as_deref(),
            "level_id" => stop.level_id.as_deref(),
            "platform_code" => stop.platform_code.as_deref(),
            _ => {
                let position = self.extras.iter().position(|c| &**c == column)?;
                stop.extra.get(position)?.as_deref()
            }
        };
        text.filter(|text| !text.trim().is_empty())
    }
}
