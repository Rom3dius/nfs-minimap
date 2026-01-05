//! Hierarchical routing engine for nfs-minimap.
//!
//! This crate provides country-scale routing capabilities for embedded devices
//! with limited memory (32MB PSRAM).
//!
//! # Architecture
//!
//! The routing system uses a three-level tile hierarchy:
//!
//! | Level | Tile Size | Roads | Memory Strategy |
//! |-------|-----------|-------|-----------------|
//! | Highway | 1.0° | motorway, trunk, primary | Always loaded |
//! | Arterial | 0.1° | secondary, tertiary | LRU cache |
//! | Local | 0.01° | residential, service | On-demand |
//!
//! # Features
//!
//! - `loader`: Enables tile loading from files (requires std)
//! - `search`: Enables place search via FST index (requires std)

#![cfg_attr(not(any(feature = "loader", feature = "search")), no_std)]

extern crate alloc;

pub mod geo;
pub mod types;

#[cfg(feature = "loader")]
pub mod algorithm;

#[cfg(feature = "loader")]
pub mod cache;

#[cfg(feature = "loader")]
pub mod hierarchy;

#[cfg(feature = "loader")]
pub mod tile;

#[cfg(feature = "loader")]
pub mod router;

#[cfg(feature = "search")]
pub mod search;

// Re-export commonly used types
pub use types::{
    CrossTileEdge, EdgeFlags, HierarchyLevel, NodeFlags, RoadClass, RoutingEdge, RoutingNode,
    RoutingTile, TileError,
};

#[cfg(feature = "loader")]
pub use algorithm::{NodeRef, RoutingResult, SingleTileRouter};

#[cfg(feature = "loader")]
pub use cache::TileCache;

#[cfg(feature = "loader")]
pub use hierarchy::{GlobalNodeId, HierarchicalRoute, HierarchicalRouter};

#[cfg(feature = "loader")]
pub use router::{Route, Router, TurnInstruction};

#[cfg(feature = "loader")]
pub use tile::TileLoader;

#[cfg(feature = "search")]
pub use search::{SearchEntry, SearchIndex, SearchIndexBuilder, SearchResult};
