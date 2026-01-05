//! High-level router combining cache and algorithm.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use crate::algorithm::RoutingResult;
use crate::cache::{CacheConfig, TileCache};
use crate::geo::haversine_distance;
use crate::tile::{TileLoader, TileLoaderError};
use crate::types::{lat_lon_to_tile, HierarchyLevel, RoutingTile};

/// A route with geometry and instructions.
#[derive(Debug, Clone)]
pub struct Route {
    /// Path as lat/lon coordinates.
    pub path: Vec<(f64, f64)>,
    /// Route segments with road names and instructions.
    pub segments: Vec<RouteSegment>,
    /// Total distance in meters.
    pub total_distance_m: f64,
    /// Total time in seconds.
    pub total_time_s: f64,
}

/// A segment of a route.
#[derive(Debug, Clone)]
pub struct RouteSegment {
    /// Start index into the path.
    pub start_idx: usize,
    /// End index into the path.
    pub end_idx: usize,
    /// Road name (if available).
    pub road_name: Option<String>,
    /// Turn instruction at the start of this segment.
    pub instruction: TurnInstruction,
    /// Distance of this segment in meters.
    pub distance_m: f64,
}

/// Turn instruction for navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnInstruction {
    /// Start of the route.
    Start,
    /// Continue straight.
    Continue,
    /// Turn left.
    TurnLeft,
    /// Turn right.
    TurnRight,
    /// Slight left.
    SlightLeft,
    /// Slight right.
    SlightRight,
    /// Sharp left.
    SharpLeft,
    /// Sharp right.
    SharpRight,
    /// U-turn.
    UTurn,
    /// Arrive at destination.
    Arrive,
}

/// High-level router with tile caching.
pub struct Router {
    cache: TileCache,
}

impl Router {
    /// Create a new router from a tile directory.
    pub fn new(tile_dir: &str) -> Result<Self, TileLoaderError> {
        let loader = TileLoader::new(tile_dir)?;
        let cache = TileCache::new(loader, CacheConfig::default());
        Ok(Self { cache })
    }

    /// Create a router with custom cache configuration.
    pub fn with_config(tile_dir: &str, config: CacheConfig) -> Result<Self, TileLoaderError> {
        let loader = TileLoader::new(tile_dir)?;
        let cache = TileCache::new(loader, config);
        Ok(Self { cache })
    }

    /// Initialize the router (load highway graph).
    pub fn initialize(&mut self) -> Result<(), TileLoaderError> {
        self.cache.initialize()
    }

    /// Find a route between two points.
    ///
    /// Automatically selects the appropriate routing strategy based on distance:
    /// - < 5km: Local tiles only (fastest for short routes)
    /// - 5-50km: Arterial level (regional routing)
    /// - > 50km: Highway backbone (long-distance routing)
    pub fn find_route(
        &mut self,
        start_lat: f64,
        start_lon: f64,
        end_lat: f64,
        end_lon: f64,
    ) -> Option<Route> {
        // Determine which hierarchy level to use based on distance
        let distance = haversine_distance(start_lat, start_lon, end_lat, end_lon);

        log::debug!("Routing {:.1}km distance", distance / 1000.0);

        if distance < 5_000.0 {
            // Short route: try optimized single-tile routing first
            if let Some(route) = self.find_local_route(start_lat, start_lon, end_lat, end_lon) {
                return Some(route);
            }
            log::debug!("Local routing failed, trying hierarchical routing as fallback");
        }

        // Medium/long route or fallback: use hierarchical routing
        self.find_hierarchical_route(start_lat, start_lon, end_lat, end_lon)
    }

    /// Find a route using the hierarchical router.
    fn find_hierarchical_route(
        &mut self,
        start_lat: f64,
        start_lon: f64,
        end_lat: f64,
        end_lon: f64,
    ) -> Option<Route> {
        use crate::hierarchy::HierarchicalRouter;

        let mut hier_router = HierarchicalRouter::new(&mut self.cache);
        let result = hier_router.find_route(start_lat, start_lon, end_lat, end_lon)?;

        log::debug!(
            "Hierarchical route found: {:.1}m, {:.1}s, {} nodes, levels: {:?}",
            result.total_distance_m,
            result.total_time_s,
            result.nodes_explored,
            result.levels_used
        );

        // Convert path to coordinates
        let path = hier_router.path_to_coordinates(&result.path);

        // Build route segments (simplified - no road names for hierarchical routes yet)
        let segments = if path.len() > 1 {
            vec![RouteSegment {
                start_idx: 0,
                end_idx: path.len() - 1,
                road_name: None,
                instruction: TurnInstruction::Start,
                distance_m: result.total_distance_m,
            }]
        } else {
            Vec::new()
        };

        Some(Route {
            path,
            segments,
            total_distance_m: result.total_distance_m,
            total_time_s: result.total_time_s,
        })
    }

    /// Find a route using local tiles only.
    fn find_local_route(
        &mut self,
        start_lat: f64,
        start_lon: f64,
        end_lat: f64,
        end_lon: f64,
    ) -> Option<Route> {
        let level = HierarchyLevel::Local;

        // Find tiles needed
        let (start_tx, start_ty) = lat_lon_to_tile(start_lat, start_lon, level);
        let (end_tx, end_ty) = lat_lon_to_tile(end_lat, end_lon, level);

        // Load tiles around start and end
        self.cache.prefetch(start_lat, start_lon, level, 1);
        self.cache.prefetch(end_lat, end_lon, level, 1);

        // If start and end are in the same tile, use single-tile routing
        if start_tx == end_tx && start_ty == end_ty {
            return self.find_single_tile_route(level, start_tx, start_ty, start_lat, start_lon, end_lat, end_lon);
        }

        // For cross-tile routing, we need to implement a more complex algorithm
        // For now, try to find a route through adjacent tiles
        log::debug!(
            "Cross-tile routing from ({}, {}) to ({}, {})",
            start_tx,
            start_ty,
            end_tx,
            end_ty
        );

        // Simple approach: try single-tile first with end tile
        // This is a placeholder - full cross-tile routing requires graph merging
        self.find_single_tile_route(level, end_tx, end_ty, start_lat, start_lon, end_lat, end_lon)
    }

