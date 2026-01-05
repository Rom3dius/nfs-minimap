//! Tile-based map data for offline use
//!
//! This crate provides:
//! - Compact tile format for storing road and POI data
//! - Tile loader for runtime use on ESP32
//! - Tile processor for converting OSM PBF to tiles (with "processor" feature)

#![cfg_attr(not(any(feature = "processor", feature = "loader")), no_std)]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors that can occur when working with tiles.
#[derive(Debug, Error)]
pub enum TileError {
    #[error("tile data too short (expected at least 5 bytes)")]
    DataTooShort,
    #[error("invalid magic bytes (expected 'MMAP')")]
    InvalidMagic,
    #[error("unsupported tile version: {0} (expected {TILE_VERSION})")]
    UnsupportedVersion(u8),
    #[error("serialization failed: {0}")]
    SerializationFailed(String),
    #[error("deserialization failed: {0}")]
    DeserializationFailed(String),
}

/// Errors that can occur when loading tiles from the filesystem.
#[cfg(feature = "loader")]
#[derive(Debug, Error)]
pub enum TileLoaderError {
    #[error("tile directory not found: {0}")]
    DirectoryNotFound(std::path::PathBuf),
    #[error("failed to read tile index: {0}")]
    IndexReadError(String),
    #[error("failed to read tile file: {0}")]
    TileReadError(String),
    #[error("tile error: {0}")]
    TileError(#[from] TileError),
}

/// Tile size in degrees (approximately 1.1km at Swiss latitudes)
pub const TILE_SIZE_DEG: f64 = 0.01;

/// Coordinate precision within a tile (i16 gives ~0.3m precision in a 0.01° tile)
pub const COORD_SCALE: f64 = 3000.0;

/// Magic bytes for tile file format
pub const TILE_MAGIC: [u8; 4] = *b"MMAP";

/// Current tile format version
pub const TILE_VERSION: u8 = 4;

/// Road types (matches minimap-core)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum RoadType {
    Primary = 0,
    Secondary = 1,
}

/// POI types (matches minimap-core)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum PoiType {
    GasStation = 0,
    Parking = 1,
    ShoppingMall = 2,
    CarWash = 3,
    FastFood = 4,
}

/// Area types for natural features
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum AreaType {
    Water = 0,      // Lakes, rivers, ponds
    Forest = 1,     // Forests, woods
    Park = 2,       // Parks, gardens
    Grass = 3,      // Meadows, grassland
}

/// Waterway types for linear water features
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum WaterwayType {
    River = 0,      // Major rivers
    Stream = 1,     // Streams, creeks
    Canal = 2,      // Canals
}

/// A road stored in a tile (coordinates relative to tile origin)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileRoad {
    pub road_type: RoadType,
    /// Points as (x, y) offsets from tile origin, scaled by COORD_SCALE
    pub points: Vec<(i16, i16)>,
}

/// A POI stored in a tile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TilePoi {
    pub poi_type: PoiType,
    /// Position as (x, y) offset from tile origin, scaled by COORD_SCALE
    pub x: i16,
    pub y: i16,
}

/// An area (polygon) stored in a tile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileArea {
    pub area_type: AreaType,
    /// Polygon points as (x, y) offsets from tile origin, scaled by COORD_SCALE
    /// The polygon is implicitly closed (first point connects to last)
    pub points: Vec<(i16, i16)>,
}

/// A waterway (linear water feature) stored in a tile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileWaterway {
    pub waterway_type: WaterwayType,
    /// Points as (x, y) offsets from tile origin, scaled by COORD_SCALE
    pub points: Vec<(i16, i16)>,
}

/// A complete tile with roads, POIs, areas, and waterways
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tile {
    /// Tile X coordinate (longitude / TILE_SIZE_DEG, floored)
    pub tile_x: i32,
    /// Tile Y coordinate (latitude / TILE_SIZE_DEG, floored)
    pub tile_y: i32,
    /// Roads in this tile
    pub roads: Vec<TileRoad>,
    /// POIs in this tile
    pub pois: Vec<TilePoi>,
    /// Areas (polygons) in this tile
    pub areas: Vec<TileArea>,
    /// Waterways (linear water features) in this tile
    #[serde(default)]
    pub waterways: Vec<TileWaterway>,
}

impl Tile {
    /// Create a new empty tile
    pub fn new(tile_x: i32, tile_y: i32) -> Self {
        Self {
            tile_x,
            tile_y,
            roads: Vec::new(),
            pois: Vec::new(),
            areas: Vec::new(),
            waterways: Vec::new(),
        }
    }

    /// Get the tile origin in degrees (bottom-left corner)
    pub fn origin_lat(&self) -> f64 {
        self.tile_y as f64 * TILE_SIZE_DEG
    }

    pub fn origin_lon(&self) -> f64 {
        self.tile_x as f64 * TILE_SIZE_DEG
    }

