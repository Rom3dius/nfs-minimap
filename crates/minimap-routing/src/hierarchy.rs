//! Hierarchical routing across multiple tiles and levels.
//!
//! This module implements multi-level routing that can handle country-scale
//! distances efficiently by using a hierarchy of road networks:
//!
//! - Level 0 (Highway): Always in memory, for long-distance backbone
//! - Level 1 (Arterial): Cached, for regional routing
//! - Level 2 (Local): On-demand, for local streets

extern crate alloc;

use alloc::collections::BinaryHeap;
use alloc::vec::Vec;
use core::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use crate::cache::TileCache;
use crate::geo::haversine_distance_micro;
use crate::types::{get_visible_tiles, lat_lon_to_tile, HierarchyLevel, RoutingNode};

/// A node identifier that spans multiple tiles.
///
/// NOTE: The same physical node may exist in multiple tiles (when edges cross
/// tile boundaries). For A* to work correctly across tiles, we use coordinates
/// for equality/hashing so that the same physical location always has the same
/// identity regardless of which tile it's stored in.
#[derive(Debug, Clone, Copy)]
pub struct GlobalNodeId {
    pub level: HierarchyLevel,
    pub tile_x: i32,
    pub tile_y: i32,
    pub node_idx: u32,
    /// Node's latitude in microdegrees (used for identity).
    pub lat_micro: i32,
    /// Node's longitude in microdegrees (used for identity).
    pub lon_micro: i32,
}

impl GlobalNodeId {
    pub fn new(level: HierarchyLevel, tile_x: i32, tile_y: i32, node_idx: u32) -> Self {
        Self {
            level,
            tile_x,
            tile_y,
            node_idx,
            lat_micro: 0,
            lon_micro: 0,
        }
    }

    pub fn with_coords(
        level: HierarchyLevel,
        tile_x: i32,
        tile_y: i32,
        node_idx: u32,
        lat_micro: i32,
        lon_micro: i32,
    ) -> Self {
        Self {
            level,
            tile_x,
            tile_y,
            node_idx,
            lat_micro,
            lon_micro,
        }
    }
}

/// Equality based on coordinates, not tile indices.
impl PartialEq for GlobalNodeId {
    fn eq(&self, other: &Self) -> bool {
        self.level == other.level && self.lat_micro == other.lat_micro && self.lon_micro == other.lon_micro
    }
}

impl Eq for GlobalNodeId {}

/// Hash based on coordinates, not tile indices.
impl core::hash::Hash for GlobalNodeId {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.level.hash(state);
        self.lat_micro.hash(state);
        self.lon_micro.hash(state);
    }
}

/// Entry in the bidirectional A* priority queue.
#[derive(Debug, Clone)]
struct SearchEntry {
    node: GlobalNodeId,
    g_cost: u32,
    f_cost: u32,
}

impl PartialEq for SearchEntry {
    fn eq(&self, other: &Self) -> bool {
        self.f_cost == other.f_cost
    }
}

impl Eq for SearchEntry {}

impl PartialOrd for SearchEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SearchEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse for min-heap
        other.f_cost.cmp(&self.f_cost)
    }
}

/// Result of hierarchical routing.
#[derive(Debug, Clone)]
pub struct HierarchicalRoute {
    /// Path as global node IDs.
    pub path: Vec<GlobalNodeId>,
    /// Total distance in meters.
    pub total_distance_m: f64,
    /// Total time in seconds.
    pub total_time_s: f64,
    /// Number of nodes explored.
    pub nodes_explored: usize,
    /// Levels used in the route.
    pub levels_used: Vec<HierarchyLevel>,
}

/// Hierarchical router that uses multiple tile levels.
pub struct HierarchicalRouter<'a> {
    cache: &'a mut TileCache,
    /// Maximum number of nodes to explore before giving up.
    max_nodes: usize,
}

impl<'a> HierarchicalRouter<'a> {
    pub fn new(cache: &'a mut TileCache) -> Self {
        Self {
            cache,
            max_nodes: 500_000, // Forward-only A* needs more nodes for long routes
        }
    }

    /// Set maximum nodes to explore.
    pub fn with_max_nodes(mut self, max: usize) -> Self {
        self.max_nodes = max;
        self
    }

    /// Select hierarchy level based on route distance.
    ///
    /// - < 5km: Local (Level 2)
    /// - 5-50km: Arterial (Level 1)
    /// - > 50km: Highway (Level 0)
    pub fn select_level(distance_m: f64) -> HierarchyLevel {
        if distance_m < 5_000.0 {
            HierarchyLevel::Local
        } else if distance_m < 50_000.0 {
            HierarchyLevel::Arterial
        } else {
            HierarchyLevel::Highway
        }
    }

    /// Find a route between two points.
    ///
    /// Uses a level-based strategy: selects the appropriate level based on distance,
    /// then prepends a local route from the start position to the highway/arterial entry point.
    pub fn find_route(
        &mut self,
        start_lat: f64,
        start_lon: f64,
        end_lat: f64,
        end_lon: f64,
    ) -> Option<HierarchicalRoute> {
        // Determine distance and appropriate starting level
        let distance = crate::geo::haversine_distance(start_lat, start_lon, end_lat, end_lon);
        let initial_level = Self::select_level(distance);

        log::debug!("Route distance: {:.1} km, initial level: {:?}", distance / 1000.0, initial_level);

        // Try levels from the selected level up to Highway, with fallback
        let levels_to_try = match initial_level {
            HierarchyLevel::Local => vec![
                HierarchyLevel::Local,
                HierarchyLevel::Arterial,
                HierarchyLevel::Highway,
            ],
            HierarchyLevel::Arterial => vec![HierarchyLevel::Arterial, HierarchyLevel::Highway],
            HierarchyLevel::Highway => vec![HierarchyLevel::Highway],
        };

        for level in levels_to_try {
            log::debug!("Trying route at level {:?}", level);
            let result = match level {
                HierarchyLevel::Local => {
                    self.find_local_route(start_lat, start_lon, end_lat, end_lon)
                }
                HierarchyLevel::Arterial => {
                    self.find_arterial_route_with_local_connection(start_lat, start_lon, end_lat, end_lon)
                }
                HierarchyLevel::Highway => {
                    self.find_highway_route_with_local_connection(start_lat, start_lon, end_lat, end_lon)
                }
            };

            if result.is_some() {
                log::info!("Route found at level {:?}", level);
                return result;
            }
            log::debug!("No route at level {:?}, trying next level", level);
        }

        log::warn!(
            "No route found at any level from ({}, {}) to ({}, {})",
            start_lat,
            start_lon,
            end_lat,
            end_lon
        );
        None
    }

