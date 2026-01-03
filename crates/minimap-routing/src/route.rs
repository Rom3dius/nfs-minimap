//! Route representation and utilities

use alloc::string::String;
use alloc::vec::Vec;
use minimap_tiles::LaneTurn;

use crate::graph::haversine_distance;
use crate::turn::TurnType;

/// A calculated route
#[derive(Debug, Clone)]
pub struct Route {
    /// Waypoints along the route (lat, lon)
    pub waypoints: Vec<(f64, f64)>,
    /// Segments making up the route
    pub segments: Vec<RouteSegment>,
    /// Total route distance in meters
    pub total_distance_m: f32,
}

/// A segment of a route (typically one road/way)
#[derive(Debug, Clone)]
pub struct RouteSegment {
    /// OSM way ID
    pub way_id: u64,
    /// Distance of this segment in meters
    pub distance_m: f32,
    /// Lane guidance at the end of this segment (if available)
    pub lane_guidance: Option<LaneGuidance>,
    /// Geometry points (lat, lon)
    pub geometry: Vec<(f64, f64)>,
    /// Turn type at the start of this segment
    pub turn_type: TurnType,
}

/// Lane guidance information for Waze-style junction display
#[derive(Debug, Clone)]
pub struct LaneGuidance {
    /// Total number of lanes
    pub total_lanes: u8,
    /// Information for each lane
    pub lanes: Vec<LaneInfo>,
    /// Recommended lane index (0-indexed)
    pub recommended_lane: u8,
    /// Destination sign text (if available)
    pub destination: Option<String>,
}

/// Information about a single lane
#[derive(Debug, Clone)]
pub struct LaneInfo {
    /// Turn direction for this lane
    pub turn: LaneTurn,
    /// Whether this lane is recommended for the route
    pub is_recommended: bool,
}

impl Route {
    /// Create an empty route (used when already at destination)
    pub fn empty() -> Self {
        Self {
            waypoints: Vec::new(),
            segments: Vec::new(),
            total_distance_m: 0.0,
        }
    }

    /// Calculate distance from a position to the nearest point on the route
    pub fn distance_to_route(&self, lat: f64, lon: f64) -> f32 {
        if self.waypoints.is_empty() {
            return f32::MAX;
        }

        let mut min_dist = f32::MAX;

        // Check distance to each segment
        for window in self.waypoints.windows(2) {
            let dist = distance_to_segment(lat, lon, window[0].0, window[0].1, window[1].0, window[1].1);
            if dist < min_dist {
                min_dist = dist;
            }
        }

        min_dist
    }

    /// Get the current segment index based on position
    pub fn current_segment_index(&self, lat: f64, lon: f64) -> usize {
        if self.segments.is_empty() {
            return 0;
        }

        let mut best_idx = 0;
        let mut best_dist = f32::MAX;

        for (i, segment) in self.segments.iter().enumerate() {
            for window in segment.geometry.windows(2) {
                let dist = distance_to_segment(lat, lon, window[0].0, window[0].1, window[1].0, window[1].1);
                if dist < best_dist {
                    best_dist = dist;
                    best_idx = i;
                }
            }
        }

        best_idx
    }

    /// Get distance along route from current position to end of current segment
    pub fn distance_to_segment_end(&self, lat: f64, lon: f64, segment_idx: usize) -> f32 {
        if segment_idx >= self.segments.len() {
            return 0.0;
        }

        let segment = &self.segments[segment_idx];
        if segment.geometry.len() < 2 {
            return segment.distance_m;
        }

        // Find nearest point on segment
        let mut min_dist = f32::MAX;
        let mut nearest_point_idx = 0;

        for (i, window) in segment.geometry.windows(2).enumerate() {
            let dist = distance_to_segment(lat, lon, window[0].0, window[0].1, window[1].0, window[1].1);
            if dist < min_dist {
                min_dist = dist;
                nearest_point_idx = i;
            }
        }

        // Calculate remaining distance on segment
        let mut remaining = 0.0;
        for i in nearest_point_idx..segment.geometry.len() - 1 {
            remaining += haversine_distance(
                segment.geometry[i].0,
                segment.geometry[i].1,
                segment.geometry[i + 1].0,
                segment.geometry[i + 1].1,
            );
        }

        remaining
    }

