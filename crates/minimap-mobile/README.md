# NFS Minimap Mobile

iOS/Android app for the NFS-style minimap.

## Prerequisites

### For iOS
- macOS with Xcode 15+
- Apple Developer account (for device testing)
- Rust with iOS targets: `rustup target add aarch64-apple-ios x86_64-apple-ios aarch64-apple-ios-sim`
- cargo-mobile2: `cargo install cargo-mobile2`

### For Android
- Android Studio with NDK
- Rust Android targets: `rustup target add aarch64-linux-android armv7-linux-androideabi`

## Setup

1. **Initialize the mobile project:**
   ```bash
   cd crates/minimap-mobile
   cargo mobile init
   ```

2. **Copy map tiles to assets:**
   ```bash
   mkdir -p assets/tiles
   cp -r ../../tiles/* assets/tiles/
   ```

   Note: For a smaller app, you may want to only include tiles for your region.

3. **Open in Xcode (iOS):**
   ```bash
   cargo apple open
   ```

4. **Build and run:**
   ```bash
   # iOS Simulator
   cargo apple run --simulator

   # iOS Device (requires signing)
   cargo apple run --device

   # Android
   cargo android run
   ```

## Architecture

```
┌─────────────────────────────────────────┐
│           Native Layer (Swift/Kotlin)    │
│  - GPS via CoreLocation/LocationManager  │
│  - Calls Rust FFI functions              │
├─────────────────────────────────────────┤
│           Rust Library                   │
│  - minimap_init(tile_dir)               │
│  - minimap_update_gps(lat, lon, ...)    │
│  - minimap_set_gps_active(active)       │
├─────────────────────────────────────────┤
│           Slint UI                       │
│  - MobileApp window                      │
│  - MinimapView component                 │
└─────────────────────────────────────────┘
```

## iOS Swift Integration

After running `cargo mobile init`, you'll need to add GPS handling to the Swift code.

Create `LocationManager.swift`:

```swift
import CoreLocation

class LocationManager: NSObject, CLLocationManagerDelegate {
    static let shared = LocationManager()
    private let manager = CLLocationManager()
    private var lastHeading: Double = 0

    override init() {
        super.init()
        manager.delegate = self
        manager.desiredAccuracy = kCLLocationAccuracyBest
        manager.distanceFilter = 5 // Update every 5 meters
    }

    func start() {
        manager.requestWhenInUseAuthorization()
        manager.startUpdatingLocation()
        manager.startUpdatingHeading()
    }

    func locationManager(_ manager: CLLocationManager, didUpdateLocations locations: [CLLocation]) {
        guard let location = locations.last else { return }

        let speed = max(0, location.speed * 3.6) // m/s to km/h

        minimap_update_gps(
            location.coordinate.latitude,
            location.coordinate.longitude,
            Float(lastHeading),
            Float(speed)
        )
        minimap_set_gps_active(true)
    }

    func locationManager(_ manager: CLLocationManager, didUpdateHeading newHeading: CLHeading) {
        lastHeading = newHeading.trueHeading
    }

    func locationManager(_ manager: CLLocationManager, didFailWithError error: Error) {
        minimap_set_gps_active(false)
    }
}
```

Then in your `AppDelegate.swift` or main app file:

```swift
@main
struct MinimapApp {
    init() {
        // Get path to bundled tiles
        let tilesPath = Bundle.main.path(forResource: "tiles", ofType: nil) ?? ""

        // Initialize the Rust library
        tilesPath.withCString { path in
            minimap_init(path)
        }

        // Start GPS
        LocationManager.shared.start()
    }
}
```

## Bundling Tiles

The full Switzerland tiles are ~150MB. For a mobile app, consider:

1. **Extract a region** - Only include tiles around your driving area
2. **On-demand download** - Download tiles as needed (requires server)
3. **Reduced detail** - Regenerate tiles with fewer road types

To extract tiles for a specific region:
```bash
# Example: Extract tiles around Zurich (roughly 47.3-47.5 lat, 8.4-8.7 lon)
mkdir zurich_tiles
for tile in tiles/tile_84[0-9]_473*.bin tiles/tile_84[0-9]_474*.bin tiles/tile_85[0-9]_473*.bin tiles/tile_85[0-9]_474*.bin; do
  cp "$tile" zurich_tiles/ 2>/dev/null
done
cp tiles/index.txt zurich_tiles/
```