    /// Find a route within a single tile.
    fn find_single_tile_route(
        &mut self,
        level: HierarchyLevel,
        tile_x: i32,
        tile_y: i32,
        start_lat: f64,
        start_lon: f64,
        end_lat: f64,
        end_lon: f64,
    ) -> Option<Route> {
        // First pass: find nodes and run A*
        let (result, path_coords, names) = {
            let tile = self.cache.get(level, tile_x, tile_y)?;

            // Find nearest nodes
            let start_idx = find_nearest_in_tile(tile, start_lat, start_lon)?;
            let end_idx = find_nearest_in_tile(tile, end_lat, end_lon)?;

            log::debug!(
                "Routing from node {} to {} in tile ({}, {})",
                start_idx,
                end_idx,
                tile_x,
                tile_y
            );

            // Run A*
            let router = crate::algorithm::SingleTileRouter::new(tile);
            let result = router.find_path(start_idx, end_idx)?;

            log::debug!(
                "Found route: {:.1}m, {:.1}s, {} nodes explored",
                result.total_distance_m,
                result.total_time_s,
                result.nodes_explored
            );

            // Extract path coordinates and names while we have the tile
            let mut path_coords = Vec::new();
            let mut names = Vec::new();

            for (i, node_ref) in result.path.iter().enumerate() {
                if let Some(node) = tile.node(node_ref.node_idx) {
                    path_coords.push(node.position());

                    // Get road name for edge
                    if i > 0 {
                        let prev_ref = &result.path[i - 1];
                        let name = get_road_name_from_tile(tile, prev_ref.node_idx, node_ref.node_idx);
                        names.push(name);
                    }
                }
            }

            (result, path_coords, names)
        };

        // Build route from extracted data
        Some(build_route(result, path_coords, names))
    }

    /// Snap a GPS position to the nearest road.
    pub fn snap_to_road(&mut self, lat: f64, lon: f64) -> Option<(f64, f64, f64)> {
        use crate::geo::haversine_distance_micro;

        let level = HierarchyLevel::Local;
        let (tile_x, tile_y) = lat_lon_to_tile(lat, lon, level);
        let lat_micro = (lat * 1_000_000.0) as i32;
        let lon_micro = (lon * 1_000_000.0) as i32;

        // Load surrounding tiles
        self.cache.prefetch(lat, lon, level, 1);

        // Find nearest node across all nearby tiles
        let mut best: Option<(f64, f64, f64)> = None;

        for dy in -1..=1i32 {
            for dx in -1..=1i32 {
                if let Some(tile) = self.cache.get(level, tile_x + dx, tile_y + dy) {
                    for node in tile.nodes.iter() {
                        let distance =
                            haversine_distance_micro(lat_micro, lon_micro, node.lat, node.lon);
                        let is_better = match &best {
                            Some((_, _, best_dist)) => distance < *best_dist,
                            None => true,
                        };
                        if is_better {
                            best = Some((node.lat_deg(), node.lon_deg(), distance));
                        }
                    }
                }
            }
        }

        best
    }

    /// Get cache statistics.
    pub fn cache_stats(&self) -> &crate::cache::CacheStats {
        self.cache.stats()
    }
}

/// Find the nearest node in a tile.
fn find_nearest_in_tile(tile: &RoutingTile, lat: f64, lon: f64) -> Option<u32> {
    use crate::geo::haversine_distance_micro;

    let lat_micro = (lat * 1_000_000.0) as i32;
    let lon_micro = (lon * 1_000_000.0) as i32;

    let mut best_idx = None;
    let mut best_distance = f64::MAX;

    for (idx, node) in tile.nodes.iter().enumerate() {
        let distance = haversine_distance_micro(lat_micro, lon_micro, node.lat, node.lon);
        if distance < best_distance {
            best_distance = distance;
            best_idx = Some(idx as u32);
        }
    }

    best_idx
}

/// Get the road name for an edge in a tile.
fn get_road_name_from_tile(tile: &RoutingTile, from_idx: u32, to_idx: u32) -> Option<String> {
    for edge in tile.edges_for_node(from_idx) {
        if edge.target == to_idx {
            return tile.name(edge.name_id).map(|s| s.to_string());
        }
    }
    None
}

/// Build a Route from extracted data.
fn build_route(
    result: RoutingResult,
    path: Vec<(f64, f64)>,
    names: Vec<Option<String>>,
) -> Route {
    let mut segments = Vec::new();
    let path_len = path.len();

    for (i, name) in names.into_iter().enumerate() {
        let instruction = if i == 0 {
            TurnInstruction::Start
        } else if i == path_len - 2 {
            TurnInstruction::Arrive
        } else {
            TurnInstruction::Continue
        };

        segments.push(RouteSegment {
            start_idx: i,
            end_idx: i + 1,
            road_name: name,
            instruction,
            distance_m: 0.0, // TODO: Calculate per-segment distance
        });
    }

    Route {
        path,
        segments,
        total_distance_m: result.total_distance_m,
        total_time_s: result.total_time_s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_turn_instruction() {
        assert_eq!(TurnInstruction::Start, TurnInstruction::Start);
        assert_ne!(TurnInstruction::Start, TurnInstruction::Arrive);
    }
}
