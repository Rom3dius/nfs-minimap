//! A* routing algorithm implementation.

extern crate alloc;

use alloc::collections::BinaryHeap;
use alloc::vec::Vec;
use core::cmp::Ordering;
use std::collections::HashMap;

use crate::geo::haversine_distance_micro;
use crate::types::{HierarchyLevel, RoutingNode, RoutingTile};

/// A node reference that can span multiple tiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeRef {
    /// Tile coordinates.
    pub tile_x: i32,
    pub tile_y: i32,
    /// Node index within the tile.
    pub node_idx: u32,
    /// Hierarchy level.
    pub level: HierarchyLevel,
}

impl NodeRef {
    pub fn new(level: HierarchyLevel, tile_x: i32, tile_y: i32, node_idx: u32) -> Self {
        Self {
            tile_x,
            tile_y,
            node_idx,
            level,
        }
    }
}

/// Entry in the A* priority queue.
#[derive(Debug, Clone)]
struct AStarEntry {
    /// Node reference.
    node: NodeRef,
    /// Cost from start (g-score).
    g_cost: u32,
    /// Estimated total cost (f-score = g + h).
    f_cost: u32,
}

impl PartialEq for AStarEntry {
    fn eq(&self, other: &Self) -> bool {
        self.f_cost == other.f_cost
    }
}

impl Eq for AStarEntry {}

impl PartialOrd for AStarEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AStarEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse ordering for min-heap behavior
        other.f_cost.cmp(&self.f_cost)
    }
}

/// Result of a routing query.
#[derive(Debug, Clone)]
pub struct RoutingResult {
    /// Sequence of node references forming the path.
    pub path: Vec<NodeRef>,
    /// Total distance in meters.
    pub total_distance_m: f64,
    /// Total time in seconds.
    pub total_time_s: f64,
    /// Number of nodes explored.
    pub nodes_explored: usize,
}

impl RoutingResult {
    /// Get the path as lat/lon coordinates.
    pub fn path_coordinates<F>(&self, get_node: F) -> Vec<(f64, f64)>
    where
        F: Fn(&NodeRef) -> Option<RoutingNode>,
    {
        self.path
            .iter()
            .filter_map(|node_ref| get_node(node_ref).map(|n| n.position()))
            .collect()
    }
}

/// A* router for a single tile.
///
/// For single-tile routing, all nodes are assumed to be in the same tile.
pub struct SingleTileRouter<'a> {
    tile: &'a RoutingTile,
}

impl<'a> SingleTileRouter<'a> {
    pub fn new(tile: &'a RoutingTile) -> Self {
        Self { tile }
    }

    /// Find the shortest path between two nodes in the tile.
    pub fn find_path(&self, start_idx: u32, end_idx: u32) -> Option<RoutingResult> {
        let start_node = self.tile.node(start_idx)?;
        let end_node = self.tile.node(end_idx)?;

        // Priority queue (min-heap via reverse ordering)
        let mut open_set = BinaryHeap::new();

        // g_cost: cost from start to node
        let mut g_costs: HashMap<u32, u32> = HashMap::new();

        // came_from: for path reconstruction
        let mut came_from: HashMap<u32, u32> = HashMap::new();

        // Initialize with start node
        let h_start = self.heuristic(start_node, end_node);
        open_set.push(AStarEntry {
            node: NodeRef::new(self.tile.level, self.tile.tile_x, self.tile.tile_y, start_idx),
            g_cost: 0,
            f_cost: h_start,
        });
        g_costs.insert(start_idx, 0);

        let mut nodes_explored = 0;

        while let Some(current) = open_set.pop() {
            nodes_explored += 1;

            let current_idx = current.node.node_idx;

            // Found the goal
            if current_idx == end_idx {
                let path = self.reconstruct_path(&came_from, end_idx);
                let (distance, time) = self.calculate_path_metrics(&path);

                return Some(RoutingResult {
                    path: path
                        .into_iter()
                        .map(|idx| {
                            NodeRef::new(self.tile.level, self.tile.tile_x, self.tile.tile_y, idx)
                        })
                        .collect(),
                    total_distance_m: distance,
                    total_time_s: time,
                    nodes_explored,
                });
            }

            // Skip if we've found a better path already
            if let Some(&best_g) = g_costs.get(&current_idx) {
                if current.g_cost > best_g {
                    continue;
                }
            }

            // Explore neighbors
            let _current_node = match self.tile.node(current_idx) {
                Some(n) => n,
                None => continue,
            };

            for edge in self.tile.edges_for_node(current_idx) {
                let neighbor_idx = edge.target;

                // Skip if target is outside this tile
                if neighbor_idx >= self.tile.nodes.len() as u32 {
                    continue;
                }

                let neighbor_node = match self.tile.node(neighbor_idx) {
                    Some(n) => n,
                    None => continue,
                };

                let edge_cost = edge.weight();
                let tentative_g = current.g_cost.saturating_add(edge_cost);

                let is_better = match g_costs.get(&neighbor_idx) {
                    Some(&existing_g) => tentative_g < existing_g,
                    None => true,
                };

                if is_better {
                    came_from.insert(neighbor_idx, current_idx);
                    g_costs.insert(neighbor_idx, tentative_g);

                    let h = self.heuristic(neighbor_node, end_node);
                    let f_cost = tentative_g.saturating_add(h);

                    open_set.push(AStarEntry {
                        node: NodeRef::new(
                            self.tile.level,
                            self.tile.tile_x,
                            self.tile.tile_y,
                            neighbor_idx,
                        ),
                        g_cost: tentative_g,
                        f_cost,
                    });
                }
            }
        }

        // No path found
        None
    }

