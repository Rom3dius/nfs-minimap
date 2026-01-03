//! Road graph construction from tile data

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use minimap_tiles::{LaneTurn, Tile, TileRoad};

use crate::pathfind;
use crate::route::Route;

/// A node in the road graph (intersection or road endpoint)
#[derive(Debug, Clone)]
pub struct GraphNode {
    /// OSM node ID
    pub node_id: u64,
    /// Latitude
    pub lat: f64,
    /// Longitude
    pub lon: f64,
}

/// An edge in the road graph (road segment)
#[derive(Debug, Clone)]
pub struct GraphEdge {
    /// Index of source node in graph
    pub from_idx: usize,
    /// Index of destination node in graph
    pub to_idx: usize,
    /// OSM way ID
    pub way_id: u64,
    /// Distance in meters
    pub distance_m: f32,
    /// Speed class (0-4)
    pub speed_class: u8,
    /// Direction: -1=reverse, 0=both, 1=forward
    pub oneway: i8,
    /// Lane count
    pub lane_count: u8,
    /// Turn lanes at end of segment
    pub turn_lanes: Option<Vec<LaneTurn>>,
    /// Destination signs
    pub dest_lanes: Option<Vec<alloc::string::String>>,
    /// Geometry points for rendering (lat, lon)
    pub geometry: Vec<(f64, f64)>,
}

/// Road graph built from tiles
pub struct RoadGraph {
    /// All nodes in the graph
    nodes: Vec<GraphNode>,
    /// Map from node_id to index in nodes vec
    node_index: BTreeMap<u64, usize>,
    /// Adjacency list: for each node index, list of outgoing edge indices
    adjacency: Vec<Vec<usize>>,
    /// All edges
    edges: Vec<GraphEdge>,
}

