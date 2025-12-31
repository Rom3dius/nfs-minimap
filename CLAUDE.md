# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

NFS2-style minimap display for ESP32-P4 (Waveshare 800x800 round display). Built with Rust and Slint UI framework, with desktop simulator and iOS/Android mobile support.

## Build Commands

```bash
# Build all crates
cargo build

# Run desktop simulator (WASD controls, 800x800 window)
cargo run -p minimap-simulator
cargo run -p minimap-simulator -- /path/to/tiles  # with pre-processed tiles

# Run tests
cargo test                      # all tests
cargo test -p minimap-core      # specific crate
cargo test -p minimap-tiles

# Process OSM data to tiles
cargo run -p minimap-tiles --release --features processor \
  --bin tile-processor -- <input.osm.pbf> <output-dir>

# Mobile builds (requires cargo-mobile2)
cargo apple run --simulator     # iOS Simulator
cargo apple run --device        # iOS Device
cargo android run               # Android
```

## Architecture

### Crate Structure

```
crates/
├── minimap-core     # Platform-agnostic rendering engine
├── minimap-tiles    # Binary tile format and loader (no_std compatible)
├── minimap-simulator # Desktop dev simulator with WASD
├── minimap-mobile   # iOS/Android FFI layer
└── minimap-esp32p4  # ESP32 firmware (currently commented out)

ui/
└── minimap.slint    # Core Slint UI component (imported by all platforms)
```

### Data Flow

```
GPS/Input → VehicleState (lat, lon, heading, speed)
     ↓
MapRenderer.render(VehicleState)
     ↓
World coords (WorldRoad/Poi/Area) → Screen coords (ScreenSegment/Poi/Triangle)
     ↓
Slint UI renders screen-coordinate data
```

### Key Types (minimap-core)

- `MapRenderer` - Main rendering engine, transforms world → screen coordinates
- `VehicleState` - Player position (lat, lon, heading, speed_kmh)
- `MinimapConfig` - Display config (dimensions, zoom, rotation)
- World types: `WorldRoad`, `WorldPoi`, `WorldArea`
- Screen types: `ScreenSegment`, `ScreenPoi`, `ScreenTriangle`

### Tile System (minimap-tiles)

Binary format with "MMAP" magic header, version 2. Tiles are 0.01° (~1.1km). Supports:
- `loader` feature: Runtime tile loading with HashMap cache
- `processor` feature: OSM PBF → tile conversion (heavy deps: osmpbf, rayon)

### Slint UI Components

- `MinimapView` (ui/minimap.slint) - Reusable component, imported by platform UIs
- Road types: 0=primary, 1=secondary, 2=highlight
- POI types: 0=gas, 1=parking, 2=mall, 3=carwash
- Area types: 0=water, 1=forest, 2=park, 3=grass

### Mobile FFI (minimap-mobile)

```rust
minimap_init(tile_dir)              // Initialize
minimap_update_gps(lat, lon, heading, speed)  // GPS update
minimap_set_gps_active(active)      // GPS status
minimap_set_status(text)            // Status text
```

## Key Source Files

- `crates/minimap-core/src/lib.rs` - Core renderer
- `crates/minimap-core/src/geo.rs` - Coordinate math (equirectangular projection, haversine)
- `crates/minimap-core/src/transform.rs` - Viewport transformations
- `crates/minimap-tiles/src/lib.rs` - Tile format and loader
- `crates/minimap-simulator/src/main.rs` - Desktop simulator entry point

## Technology

- UI: Slint 1.9 (GPLv3 for open source)
- Math: nalgebra 0.33
- Serialization: serde, bincode
- License: GPL-3.0-or-later
