//! Place search index using Finite State Transducers (FST).
//!
//! This module provides efficient prefix search for place names using
//! the `fst` crate. The index is memory-mapped for efficient querying
//! without loading the entire index into memory.
//!
//! # File Format
//!
//! The search index consists of three files:
//! - `search.fst` - FST map: normalized_name → entry_id
//! - `search_entries.bin` - Entry metadata: lat, lon, category, name_offset, name_len
//! - `search_names.bin` - UTF-8 name strings
//!
//! # Example
//!
//! ```ignore
//! // Building an index
//! let mut builder = SearchIndexBuilder::new();
//! builder.add("Zurich", 47.3769, 8.5417, PlaceCategory::City);
//! builder.add("Zurich Airport", 47.4647, 8.5492, PlaceCategory::Transport);
//! builder.write("search_dir/")?;
//!
//! // Querying
//! let index = SearchIndex::open("search_dir/")?;
//! for result in index.prefix_search("zur", 10) {
//!     println!("{}: ({}, {})", result.name, result.lat, result.lon);
//! }
//! ```

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use fst::automaton::Str;
use fst::{Automaton, IntoStreamer, Map, MapBuilder, Streamer};
use memmap2::Mmap;

/// Category of a place for filtering search results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PlaceCategory {
    /// City, town, village
    Settlement = 0,
    /// Street, road
    Street = 1,
    /// POI (gas station, restaurant, etc.)
    Poi = 2,
    /// Transport (airport, station, etc.)
    Transport = 3,
    /// Natural feature (lake, mountain, etc.)
    Natural = 4,
    /// Other/unknown
    Other = 255,
}

impl PlaceCategory {
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => PlaceCategory::Settlement,
            1 => PlaceCategory::Street,
            2 => PlaceCategory::Poi,
            3 => PlaceCategory::Transport,
            4 => PlaceCategory::Natural,
            _ => PlaceCategory::Other,
        }
    }
}

/// A search entry stored in the index.
#[derive(Debug, Clone)]
pub struct SearchEntry {
    /// Display name
    pub name: String,
    /// Latitude in degrees
    pub lat: f64,
    /// Longitude in degrees
    pub lon: f64,
    /// Place category
    pub category: PlaceCategory,
}

/// Result of a search query.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Display name
    pub name: String,
    /// Latitude in degrees
    pub lat: f64,
    /// Longitude in degrees
    pub lon: f64,
    /// Place category
    pub category: PlaceCategory,
    /// Match score (lower is better, 0 = exact prefix match)
    pub score: u32,
}

/// Entry metadata stored in binary format.
/// Layout: lat (f64) + lon (f64) + category (u8) + name_offset (u32) + name_len (u16) = 27 bytes
const ENTRY_SIZE: usize = 8 + 8 + 1 + 4 + 2; // 23 bytes

/// Builder for creating a search index.
pub struct SearchIndexBuilder {
    /// Entries sorted by normalized name for FST building
    entries: BTreeMap<String, Vec<(String, f64, f64, PlaceCategory)>>,
    /// Total entry count
    count: usize,
}