    /// Find a route using local tiles with arterial connectors.
    ///
    /// This uses multi-level routing: start/end are snapped to Local roads,
    /// but the search can use Arterial roads as connectors when Local roads
    /// are disconnected.
    fn find_local_route(
        &mut self,
        start_lat: f64,
        start_lon: f64,
        end_lat: f64,
        end_lon: f64,
    ) -> Option<HierarchicalRoute> {
        // Load Local tiles around start and end
        let local_start_tiles = get_visible_tiles(start_lat, start_lon, 0.02, HierarchyLevel::Local);
        let local_end_tiles = get_visible_tiles(end_lat, end_lon, 0.02, HierarchyLevel::Local);

        log::debug!("Multi-level route: loading Local and Arterial tiles");

        // Prefetch Local tiles
        for (tx, ty) in local_start_tiles.iter().chain(local_end_tiles.iter()) {
            let _ = self.cache.get(HierarchyLevel::Local, *tx, *ty);
        }

        // Also load Arterial tiles for the search area (as connectors)
        let arterial_start_tiles = get_visible_tiles(start_lat, start_lon, 0.2, HierarchyLevel::Arterial);
        let arterial_end_tiles = get_visible_tiles(end_lat, end_lon, 0.2, HierarchyLevel::Arterial);

        for (tx, ty) in arterial_start_tiles.iter().chain(arterial_end_tiles.iter()) {
            let _ = self.cache.get(HierarchyLevel::Arterial, *tx, *ty);
        }

        // Find start node on Local level
        let start_node = self.find_nearest_node(start_lat, start_lon, HierarchyLevel::Local);
        if start_node.is_none() {
            log::warn!("Could not find local start node near ({}, {})", start_lat, start_lon);
            return None;
        }
        let start_node = start_node.unwrap();
        log::debug!("Local start node: {:?}", start_node);

        // Find end node on Local level
        let end_node = self.find_nearest_node(end_lat, end_lon, HierarchyLevel::Local);
        if end_node.is_none() {
            log::warn!("Could not find local end node near ({}, {})", end_lat, end_lon);
            return None;
        }
        let end_node = end_node.unwrap();
        log::debug!("Local end node: {:?}", end_node);

        // Run multi-level A* (can traverse both Local and Arterial edges)
        self.astar_multi_level(start_node, end_node)
    }

    /// Find a route using arterial level with local road connections.
    ///
    /// This method:
    /// 1. Finds the nearest Local road to start/end (for the short straight line from car)
    /// 2. Finds the nearest Arterial entry/exit points
    /// 3. Routes from Local start to Arterial entry using local roads
    /// 4. Routes on Arterial network
    /// 5. Routes from Arterial exit to Local end using local roads
    /// 6. Combines all paths
    fn find_arterial_route_with_local_connection(
        &mut self,
        start_lat: f64,
        start_lon: f64,
        end_lat: f64,
        end_lon: f64,
    ) -> Option<HierarchicalRoute> {
        // Load Local tiles around start and end
        let local_start_tiles = get_visible_tiles(start_lat, start_lon, 0.02, HierarchyLevel::Local);
        let local_end_tiles = get_visible_tiles(end_lat, end_lon, 0.02, HierarchyLevel::Local);
        for (tx, ty) in local_start_tiles.iter().chain(local_end_tiles.iter()) {
            let _ = self.cache.get(HierarchyLevel::Local, *tx, *ty);
        }

        // Load Arterial tiles around start and end
        let arterial_start_tiles = get_visible_tiles(start_lat, start_lon, 0.2, HierarchyLevel::Arterial);
        let arterial_end_tiles = get_visible_tiles(end_lat, end_lon, 0.2, HierarchyLevel::Arterial);
        for (tx, ty) in arterial_start_tiles.iter().chain(arterial_end_tiles.iter()) {
            let _ = self.cache.get(HierarchyLevel::Arterial, *tx, *ty);
        }

        // Find nearest Local nodes (where the car actually is)
        let local_start = self.find_nearest_node(start_lat, start_lon, HierarchyLevel::Local)?;
        let local_end = self.find_nearest_node(end_lat, end_lon, HierarchyLevel::Local)?;
        log::debug!("Local start: {:?}", local_start);
        log::debug!("Local end: {:?}", local_end);

        // Find nearest Arterial entry/exit points
        let arterial_start = self.find_nearest_node(start_lat, start_lon, HierarchyLevel::Arterial)?;
        let arterial_end = self.find_nearest_node(end_lat, end_lon, HierarchyLevel::Arterial)?;
        log::debug!("Arterial start: {:?}", arterial_start);
        log::debug!("Arterial end: {:?}", arterial_end);

        // Route on Arterial network
        let arterial_route = self.astar(arterial_start, arterial_end, HierarchyLevel::Arterial)?;
        log::debug!("Arterial route found: {} nodes", arterial_route.path.len());

        // Build the combined path
        let mut combined_path = Vec::new();
        let mut levels_used = Vec::new();

        // Add local start node (the nearest road to the car)
        combined_path.push(local_start);
        levels_used.push(HierarchyLevel::Local);

        // Add the arterial route
        combined_path.extend(arterial_route.path.iter().cloned());
        if !levels_used.contains(&HierarchyLevel::Arterial) {
            levels_used.push(HierarchyLevel::Arterial);
        }

        // Add local end node if different from last arterial node
        if local_end.lat_micro != arterial_end.lat_micro || local_end.lon_micro != arterial_end.lon_micro {
            combined_path.push(local_end);
        }

        Some(HierarchicalRoute {
            path: combined_path,
            total_distance_m: arterial_route.total_distance_m,
            total_time_s: arterial_route.total_time_s,
            nodes_explored: arterial_route.nodes_explored,
            levels_used,
        })
    }

