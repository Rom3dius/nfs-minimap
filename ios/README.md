# NFS Minimap iOS App

This directory contains the iOS wrapper for the NFS Minimap Rust library.

## Prerequisites

- macOS with Xcode 14+ (for iOS 16+ SDK)
- Rust with `aarch64-apple-ios` target installed
- iOS device (jailbroken for ad-hoc installation)

## Building

### 1. Build the Rust library (can be done on Linux)

```bash
# Add iOS target if not already installed
rustup target add aarch64-apple-ios

# Build the static library
cargo build -p minimap-mobile --target aarch64-apple-ios --release
```

The static library will be at:
`target/aarch64-apple-ios/release/libminimap_mobile.a`

### 2. Build the iOS app (requires macOS)

Option A: Use Xcode
1. Open `Minimap.xcodeproj` in Xcode
2. Add `libminimap_mobile.a` to the project
3. Build for device (not simulator)

Option B: Use command line
```bash
xcodebuild -project Minimap.xcodeproj \
    -scheme Minimap \
    -configuration Release \
    -sdk iphoneos \
    CODE_SIGN_IDENTITY="" \
    CODE_SIGNING_REQUIRED=NO \
    build
```

### 3. Create unsigned IPA

```bash
# Find the .app bundle
APP_PATH=$(find ~/Library/Developer/Xcode/DerivedData -name "Minimap.app" | head -1)

# Create Payload directory
mkdir -p Payload
cp -r "$APP_PATH" Payload/

# Create IPA
zip -r Minimap-unsigned.ipa Payload
rm -rf Payload
```

## Installing on Jailbroken Device

### Using AppSync Unified
1. Install AppSync Unified from Karen's Repo
2. Transfer `Minimap-unsigned.ipa` to device
3. Install via Filza or AppSync

### Using ldid + SSH
```bash
# On your computer
scp Minimap-unsigned.ipa root@<device-ip>:/var/mobile/

# On the device (via SSH)
cd /var/mobile
unzip Minimap-unsigned.ipa
ldid -S Payload/Minimap.app/Minimap
appinst Payload/Minimap.app
```

## Architecture

```
┌────────────────────────────────────────┐
│           Swift/SwiftUI App            │
│  ┌──────────────────────────────────┐  │
│  │      LocationManager            │  │
│  │   (CoreLocation GPS updates)    │  │
│  └──────────────────────────────────┘  │
│              │                         │
│              ▼ FFI calls               │
│  ┌──────────────────────────────────┐  │
│  │     libminimap_mobile.a         │  │
│  │  ┌──────────────────────────┐   │  │
│  │  │    Slint UI Renderer     │   │  │
│  │  ├──────────────────────────┤   │  │
│  │  │    minimap-core          │   │  │
│  │  ├──────────────────────────┤   │  │
│  │  │    minimap-tiles         │   │  │
│  │  └──────────────────────────┘   │  │
│  └──────────────────────────────────┘  │
└────────────────────────────────────────┘
```

## FFI Interface

The Rust library exposes these C functions:

```c
// Initialize the app
bool minimap_init(const char* tile_dir);

// Update GPS position (called by Swift LocationManager)
void minimap_update_gps(double lat, double lon, float heading, float speed_kmh);

// Set GPS active status
void minimap_set_gps_active(bool active);

// Set status text
void minimap_set_status(const char* text);
```

## Tiles

Copy your pre-generated map tiles to the app bundle:
- `Minimap.app/tiles/` directory

Generate tiles using:
```bash
cargo run -p minimap-tiles --release --bin tile-processor -- \
    path/to/switzerland.osm.pbf \
    tiles/
```