impl SearchIndexBuilder {
    /// Create a new empty builder.
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            count: 0,
        }
    }

    /// Add a place to the index.
    pub fn add(&mut self, name: &str, lat: f64, lon: f64, category: PlaceCategory) {
        let normalized = normalize_name(name);
        if normalized.is_empty() {
            return;
        }

        self.entries
            .entry(normalized)
            .or_default()
            .push((name.to_string(), lat, lon, category));
        self.count += 1;
    }

    /// Get the number of entries added.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Check if the builder is empty.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Write the index to a directory.
    pub fn write<P: AsRef<Path>>(&self, dir: P) -> io::Result<()> {
        let dir = dir.as_ref();
        std::fs::create_dir_all(dir)?;

        // Build FST map and collect entries in order
        let fst_path = dir.join("search.fst");
        let entries_path = dir.join("search_entries.bin");
        let names_path = dir.join("search_names.bin");

        let fst_file = BufWriter::new(File::create(&fst_path)?);
        let mut fst_builder = MapBuilder::new(fst_file)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        let mut entries_data = Vec::new();
        let mut names_data = Vec::new();
        let mut entry_id: u64 = 0;

        for (normalized, places) in &self.entries {
            // FST value is the first entry ID for this normalized name
            let first_entry_id = entry_id;

            for (name, lat, lon, category) in places {
                // Write entry metadata
                let name_offset = names_data.len() as u32;
                let name_bytes = name.as_bytes();
                let name_len = name_bytes.len().min(u16::MAX as usize) as u16;

                entries_data.extend_from_slice(&lat.to_le_bytes());
                entries_data.extend_from_slice(&lon.to_le_bytes());
                entries_data.push(*category as u8);
                entries_data.extend_from_slice(&name_offset.to_le_bytes());
                entries_data.extend_from_slice(&name_len.to_le_bytes());

                // Write name string
                names_data.extend_from_slice(&name_bytes[..name_len as usize]);

                entry_id += 1;
            }

            // Encode entry count in high bits, first entry ID in low bits
            // Format: (count << 40) | first_entry_id
            // This allows up to 16M entries per normalized name and 1T total entries
            let count = (places.len() as u64).min(0xFFFFFF);
            let fst_value = (count << 40) | (first_entry_id & 0xFF_FFFF_FFFF);

            fst_builder
                .insert(normalized.as_bytes(), fst_value)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        }

        fst_builder
            .finish()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        // Write entries and names
        File::create(&entries_path)?.write_all(&entries_data)?;
        File::create(&names_path)?.write_all(&names_data)?;

        log::info!(
            "Search index written: {} entries, FST={} bytes, entries={} bytes, names={} bytes",
            self.count,
            std::fs::metadata(&fst_path)?.len(),
            entries_data.len(),
            names_data.len()
        );

        Ok(())
    }
}

impl Default for SearchIndexBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Search index for querying places.
pub struct SearchIndex {
    /// Memory-mapped FST
    fst: Map<Mmap>,
    /// Memory-mapped entry data
    entries_mmap: Mmap,
    /// Memory-mapped name strings
    names_mmap: Mmap,
}

impl SearchIndex {
    /// Open a search index from a directory.
    pub fn open<P: AsRef<Path>>(dir: P) -> io::Result<Self> {
        let dir = dir.as_ref();

        let fst_path = dir.join("search.fst");
        let entries_path = dir.join("search_entries.bin");
        let names_path = dir.join("search_names.bin");

        // Memory-map the FST
        let fst_file = File::open(&fst_path)?;
        let fst_mmap = unsafe { Mmap::map(&fst_file)? };
        let fst = Map::new(fst_mmap)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

        // Memory-map entries and names
        let entries_file = File::open(&entries_path)?;
        let entries_mmap = unsafe { Mmap::map(&entries_file)? };

        let names_file = File::open(&names_path)?;
        let names_mmap = unsafe { Mmap::map(&names_file)? };

        log::info!(
            "Search index opened: {} unique keys, {} entries",
            fst.len(),
            entries_mmap.len() / ENTRY_SIZE
        );

        Ok(Self {
            fst,
            entries_mmap,
            names_mmap,
        })
    }

    /// Get total number of entries in the index.
    pub fn entry_count(&self) -> usize {
        self.entries_mmap.len() / ENTRY_SIZE
    }

    /// Search for places with names starting with the given prefix.
    pub fn prefix_search(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        let normalized = normalize_name(query);
        if normalized.is_empty() {
            return Vec::new();
        }

        let mut results = Vec::new();
        let automaton = Str::new(&normalized).starts_with();
        let mut stream = self.fst.search(automaton).into_stream();

        while let Some((key, value)) = stream.next() {
            if results.len() >= limit {
                break;
            }

            // Decode value: (count << 40) | first_entry_id
            let count = ((value >> 40) & 0xFFFFFF) as usize;
            let first_entry_id = (value & 0xFF_FFFF_FFFF) as usize;

            // Calculate score based on key length difference (shorter = better match)
            let key_str = std::str::from_utf8(key).unwrap_or("");
            let score = (key_str.len() as i32 - normalized.len() as i32).unsigned_abs();

            // Load entries for this key
            for i in 0..count.min(limit - results.len()) {
                if let Some(entry) = self.read_entry(first_entry_id + i) {
                    results.push(SearchResult {
                        name: entry.name,
                        lat: entry.lat,
                        lon: entry.lon,
                        category: entry.category,
                        score,
                    });
                }
            }
        }

        // Sort by score (better matches first)
        results.sort_by_key(|r| r.score);
        results
    }

