# NFS2-Style Minimap for ESP32-P4

A Need for Speed 2 inspired minimap display built with Rust and Slint, targeting the Waveshare ESP32-P4-WIFI6-Touch-LCD-3.4C (800x800 round display).

## Project Structure

```
nfs-minimap/
├── Cargo.toml              # Workspace configuration
├── ui/
│   └── minimap.slint       # Slint UI definition
├── crates/
│   ├── minimap-core/       # Platform-agnostic map logic
│   │   ├── src/
│   │   │   ├── lib.rs      # Main library, renderer
│   │   │   ├── geo.rs      # Coordinate conversions
│   │   │   ├── map_data.rs # GeoJSON loading, road data
│   │   │   └── transform.rs # Viewport transformations
│   │   └── build.rs        # Slint compilation
│   │
│   ├── minimap-simulator/  # Desktop simulator for development
│   │   └── src/main.rs     # SDL window, fake GPS
│   │
│   └── minimap-esp32p4/    # Device firmware (template)
│       └── src/main.rs     # ESP32-P4 specific code
```

## Quick Start (Simulator)

### Prerequisites

1. **Rust toolchain** (stable)
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **System dependencies** (for Slint's winit backend)
   
   **Ubuntu/Debian:**
   ```bash
   sudo apt install libfontconfig1-dev libfreetype6-dev
   ```
   
   **macOS:**
   ```bash
   # Usually works out of the box with Xcode command line tools
   xcode-select --install
   ```
   
   **Windows:**
   - Visual Studio Build Tools with C++ workload

### Running the Simulator

```bash
cd nfs-minimap
cargo run -p minimap-simulator
```

This opens an 800x800 window showing the minimap with:
- Sample road grid
- Simulated vehicle movement
- Rotating map based on heading
- Speed and heading display

## Using Real Map Data

### Option 1: GeoJSON from OpenStreetMap

1. Go to [Overpass Turbo](https://overpass-turbo.eu/)
2. Run a query like:
   ```
   [out:json];
   way["highway"]({{bbox}});
   out geom;
   ```
3. Export as GeoJSON
4. Load in code:
   ```rust
   let roads = map_data::load_from_geojson(&json_string)?;
   renderer.set_roads(roads);
   ```

### Option 2: Pre-processed tiles

For larger areas, pre-process OSM data into tiles:
- Use [tilemaker](https://tilemaker.org/) to create vector tiles
- Convert to simple JSON format for device storage

## ESP32-P4 Deployment

### Prerequisites

1. **ESP-IDF Rust toolchain**
   ```bash
   cargo install espup
   espup install
   . ~/export-esp.sh
   ```

2. **Add the ESP32-P4 crate to workspace**
   
   Uncomment in `Cargo.toml`:
   ```toml
   members = [
       "crates/minimap-core",
       "crates/minimap-simulator",
       "crates/minimap-esp32p4",  # Uncomment this
   ]
   ```

### Building

```bash
cd crates/minimap-esp32p4
cargo build --release
```

### Flashing

```bash
cargo espflash flash --release --monitor
```

## Configuration

### Map Settings (`MinimapConfig`)

| Setting | Default | Description |
|---------|---------|-------------|
| `screen_width` | 800.0 | Display width in pixels |
| `screen_height` | 800.0 | Display height in pixels |
| `meters_per_pixel` | 1.5 | Zoom level (~600m visible radius) |
| `rotate_with_heading` | true | Rotate map with vehicle heading |

### Color Scheme

Edit `ui/minimap.slint` to customize the NFS2 aesthetic:

```slint
property <NfsColors> colors: {
    background: #1a1a2e,      // Dark blue background
    road-primary: #4a90a4,    // Main roads
    road-secondary: #2d5a6b,  // Side streets
    road-highlight: #e94560,  // Route highlight
    player-marker: #f39c12,   // Player triangle
    text-primary: #eee,       // Text color
    compass-ring: #4a90a4,    // Compass circle
};
```

## Hardware Setup (ESP32-P4)

### GPS Module

Connect a GPS module (e.g., NEO-6M, BN-220) to UART:

| GPS Pin | ESP32-P4 Pin |
|---------|--------------|
| TX | GPIO5 (RX) |
| RX | GPIO4 (TX) |
| VCC | 3.3V |
| GND | GND |

### SD Card (for map data)

The board has built-in SDIO 3.0 slot. Store map tiles as:
```
/sd/
└── tiles/
    └── roads.json
```

## License

This project uses the Slint UI framework which is available under:
- **GPLv3** - Free for open source projects
- **Commercial License** - Required for proprietary embedded devices

See [Slint Licensing](https://slint.dev/pricing) for details.

The project code itself is licensed under GPL-3.0-or-later.

## Roadmap

- [x] Core map rendering logic
- [x] Desktop simulator
- [x] Slint UI with NFS2 styling
- [ ] ESP32-P4 MIPI-DSI display driver integration
- [ ] GPS NMEA parsing
- [ ] SD card tile loading
- [ ] Touch controls for zoom/pan
- [ ] Route highlighting
- [ ] POI markers
- [ ] Day/night color schemes

## Resources

- [Waveshare ESP32-P4 Wiki](https://www.waveshare.com/wiki/ESP32-P4-WIFI6-Touch-LCD-3.4C)
- [Slint Documentation](https://slint.dev/docs/rust/)
- [ESP-RS Book](https://docs.esp-rs.org/book/)
- [OpenStreetMap](https://www.openstreetmap.org/)