    /// Calculate heuristic (estimated cost to goal).
    ///
    /// Uses haversine distance converted to time estimate.
    fn heuristic(&self, from: &RoutingNode, to: &RoutingNode) -> u32 {
        let distance_m = haversine_distance_micro(from.lat, from.lon, to.lat, to.lon);
        // Assume max speed of 130 km/h for heuristic (optimistic)
        // Convert to centiseconds to match edge weights
        let time_s = distance_m / (130.0 * 1000.0 / 3600.0);
        (time_s * 100.0) as u32
    }

    /// Reconstruct path from came_from map.
    fn reconstruct_path(&self, came_from: &HashMap<u32, u32>, end_idx: u32) -> Vec<u32> {
        let mut path = Vec::new();
        let mut current = end_idx;

        path.push(current);
        while let Some(&prev) = came_from.get(&current) {
            path.push(prev);
            current = prev;
        }

        path.reverse();
        path
    }

    /// Calculate total distance and time for a path.
    fn calculate_path_metrics(&self, path: &[u32]) -> (f64, f64) {
        let mut total_distance = 0.0;
        let mut total_time = 0.0;

        for window in path.windows(2) {
            let from_idx = window[0];
            let to_idx = window[1];

            // Find the edge between these nodes
            for edge in self.tile.edges_for_node(from_idx) {
                if edge.target == to_idx {
                    total_distance += edge.distance_m();
                    total_time += edge.travel_time_s();
                    break;
                }
            }
        }

        (total_distance, total_time)
    }

    /// Find the nearest node to a lat/lon position.
    pub fn find_nearest_node(&self, lat: f64, lon: f64) -> Option<(u32, f64)> {
        let lat_micro = (lat * 1_000_000.0) as i32;
        let lon_micro = (lon * 1_000_000.0) as i32;

        let mut best_idx = None;
        let mut best_distance = f64::MAX;

        for (idx, node) in self.tile.nodes.iter().enumerate() {
            let distance = haversine_distance_micro(lat_micro, lon_micro, node.lat, node.lon);
            if distance < best_distance {
                best_distance = distance;
                best_idx = Some(idx as u32);
            }
        }

        best_idx.map(|idx| (idx, best_distance))
    }
}

/// Snap a GPS position to the nearest road.
///
/// Returns the node index and distance to that node.
pub fn snap_to_road(tile: &RoutingTile, lat: f64, lon: f64) -> Option<(u32, f64)> {
    SingleTileRouter::new(tile).find_nearest_node(lat, lon)
}

