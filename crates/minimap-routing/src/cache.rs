//! LRU tile cache for memory-constrained routing.

use lru::LruCache;
use std::num::NonZeroUsize;

use crate::tile::{TileLoader, TileLoaderError};
use crate::types::{HierarchyLevel, RoutingTile};

/// Configuration for the tile cache.
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Maximum number of Level 1 (arterial) tiles to cache.
    pub level1_capacity: usize,
    /// Maximum number of Level 2 (local) tiles to cache.
    pub level2_capacity: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            // ~12 MB for arterial tiles (assuming ~30KB per tile, ~400 tiles)
            level1_capacity: 400,
            // ~2 MB for local tiles (assuming ~5KB per tile, ~400 tiles)
            level2_capacity: 400,
        }
    }
}

/// Tile cache key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub level: HierarchyLevel,
    pub tile_x: i32,
    pub tile_y: i32,
}

impl TileKey {
    pub fn new(level: HierarchyLevel, tile_x: i32, tile_y: i32) -> Self {
        Self { level, tile_x, tile_y }
    }
}

/// LRU cache for routing tiles.
///
/// Manages tile loading with memory constraints:
/// - Level 0 (Highway): Always kept in memory
/// - Level 1 (Arterial): LRU cache with configurable capacity
/// - Level 2 (Local): LRU cache with configurable capacity
pub struct TileCache {
    loader: TileLoader,
    config: CacheConfig,
    /// Highway graph is always loaded.
    highway_graph: Option<RoutingTile>,
    /// LRU cache for arterial tiles.
    level1_cache: LruCache<(i32, i32), RoutingTile>,
    /// LRU cache for local tiles.
    level2_cache: LruCache<(i32, i32), RoutingTile>,
    /// Statistics.
    stats: CacheStats,
}

/// Cache statistics for monitoring.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub loads: u64,
    pub evictions: u64,
}

impl CacheStats {
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

impl TileCache {
    /// Create a new tile cache.
    pub fn new(loader: TileLoader, config: CacheConfig) -> Self {
        let level1_cap = NonZeroUsize::new(config.level1_capacity).unwrap_or(NonZeroUsize::new(1).unwrap());
        let level2_cap = NonZeroUsize::new(config.level2_capacity).unwrap_or(NonZeroUsize::new(1).unwrap());

        Self {
            loader,
            config,
            highway_graph: None,
            level1_cache: LruCache::new(level1_cap),
            level2_cache: LruCache::new(level2_cap),
            stats: CacheStats::default(),
        }
    }