    /// Get lane guidance for the next junction
    pub fn get_lane_guidance(&self, lat: f64, lon: f64) -> Option<(LaneGuidance, f32)> {
        let current_idx = self.current_segment_index(lat, lon);

        // Look for lane guidance in current or next few segments
        for i in current_idx..self.segments.len().min(current_idx + 3) {
            if let Some(guidance) = &self.segments[i].lane_guidance {
                // Calculate distance to this segment
                let distance = if i == current_idx {
                    self.distance_to_segment_end(lat, lon, i)
                } else {
                    let mut dist = self.distance_to_segment_end(lat, lon, current_idx);
                    for j in current_idx + 1..=i {
                        if j < i {
                            dist += self.segments[j].distance_m;
                        }
                    }
                    dist
                };

                return Some((guidance.clone(), distance));
            }
        }

        None
    }

    /// Get the next maneuver (turn type and distance)
    pub fn get_next_maneuver(&self, lat: f64, lon: f64) -> Option<(TurnType, f32)> {
        let current_idx = self.current_segment_index(lat, lon);

        // Look for non-straight turn in upcoming segments
        for i in current_idx + 1..self.segments.len() {
            let segment = &self.segments[i];
            if segment.turn_type != TurnType::Straight && segment.turn_type != TurnType::Start {
                // Calculate distance to this turn
                let mut distance = self.distance_to_segment_end(lat, lon, current_idx);
                for j in current_idx + 1..i {
                    distance += self.segments[j].distance_m;
                }
                return Some((segment.turn_type, distance));
            }
        }

        // If no turns, return distance to destination
        if !self.segments.is_empty() {
            let remaining = self.remaining_distance(lat, lon);
            return Some((TurnType::Destination, remaining));
        }

        None
    }

    /// Get remaining distance to destination
    pub fn remaining_distance(&self, lat: f64, lon: f64) -> f32 {
        let current_idx = self.current_segment_index(lat, lon);

        let mut remaining = self.distance_to_segment_end(lat, lon, current_idx);
        for i in current_idx + 1..self.segments.len() {
            remaining += self.segments[i].distance_m;
        }

        remaining
    }
}

/// Calculate distance from a point to a line segment
fn distance_to_segment(
    px: f64, py: f64,
    ax: f64, ay: f64,
    bx: f64, by: f64,
) -> f32 {
    // Project point onto line defined by segment
    let dx = bx - ax;
    let dy = by - ay;
    let len_sq = dx * dx + dy * dy;

    if len_sq < 1e-10 {
        // Segment is essentially a point
        return haversine_distance(px, py, ax, ay);
    }

    // Parameter t for projection point on line
    let t = ((px - ax) * dx + (py - ay) * dy) / len_sq;

    // Clamp to segment
    let t = t.max(0.0).min(1.0);

    // Projected point
    let proj_x = ax + t * dx;
    let proj_y = ay + t * dy;

    haversine_distance(px, py, proj_x, proj_y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_route() {
        let route = Route::empty();
        assert_eq!(route.total_distance_m, 0.0);
        assert!(route.waypoints.is_empty());
    }

    #[test]
    fn test_distance_to_route() {
        let route = Route {
            waypoints: vec![(47.0, 8.0), (47.1, 8.0)],
            segments: Vec::new(),
            total_distance_m: 11_000.0,
        };

        // Point on the route should have ~0 distance
        let dist = route.distance_to_route(47.05, 8.0);
        assert!(dist < 100.0); // Should be very close

        // Point far from route
        let dist = route.distance_to_route(47.05, 9.0);
        assert!(dist > 50_000.0); // Should be far
    }
}