    /// Serialize tile to bytes
    pub fn to_bytes(&self) -> Result<Vec<u8>, TileError> {
        let mut data = Vec::new();
        data.extend_from_slice(&TILE_MAGIC);
        data.push(TILE_VERSION);

        let encoded = bincode::serialize(self)
            .map_err(|e| TileError::SerializationFailed(e.to_string()))?;
        data.extend_from_slice(&encoded);
        Ok(data)
    }

    /// Deserialize tile from bytes
    pub fn from_bytes(data: &[u8]) -> Result<Self, TileError> {
        if data.len() < 5 {
            return Err(TileError::DataTooShort);
        }
        if &data[0..4] != &TILE_MAGIC {
            return Err(TileError::InvalidMagic);
        }
        if data[4] != TILE_VERSION {
            return Err(TileError::UnsupportedVersion(data[4]));
        }

        bincode::deserialize(&data[5..])
            .map_err(|e| TileError::DeserializationFailed(e.to_string()))
    }
}

/// Convert lat/lon to tile coordinates
pub fn lat_lon_to_tile(lat: f64, lon: f64) -> (i32, i32) {
    let tile_x = (lon / TILE_SIZE_DEG).floor() as i32;
    let tile_y = (lat / TILE_SIZE_DEG).floor() as i32;
    (tile_x, tile_y)
}

/// Convert lat/lon to offset within a tile (scaled i16)
pub fn lat_lon_to_tile_offset(lat: f64, lon: f64, tile_x: i32, tile_y: i32) -> (i16, i16) {
    let origin_lon = tile_x as f64 * TILE_SIZE_DEG;
    let origin_lat = tile_y as f64 * TILE_SIZE_DEG;

    let offset_x = ((lon - origin_lon) * COORD_SCALE / TILE_SIZE_DEG) as i16;
    let offset_y = ((lat - origin_lat) * COORD_SCALE / TILE_SIZE_DEG) as i16;

    (offset_x, offset_y)
}

/// Convert tile offset back to lat/lon
pub fn tile_offset_to_lat_lon(offset_x: i16, offset_y: i16, tile_x: i32, tile_y: i32) -> (f64, f64) {
    let origin_lon = tile_x as f64 * TILE_SIZE_DEG;
    let origin_lat = tile_y as f64 * TILE_SIZE_DEG;

    let lon = origin_lon + (offset_x as f64 * TILE_SIZE_DEG / COORD_SCALE);
    let lat = origin_lat + (offset_y as f64 * TILE_SIZE_DEG / COORD_SCALE);

    (lat, lon)
}

/// Get tile filename for given coordinates
pub fn tile_filename(tile_x: i32, tile_y: i32) -> alloc::string::String {
    alloc::format!("tile_{}_{}.bin", tile_x, tile_y)
}

/// Get all tile coordinates that might be visible from a position with given radius
pub fn get_visible_tiles(lat: f64, lon: f64, radius_deg: f64) -> Vec<(i32, i32)> {
    let mut tiles = Vec::new();

    let min_tile_x = ((lon - radius_deg) / TILE_SIZE_DEG).floor() as i32;
    let max_tile_x = ((lon + radius_deg) / TILE_SIZE_DEG).floor() as i32;
    let min_tile_y = ((lat - radius_deg) / TILE_SIZE_DEG).floor() as i32;
    let max_tile_y = ((lat + radius_deg) / TILE_SIZE_DEG).floor() as i32;

    for tx in min_tile_x..=max_tile_x {
        for ty in min_tile_y..=max_tile_y {
            tiles.push((tx, ty));
        }
    }

    tiles
}