    /// Find a route using highway level with local road connections.
    ///
    /// Similar to find_arterial_route_with_local_connection but for Highway level.
    fn find_highway_route_with_local_connection(
        &mut self,
        start_lat: f64,
        start_lon: f64,
        end_lat: f64,
        end_lon: f64,
    ) -> Option<HierarchicalRoute> {
        // Load Local tiles around start and end
        let local_start_tiles = get_visible_tiles(start_lat, start_lon, 0.02, HierarchyLevel::Local);
        let local_end_tiles = get_visible_tiles(end_lat, end_lon, 0.02, HierarchyLevel::Local);
        for (tx, ty) in local_start_tiles.iter().chain(local_end_tiles.iter()) {
            let _ = self.cache.get(HierarchyLevel::Local, *tx, *ty);
        }

        // Load Highway tiles around start and end
        let highway_start_tiles = get_visible_tiles(start_lat, start_lon, 1.0, HierarchyLevel::Highway);
        let highway_end_tiles = get_visible_tiles(end_lat, end_lon, 1.0, HierarchyLevel::Highway);
        for (tx, ty) in highway_start_tiles.iter().chain(highway_end_tiles.iter()) {
            let _ = self.cache.get(HierarchyLevel::Highway, *tx, *ty);
        }

        // Find nearest Local nodes (where the car actually is)
        let local_start = self.find_nearest_node(start_lat, start_lon, HierarchyLevel::Local)?;
        let local_end = self.find_nearest_node(end_lat, end_lon, HierarchyLevel::Local)?;
        log::debug!("Local start: {:?}", local_start);
        log::debug!("Local end: {:?}", local_end);

        // Find nearest Highway entry/exit points
        let highway_start = self.find_nearest_node(start_lat, start_lon, HierarchyLevel::Highway)?;
        let highway_end = self.find_nearest_node(end_lat, end_lon, HierarchyLevel::Highway)?;
        log::debug!("Highway start: {:?}", highway_start);
        log::debug!("Highway end: {:?}", highway_end);

        // Route on Highway network
        let highway_route = self.astar(highway_start, highway_end, HierarchyLevel::Highway)?;
        log::debug!("Highway route found: {} nodes", highway_route.path.len());

        // Build the combined path
        let mut combined_path = Vec::new();
        let mut levels_used = Vec::new();

        // Add local start node (the nearest road to the car)
        combined_path.push(local_start);
        levels_used.push(HierarchyLevel::Local);

        // Add the highway route
        combined_path.extend(highway_route.path.iter().cloned());
        if !levels_used.contains(&HierarchyLevel::Highway) {
            levels_used.push(HierarchyLevel::Highway);
        }

        // Add local end node if different from last highway node
        if local_end.lat_micro != highway_end.lat_micro || local_end.lon_micro != highway_end.lon_micro {
            combined_path.push(local_end);
        }

        Some(HierarchicalRoute {
            path: combined_path,
            total_distance_m: highway_route.total_distance_m,
            total_time_s: highway_route.total_time_s,
            nodes_explored: highway_route.nodes_explored,
            levels_used,
        })
    }

    /// Find a route using arterial level (Level 1).
    ///
    /// First snaps start/end to Local level to find the nearest road,
    /// then routes through the Arterial network.
    ///
    /// Note: Currently unused - kept for potential future use.
    #[allow(dead_code)]
    fn find_arterial_route(
        &mut self,
        start_lat: f64,
        start_lon: f64,
        end_lat: f64,
        end_lon: f64,
    ) -> Option<HierarchicalRoute> {
        let level = HierarchyLevel::Arterial;

        // First, snap to Local level to find the actual nearest road
        let local_start = self.find_nearest_node(start_lat, start_lon, HierarchyLevel::Local);
        let local_end = self.find_nearest_node(end_lat, end_lon, HierarchyLevel::Local);

        // Use Local-snapped coordinates for finding Arterial access points
        let (arterial_start_lat, arterial_start_lon) = if let Some(ref node) = local_start {
            (node.lat_micro as f64 / 1_000_000.0, node.lon_micro as f64 / 1_000_000.0)
        } else {
            (start_lat, start_lon)
        };
        let (arterial_end_lat, arterial_end_lon) = if let Some(ref node) = local_end {
            (node.lat_micro as f64 / 1_000_000.0, node.lon_micro as f64 / 1_000_000.0)
        } else {
            (end_lat, end_lon)
        };

        // Load tiles around start and end
        let start_tiles = get_visible_tiles(arterial_start_lat, arterial_start_lon, 0.2, level);
        let end_tiles = get_visible_tiles(arterial_end_lat, arterial_end_lon, 0.2, level);

        log::debug!("Arterial route: loading {} start tiles, {} end tiles", start_tiles.len(), end_tiles.len());

        for (tx, ty) in start_tiles.iter().chain(end_tiles.iter()) {
            if let Some(tile) = self.cache.get(level, *tx, *ty) {
                log::debug!("  Tile ({}, {}): {} nodes, {} edges", tx, ty, tile.nodes.len(), tile.edges.len());
            } else {
                log::debug!("  Tile ({}, {}): NOT FOUND", tx, ty);
            }
        }

        // Find start and end nodes on arterial network
        let arterial_start = self.find_nearest_node(arterial_start_lat, arterial_start_lon, level);
        if arterial_start.is_none() {
            log::warn!("Could not find arterial start node near ({}, {})", arterial_start_lat, arterial_start_lon);
            return None;
        }
        let arterial_start = arterial_start.unwrap();
        log::debug!("Arterial start node: {:?}", arterial_start);

        let arterial_end = self.find_nearest_node(arterial_end_lat, arterial_end_lon, level);
        if arterial_end.is_none() {
            log::warn!("Could not find arterial end node near ({}, {})", arterial_end_lat, arterial_end_lon);
            return None;
        }
        let arterial_end = arterial_end.unwrap();
        log::debug!("Arterial end node: {:?}", arterial_end);

        // Run A* on arterial network
        let mut route = self.astar(arterial_start, arterial_end, level)?;

        // Prepend Local start node if different from Arterial start
        if let Some(local_node) = local_start {
            if local_node.lat_micro != arterial_start.lat_micro
                || local_node.lon_micro != arterial_start.lon_micro
            {
                route.path.insert(0, local_node);
                route.levels_used.push(HierarchyLevel::Local);
            }
        }

        // Append Local end node if different from Arterial end
        if let Some(local_node) = local_end {
            if local_node.lat_micro != arterial_end.lat_micro
                || local_node.lon_micro != arterial_end.lon_micro
            {
                route.path.push(local_node);
                if !route.levels_used.contains(&HierarchyLevel::Local) {
                    route.levels_used.push(HierarchyLevel::Local);
                }
            }
        }

        Some(route)
    }

