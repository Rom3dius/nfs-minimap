//! Tile loading from filesystem.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::types::{HierarchyLevel, RoutingTile, TileError};

/// Loads routing tiles from the filesystem.
pub struct TileLoader {
    base_dir: PathBuf,
    /// Available tiles at each level, indexed by (tile_x, tile_y).
    available: [HashSet<(i32, i32)>; 3],
}

impl TileLoader {
    /// Create a new tile loader for the given directory.
    ///
    /// Expected directory structure:
    /// ```text
    /// base_dir/
    ///   routing/
    ///     level0/
    ///       graph.bin         # Full highway graph
    ///       index.bin         # Optional tile index
    ///     level1/
    ///       index.bin
    ///       tile_{x}_{y}.bin
    ///     level2/
    ///       index.bin
    ///       tile_{x}_{y}.bin
    /// ```
    pub fn new(base_dir: impl AsRef<Path>) -> Result<Self, TileLoaderError> {
        let base_dir = base_dir.as_ref().to_path_buf();
        let routing_dir = base_dir.join("routing");

        if !routing_dir.exists() {
            return Err(TileLoaderError::DirectoryNotFound(routing_dir));
        }

        let mut loader = Self {
            base_dir,
            available: [HashSet::new(), HashSet::new(), HashSet::new()],
        };

        // Load indices for each level
        for level in [HierarchyLevel::Highway, HierarchyLevel::Arterial, HierarchyLevel::Local] {
            loader.load_index(level)?;
        }

        Ok(loader)
    }

    /// Load the tile index for a hierarchy level.
    fn load_index(&mut self, level: HierarchyLevel) -> Result<(), TileLoaderError> {
        let level_dir = self.level_dir(level);
        let index_path = level_dir.join("index.txt");

        if !index_path.exists() {
            // Try to scan directory for tiles
            return self.scan_directory(level);
        }

        let content = fs::read_to_string(&index_path)
            .map_err(|e| TileLoaderError::IoError(e.to_string()))?;

        let level_idx = level as usize;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((x, y)) = parse_tile_coords(line) {
                self.available[level_idx].insert((x, y));
            }
        }

        Ok(())
    }

    /// Scan directory for tile files.
    fn scan_directory(&mut self, level: HierarchyLevel) -> Result<(), TileLoaderError> {
        let level_dir = self.level_dir(level);
        if !level_dir.exists() {
            return Ok(());
        }

        let level_idx = level as usize;
        if let Ok(entries) = fs::read_dir(&level_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("tile_") && name.ends_with(".bin") {
                    if let Some((x, y)) = parse_tile_filename(&name) {
                        self.available[level_idx].insert((x, y));
                    }
                }
            }
        }

        Ok(())
    }

    /// Get the directory for a hierarchy level.
    fn level_dir(&self, level: HierarchyLevel) -> PathBuf {
        let level_name = match level {
            HierarchyLevel::Highway => "level0",
            HierarchyLevel::Arterial => "level1",
            HierarchyLevel::Local => "level2",
        };
        self.base_dir.join("routing").join(level_name)
    }

    /// Check if a tile is available.
    pub fn has_tile(&self, level: HierarchyLevel, tile_x: i32, tile_y: i32) -> bool {
        self.available[level as usize].contains(&(tile_x, tile_y))
    }

    /// Get all available tiles for a level.
    pub fn available_tiles(&self, level: HierarchyLevel) -> &HashSet<(i32, i32)> {
        &self.available[level as usize]
    }

    /// Load a specific tile.
    pub fn load_tile(
        &self,
        level: HierarchyLevel,
        tile_x: i32,
        tile_y: i32,
    ) -> Result<RoutingTile, TileLoaderError> {
        let path = self.tile_path(level, tile_x, tile_y);
        let data = fs::read(&path).map_err(|e| TileLoaderError::IoError(e.to_string()))?;
        RoutingTile::from_bytes(&data).map_err(TileLoaderError::TileError)
    }

    /// Get the file path for a tile.
    fn tile_path(&self, level: HierarchyLevel, tile_x: i32, tile_y: i32) -> PathBuf {
        let level_dir = self.level_dir(level);
        level_dir.join(format!("tile_{}_{}.bin", tile_x, tile_y))
    }

    /// Load the full highway graph (Level 0).
    ///
    /// For the highway level, we store all nodes/edges in a single file
    /// since it's small enough to keep in memory.
    pub fn load_highway_graph(&self) -> Result<RoutingTile, TileLoaderError> {
        let path = self.level_dir(HierarchyLevel::Highway).join("graph.bin");
        if path.exists() {
            let data = fs::read(&path).map_err(|e| TileLoaderError::IoError(e.to_string()))?;
            RoutingTile::from_bytes(&data).map_err(TileLoaderError::TileError)
        } else {
            // Fall back to loading individual tiles and merging
            Err(TileLoaderError::FileNotFound(path))
        }
    }
}

/// Errors from tile loading operations.
#[derive(Debug, Error)]
pub enum TileLoaderError {
    #[error("routing directory not found: {}", .0.display())]
    DirectoryNotFound(PathBuf),
    #[error("tile file not found: {}", .0.display())]
    FileNotFound(PathBuf),
    #[error("IO error: {0}")]
    IoError(String),
    #[error("tile error: {0}")]
    TileError(#[from] TileError),
}

/// Parse tile coordinates from "x,y" format.
fn parse_tile_coords(s: &str) -> Option<(i32, i32)> {
    let mut parts = s.split(',');
    let x = parts.next()?.trim().parse().ok()?;
    let y = parts.next()?.trim().parse().ok()?;
    Some((x, y))
}

/// Parse tile coordinates from filename "tile_{x}_{y}.bin".
fn parse_tile_filename(name: &str) -> Option<(i32, i32)> {
    let name = name.strip_prefix("tile_")?;
    let name = name.strip_suffix(".bin")?;
    let mut parts = name.split('_');
    let x = parts.next()?.parse().ok()?;
    let y = parts.next()?.parse().ok()?;
    Some((x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tile_coords() {
        assert_eq!(parse_tile_coords("843,4714"), Some((843, 4714)));
        assert_eq!(parse_tile_coords("-10, 20"), Some((-10, 20)));
        assert_eq!(parse_tile_coords("invalid"), None);
    }

    #[test]
    fn test_parse_tile_filename() {
        assert_eq!(parse_tile_filename("tile_843_4714.bin"), Some((843, 4714)));
        assert_eq!(parse_tile_filename("tile_-10_20.bin"), Some((-10, 20)));
        assert_eq!(parse_tile_filename("invalid.bin"), None);
    }
}
