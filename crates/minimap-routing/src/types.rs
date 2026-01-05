//! Core routing data structures.
//!
//! These types are designed to be compact for embedded use:
//! - RoutingNode: 16 bytes (with alignment)
//! - RoutingEdge: 12 bytes

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use serde::{Deserialize, Serialize};

/// Routing hierarchy levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum HierarchyLevel {
    /// Highway level: motorway, trunk, primary roads
    /// Tile size: 1.0 degree (~111 km)
    Highway = 0,
    /// Arterial level: secondary, tertiary roads
    /// Tile size: 0.1 degree (~11 km)
    Arterial = 1,
    /// Local level: residential, service, other roads
    /// Tile size: 0.01 degree (~1.1 km)
    Local = 2,
}

impl HierarchyLevel {
    /// Get the tile size in degrees for this hierarchy level.
    pub fn tile_size_deg(&self) -> f64 {
        match self {
            HierarchyLevel::Highway => 1.0,
            HierarchyLevel::Arterial => 0.1,
            HierarchyLevel::Local => 0.01,
        }
    }

    /// Get the approximate tile size in kilometers.
    pub fn tile_size_km(&self) -> f64 {
        match self {
            HierarchyLevel::Highway => 111.0,
            HierarchyLevel::Arterial => 11.1,
            HierarchyLevel::Local => 1.1,
        }
    }
}

/// Road classification for routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum RoadClass {
    Motorway = 0,
    Trunk = 1,
    Primary = 2,
    Secondary = 3,
    Tertiary = 4,
    Residential = 5,
    Service = 6,
    Other = 7,
}

impl RoadClass {
    /// Get the hierarchy level this road class belongs to.
    pub fn hierarchy_level(&self) -> HierarchyLevel {
        match self {
            RoadClass::Motorway | RoadClass::Trunk | RoadClass::Primary => HierarchyLevel::Highway,
            RoadClass::Secondary | RoadClass::Tertiary => HierarchyLevel::Arterial,
            RoadClass::Residential | RoadClass::Service | RoadClass::Other => HierarchyLevel::Local,
        }
    }

    /// Get default speed in km/h for this road class.
    pub fn default_speed_kmh(&self) -> u8 {
        match self {
            RoadClass::Motorway => 120,
            RoadClass::Trunk => 100,
            RoadClass::Primary => 80,
            RoadClass::Secondary => 60,
            RoadClass::Tertiary => 50,
            RoadClass::Residential => 30,
            RoadClass::Service => 20,
            RoadClass::Other => 30,
        }
    }
}

/// Flags for routing nodes.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct NodeFlags(pub u8);

impl NodeFlags {
    pub const BOUNDARY: u8 = 0b0000_0001;
    pub const TRAFFIC_SIGNAL: u8 = 0b0000_0010;
    pub const BARRIER: u8 = 0b0000_0100;

    pub fn is_boundary(&self) -> bool {
        self.0 & Self::BOUNDARY != 0
    }

    pub fn is_traffic_signal(&self) -> bool {
        self.0 & Self::TRAFFIC_SIGNAL != 0
    }

    pub fn is_barrier(&self) -> bool {
        self.0 & Self::BARRIER != 0
    }

    pub fn set_boundary(&mut self, value: bool) {
        if value {
            self.0 |= Self::BOUNDARY;
        } else {
            self.0 &= !Self::BOUNDARY;
        }
    }
}

/// Flags for routing edges.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct EdgeFlags(pub u8);

impl EdgeFlags {
    pub const ONEWAY: u8 = 0b0000_0001;
    pub const TUNNEL: u8 = 0b0000_0010;
    pub const BRIDGE: u8 = 0b0000_0100;
    pub const TOLL: u8 = 0b0000_1000;

    pub fn is_oneway(&self) -> bool {
        self.0 & Self::ONEWAY != 0
    }

    pub fn is_tunnel(&self) -> bool {
        self.0 & Self::TUNNEL != 0
    }

    pub fn is_bridge(&self) -> bool {
        self.0 & Self::BRIDGE != 0
    }

    pub fn is_toll(&self) -> bool {
        self.0 & Self::TOLL != 0
    }

    pub fn set_oneway(&mut self, value: bool) {
        if value {
            self.0 |= Self::ONEWAY;
        } else {
            self.0 &= !Self::ONEWAY;
        }
    }
}