/// Find nearest node across multiple tiles.
pub fn snap_to_road_multi<'a, I>(tiles: I, lat: f64, lon: f64) -> Option<(NodeRef, f64)>
where
    I: IntoIterator<Item = &'a RoutingTile>,
{
    let lat_micro = (lat * 1_000_000.0) as i32;
    let lon_micro = (lon * 1_000_000.0) as i32;

    let mut best: Option<(NodeRef, f64)> = None;

    for tile in tiles {
        for (idx, node) in tile.nodes.iter().enumerate() {
            let distance = haversine_distance_micro(lat_micro, lon_micro, node.lat, node.lon);
            let is_better = match &best {
                Some((_, best_dist)) => distance < *best_dist,
                None => true,
            };

            if is_better {
                best = Some((
                    NodeRef::new(tile.level, tile.tile_x, tile.tile_y, idx as u32),
                    distance,
                ));
            }
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{RoadClass, RoutingEdge, RoutingNode, RoutingTile};

    fn create_test_tile() -> RoutingTile {
        // Create a simple grid of nodes:
        // 0 -- 1 -- 2
        // |    |    |
        // 3 -- 4 -- 5
        // |    |    |
        // 6 -- 7 -- 8

        let mut tile = RoutingTile::new(HierarchyLevel::Local, 0, 0);

        // Add nodes in a 3x3 grid (0.001 degree spacing = ~111m)
        let base_lat = 47.0;
        let base_lon = 8.0;
        let spacing = 0.001; // ~111m

        for row in 0..3 {
            for col in 0..3 {
                let lat = base_lat + row as f64 * spacing;
                let lon = base_lon + col as f64 * spacing;
                tile.nodes.push(RoutingNode::new(lat, lon));
            }
        }

        // Add edges (bidirectional)
        let mut edges = Vec::new();

        // Helper to add bidirectional edges
        let add_edge = |edges: &mut Vec<(u32, u32)>, from: u32, to: u32| {
            edges.push((from, to));
            edges.push((to, from));
        };

        // Horizontal edges
        add_edge(&mut edges, 0, 1);
        add_edge(&mut edges, 1, 2);
        add_edge(&mut edges, 3, 4);
        add_edge(&mut edges, 4, 5);
        add_edge(&mut edges, 6, 7);
        add_edge(&mut edges, 7, 8);

        // Vertical edges
        add_edge(&mut edges, 0, 3);
        add_edge(&mut edges, 3, 6);
        add_edge(&mut edges, 1, 4);
        add_edge(&mut edges, 4, 7);
        add_edge(&mut edges, 2, 5);
        add_edge(&mut edges, 5, 8);

        // Sort edges by source node
        edges.sort_by_key(|(from, _)| *from);

        // Group edges by source and create RoutingEdges
        let mut current_node = 0u32;
        let mut edge_start = 0u32;

        for (i, (from, to)) in edges.iter().enumerate() {
            if *from != current_node {
                // Update previous node's edge info
                if current_node < tile.nodes.len() as u32 {
                    tile.nodes[current_node as usize].first_edge = edge_start;
                    tile.nodes[current_node as usize].edge_count = (i as u32 - edge_start) as u8;
                }
                edge_start = i as u32;
                current_node = *from;
            }

            // Create edge with ~111m distance
            tile.edges.push(RoutingEdge::new(*to, 111.0, RoadClass::Residential));
        }

        // Update last node's edge info
        if current_node < tile.nodes.len() as u32 {
            tile.nodes[current_node as usize].first_edge = edge_start;
            tile.nodes[current_node as usize].edge_count =
                (tile.edges.len() as u32 - edge_start) as u8;
        }

        tile
    }

    #[test]
    fn test_find_path_simple() {
        let tile = create_test_tile();
        let router = SingleTileRouter::new(&tile);

        // Find path from corner to corner (0 -> 8)
        let result = router.find_path(0, 8).expect("should find path");

        // Path should exist
        assert!(!result.path.is_empty());

        // Distance should be approximately 4 * 111m = 444m (Manhattan distance on grid)
        assert!(result.total_distance_m > 400.0 && result.total_distance_m < 500.0);
    }

    #[test]
    fn test_find_path_adjacent() {
        let tile = create_test_tile();
        let router = SingleTileRouter::new(&tile);

        // Find path between adjacent nodes (0 -> 1)
        let result = router.find_path(0, 1).expect("should find path");

        assert_eq!(result.path.len(), 2);
        assert!((result.total_distance_m - 111.0).abs() < 5.0);
    }

    #[test]
    fn test_find_nearest_node() {
        let tile = create_test_tile();
        let router = SingleTileRouter::new(&tile);

        // Find nearest to center node
        let (idx, distance) = router
            .find_nearest_node(47.001, 8.001)
            .expect("should find node");

        assert_eq!(idx, 4); // Center node
        assert!(distance < 1.0); // Should be very close
    }

    #[test]
    fn test_snap_to_road() {
        let tile = create_test_tile();

        // Snap to near node 0 (base_lat=47.0, base_lon=8.0)
        let (idx, distance) = snap_to_road(&tile, 47.00005, 8.00005).expect("should snap");

        // Should snap to node 0 (closest to this position)
        assert_eq!(idx, 0);
        assert!(distance < 100.0);

        // Snap to near center node (47.001, 8.001)
        let (idx2, _) = snap_to_road(&tile, 47.001, 8.001).expect("should snap");
        assert_eq!(idx2, 4); // Center node
    }
}