    /// Find a route using highway backbone (Level 0).
    ///
    /// First snaps start/end to Local level to find the nearest road,
    /// then routes through the Highway network.
    ///
    /// Note: Currently unused - kept for potential future use.
    #[allow(dead_code)]
    fn find_highway_route(
        &mut self,
        start_lat: f64,
        start_lon: f64,
        end_lat: f64,
        end_lon: f64,
    ) -> Option<HierarchicalRoute> {
        let highway_level = HierarchyLevel::Highway;

        // First, snap to Local level to find the actual nearest road
        let local_start = self.find_nearest_node(start_lat, start_lon, HierarchyLevel::Local);
        let local_end = self.find_nearest_node(end_lat, end_lon, HierarchyLevel::Local);

        // Use Local-snapped coordinates for finding Highway access points
        let (highway_start_lat, highway_start_lon) = if let Some(ref node) = local_start {
            (node.lat_micro as f64 / 1_000_000.0, node.lon_micro as f64 / 1_000_000.0)
        } else {
            (start_lat, start_lon)
        };
        let (highway_end_lat, highway_end_lon) = if let Some(ref node) = local_end {
            (node.lat_micro as f64 / 1_000_000.0, node.lon_micro as f64 / 1_000_000.0)
        } else {
            (end_lat, end_lon)
        };

        // Find nearest highway nodes
        let start_highway = self.find_nearest_node(highway_start_lat, highway_start_lon, highway_level);
        if start_highway.is_none() {
            log::warn!("Could not find highway start node near ({}, {})", highway_start_lat, highway_start_lon);
            return None;
        }
        let start_highway = start_highway.unwrap();
        log::debug!("Highway start node: {:?}", start_highway);

        let end_highway = self.find_nearest_node(highway_end_lat, highway_end_lon, highway_level);
        if end_highway.is_none() {
            log::warn!("Could not find highway end node near ({}, {})", highway_end_lat, highway_end_lon);
            return None;
        }
        let end_highway = end_highway.unwrap();
        log::debug!("Highway end node: {:?}", end_highway);

        // Route on highway network
        let mut route = self.astar(start_highway, end_highway, highway_level)?;

        // Prepend Local start node if different from Highway start
        if let Some(local_node) = local_start {
            if local_node.lat_micro != start_highway.lat_micro
                || local_node.lon_micro != start_highway.lon_micro
            {
                route.path.insert(0, local_node);
            }
        }

        // Append Local end node if different from Highway end
        if let Some(local_node) = local_end {
            if local_node.lat_micro != end_highway.lat_micro
                || local_node.lon_micro != end_highway.lon_micro
            {
                route.path.push(local_node);
            }
        }

        // Update levels used
        let has_local = local_start.is_some() || local_end.is_some();
        Some(HierarchicalRoute {
            levels_used: if has_local {
                vec![HierarchyLevel::Local, HierarchyLevel::Highway]
            } else {
                vec![HierarchyLevel::Highway]
            },
            ..route
        })
    }

    /// Find the nearest node to a position at a given level.
    fn find_nearest_node(
        &mut self,
        lat: f64,
        lon: f64,
        level: HierarchyLevel,
    ) -> Option<GlobalNodeId> {
        let (tile_x, tile_y) = lat_lon_to_tile(lat, lon, level);
        let lat_micro = (lat * 1_000_000.0) as i32;
        let lon_micro = (lon * 1_000_000.0) as i32;

        let mut best: Option<(GlobalNodeId, f64)> = None;

        // Search in center tile and neighbors
        for dy in -1..=1i32 {
            for dx in -1..=1i32 {
                let tx = tile_x + dx;
                let ty = tile_y + dy;

                if let Some(tile) = self.cache.get(level, tx, ty) {
                    for (idx, node) in tile.nodes.iter().enumerate() {
                        let dist = haversine_distance_micro(lat_micro, lon_micro, node.lat, node.lon);
                        let is_better = match &best {
                            Some((_, best_dist)) => dist < *best_dist,
                            None => true,
                        };
                        if is_better {
                            // Include node coordinates for cross-tile identity
                            best = Some((
                                GlobalNodeId::with_coords(level, tx, ty, idx as u32, node.lat, node.lon),
                                dist,
                            ));
                        }
                    }
                }
            }
        }

        best.map(|(id, _)| id)
    }