impl RoadGraph {
    /// Create an empty graph
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            node_index: BTreeMap::new(),
            adjacency: Vec::new(),
            edges: Vec::new(),
        }
    }

    /// Build graph from loaded tiles (owned slice)
    pub fn from_tiles(tiles: &[Tile]) -> Self {
        let refs: Vec<&Tile> = tiles.iter().collect();
        Self::from_tile_refs(&refs)
    }

    /// Build graph from tile references
    pub fn from_tile_refs(tiles: &[&Tile]) -> Self {
        let mut graph = Self::new();

        // Count roads with valid node IDs for debugging
        let mut total_roads = 0;
        let mut roads_with_nodes = 0;

        // First pass: collect all unique nodes from road endpoints
        for tile in tiles {
            for road in &tile.roads {
                total_roads += 1;
                if road.start_node != 0 && road.end_node != 0 {
                    roads_with_nodes += 1;
                }
                if road.start_node != 0 {
                    graph.get_or_create_node(road.start_node, tile, road, true);
                }
                if road.end_node != 0 {
                    graph.get_or_create_node(road.end_node, tile, road, false);
                }
            }
        }

        log::info!(
            "Graph building: {}/{} roads have valid node IDs",
            roads_with_nodes,
            total_roads
        );

        // Initialize adjacency lists
        graph.adjacency.resize(graph.nodes.len(), Vec::new());

        // Second pass: create edges
        for tile in tiles {
            for road in &tile.roads {
                graph.add_road_edge(tile, road);
            }
        }

        graph
    }

    /// Get or create a node for the given node_id
    fn get_or_create_node(&mut self, node_id: u64, tile: &Tile, road: &TileRoad, is_start: bool) -> usize {
        if let Some(&idx) = self.node_index.get(&node_id) {
            return idx;
        }

        // Calculate position from tile offset
        let (ox, oy) = if is_start {
            road.points.first().copied().unwrap_or((0, 0))
        } else {
            road.points.last().copied().unwrap_or((0, 0))
        };

        let (lat, lon) = minimap_tiles::tile_offset_to_lat_lon(ox, oy, tile.tile_x, tile.tile_y);

        let idx = self.nodes.len();
        self.nodes.push(GraphNode { node_id, lat, lon });
        self.node_index.insert(node_id, idx);
        idx
    }

    /// Add edges for a road segment
    fn add_road_edge(&mut self, tile: &Tile, road: &TileRoad) {
        if road.start_node == 0 || road.end_node == 0 {
            return; // Skip roads without node IDs
        }

        let from_idx = match self.node_index.get(&road.start_node) {
            Some(&idx) => idx,
            None => return,
        };
        let to_idx = match self.node_index.get(&road.end_node) {
            Some(&idx) => idx,
            None => return,
        };

        // Convert points to lat/lon geometry
        let geometry: Vec<(f64, f64)> = road
            .points
            .iter()
            .map(|(ox, oy)| minimap_tiles::tile_offset_to_lat_lon(*ox, *oy, tile.tile_x, tile.tile_y))
            .collect();

        // Calculate distance
        let distance_m = calculate_path_distance(&geometry);

        let edge = GraphEdge {
            from_idx,
            to_idx,
            way_id: road.way_id,
            distance_m,
            speed_class: road.speed_class,
            oneway: road.oneway,
            lane_count: road.lane_count,
            turn_lanes: road.turn_lanes.clone(),
            dest_lanes: road.dest_lanes.clone(),
            geometry,
        };

        let edge_idx = self.edges.len();
        self.edges.push(edge);

        // Add to adjacency list based on oneway
        match road.oneway {
            1 => {
                // Forward only
                self.adjacency[from_idx].push(edge_idx);
            }
            -1 => {
                // Reverse only
                self.adjacency[to_idx].push(edge_idx);
            }
            _ => {
                // Both directions
                self.adjacency[from_idx].push(edge_idx);
                self.adjacency[to_idx].push(edge_idx);
            }
        }
    }

    /// Number of nodes in the graph
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Number of edges in the graph
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Get a node by index
    pub fn node(&self, idx: usize) -> Option<&GraphNode> {
        self.nodes.get(idx)
    }

    /// Get an edge by index
    pub fn edge(&self, idx: usize) -> Option<&GraphEdge> {
        self.edges.get(idx)
    }

    /// Get outgoing edge indices for a node
    pub fn outgoing_edges(&self, node_idx: usize) -> &[usize] {
        self.adjacency.get(node_idx).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Find the nearest node to a position within max_distance
    pub fn find_nearest_node(&self, lat: f64, lon: f64, max_distance_m: f32) -> Option<usize> {
        let mut best_idx = None;
        let mut best_dist = max_distance_m;

        for (idx, node) in self.nodes.iter().enumerate() {
            let dist = haversine_distance(lat, lon, node.lat, node.lon);
            if dist < best_dist {
                best_dist = dist;
                best_idx = Some(idx);
            }
        }

        best_idx
    }

    /// Find route between two positions
    pub fn find_route(
        &self,
        from: (f64, f64),
        to: (f64, f64),
        snap_distance_m: f32,
        max_nodes: usize,
    ) -> Option<Route> {
        log::debug!(
            "find_route: from=({:.4}, {:.4}) to=({:.4}, {:.4}), snap_dist={}m, graph has {} nodes",
            from.0, from.1, to.0, to.1, snap_distance_m, self.nodes.len()
        );

        let start_idx = self.find_nearest_node(from.0, from.1, snap_distance_m);
        let end_idx = self.find_nearest_node(to.0, to.1, snap_distance_m);

        log::debug!(
            "find_route: start_idx={:?}, end_idx={:?}",
            start_idx, end_idx
        );

        let start_idx = start_idx?;
        let end_idx = end_idx?;

        pathfind::astar(self, start_idx, end_idx, max_nodes)
    }

    /// Check if an edge can be traversed from the given direction
    pub fn can_traverse(&self, edge_idx: usize, from_node_idx: usize) -> bool {
        if let Some(edge) = self.edges.get(edge_idx) {
            match edge.oneway {
                1 => edge.from_idx == from_node_idx,   // Forward only
                -1 => edge.to_idx == from_node_idx,    // Reverse only
                _ => true,                              // Both directions
            }
        } else {
            false
        }
    }

    /// Get the destination node index when traversing an edge from a given node
    pub fn edge_destination(&self, edge_idx: usize, from_node_idx: usize) -> Option<usize> {
        let edge = self.edges.get(edge_idx)?;
        if edge.from_idx == from_node_idx {
            Some(edge.to_idx)
        } else if edge.to_idx == from_node_idx {
            Some(edge.from_idx)
        } else {
            None
        }
    }

    /// Get the cost to traverse an edge (distance adjusted by speed class)
    pub fn edge_cost(&self, edge_idx: usize) -> f32 {
        if let Some(edge) = self.edges.get(edge_idx) {
            // Cost is distance divided by speed factor
            // Higher speed class = lower cost per meter
            let speed_factor = match edge.speed_class {
                0 => 1.0,  // Unknown - assume slow
                1 => 1.0,  // Slow roads
                2 => 1.5,  // Urban
                3 => 2.0,  // Fast
                4 => 2.5,  // Highway
                _ => 1.0,
            };
            edge.distance_m / speed_factor
        } else {
            f32::MAX
        }
    }
}

