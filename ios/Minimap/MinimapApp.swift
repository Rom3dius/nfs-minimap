import SwiftUI
import CoreLocation

// Bridge to Rust library
@_silgen_name("minimap_init")
func minimap_init(_ tile_dir: UnsafePointer<CChar>?) -> Bool

@_silgen_name("minimap_update_gps")
func minimap_update_gps(_ lat: Double, _ lon: Double, _ heading: Float, _ speed_kmh: Float)

@_silgen_name("minimap_set_gps_active")
func minimap_set_gps_active(_ active: Bool)

@_silgen_name("minimap_set_status")
func minimap_set_status(_ text: UnsafePointer<CChar>?)

// Global location manager - must be global so it persists and receives callbacks
let globalLocationManager = LocationManager()

@main
struct MinimapApp: App {
    init() {
        // Start location services BEFORE Slint takes over
        // The permission dialog will appear over Slint's UI
        globalLocationManager.startUpdating()

        // Get bundle path for tiles
        let tilePath = Bundle.main.path(forResource: "tiles", ofType: nil)

        // Initialize Rust library (this blocks with ui.run())
        if let path = tilePath {
            _ = path.withCString { ptr in
                minimap_init(ptr)
            }
        } else {
            _ = minimap_init(nil)
        }
    }

    var body: some Scene {
        WindowGroup {
            // Placeholder - Slint takes over the UI
            Color.black.edgesIgnoringSafeArea(.all)
        }
    }
}

class LocationManager: NSObject, CLLocationManagerDelegate {
    private let manager = CLLocationManager()
    var lastLocation: CLLocation?
    var lastHeading: CLHeading?

    override init() {
        super.init()
        manager.delegate = self
        manager.desiredAccuracy = kCLLocationAccuracyBestForNavigation
        manager.distanceFilter = 5
        manager.allowsBackgroundLocationUpdates = false
        manager.pausesLocationUpdatesAutomatically = false
    }

    func startUpdating() {
        manager.requestWhenInUseAuthorization()

        // Start updates immediately if already authorized
        if manager.authorizationStatus == .authorizedWhenInUse ||
           manager.authorizationStatus == .authorizedAlways {
            manager.startUpdatingLocation()
            manager.startUpdatingHeading()
        }
    }

    func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        switch manager.authorizationStatus {
        case .authorizedWhenInUse, .authorizedAlways:
            manager.startUpdatingLocation()
            manager.startUpdatingHeading()
            minimap_set_gps_active(true)
            "GPS Active".withCString { ptr in
                minimap_set_status(ptr)
            }
        case .notDetermined:
            // Wait for user response
            break
        case .denied, .restricted:
            minimap_set_gps_active(false)
            "GPS Denied - Enable in Settings".withCString { ptr in
                minimap_set_status(ptr)
            }
        @unknown default:
            break
        }
    }

    func locationManager(_ manager: CLLocationManager, didUpdateLocations locations: [CLLocation]) {
        guard let location = locations.last else { return }
        lastLocation = location

        let heading = lastHeading?.trueHeading ?? 0
        let speed = location.speed >= 0 ? location.speed * 3.6 : 0

        minimap_update_gps(
            location.coordinate.latitude,
            location.coordinate.longitude,
            Float(heading),
            Float(speed)
        )
    }

    func locationManager(_ manager: CLLocationManager, didUpdateHeading newHeading: CLHeading) {
        lastHeading = newHeading

        if let location = lastLocation {
            let speed = location.speed >= 0 ? location.speed * 3.6 : 0
            minimap_update_gps(
                location.coordinate.latitude,
                location.coordinate.longitude,
                Float(newHeading.trueHeading),
                Float(speed)
            )
        }
    }

    func locationManager(_ manager: CLLocationManager, didFailWithError error: Error) {
        minimap_set_gps_active(false)
        "GPS Error: \(error.localizedDescription)".withCString { ptr in
            minimap_set_status(ptr)
        }
    }
}