    /// A* search (forward-only).
    ///
    /// Simpler than bidirectional A* and correctly handles one-way roads
    /// without needing a reverse edge index.
    fn astar(
        &mut self,
        start: GlobalNodeId,
        end: GlobalNodeId,
        level: HierarchyLevel,
    ) -> Option<HierarchicalRoute> {
        // Get target coordinates for heuristic
        let end_node = self.get_node(&end)?;
        let start_node = self.get_node(&start)?;

        let end_lat = end_node.lat;
        let end_lon = end_node.lon;

        // Open set (priority queue)
        let mut open = BinaryHeap::new();
        // Cost to reach each node
        let mut g_costs: HashMap<GlobalNodeId, u32> = HashMap::new();
        // Parent pointers for path reconstruction
        let mut came_from: HashMap<GlobalNodeId, GlobalNodeId> = HashMap::new();
        // Closed set
        let mut closed: HashSet<GlobalNodeId> = HashSet::new();

        // Initialize with start node
        open.push(SearchEntry {
            node: start,
            g_cost: 0,
            f_cost: self.heuristic_micro(start_node.lat, start_node.lon, end_lat, end_lon),
        });
        g_costs.insert(start, 0);

        let mut nodes_explored = 0;

        while let Some(current) = open.pop() {
            nodes_explored += 1;

            if nodes_explored >= self.max_nodes {
                log::debug!("A* hit max nodes limit ({})", self.max_nodes);
                break;
            }

            // Skip if already processed
            if closed.contains(&current.node) {
                continue;
            }
            closed.insert(current.node);

            // Check if we've reached the goal
            if current.node == end {
                log::debug!("A* found path: explored {} nodes", nodes_explored);

                // Reconstruct path
                let path = self.reconstruct_path(&came_from, &end);
                let (total_distance, total_time) = self.calculate_path_metrics(&path, level);

                return Some(HierarchicalRoute {
                    path,
                    total_distance_m: total_distance,
                    total_time_s: total_time,
                    nodes_explored,
                    levels_used: vec![level],
                });
            }

            // Expand neighbors
            self.expand_node(
                &current.node,
                current.g_cost,
                end_lat,
                end_lon,
                level,
                &mut open,
                &mut g_costs,
                &mut came_from,
                &closed,
            );
        }

        log::debug!(
            "A* finished without finding path: explored {} nodes, closed {}",
            nodes_explored,
            closed.len()
        );
        None
    }

    /// Multi-level A* search that can traverse both Local and Arterial roads.
    ///
    /// This allows routes to start on local roads, use arterial roads as
    /// connectors, and end on local roads.
    fn astar_multi_level(
        &mut self,
        start: GlobalNodeId,
        end: GlobalNodeId,
    ) -> Option<HierarchicalRoute> {
        // Get target coordinates for heuristic
        let end_node = self.get_node(&end)?;
        let start_node = self.get_node(&start)?;

        let end_lat = end_node.lat;
        let end_lon = end_node.lon;

        // Open set (priority queue)
        let mut open = BinaryHeap::new();
        // Cost to reach each node
        let mut g_costs: HashMap<GlobalNodeId, u32> = HashMap::new();
        // Parent pointers for path reconstruction
        let mut came_from: HashMap<GlobalNodeId, GlobalNodeId> = HashMap::new();
        // Closed set
        let mut closed: HashSet<GlobalNodeId> = HashSet::new();

        // Initialize with start node
        open.push(SearchEntry {
            node: start,
            g_cost: 0,
            f_cost: self.heuristic_micro(start_node.lat, start_node.lon, end_lat, end_lon),
        });
        g_costs.insert(start, 0);

        let mut nodes_explored = 0;
        let mut levels_used = HashSet::new();
        levels_used.insert(start.level);

        while let Some(current) = open.pop() {
            nodes_explored += 1;

            if nodes_explored >= self.max_nodes {
                log::debug!("Multi-level A* hit max nodes limit ({})", self.max_nodes);
                break;
            }

            // Skip if already processed
            if closed.contains(&current.node) {
                continue;
            }
            closed.insert(current.node);
            levels_used.insert(current.node.level);

            // Check if we've reached the goal (same coordinates, any level)
            if current.node.lat_micro == end.lat_micro && current.node.lon_micro == end.lon_micro {
                log::debug!(
                    "Multi-level A* found path: explored {} nodes, levels: {:?}",
                    nodes_explored,
                    levels_used
                );

                // Reconstruct path
                let path = self.reconstruct_path(&came_from, &current.node);
                let (total_distance, total_time) = self.calculate_path_metrics_multi(&path);

                return Some(HierarchicalRoute {
                    path,
                    total_distance_m: total_distance,
                    total_time_s: total_time,
                    nodes_explored,
                    levels_used: levels_used.into_iter().collect(),
                });
            }

            // Expand neighbors at current level
            self.expand_node_multi_level(
                &current.node,
                current.g_cost,
                end_lat,
                end_lon,
                &mut open,
                &mut g_costs,
                &mut came_from,
                &closed,
            );
        }

        log::debug!(
            "Multi-level A* finished without finding path: explored {} nodes, closed {}",
            nodes_explored,
            closed.len()
        );
        None
    }

    /// Expand a node in multi-level search.
    ///
    /// This expands edges at the current level AND looks for the same
    /// physical location at other levels (level transitions).
    #[allow(clippy::too_many_arguments)]
    fn expand_node_multi_level(
        &mut self,
        node: &GlobalNodeId,
        g_cost: u32,
        target_lat: i32,
        target_lon: i32,
        open: &mut BinaryHeap<SearchEntry>,
        g_costs: &mut HashMap<GlobalNodeId, u32>,
        came_from: &mut HashMap<GlobalNodeId, GlobalNodeId>,
        closed: &HashSet<GlobalNodeId>,
    ) {
        // 1. Expand edges at current level (normal edge traversal)
        self.expand_node(
            node,
            g_cost,
            target_lat,
            target_lon,
            node.level,
            open,
            g_costs,
            came_from,
            closed,
        );

        // 2. Add level transitions (same physical location, different level)
        // This allows switching between all hierarchy levels: Local ↔ Arterial ↔ Highway
        let other_levels: &[HierarchyLevel] = match node.level {
            HierarchyLevel::Local => &[HierarchyLevel::Arterial],
            HierarchyLevel::Arterial => &[HierarchyLevel::Local, HierarchyLevel::Highway],
            HierarchyLevel::Highway => &[HierarchyLevel::Arterial],
        };

        for &other_level in other_levels {
            // Find the same location at the other level
            let lat = node.lat_micro as f64 / 1_000_000.0;
            let lon = node.lon_micro as f64 / 1_000_000.0;
            let (tile_x, tile_y) = lat_lon_to_tile(lat, lon, other_level);

            // Find the same location at other level (extract data to avoid borrow conflict)
            let transition_node = if let Some(tile) = self.cache.get(other_level, tile_x, tile_y) {
                tile.nodes
                    .iter()
                    .enumerate()
                    .find(|(_, n)| n.lat == node.lat_micro && n.lon == node.lon_micro)
                    .map(|(idx, n)| {
                        GlobalNodeId::with_coords(other_level, tile_x, tile_y, idx as u32, n.lat, n.lon)
                    })
            } else {
                None
            };

            if let Some(neighbor_id) = transition_node {
                if !closed.contains(&neighbor_id) {
                    // Level transition has zero cost (same physical location)
                    let tentative_g = g_cost;

                    let is_better = match g_costs.get(&neighbor_id) {
                        Some(&existing) => tentative_g < existing,
                        None => true,
                    };

                    if is_better {
                        came_from.insert(neighbor_id, *node);
                        g_costs.insert(neighbor_id, tentative_g);

                        let h = self.heuristic_micro(neighbor_id.lat_micro, neighbor_id.lon_micro, target_lat, target_lon);
                        open.push(SearchEntry {
                            node: neighbor_id,
                            g_cost: tentative_g,
                            f_cost: tentative_g.saturating_add(h),
                        });
                    }
                }
            }
        }
    }