impl Default for RoadGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// Calculate haversine distance between two points in meters
pub fn haversine_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f32 {
    const EARTH_RADIUS_M: f64 = 6_371_000.0;

    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();
    let delta_lat = (lat2 - lat1).to_radians();
    let delta_lon = (lon2 - lon1).to_radians();

    let a = (delta_lat / 2.0).sin().powi(2)
        + lat1_rad.cos() * lat2_rad.cos() * (delta_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();

    (EARTH_RADIUS_M * c) as f32
}

/// Calculate total distance of a path
fn calculate_path_distance(points: &[(f64, f64)]) -> f32 {
    let mut total = 0.0;
    for window in points.windows(2) {
        total += haversine_distance(window[0].0, window[0].1, window[1].0, window[1].1);
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_haversine_distance() {
        // Zurich to Bern is approximately 95km
        let dist = haversine_distance(47.3769, 8.5417, 46.9480, 7.4474);
        assert!(dist > 90_000.0 && dist < 100_000.0);
    }

    #[test]
    fn test_empty_graph() {
        let graph = RoadGraph::new();
        assert_eq!(graph.node_count(), 0);
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn test_graph_from_synthetic_tile() {
        // Create a synthetic tile with connected roads
        let mut tile = minimap_tiles::Tile::new(843, 4714);

        // Road 1: A to B
        tile.roads.push(minimap_tiles::TileRoad {
            road_type: minimap_tiles::RoadType::Primary,
            points: vec![(0, 0), (100, 0)],
            way_id: 1,
            start_node: 1000,
            end_node: 1001,
            oneway: 0,
            speed_class: 2,
            lane_count: 2,
            turn_lanes: None,
            dest_lanes: None,
        });

        // Road 2: B to C
        tile.roads.push(minimap_tiles::TileRoad {
            road_type: minimap_tiles::RoadType::Primary,
            points: vec![(100, 0), (200, 0)],
            way_id: 2,
            start_node: 1001,
            end_node: 1002,
            oneway: 0,
            speed_class: 2,
            lane_count: 2,
            turn_lanes: None,
            dest_lanes: None,
        });

        // Road 3: C to D
        tile.roads.push(minimap_tiles::TileRoad {
            road_type: minimap_tiles::RoadType::Primary,
            points: vec![(200, 0), (300, 0)],
            way_id: 3,
            start_node: 1002,
            end_node: 1003,
            oneway: 0,
            speed_class: 2,
            lane_count: 2,
            turn_lanes: None,
            dest_lanes: None,
        });

        let tiles = vec![&tile];
        let graph = RoadGraph::from_tile_refs(&tiles);

        // Should have 4 nodes (A, B, C, D)
        assert_eq!(graph.node_count(), 4);
        // Should have 3 edges
        assert_eq!(graph.edge_count(), 3);

        // Find route from node 0 (A) to node 3 (D)
        let route = crate::pathfind::astar(&graph, 0, 3, 100);
        assert!(route.is_some(), "Should find a route from A to D");

        let route = route.unwrap();
        assert_eq!(route.segments.len(), 3, "Route should have 3 segments");
    }

    #[test]
    #[ignore] // Run with: cargo test --ignored
    fn test_real_tile_routing() {
        // This test loads a real tile and verifies routing works
        let tile_path = std::path::Path::new("../../tiles/tile_843_4714.bin");
        if !tile_path.exists() {
            eprintln!("Skipping test: tile file not found");
            return;
        }

        let data = std::fs::read(tile_path).expect("Failed to read tile");
        let tile = minimap_tiles::Tile::from_bytes(&data).expect("Failed to deserialize");

        println!("Tile {},{} has {} roads", tile.tile_x, tile.tile_y, tile.roads.len());

        // Count roads with valid node IDs
        let roads_with_nodes: Vec<_> = tile.roads.iter()
            .filter(|r| r.start_node != 0 && r.end_node != 0)
            .collect();
        println!("Roads with valid node IDs: {}/{}", roads_with_nodes.len(), tile.roads.len());

        // Print first few roads
        for (i, road) in tile.roads.iter().take(5).enumerate() {
            println!(
                "Road {}: way_id={}, start_node={}, end_node={}, points={}",
                i, road.way_id, road.start_node, road.end_node, road.points.len()
            );
        }

        let tiles = vec![&tile];
        let graph = RoadGraph::from_tile_refs(&tiles);

        println!("Graph: {} nodes, {} edges", graph.node_count(), graph.edge_count());

        // Print adjacency info
        for i in 0..graph.node_count().min(5) {
            let edges = graph.outgoing_edges(i);
            println!("Node {}: {} outgoing edges", i, edges.len());
        }

        // Try to find a route between the first two nodes
        if graph.node_count() >= 2 {
            let route = crate::pathfind::astar(&graph, 0, 1, 100);
            println!("Route from 0 to 1: {:?}", route.as_ref().map(|r| r.segments.len()));
        }

        // Print some node positions to find valid coordinates
        println!("First 5 node positions:");
        for i in 0..graph.node_count().min(5) {
            if let Some(node) = graph.node(i) {
                println!("  Node {}: ({:.6}, {:.6})", i, node.lat, node.lon);
            }
        }

        // Test find_route using actual node coordinates
        if graph.node_count() >= 2 {
            let node0 = graph.node(0).unwrap();
            let node1 = graph.node(1).unwrap();
            let from = (node0.lat, node0.lon);
            let to = (node1.lat, node1.lon);

            println!("Testing route from ({:.6}, {:.6}) to ({:.6}, {:.6})", from.0, from.1, to.0, to.1);
            let route = graph.find_route(from, to, 200.0, 10000);
            println!("Route from lat/lon: {:?}", route.as_ref().map(|r| (r.waypoints.len(), r.segments.len())));
            assert!(route.is_some(), "Should find a route between node positions");
        }
    }
}