    /// Initialize the cache by loading the highway graph.
    pub fn initialize(&mut self) -> Result<(), TileLoaderError> {
        if self.highway_graph.is_none() {
            match self.loader.load_highway_graph() {
                Ok(graph) => {
                    log::info!(
                        "Loaded highway graph: {} nodes, {} edges",
                        graph.nodes.len(),
                        graph.edges.len()
                    );
                    self.highway_graph = Some(graph);
                }
                Err(TileLoaderError::FileNotFound(_)) => {
                    log::warn!("Highway graph not found, will load tiles on demand");
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Get the highway graph (Level 0).
    pub fn highway_graph(&self) -> Option<&RoutingTile> {
        self.highway_graph.as_ref()
    }

    /// Get a tile, loading from disk if necessary.
    pub fn get(&mut self, level: HierarchyLevel, tile_x: i32, tile_y: i32) -> Option<&RoutingTile> {
        match level {
            HierarchyLevel::Highway => {
                // Try the merged highway graph first, then fall back to individual tiles
                if self.highway_graph.is_some() {
                    self.highway_graph.as_ref()
                } else {
                    // Load Highway tiles on demand like Arterial
                    self.get_highway_tile(tile_x, tile_y)
                }
            }
            HierarchyLevel::Arterial => self.get_arterial(tile_x, tile_y),
            HierarchyLevel::Local => self.get_local(tile_x, tile_y),
        }
    }

    /// Get a highway tile (when no merged graph is available).
    fn get_highway_tile(&mut self, tile_x: i32, tile_y: i32) -> Option<&RoutingTile> {
        let key = (tile_x, tile_y);

        // Check cache first (reuse arterial cache for simplicity)
        if self.level1_cache.contains(&key) {
            self.stats.hits += 1;
            return self.level1_cache.get(&key);
        }

        // Load from disk
        self.stats.misses += 1;
        if !self.loader.has_tile(HierarchyLevel::Highway, tile_x, tile_y) {
            return None;
        }

        match self.loader.load_tile(HierarchyLevel::Highway, tile_x, tile_y) {
            Ok(tile) => {
                self.stats.loads += 1;
                if self.level1_cache.len() >= self.config.level1_capacity {
                    self.stats.evictions += 1;
                }
                self.level1_cache.put(key, tile);
                self.level1_cache.get(&key)
            }
            Err(e) => {
                log::warn!("Failed to load highway tile ({}, {}): {}", tile_x, tile_y, e);
                None
            }
        }
    }

    /// Get an arterial tile.
    fn get_arterial(&mut self, tile_x: i32, tile_y: i32) -> Option<&RoutingTile> {
        let key = (tile_x, tile_y);

        // Check cache first
        if self.level1_cache.contains(&key) {
            self.stats.hits += 1;
            return self.level1_cache.get(&key);
        }

        // Load from disk
        self.stats.misses += 1;
        if !self.loader.has_tile(HierarchyLevel::Arterial, tile_x, tile_y) {
            return None;
        }

        match self.loader.load_tile(HierarchyLevel::Arterial, tile_x, tile_y) {
            Ok(tile) => {
                self.stats.loads += 1;
                // Track evictions
                if self.level1_cache.len() >= self.config.level1_capacity {
                    self.stats.evictions += 1;
                }
                self.level1_cache.put(key, tile);
                self.level1_cache.get(&key)
            }
            Err(e) => {
                log::warn!("Failed to load arterial tile ({}, {}): {}", tile_x, tile_y, e);
                None
            }
        }
    }

    /// Get a local tile.
    fn get_local(&mut self, tile_x: i32, tile_y: i32) -> Option<&RoutingTile> {
        let key = (tile_x, tile_y);

        // Check cache first
        if self.level2_cache.contains(&key) {
            self.stats.hits += 1;
            return self.level2_cache.get(&key);
        }

        // Load from disk
        self.stats.misses += 1;
        if !self.loader.has_tile(HierarchyLevel::Local, tile_x, tile_y) {
            return None;
        }

        match self.loader.load_tile(HierarchyLevel::Local, tile_x, tile_y) {
            Ok(tile) => {
                self.stats.loads += 1;
                // Track evictions
                if self.level2_cache.len() >= self.config.level2_capacity {
                    self.stats.evictions += 1;
                }
                self.level2_cache.put(key, tile);
                self.level2_cache.get(&key)
            }
            Err(e) => {
                log::warn!("Failed to load local tile ({}, {}): {}", tile_x, tile_y, e);
                None
            }
        }
    }

    /// Prefetch tiles around a location.
    ///
    /// This is useful for prefetching tiles along a route corridor.
    pub fn prefetch(&mut self, lat: f64, lon: f64, level: HierarchyLevel, radius_tiles: i32) {
        let (center_x, center_y) = crate::types::lat_lon_to_tile(lat, lon, level);

        for dy in -radius_tiles..=radius_tiles {
            for dx in -radius_tiles..=radius_tiles {
                let tile_x = center_x + dx;
                let tile_y = center_y + dy;
                // Just touch the tile to load it into cache
                let _ = self.get(level, tile_x, tile_y);
            }
        }
    }

    /// Clear all cached tiles (keeps highway graph).
    pub fn clear(&mut self) {
        self.level1_cache.clear();
        self.level2_cache.clear();
    }

    /// Get cache statistics.
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }

    /// Get current memory usage estimate in bytes.
    pub fn memory_usage_estimate(&self) -> usize {
        let mut total = 0;

        // Highway graph
        if let Some(graph) = &self.highway_graph {
            total += estimate_tile_memory(graph);
        }

        // Arterial cache
        for (_, tile) in self.level1_cache.iter() {
            total += estimate_tile_memory(tile);
        }

        // Local cache
        for (_, tile) in self.level2_cache.iter() {
            total += estimate_tile_memory(tile);
        }

        total
    }

    /// Get the number of cached tiles.
    pub fn cached_count(&self) -> (usize, usize) {
        (self.level1_cache.len(), self.level2_cache.len())
    }
}

/// Estimate memory usage of a tile.
fn estimate_tile_memory(tile: &RoutingTile) -> usize {
    let node_size = std::mem::size_of::<crate::types::RoutingNode>();
    let edge_size = std::mem::size_of::<crate::types::RoutingEdge>();

    tile.nodes.len() * node_size
        + tile.edges.len() * edge_size
        + tile.boundary_nodes.len() * 2
        + tile.names.iter().map(|s| s.len()).sum::<usize>()
        + 64 // overhead
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_config_default() {
        let config = CacheConfig::default();
        assert_eq!(config.level1_capacity, 400);
        assert_eq!(config.level2_capacity, 400);
    }

    #[test]
    fn test_cache_stats() {
        let mut stats = CacheStats::default();
        assert_eq!(stats.hit_rate(), 0.0);

        stats.hits = 80;
        stats.misses = 20;
        assert!((stats.hit_rate() - 0.8).abs() < 0.001);
    }
}
