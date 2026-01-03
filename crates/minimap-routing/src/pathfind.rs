//! A* pathfinding algorithm

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::graph::{haversine_distance, RoadGraph};
use crate::route::Route;

/// A* pathfinding from start node to end node
pub fn astar(
    graph: &RoadGraph,
    start_idx: usize,
    end_idx: usize,
    max_nodes: usize,
) -> Option<Route> {
    if start_idx == end_idx {
        // Already at destination
        return Some(Route::empty());
    }

    let end_node = graph.node(end_idx)?;
    let end_lat = end_node.lat;
    let end_lon = end_node.lon;

    // Priority queue: (f_score, node_idx)
    // Using BTreeMap as a simple priority queue (sorted by f_score)
    let mut open_set: BTreeMap<OrderedFloat, usize> = BTreeMap::new();
    let mut open_set_lookup: BTreeMap<usize, OrderedFloat> = BTreeMap::new();

    // g_score: cost from start to node
    let mut g_score: BTreeMap<usize, f32> = BTreeMap::new();

    // came_from: (node_idx, edge_idx) - which edge we used to reach this node
    let mut came_from: BTreeMap<usize, (usize, usize)> = BTreeMap::new();

    // Initialize with start node
    let start_node = graph.node(start_idx)?;
    let h_start = haversine_distance(start_node.lat, start_node.lon, end_lat, end_lon);
    let f_start = OrderedFloat(h_start);

    open_set.insert(f_start, start_idx);
    open_set_lookup.insert(start_idx, f_start);
    g_score.insert(start_idx, 0.0);

    let mut nodes_explored = 0;

    while !open_set.is_empty() && nodes_explored < max_nodes {
        // Get node with lowest f_score
        let (&f, &current) = open_set.iter().next()?;
        open_set.remove(&f);
        open_set_lookup.remove(&current);
        nodes_explored += 1;

        // Check if we reached the goal
        if current == end_idx {
            return Some(reconstruct_path(graph, &came_from, current));
        }

        let current_g = *g_score.get(&current).unwrap_or(&f32::MAX);

        // Explore neighbors
        for &edge_idx in graph.outgoing_edges(current) {
            // Check if we can traverse this edge from current node
            if !graph.can_traverse(edge_idx, current) {
                continue;
            }

            let neighbor = match graph.edge_destination(edge_idx, current) {
                Some(n) => n,
                None => continue,
            };

            // Calculate tentative g_score
            let edge_cost = graph.edge_cost(edge_idx);
            let tentative_g = current_g + edge_cost;

            let neighbor_g = *g_score.get(&neighbor).unwrap_or(&f32::MAX);
            if tentative_g < neighbor_g {
                // This path is better
                came_from.insert(neighbor, (current, edge_idx));
                g_score.insert(neighbor, tentative_g);

                // Calculate heuristic (distance to goal)
                let neighbor_node = match graph.node(neighbor) {
                    Some(n) => n,
                    None => continue,
                };
                let h = haversine_distance(neighbor_node.lat, neighbor_node.lon, end_lat, end_lon);
                let f_new = OrderedFloat(tentative_g + h);

                // Update open set
                if let Some(&old_f) = open_set_lookup.get(&neighbor) {
                    open_set.remove(&old_f);
                }
                open_set.insert(f_new, neighbor);
                open_set_lookup.insert(neighbor, f_new);
            }
        }
    }

    // No path found
    log::debug!(
        "astar: no path found after exploring {} nodes",
        nodes_explored
    );
    None
}

/// Reconstruct the path from came_from map
fn reconstruct_path(
    graph: &RoadGraph,
    came_from: &BTreeMap<usize, (usize, usize)>,
    end_idx: usize,
) -> Route {
    use crate::route::RouteSegment;
    use crate::turn::TurnType;

    let mut segments = Vec::new();
    let mut waypoints = Vec::new();
    let mut total_distance = 0.0f32;

    // Walk back from end to start
    let mut current = end_idx;
    while let Some(&(prev, edge_idx)) = came_from.get(&current) {
        if let Some(edge) = graph.edge(edge_idx) {
            // Add geometry points (in reverse order, we'll reverse at the end)
            let mut geom = edge.geometry.clone();
            if edge.to_idx != current {
                // We traversed in reverse
                geom.reverse();
            }
            for point in &geom {
                waypoints.push(*point);
            }

            // Create route segment
            let segment = RouteSegment {
                way_id: edge.way_id,
                distance_m: edge.distance_m,
                lane_guidance: if edge.turn_lanes.is_some() || edge.dest_lanes.is_some() {
                    Some(crate::route::LaneGuidance {
                        total_lanes: edge.lane_count,
                        lanes: edge.turn_lanes.as_ref().map(|turns| {
                            turns.iter().map(|&turn| crate::route::LaneInfo {
                                turn,
                                is_recommended: false, // Will be computed later
                            }).collect()
                        }).unwrap_or_default(),
                        recommended_lane: 0,
                        destination: edge.dest_lanes.as_ref()
                            .and_then(|d| d.first().cloned()),
                    })
                } else {
                    None
                },
                geometry: geom,
                turn_type: TurnType::Straight, // Will be computed later
            };

            total_distance += edge.distance_m;
            segments.push(segment);
        }
        current = prev;
    }

    // Reverse to get start-to-end order
    segments.reverse();
    waypoints.reverse();

    // Remove duplicate waypoints
    waypoints.dedup();

    // Compute turn types between segments
    for i in 1..segments.len() {
        let prev_heading = segment_end_heading(&segments[i - 1]);
        let curr_heading = segment_start_heading(&segments[i]);
        segments[i].turn_type = compute_turn_type(prev_heading, curr_heading);
    }

    // Mark first and last turn types
    if !segments.is_empty() {
        segments[0].turn_type = TurnType::Start;
        let last = segments.len() - 1;
        segments[last].turn_type = TurnType::Destination;
    }

    // Compute recommended lanes based on turn type
    for segment in &mut segments {
        if let Some(ref mut guidance) = segment.lane_guidance {
            compute_recommended_lanes(guidance, segment.turn_type);
        }
    }

    Route {
        waypoints,
        segments,
        total_distance_m: total_distance,
    }
}