    /// Search for an exact match (case-insensitive).
    pub fn exact_search(&self, query: &str) -> Option<SearchResult> {
        let normalized = normalize_name(query);
        if normalized.is_empty() {
            return None;
        }

        let value = self.fst.get(normalized.as_bytes())?;

        // Decode value
        let first_entry_id = (value & 0xFF_FFFF_FFFF) as usize;

        let entry = self.read_entry(first_entry_id)?;
        Some(SearchResult {
            name: entry.name,
            lat: entry.lat,
            lon: entry.lon,
            category: entry.category,
            score: 0,
        })
    }

    /// Read an entry by ID.
    fn read_entry(&self, entry_id: usize) -> Option<SearchEntry> {
        let offset = entry_id * ENTRY_SIZE;
        if offset + ENTRY_SIZE > self.entries_mmap.len() {
            return None;
        }

        let data = &self.entries_mmap[offset..offset + ENTRY_SIZE];

        let lat = f64::from_le_bytes(data[0..8].try_into().ok()?);
        let lon = f64::from_le_bytes(data[8..16].try_into().ok()?);
        let category = PlaceCategory::from_u8(data[16]);
        let name_offset = u32::from_le_bytes(data[17..21].try_into().ok()?) as usize;
        let name_len = u16::from_le_bytes(data[21..23].try_into().ok()?) as usize;

        // Read name
        if name_offset + name_len > self.names_mmap.len() {
            return None;
        }
        let name = std::str::from_utf8(&self.names_mmap[name_offset..name_offset + name_len])
            .ok()?
            .to_string();

        Some(SearchEntry {
            name,
            lat,
            lon,
            category,
        })
    }
}

/// Normalize a name for indexing/searching.
/// Converts to lowercase and removes diacritics.
fn normalize_name(name: &str) -> String {
    name.chars()
        .filter_map(|c| {
            if c.is_alphanumeric() || c == ' ' {
                Some(c.to_lowercase().next().unwrap_or(c))
            } else {
                None
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_normalize_name() {
        assert_eq!(normalize_name("Zürich"), "zürich");
        assert_eq!(normalize_name("New York"), "new york");
        assert_eq!(normalize_name("St. Gallen"), "st gallen");
        assert_eq!(normalize_name("  Hello  "), "hello");
    }

    #[test]
    fn test_place_category() {
        assert_eq!(PlaceCategory::from_u8(0), PlaceCategory::Settlement);
        assert_eq!(PlaceCategory::from_u8(1), PlaceCategory::Street);
        assert_eq!(PlaceCategory::from_u8(255), PlaceCategory::Other);
        assert_eq!(PlaceCategory::from_u8(99), PlaceCategory::Other);
    }

    #[test]
    fn test_search_index_roundtrip() {
        let temp_dir = std::env::temp_dir().join("minimap_search_test");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        // Build index
        let mut builder = SearchIndexBuilder::new();
        builder.add("Zurich", 47.3769, 8.5417, PlaceCategory::Settlement);
        builder.add("Zurich Airport", 47.4647, 8.5492, PlaceCategory::Transport);
        builder.add("Zug", 47.1724, 8.5172, PlaceCategory::Settlement);
        builder.add("Bern", 46.9481, 7.4474, PlaceCategory::Settlement);
        assert_eq!(builder.len(), 4);

        builder.write(&temp_dir).unwrap();

        // Query index
        let index = SearchIndex::open(&temp_dir).unwrap();
        assert_eq!(index.entry_count(), 4);

        // Prefix search "zur" should find Zurich entries
        let results = index.prefix_search("zur", 10);
        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|r| r.name == "Zurich"));
        assert!(results.iter().any(|r| r.name == "Zurich Airport"));

        // Prefix search "z" should find all Z entries
        let results = index.prefix_search("z", 10);
        assert_eq!(results.len(), 3); // Zurich, Zurich Airport, Zug

        // Exact search
        let result = index.exact_search("Bern").unwrap();
        assert_eq!(result.name, "Bern");
        assert!((result.lat - 46.9481).abs() < 0.0001);

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