    /// Calculate path metrics for a multi-level path.
    fn calculate_path_metrics_multi(&mut self, path: &[GlobalNodeId]) -> (f64, f64) {
        let mut total_distance = 0.0;
        let mut total_time = 0.0;

        for window in path.windows(2) {
            let from = &window[0];
            let to = &window[1];

            // Skip level transitions (same coordinates, different level)
            if from.lat_micro == to.lat_micro && from.lon_micro == to.lon_micro {
                continue;
            }

            // Try to find the edge
            if let Some(tile) = self.cache.get(from.level, from.tile_x, from.tile_y) {
                // Check same-tile edges first
                let mut found = false;
                if to.tile_x == from.tile_x && to.tile_y == from.tile_y && to.level == from.level {
                    for edge in tile.edges_for_node(from.node_idx) {
                        if edge.target == to.node_idx {
                            total_distance += edge.distance_m();
                            total_time += edge.travel_time_s();
                            found = true;
                            break;
                        }
                    }
                }

                // Check cross-tile edges
                if !found && to.level == from.level {
                    for cross_edge in tile.cross_tile_edges_for_node(from.node_idx) {
                        if cross_edge.target_tile_x == to.tile_x
                            && cross_edge.target_tile_y == to.tile_y
                            && cross_edge.target_node == to.node_idx
                        {
                            total_distance += cross_edge.distance_m();
                            total_time += cross_edge.travel_time_s();
                            break;
                        }
                    }
                }
            }
        }

        (total_distance, total_time)
    }

    /// Reconstruct path from start to goal.
    fn reconstruct_path(
        &self,
        came_from: &HashMap<GlobalNodeId, GlobalNodeId>,
        goal: &GlobalNodeId,
    ) -> Vec<GlobalNodeId> {
        let mut path = Vec::new();
        let mut current = *goal;

        path.push(current);
        while let Some(&prev) = came_from.get(&current) {
            path.push(prev);
            current = prev;
        }

        path.reverse();
        path
    }

    /// Get a node from the cache.
    fn get_node(&mut self, id: &GlobalNodeId) -> Option<RoutingNode> {
        let tile = self.cache.get(id.level, id.tile_x, id.tile_y)?;
        tile.node(id.node_idx).copied()
    }

    /// Calculate heuristic using microdegree coordinates.
    fn heuristic_micro(&self, lat1: i32, lon1: i32, lat2: i32, lon2: i32) -> u32 {
        let distance_m = haversine_distance_micro(lat1, lon1, lat2, lon2);
        // Assume max speed of 130 km/h, convert to centiseconds
        let time_s = distance_m / (130.0 * 1000.0 / 3600.0);
        (time_s * 100.0) as u32
    }