/// A routing node in the graph.
///
/// Size: 16 bytes (with alignment padding)
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RoutingNode {
    /// Latitude in microdegrees (value / 1_000_000 = degrees).
    pub lat: i32,
    /// Longitude in microdegrees (value / 1_000_000 = degrees).
    pub lon: i32,
    /// Index of first outgoing edge in the edge array.
    pub first_edge: u32,
    /// Number of outgoing edges (max 255).
    pub edge_count: u8,
    /// Node flags (boundary, traffic signal, etc.).
    pub flags: NodeFlags,
}

impl RoutingNode {
    /// Create a new routing node.
    pub fn new(lat: f64, lon: f64) -> Self {
        Self {
            lat: (lat * 1_000_000.0) as i32,
            lon: (lon * 1_000_000.0) as i32,
            first_edge: 0,
            edge_count: 0,
            flags: NodeFlags::default(),
        }
    }

    /// Get latitude as f64 degrees.
    pub fn lat_deg(&self) -> f64 {
        self.lat as f64 / 1_000_000.0
    }

    /// Get longitude as f64 degrees.
    pub fn lon_deg(&self) -> f64 {
        self.lon as f64 / 1_000_000.0
    }

    /// Get position as (lat, lon) tuple.
    pub fn position(&self) -> (f64, f64) {
        (self.lat_deg(), self.lon_deg())
    }
}

/// A routing edge in the graph.
///
/// Size: 12 bytes (packed)
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RoutingEdge {
    /// Target node index (within the same tile).
    pub target: u32,
    /// Distance in centimeters (max ~42 km per edge).
    pub distance_cm: u32,
    /// Speed limit in km/h.
    pub speed_kmh: u8,
    /// Road classification.
    pub road_class: RoadClass,
    /// Edge flags (oneway, tunnel, etc.).
    pub flags: EdgeFlags,
    /// Name index (0 = unnamed, references name table in tile).
    pub name_id: u8,
}

/// A cross-tile edge referencing a node in another tile.
///
/// Used for edges that cross tile boundaries with canonical tile ownership.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CrossTileEdge {
    /// Source node index (within this tile).
    pub source: u32,
    /// Target tile X coordinate.
    pub target_tile_x: i32,
    /// Target tile Y coordinate.
    pub target_tile_y: i32,
    /// Target node index within the target tile.
    pub target_node: u32,
    /// Distance in centimeters.
    pub distance_cm: u32,
    /// Speed limit in km/h.
    pub speed_kmh: u8,
    /// Road classification.
    pub road_class: RoadClass,
    /// Edge flags (oneway, tunnel, etc.).
    pub flags: EdgeFlags,
    /// Name index (0 = unnamed, references name table in tile).
    pub name_id: u8,
}

impl CrossTileEdge {
    /// Create a new cross-tile edge.
    pub fn new(
        source: u32,
        target_tile_x: i32,
        target_tile_y: i32,
        target_node: u32,
        distance_m: f64,
        road_class: RoadClass,
    ) -> Self {
        Self {
            source,
            target_tile_x,
            target_tile_y,
            target_node,
            distance_cm: (distance_m * 100.0) as u32,
            speed_kmh: road_class.default_speed_kmh(),
            road_class,
            flags: EdgeFlags::default(),
            name_id: 0,
        }
    }

    /// Get distance in meters.
    pub fn distance_m(&self) -> f64 {
        self.distance_cm as f64 / 100.0
    }

    /// Calculate travel time in seconds.
    pub fn travel_time_s(&self) -> f64 {
        let distance_km = self.distance_cm as f64 / 100_000.0;
        let speed_kmh = self.speed_kmh as f64;
        if speed_kmh > 0.0 {
            distance_km / speed_kmh * 3600.0
        } else {
            f64::INFINITY
        }
    }

    /// Get the weight for routing (time in centiseconds).
    pub fn weight(&self) -> u32 {
        (self.travel_time_s() * 100.0) as u32
    }
}

impl RoutingEdge {
    /// Create a new routing edge.
    pub fn new(target: u32, distance_m: f64, road_class: RoadClass) -> Self {
        Self {
            target,
            distance_cm: (distance_m * 100.0) as u32,
            speed_kmh: road_class.default_speed_kmh(),
            road_class,
            flags: EdgeFlags::default(),
            name_id: 0,
        }
    }

    /// Get distance in meters.
    pub fn distance_m(&self) -> f64 {
        self.distance_cm as f64 / 100.0
    }

