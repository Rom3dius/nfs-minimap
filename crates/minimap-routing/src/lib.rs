//! Routing engine for NFS-style minimap
//!
//! Provides A* pathfinding and lane guidance based on tile data.
//! Supports both std and no_std (ESP32) environments.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod graph;
mod pathfind;
mod route;
mod turn;

pub use graph::{GraphEdge, GraphNode, RoadGraph};
pub use route::{LaneGuidance, LaneInfo, Route, RouteSegment};
pub use turn::TurnType;

// Re-export for convenience
pub use minimap_tiles;
pub use minimap_tiles::LaneTurn;

/// Vehicle state for routing calculations
#[derive(Debug, Clone, Copy)]
pub struct VehiclePosition {
    pub lat: f64,
    pub lon: f64,
    pub heading: f32,
}

/// Configuration for the routing engine
#[derive(Debug, Clone)]
pub struct RoutingConfig {
    /// Maximum distance (meters) to snap start/end points to road network
    pub snap_distance_m: f32,
    /// Distance threshold (meters) for considering vehicle off-route
    pub off_route_threshold_m: f32,
    /// Minimum time (seconds) between re-route calculations
    pub reroute_debounce_s: f32,
    /// Maximum nodes to explore in A* (prevents runaway on large graphs)
    pub max_search_nodes: usize,
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            snap_distance_m: 500.0, // 500m snap radius to handle sparse node coverage
            off_route_threshold_m: 100.0,
            reroute_debounce_s: 5.0,
            max_search_nodes: 10000,
        }
    }
}

/// Main routing engine
pub struct Router {
    config: RoutingConfig,
    graph: RoadGraph,
    active_route: Option<Route>,
    last_reroute_time: f32,
}

impl Router {
    /// Create a new router with the given configuration
    pub fn new(config: RoutingConfig) -> Self {
        Self {
            config,
            graph: RoadGraph::new(),
            active_route: None,
            last_reroute_time: 0.0,
        }
    }

    /// Create a router with default configuration
    pub fn with_defaults() -> Self {
        Self::new(RoutingConfig::default())
    }

    /// Build/rebuild the road graph from loaded tiles
    pub fn build_graph(&mut self, tiles: &[&minimap_tiles::Tile]) {
        self.graph = RoadGraph::from_tile_refs(tiles);
        log::info!(
            "Built road graph: {} nodes, {} edges",
            self.graph.node_count(),
            self.graph.edge_count()
        );
    }

    /// Calculate a route from current position to destination
    pub fn calculate_route(
        &mut self,
        from: VehiclePosition,
        to: (f64, f64),
    ) -> Option<&Route> {
        log::info!(
            "calculate_route: from=({:.4}, {:.4}) to=({:.4}, {:.4}), graph has {} nodes, {} edges",
            from.lat, from.lon, to.0, to.1,
            self.graph.node_count(), self.graph.edge_count()
        );

        let route = self.graph.find_route(
            (from.lat, from.lon),
            to,
            self.config.snap_distance_m,
            self.config.max_search_nodes,
        );

        match &route {
            Some(r) => log::info!("Route found: {} waypoints, {:.0}m", r.waypoints.len(), r.total_distance_m),
            None => log::info!("No route found"),
        }

        self.active_route = route;
        self.active_route.as_ref()
    }

    /// Clear the current route
    pub fn clear_route(&mut self) {
        self.active_route = None;
    }

    /// Get the active route, if any
    pub fn active_route(&self) -> Option<&Route> {
        self.active_route.as_ref()
    }

    /// Check if vehicle is off-route and needs re-routing
    /// Returns true if a re-route is needed
    pub fn check_off_route(&self, vehicle: VehiclePosition) -> bool {
        if let Some(route) = &self.active_route {
            let distance = route.distance_to_route(vehicle.lat, vehicle.lon);
            distance > self.config.off_route_threshold_m
        } else {
            false
        }
    }

    /// Update routing state with current time (for debouncing)
    /// Returns true if re-routing is allowed (debounce period passed)
    pub fn can_reroute(&self, current_time_s: f32) -> bool {
        current_time_s - self.last_reroute_time >= self.config.reroute_debounce_s
    }

    /// Mark that a re-route was performed
    pub fn mark_rerouted(&mut self, current_time_s: f32) {
        self.last_reroute_time = current_time_s;
    }

    /// Get lane guidance for the next upcoming junction
    /// Returns None if no route is active or no junction is upcoming
    pub fn get_lane_guidance(
        &self,
        vehicle: VehiclePosition,
    ) -> Option<(LaneGuidance, f32)> {
        let route = self.active_route.as_ref()?;
        route.get_lane_guidance(vehicle.lat, vehicle.lon)
    }

    /// Get the next maneuver distance and type
    pub fn get_next_maneuver(&self, vehicle: VehiclePosition) -> Option<(TurnType, f32)> {
        let route = self.active_route.as_ref()?;
        route.get_next_maneuver(vehicle.lat, vehicle.lon)
    }

    /// Get the remaining distance to destination in meters
    pub fn remaining_distance(&self, vehicle: VehiclePosition) -> Option<f32> {
        let route = self.active_route.as_ref()?;
        Some(route.remaining_distance(vehicle.lat, vehicle.lon))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = RoutingConfig::default();
        assert_eq!(config.snap_distance_m, 500.0);
        assert_eq!(config.off_route_threshold_m, 100.0);
    }

    #[test]
    fn test_router_creation() {
        let router = Router::with_defaults();
        assert!(router.active_route().is_none());
    }
}