    /// Expand a node by adding its neighbors to the open set.
    ///
    /// Supports both v1 tiles (using canonical tile lookup for boundary nodes)
    /// and v2 tiles (using explicit cross-tile edges).
    #[allow(clippy::too_many_arguments)]
    fn expand_node(
        &mut self,
        node: &GlobalNodeId,
        g_cost: u32,
        target_lat: i32,
        target_lon: i32,
        _level: HierarchyLevel,
        open: &mut BinaryHeap<SearchEntry>,
        g_costs: &mut HashMap<GlobalNodeId, u32>,
        came_from: &mut HashMap<GlobalNodeId, GlobalNodeId>,
        closed: &HashSet<GlobalNodeId>,
    ) {
        // Collect neighbors from same-tile edges and cross-tile edges
        let mut neighbors: Vec<(GlobalNodeId, u32, i32, i32)> = {
            let tile = match self.cache.get(node.level, node.tile_x, node.tile_y) {
                Some(t) => t,
                None => return,
            };

            // 1. Same-tile edges
            let mut neighbors: Vec<_> = tile
                .edges_for_node(node.node_idx)
                .iter()
                .filter_map(|edge| {
                    let target_node = tile.node(edge.target)?;
                    Some((
                        GlobalNodeId::with_coords(
                            node.level,
                            node.tile_x,
                            node.tile_y,
                            edge.target,
                            target_node.lat,
                            target_node.lon,
                        ),
                        edge.weight(),
                        target_node.lat,
                        target_node.lon,
                    ))
                })
                .collect();

            // 2. Cross-tile edges (v2 format - stored separately in the tile)
            for cross_edge in tile.cross_tile_edges_for_node(node.node_idx) {
                neighbors.push((
                    GlobalNodeId::with_coords(
                        node.level,
                        cross_edge.target_tile_x,
                        cross_edge.target_tile_y,
                        cross_edge.target_node,
                        0, // Will be filled in below
                        0,
                    ),
                    cross_edge.weight(),
                    cross_edge.target_tile_x, // Placeholder
                    cross_edge.target_tile_y,
                ));
            }

            neighbors
        };

        // 3. V1 compatibility: look for edges in the canonical tile if this node
        //    has a different tile than its coordinates suggest (boundary node case)
        let canonical_tile = lat_lon_to_tile(
            node.lat_micro as f64 / 1_000_000.0,
            node.lon_micro as f64 / 1_000_000.0,
            node.level,
        );
        if canonical_tile != (node.tile_x, node.tile_y) {
            // Find this node in the canonical tile and get its edges
            if let Some(tile) = self.cache.get(node.level, canonical_tile.0, canonical_tile.1) {
                for (idx, n) in tile.nodes.iter().enumerate() {
                    if n.lat == node.lat_micro && n.lon == node.lon_micro {
                        // Found the same node, get its edges
                        let extra_neighbors: Vec<_> = tile
                            .edges_for_node(idx as u32)
                            .iter()
                            .filter_map(|edge| {
                                let target_node = tile.node(edge.target)?;
                                Some((
                                    GlobalNodeId::with_coords(
                                        node.level,
                                        canonical_tile.0,
                                        canonical_tile.1,
                                        edge.target,
                                        target_node.lat,
                                        target_node.lon,
                                    ),
                                    edge.weight(),
                                    target_node.lat,
                                    target_node.lon,
                                ))
                            })
                            .collect();
                        neighbors.extend(extra_neighbors);
                        break;
                    }
                }
            }
        }

        // Process neighbors, loading target tiles for cross-tile edges if needed
        for (mut neighbor_id, edge_weight, lat_or_tile_x, lon_or_tile_y) in neighbors {
            // If this is a cross-tile edge (lat/lon are 0), load target tile to get coordinates
            if neighbor_id.lat_micro == 0 && neighbor_id.lon_micro == 0 {
                let target_tile = self.cache.get(
                    node.level,
                    neighbor_id.tile_x,
                    neighbor_id.tile_y,
                );
                if let Some(tile) = target_tile {
                    if let Some(target_node) = tile.node(neighbor_id.node_idx) {
                        neighbor_id.lat_micro = target_node.lat;
                        neighbor_id.lon_micro = target_node.lon;
                    } else {
                        continue; // Target node not found
                    }
                } else {
                    continue; // Target tile not loaded
                }
            }

            if closed.contains(&neighbor_id) {
                continue;
            }

            let tentative_g = g_cost.saturating_add(edge_weight);

            let is_better = match g_costs.get(&neighbor_id) {
                Some(&existing) => tentative_g < existing,
                None => true,
            };

            if is_better {
                came_from.insert(neighbor_id, *node);
                g_costs.insert(neighbor_id, tentative_g);

                // Use the actual coordinates for heuristic
                let neighbor_lat = if neighbor_id.lat_micro != 0 {
                    neighbor_id.lat_micro
                } else {
                    lat_or_tile_x
                };
                let neighbor_lon = if neighbor_id.lon_micro != 0 {
                    neighbor_id.lon_micro
                } else {
                    lon_or_tile_y
                };

                let h = self.heuristic_micro(neighbor_lat, neighbor_lon, target_lat, target_lon);
                open.push(SearchEntry {
                    node: neighbor_id,
                    g_cost: tentative_g,
                    f_cost: tentative_g.saturating_add(h),
                });
            }
        }
    }

    /// Calculate total distance and time for a path.
    fn calculate_path_metrics(&mut self, path: &[GlobalNodeId], _level: HierarchyLevel) -> (f64, f64) {
        let mut total_distance = 0.0;
        let mut total_time = 0.0;

        for window in path.windows(2) {
            let from = &window[0];
            let to = &window[1];

            if let Some(tile) = self.cache.get(from.level, from.tile_x, from.tile_y) {
                // Check same-tile edges first
                let mut found = false;
                if to.tile_x == from.tile_x && to.tile_y == from.tile_y {
                    for edge in tile.edges_for_node(from.node_idx) {
                        if edge.target == to.node_idx {
                            total_distance += edge.distance_m();
                            total_time += edge.travel_time_s();
                            found = true;
                            break;
                        }
                    }
                }

                // Check cross-tile edges
                if !found {
                    for cross_edge in tile.cross_tile_edges_for_node(from.node_idx) {
                        if cross_edge.target_tile_x == to.tile_x
                            && cross_edge.target_tile_y == to.tile_y
                            && cross_edge.target_node == to.node_idx
                        {
                            total_distance += cross_edge.distance_m();
                            total_time += cross_edge.travel_time_s();
                            break;
                        }
                    }
                }
            }
        }

        (total_distance, total_time)
    }