    /// Calculate travel time in seconds.
    pub fn travel_time_s(&self) -> f64 {
        let distance_km = self.distance_cm as f64 / 100_000.0;
        let speed_kmh = self.speed_kmh as f64;
        if speed_kmh > 0.0 {
            distance_km / speed_kmh * 3600.0
        } else {
            f64::INFINITY
        }
    }

    /// Get the weight for routing (time in centiseconds for now).
    pub fn weight(&self) -> u32 {
        (self.travel_time_s() * 100.0) as u32
    }
}

/// V1 routing tile format (for backwards compatibility).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RoutingTileV1 {
    level: HierarchyLevel,
    tile_x: i32,
    tile_y: i32,
    nodes: Vec<RoutingNode>,
    edges: Vec<RoutingEdge>,
    boundary_nodes: Vec<u16>,
    names: Vec<String>,
}

impl From<RoutingTileV1> for RoutingTile {
    fn from(v1: RoutingTileV1) -> Self {
        Self {
            level: v1.level,
            tile_x: v1.tile_x,
            tile_y: v1.tile_y,
            nodes: v1.nodes,
            edges: v1.edges,
            cross_tile_edges: Vec::new(), // V1 had no cross-tile edges
            boundary_nodes: v1.boundary_nodes,
            names: v1.names,
        }
    }
}

/// A routing tile containing nodes and edges for a region.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingTile {
    /// Hierarchy level of this tile.
    pub level: HierarchyLevel,
    /// Tile X coordinate (lon / tile_size, floored).
    pub tile_x: i32,
    /// Tile Y coordinate (lat / tile_size, floored).
    pub tile_y: i32,
    /// All nodes in this tile (each node belongs to exactly one canonical tile).
    pub nodes: Vec<RoutingNode>,
    /// All edges within this tile (source and target nodes both in this tile).
    pub edges: Vec<RoutingEdge>,
    /// Edges that cross tile boundaries (source in this tile, target in another).
    #[serde(default)]
    pub cross_tile_edges: Vec<CrossTileEdge>,
    /// Indices of boundary nodes (nodes at tile edges).
    pub boundary_nodes: Vec<u16>,
    /// Road names referenced by edges (indexed by name_id - 1).
    pub names: Vec<String>,
}

impl RoutingTile {
    /// Magic bytes for routing tile format.
    pub const MAGIC: &'static [u8; 4] = b"RTIL";
    /// Current format version (2 = canonical tile ownership with cross-tile edges).
    pub const VERSION: u8 = 2;

    /// Create a new empty routing tile.
    pub fn new(level: HierarchyLevel, tile_x: i32, tile_y: i32) -> Self {
        Self {
            level,
            tile_x,
            tile_y,
            nodes: Vec::new(),
            edges: Vec::new(),
            cross_tile_edges: Vec::new(),
            boundary_nodes: Vec::new(),
            names: Vec::new(),
        }
    }

    /// Get the southwest corner of this tile in degrees.
    pub fn origin(&self) -> (f64, f64) {
        let size = self.level.tile_size_deg();
        let lat = self.tile_y as f64 * size;
        let lon = self.tile_x as f64 * size;
        (lat, lon)
    }

    /// Serialize the tile to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(Self::MAGIC);
        data.push(Self::VERSION);
        data.extend_from_slice(&bincode::serialize(self).expect("serialization failed"));
        data
    }

    /// Deserialize a tile from bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, TileError> {
        if data.len() < 5 {
            return Err(TileError::TooShort);
        }
        if &data[0..4] != Self::MAGIC {
            return Err(TileError::InvalidMagic);
        }
        let version = data[4];

        match version {
            1 => {
                // V1 format: no cross_tile_edges field
                let v1: RoutingTileV1 = bincode::deserialize(&data[5..])
                    .map_err(|e| TileError::DeserializationFailed(e.to_string()))?;
                Ok(v1.into())
            }
            2 => {
                // V2 format: has cross_tile_edges
                bincode::deserialize(&data[5..])
                    .map_err(|e| TileError::DeserializationFailed(e.to_string()))
            }
            _ => Err(TileError::UnsupportedVersion(version)),
        }
    }

    /// Get a node by index.
    pub fn node(&self, index: u32) -> Option<&RoutingNode> {
        self.nodes.get(index as usize)
    }

    /// Get edges for a node (within this tile).
    pub fn edges_for_node(&self, node_idx: u32) -> &[RoutingEdge] {
        if let Some(node) = self.node(node_idx) {
            let start = node.first_edge as usize;
            let end = start + node.edge_count as usize;
            &self.edges[start..end.min(self.edges.len())]
        } else {
            &[]
        }
    }

    /// Get cross-tile edges for a node (edges going to other tiles).
    pub fn cross_tile_edges_for_node(&self, node_idx: u32) -> impl Iterator<Item = &CrossTileEdge> {
        self.cross_tile_edges
            .iter()
            .filter(move |e| e.source == node_idx)
    }

    /// Get a road name by index.
    pub fn name(&self, name_id: u8) -> Option<&str> {
        if name_id == 0 {
            None
        } else {
            self.names.get((name_id - 1) as usize).map(|s| s.as_str())
        }
    }
}