/// Tile loader for runtime use
#[cfg(feature = "loader")]
pub mod loader {
    use super::*;
    use std::collections::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};

    // Re-export TileLoaderError for convenience
    pub use super::TileLoaderError;

    /// Runtime tile loader
    pub struct TileLoader {
        tile_dir: PathBuf,
        loaded_tiles: HashMap<(i32, i32), Tile>,
        available_tiles: std::collections::HashSet<(i32, i32)>,
    }

    impl TileLoader {
        /// Create a new tile loader from a directory
        pub fn new<P: AsRef<Path>>(tile_dir: P) -> Result<Self, TileLoaderError> {
            let tile_dir = tile_dir.as_ref().to_path_buf();

            if !tile_dir.exists() {
                return Err(TileLoaderError::DirectoryNotFound(tile_dir));
            }

            // Read index file to know which tiles are available
            let index_path = tile_dir.join("index.txt");
            let mut available_tiles = std::collections::HashSet::new();

            if let Ok(content) = fs::read_to_string(&index_path) {
                for line in content.lines() {
                    let parts: Vec<&str> = line.split(',').collect();
                    if parts.len() == 2 {
                        if let (Ok(tx), Ok(ty)) = (parts[0].parse(), parts[1].parse()) {
                            available_tiles.insert((tx, ty));
                        }
                    }
                }
            }

            log::info!("TileLoader: {} tiles available", available_tiles.len());

            Ok(Self {
                tile_dir,
                loaded_tiles: HashMap::new(),
                available_tiles,
            })
        }

        /// Load tiles visible from a position
        pub fn load_visible(&mut self, lat: f64, lon: f64, radius_deg: f64) {
            let visible = get_visible_tiles(lat, lon, radius_deg);

            // Unload tiles that are no longer visible
            let visible_set: std::collections::HashSet<_> = visible.iter().cloned().collect();
            self.loaded_tiles.retain(|k, _| visible_set.contains(k));

            // Load new tiles
            for (tx, ty) in visible {
                if self.loaded_tiles.contains_key(&(tx, ty)) {
                    continue;
                }

                if !self.available_tiles.contains(&(tx, ty)) {
                    continue;
                }

                let filename = tile_filename(tx, ty);
                let path = self.tile_dir.join(&filename);

                if let Ok(data) = fs::read(&path) {
                    if let Ok(tile) = Tile::from_bytes(&data) {
                        log::debug!("Loaded tile ({}, {})", tx, ty);
                        self.loaded_tiles.insert((tx, ty), tile);
                    }
                }
            }
        }

        /// Get all roads from loaded tiles, converted to world coordinates
        pub fn get_roads(&self) -> Vec<(RoadType, Vec<(f64, f64)>)> {
            let mut roads = Vec::new();

            for tile in self.loaded_tiles.values() {
                for road in &tile.roads {
                    let points: Vec<(f64, f64)> = road
                        .points
                        .iter()
                        .map(|(ox, oy)| tile_offset_to_lat_lon(*ox, *oy, tile.tile_x, tile.tile_y))
                        .collect();

                    roads.push((road.road_type, points));
                }
            }

            roads
        }

        /// Get all POIs from loaded tiles, converted to world coordinates
        pub fn get_pois(&self) -> Vec<(PoiType, f64, f64)> {
            let mut pois = Vec::new();

            for tile in self.loaded_tiles.values() {
                for poi in &tile.pois {
                    let (lat, lon) = tile_offset_to_lat_lon(poi.x, poi.y, tile.tile_x, tile.tile_y);
                    pois.push((poi.poi_type, lat, lon));
                }
            }

            pois
        }

        /// Get all areas from loaded tiles, converted to world coordinates
        pub fn get_areas(&self) -> Vec<(AreaType, Vec<(f64, f64)>)> {
            let mut areas = Vec::new();

            for tile in self.loaded_tiles.values() {
                for area in &tile.areas {
                    let points: Vec<(f64, f64)> = area
                        .points
                        .iter()
                        .map(|(ox, oy)| tile_offset_to_lat_lon(*ox, *oy, tile.tile_x, tile.tile_y))
                        .collect();

                    areas.push((area.area_type, points));
                }
            }

            areas
        }

        /// Get all waterways from loaded tiles, converted to world coordinates
        pub fn get_waterways(&self) -> Vec<(WaterwayType, Vec<(f64, f64)>)> {
            let mut waterways = Vec::new();

            for tile in self.loaded_tiles.values() {
                for waterway in &tile.waterways {
                    let points: Vec<(f64, f64)> = waterway
                        .points
                        .iter()
                        .map(|(ox, oy)| tile_offset_to_lat_lon(*ox, *oy, tile.tile_x, tile.tile_y))
                        .collect();

                    waterways.push((waterway.waterway_type, points));
                }
            }

            waterways
        }

        /// Number of loaded tiles
        pub fn loaded_count(&self) -> usize {
            self.loaded_tiles.len()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn test_tile_coords() {
        // Rotkreuz, Switzerland
        let (tx, ty) = lat_lon_to_tile(47.1415, 8.4320);
        assert_eq!(tx, 843);
        assert_eq!(ty, 4714);
    }

    #[test]
    fn test_offset_roundtrip() {
        let lat = 47.1415;
        let lon = 8.4320;
        let (tx, ty) = lat_lon_to_tile(lat, lon);
        let (ox, oy) = lat_lon_to_tile_offset(lat, lon, tx, ty);
        let (lat2, lon2) = tile_offset_to_lat_lon(ox, oy, tx, ty);

        assert!((lat - lat2).abs() < 0.0001);
        assert!((lon - lon2).abs() < 0.0001);
    }

    #[test]
    fn test_tile_serialization() {
        let mut tile = Tile::new(843, 4714);
        tile.roads.push(TileRoad {
            road_type: RoadType::Primary,
            points: vec![(100, 200), (300, 400)],
        });
        tile.pois.push(TilePoi {
            poi_type: PoiType::GasStation,
            x: 500,
            y: 600,
        });

        let bytes = tile.to_bytes().unwrap();
        let tile2 = Tile::from_bytes(&bytes).unwrap();

        assert_eq!(tile.tile_x, tile2.tile_x);
        assert_eq!(tile.roads.len(), tile2.roads.len());
        assert_eq!(tile.pois.len(), tile2.pois.len());
    }
}