    /// Get path as lat/lon coordinates.
    pub fn path_to_coordinates(&mut self, path: &[GlobalNodeId]) -> Vec<(f64, f64)> {
        path.iter()
            .filter_map(|id| self.get_node(id).map(|n| n.position()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{RoadClass, RoutingEdge, RoutingNode, RoutingTile};

    fn create_test_tile() -> RoutingTile {
        // Create a simple 3x3 grid
        let mut tile = RoutingTile::new(HierarchyLevel::Local, 0, 0);

        let base_lat = 47.0;
        let base_lon = 8.0;
        let spacing = 0.001;

        for row in 0..3 {
            for col in 0..3 {
                let lat = base_lat + row as f64 * spacing;
                let lon = base_lon + col as f64 * spacing;
                tile.nodes.push(RoutingNode::new(lat, lon));
            }
        }

        // Add edges (simplified - just connects adjacent nodes)
        let mut edges = Vec::new();
        let add_edge = |edges: &mut Vec<(u32, u32)>, from: u32, to: u32| {
            edges.push((from, to));
            edges.push((to, from));
        };

        // Horizontal
        add_edge(&mut edges, 0, 1);
        add_edge(&mut edges, 1, 2);
        add_edge(&mut edges, 3, 4);
        add_edge(&mut edges, 4, 5);
        add_edge(&mut edges, 6, 7);
        add_edge(&mut edges, 7, 8);

        // Vertical
        add_edge(&mut edges, 0, 3);
        add_edge(&mut edges, 3, 6);
        add_edge(&mut edges, 1, 4);
        add_edge(&mut edges, 4, 7);
        add_edge(&mut edges, 2, 5);
        add_edge(&mut edges, 5, 8);

        edges.sort_by_key(|(from, _)| *from);

        let mut current_node = 0u32;
        let mut edge_start = 0u32;

        for (i, (from, to)) in edges.iter().enumerate() {
            if *from != current_node {
                if current_node < tile.nodes.len() as u32 {
                    tile.nodes[current_node as usize].first_edge = edge_start;
                    tile.nodes[current_node as usize].edge_count = (i as u32 - edge_start) as u8;
                }
                edge_start = i as u32;
                current_node = *from;
            }
            tile.edges.push(RoutingEdge::new(*to, 111.0, RoadClass::Residential));
        }

        if current_node < tile.nodes.len() as u32 {
            tile.nodes[current_node as usize].first_edge = edge_start;
            tile.nodes[current_node as usize].edge_count =
                (tile.edges.len() as u32 - edge_start) as u8;
        }

        tile
    }

    #[test]
    fn test_global_node_id() {
        let id = GlobalNodeId::new(HierarchyLevel::Local, 10, 20, 5);
        assert_eq!(id.level, HierarchyLevel::Local);
        assert_eq!(id.tile_x, 10);
        assert_eq!(id.tile_y, 20);
        assert_eq!(id.node_idx, 5);
    }

    #[test]
    fn test_search_entry_ordering() {
        let e1 = SearchEntry {
            node: GlobalNodeId::new(HierarchyLevel::Local, 0, 0, 0),
            g_cost: 10,
            f_cost: 100,
        };
        let e2 = SearchEntry {
            node: GlobalNodeId::new(HierarchyLevel::Local, 0, 0, 1),
            g_cost: 20,
            f_cost: 50,
        };

        // Lower f_cost should be "greater" for min-heap
        assert!(e2 > e1);
    }

    #[test]
    fn test_global_node_id_hash_eq() {
        use std::collections::HashSet;

        // GlobalNodeId equality is based on coordinates, not tile/index
        let id1 = GlobalNodeId::with_coords(HierarchyLevel::Local, 10, 20, 5, 47000000, 8000000);
        let id2 = GlobalNodeId::with_coords(HierarchyLevel::Local, 10, 20, 5, 47000000, 8000000);
        let id3 = GlobalNodeId::with_coords(HierarchyLevel::Local, 10, 20, 6, 47000100, 8000000);
        let id4 = GlobalNodeId::with_coords(HierarchyLevel::Arterial, 10, 20, 5, 47000000, 8000000);

        // Same coordinates should be equal
        assert_eq!(id1, id2);
        // Different coordinates
        assert_ne!(id1, id3);
        // Different level
        assert_ne!(id1, id4);

        // Same physical location in different tiles should be equal
        let id5 = GlobalNodeId::with_coords(HierarchyLevel::Local, 11, 20, 99, 47000000, 8000000);
        assert_eq!(id1, id5); // Same coords, different tile

        // Should work in HashSet
        let mut set = HashSet::new();
        set.insert(id1);
        assert!(set.contains(&id2));
        assert!(!set.contains(&id3));
        assert!(set.contains(&id5)); // Same coords, different tile
    }

    #[test]
    fn test_level_selection_short() {
        // < 5km should use Local
        let level = HierarchicalRouter::<'static>::select_level(4_000.0);
        assert_eq!(level, HierarchyLevel::Local);
    }

    #[test]
    fn test_level_selection_medium() {
        // 5-50km should use Arterial
        let level = HierarchicalRouter::<'static>::select_level(25_000.0);
        assert_eq!(level, HierarchyLevel::Arterial);
    }

    #[test]
    fn test_level_selection_long() {
        // > 50km should use Highway
        let level = HierarchicalRouter::<'static>::select_level(100_000.0);
        assert_eq!(level, HierarchyLevel::Highway);
    }

    #[test]
    fn test_hierarchical_route_struct() {
        let route = HierarchicalRoute {
            path: vec![
                GlobalNodeId::new(HierarchyLevel::Local, 0, 0, 0),
                GlobalNodeId::new(HierarchyLevel::Local, 0, 0, 1),
            ],
            total_distance_m: 100.0,
            total_time_s: 10.0,
            nodes_explored: 5,
            levels_used: vec![HierarchyLevel::Local],
        };

        assert_eq!(route.path.len(), 2);
        assert_eq!(route.levels_used.len(), 1);
    }

    #[test]
    fn test_create_test_tile_structure() {
        // Verify the test tile is correctly structured
        let tile = create_test_tile();

        assert_eq!(tile.nodes.len(), 9); // 3x3 grid
        assert_eq!(tile.level, HierarchyLevel::Local);
        assert_eq!(tile.tile_x, 0);
        assert_eq!(tile.tile_y, 0);

        // Check that edges exist
        assert!(!tile.edges.is_empty());

        // Node 0 should have edges to nodes 1 and 3
        let node0 = &tile.nodes[0];
        assert!(node0.edge_count > 0);

        // Center node (4) should have 4 neighbors
        let node4 = &tile.nodes[4];
        assert_eq!(node4.edge_count, 4);
    }

    #[test]
    fn test_search_entry_equality() {
        let e1 = SearchEntry {
            node: GlobalNodeId::new(HierarchyLevel::Local, 0, 0, 0),
            g_cost: 10,
            f_cost: 100,
        };
        let e2 = SearchEntry {
            node: GlobalNodeId::new(HierarchyLevel::Local, 0, 0, 0),
            g_cost: 10,
            f_cost: 100,
        };
        let e3 = SearchEntry {
            node: GlobalNodeId::new(HierarchyLevel::Local, 0, 0, 0),
            g_cost: 20,
            f_cost: 100, // Same f_cost, different g_cost
        };

        assert_eq!(e1, e2);
        assert_eq!(e1, e3); // Equality only checks f_cost
    }
}