/// Errors that can occur when loading tiles.
#[derive(Debug, Clone)]
pub enum TileError {
    TooShort,
    InvalidMagic,
    UnsupportedVersion(u8),
    DeserializationFailed(String),
}

impl fmt::Display for TileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TileError::TooShort => write!(f, "tile data too short"),
            TileError::InvalidMagic => write!(f, "invalid magic bytes"),
            TileError::UnsupportedVersion(v) => write!(f, "unsupported version: {}", v),
            TileError::DeserializationFailed(e) => write!(f, "deserialization failed: {}", e),
        }
    }
}

#[cfg(feature = "loader")]
impl std::error::Error for TileError {}

/// Tile coordinate helper functions.
pub fn lat_lon_to_tile(lat: f64, lon: f64, level: HierarchyLevel) -> (i32, i32) {
    let size = level.tile_size_deg();
    let tile_x = (lon / size).floor() as i32;
    let tile_y = (lat / size).floor() as i32;
    (tile_x, tile_y)
}

/// Get the visible tiles around a position.
pub fn get_visible_tiles(lat: f64, lon: f64, radius_deg: f64, level: HierarchyLevel) -> Vec<(i32, i32)> {
    let size = level.tile_size_deg();
    let min_x = ((lon - radius_deg) / size).floor() as i32;
    let max_x = ((lon + radius_deg) / size).floor() as i32;
    let min_y = ((lat - radius_deg) / size).floor() as i32;
    let max_y = ((lat + radius_deg) / size).floor() as i32;

    let mut tiles = Vec::new();
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            tiles.push((x, y));
        }
    }
    tiles
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;

    #[test]
    fn test_routing_node_size() {
        // Verify the node size is what we expect (16 bytes with padding)
        assert_eq!(size_of::<RoutingNode>(), 16);
    }

    #[test]
    fn test_routing_edge_size() {
        // Verify the edge size is what we expect
        assert_eq!(size_of::<RoutingEdge>(), 12);
    }

    #[test]
    fn test_node_coordinates() {
        let node = RoutingNode::new(47.3769, 8.5417);
        assert!((node.lat_deg() - 47.3769).abs() < 0.000001);
        assert!((node.lon_deg() - 8.5417).abs() < 0.000001);
    }

    #[test]
    fn test_edge_travel_time() {
        let edge = RoutingEdge::new(0, 1000.0, RoadClass::Primary); // 1km, 80km/h
        let time = edge.travel_time_s();
        assert!((time - 45.0).abs() < 0.1); // ~45 seconds
    }

    #[test]
    fn test_tile_serialization() {
        let mut tile = RoutingTile::new(HierarchyLevel::Local, 843, 4714);
        tile.nodes.push(RoutingNode::new(47.14, 8.43));
        tile.edges.push(RoutingEdge::new(0, 100.0, RoadClass::Residential));

        let bytes = tile.to_bytes();
        let loaded = RoutingTile::from_bytes(&bytes).expect("should deserialize");

        assert_eq!(loaded.level, HierarchyLevel::Local);
        assert_eq!(loaded.tile_x, 843);
        assert_eq!(loaded.tile_y, 4714);
        assert_eq!(loaded.nodes.len(), 1);
        assert_eq!(loaded.edges.len(), 1);
    }

    #[test]
    fn test_hierarchy_levels() {
        assert_eq!(RoadClass::Motorway.hierarchy_level(), HierarchyLevel::Highway);
        assert_eq!(RoadClass::Secondary.hierarchy_level(), HierarchyLevel::Arterial);
        assert_eq!(RoadClass::Residential.hierarchy_level(), HierarchyLevel::Local);
    }
}
