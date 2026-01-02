# NFS2-Style Minimap

A Need for Speed 2 inspired minimap display built with Rust and Slint. Runs on desktop (simulator), iOS, and targets the Waveshare ESP32-P4-WIFI6-Touch-LCD-3.4C (800x800 round display).

## Features

- **Real OSM data**: Loads roads, POIs, water features, forests, and parks from OpenStreetMap
- **Custom tile format**: Compact binary tiles for offline use
- **Multi-platform**: Desktop simulator, iOS app, ESP32-P4 (planned)
- **Zoom with road filtering**: Automatically hides minor roads when zoomed out to prevent clutter
- **GPS integration**: Real-time location on iOS, simulated movement on desktop

## Project Structure

```
nfs-minimap/
├── ui/
│   └── minimap.slint       # Shared Slint UI definition
├── crates/
│   ├── minimap-core/       # Platform-agnostic map logic & rendering
│   ├── minimap-tiles/      # Tile format & processor
│   ├── minimap-simulator/  # Desktop simulator (WASD + zoom)
│   └── minimap-mobile/     # iOS/Android app with GPS
├── ios/                    # iOS build scripts & Swift code
└── tiles/                  # Generated map tiles (gitignored)
```

## Quick Start

### Prerequisites

1. **Rust toolchain** (stable)
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **System dependencies** (for Slint)

   **Ubuntu/Debian:**
   ```bash
   sudo apt install libfontconfig1-dev libfreetype6-dev
   ```

   **macOS:**
   ```bash
   xcode-select --install
   ```

### Running the Simulator

```bash
# Without tiles (uses Overpass API for live data)
cargo run -p minimap-simulator

# With pre-generated tiles
cargo run -p minimap-simulator -- tiles/
```

**Controls:**
- **WASD**: Move and rotate
- **Q/E**: Zoom in/out

### Generating Map Tiles

Download OSM data and generate tiles:

```bash
# Download Switzerland data (~400MB)
curl -L -o switzerland.osm.pbf \
  https://download.geofabrik.de/europe/switzerland-latest.osm.pbf

# Generate tiles
cargo run -p minimap-tiles --release --features processor --bin tile-processor -- \
  switzerland.osm.pbf tiles/
```

## iOS App

### Building

Requires macOS with Xcode Command Line Tools.

```bash
# Add iOS target
rustup target add aarch64-apple-ios

# Build Rust library
cargo build -p minimap-mobile --target aarch64-apple-ios --release

# Build iOS app (requires tiles to be generated first)
cd ios && ./build.sh
```

### Installing on Jailbroken Device

The build produces `ios/build/Minimap-signed.ipa` signed with ldid for TrollStore/jailbroken devices:

1. Transfer IPA to device
2. Install via TrollStore or Filza + AppSync Unified

### GitHub Actions

Push a tag to trigger automated builds:

```bash
git tag ios-v1.0 && git push origin ios-v1.0
```

## Configuration

### Zoom Levels

| Setting | Value | Description |
|---------|-------|-------------|
| `ZOOM_MIN` | 0.75 m/px | Most zoomed in (~300m radius) |
| `ZOOM_DEFAULT` | 1.5 m/px | Default view (~600m radius) |
| `ZOOM_MAX` | 3.0 m/px | Most zoomed out (~1.2km radius) |

### Color Scheme

Edit `ui/minimap.slint` to customize colors:

```slint
export global MinimapColors {
    out property <color> bg: #1a1a2e;           // Dark blue background
    out property <color> road-primary: #4a90a4; // Main roads
    out property <color> road-secondary: #2d5a6b; // Side streets
    out property <color> player-color: #f39c12; // Player marker
    // ... more colors
}
```

## ESP32-P4 (Planned)

Target hardware: Waveshare ESP32-P4-WIFI6-Touch-LCD-3.4C (800x800 round display)

### Hardware Setup

**GPS Module** (e.g., NEO-6M, BN-220):

| GPS Pin | ESP32-P4 Pin |
|---------|--------------|
| TX | GPIO5 (RX) |
| RX | GPIO4 (TX) |
| VCC | 3.3V |
| GND | GND |

**SD Card**: Use built-in SDIO 3.0 slot for tile storage.

## Tile Format

Custom binary format (`.bin` files) optimized for embedded use:

- **Magic**: `MMAP` (4 bytes)
- **Version**: 3 (1 byte)
- **Data**: bincode-serialized tile data

Each tile covers 0.01° × 0.01° (~1.1km at Swiss latitudes) with ~0.3m coordinate precision.

## License

This project uses the Slint UI framework:
- **GPLv3** - Free for open source projects
- **Commercial License** - Required for proprietary embedded devices

See [Slint Licensing](https://slint.dev/pricing) for details.

Project code is licensed under GPL-3.0-or-later.

## Roadmap

- [x] Core map rendering
- [x] Desktop simulator
- [x] Slint UI with NFS2 styling
- [x] Custom tile format & processor
- [x] iOS app with GPS
- [x] Zoom with road filtering
- [x] POI markers (gas stations, parking, etc.)
- [x] Natural areas (water, forests, parks)
- [x] Waterways (rivers, streams)
- [ ] ESP32-P4 firmware
- [ ] Touch controls for ESP32-P4
- [ ] Route highlighting / navigation
- [ ] Day/night color schemes

## Resources

- [Waveshare ESP32-P4 Wiki](https://www.waveshare.com/wiki/ESP32-P4-WIFI6-Touch-LCD-3.4C)
- [Slint Documentation](https://slint.dev/docs/rust/)
- [OpenStreetMap](https://www.openstreetmap.org/)
- [Geofabrik Downloads](https://download.geofabrik.de/)