/// Get the heading at the end of a segment (degrees, 0=north)
fn segment_end_heading(segment: &crate::route::RouteSegment) -> f32 {
    if segment.geometry.len() >= 2 {
        let p1 = segment.geometry[segment.geometry.len() - 2];
        let p2 = segment.geometry[segment.geometry.len() - 1];
        bearing(p1.0, p1.1, p2.0, p2.1)
    } else {
        0.0
    }
}

/// Get the heading at the start of a segment (degrees, 0=north)
fn segment_start_heading(segment: &crate::route::RouteSegment) -> f32 {
    if segment.geometry.len() >= 2 {
        let p1 = segment.geometry[0];
        let p2 = segment.geometry[1];
        bearing(p1.0, p1.1, p2.0, p2.1)
    } else {
        0.0
    }
}

/// Calculate bearing from point 1 to point 2 (degrees, 0=north)
fn bearing(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f32 {
    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();
    let delta_lon = (lon2 - lon1).to_radians();

    let x = delta_lon.sin() * lat2_rad.cos();
    let y = lat1_rad.cos() * lat2_rad.sin() - lat1_rad.sin() * lat2_rad.cos() * delta_lon.cos();

    let bearing_rad = x.atan2(y);
    let bearing_deg = bearing_rad.to_degrees();

    // Normalize to 0-360
    ((bearing_deg + 360.0) % 360.0) as f32
}

/// Compute turn type from heading change
fn compute_turn_type(prev_heading: f32, curr_heading: f32) -> crate::turn::TurnType {
    use crate::turn::TurnType;

    // Calculate angle difference
    let mut diff = curr_heading - prev_heading;
    if diff > 180.0 {
        diff -= 360.0;
    }
    if diff < -180.0 {
        diff += 360.0;
    }

    // Classify turn based on angle
    match diff {
        d if d > 150.0 || d < -150.0 => TurnType::UTurn,
        d if d > 60.0 => TurnType::SharpRight,
        d if d > 30.0 => TurnType::Right,
        d if d > 10.0 => TurnType::SlightRight,
        d if d < -60.0 => TurnType::SharpLeft,
        d if d < -30.0 => TurnType::Left,
        d if d < -10.0 => TurnType::SlightLeft,
        _ => TurnType::Straight,
    }
}

/// Compute which lanes are recommended based on turn type
fn compute_recommended_lanes(guidance: &mut crate::route::LaneGuidance, turn: crate::turn::TurnType) {
    use minimap_tiles::LaneTurn;
    use crate::turn::TurnType;

    let target_lane_turns: &[LaneTurn] = match turn {
        TurnType::Left | TurnType::SharpLeft => &[LaneTurn::Left, LaneTurn::SlightLeft],
        TurnType::SlightLeft => &[LaneTurn::SlightLeft, LaneTurn::Left, LaneTurn::Through],
        TurnType::Right | TurnType::SharpRight => &[LaneTurn::Right, LaneTurn::SlightRight],
        TurnType::SlightRight => &[LaneTurn::SlightRight, LaneTurn::Right, LaneTurn::Through],
        TurnType::UTurn => &[LaneTurn::UTurn, LaneTurn::Left],
        _ => &[LaneTurn::Through, LaneTurn::None],
    };

    let mut found_recommended = false;
    for (i, lane) in guidance.lanes.iter_mut().enumerate() {
        lane.is_recommended = target_lane_turns.contains(&lane.turn);
        if lane.is_recommended && !found_recommended {
            guidance.recommended_lane = i as u8;
            found_recommended = true;
        }
    }
}

/// Wrapper for f32 that implements Ord (for use in BTreeMap)
#[derive(Debug, Clone, Copy, PartialEq)]
struct OrderedFloat(f32);

impl Eq for OrderedFloat {}

impl PartialOrd for OrderedFloat {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedFloat {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        // NaN is treated as greater than all other values
        self.0.partial_cmp(&other.0).unwrap_or(core::cmp::Ordering::Equal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bearing_north() {
        // Due north
        let b = bearing(47.0, 8.0, 48.0, 8.0);
        assert!(b < 1.0 || b > 359.0);
    }

    #[test]
    fn test_bearing_east() {
        // Due east
        let b = bearing(47.0, 8.0, 47.0, 9.0);
        assert!(b > 85.0 && b < 95.0);
    }

    #[test]
    fn test_compute_turn_straight() {
        use crate::turn::TurnType;
        let turn = compute_turn_type(0.0, 5.0);
        assert_eq!(turn, TurnType::Straight);
    }

    #[test]
    fn test_compute_turn_right() {
        use crate::turn::TurnType;
        let turn = compute_turn_type(0.0, 90.0);
        assert_eq!(turn, TurnType::SharpRight);
    }
}
