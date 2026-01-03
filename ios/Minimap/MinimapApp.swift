import SwiftUI
import CoreLocation

// Bridge to Rust library
@_silgen_name("minimap_init")
func minimap_init(_ tile_dir: UnsafePointer<CChar>?) -> Bool

@_silgen_name("minimap_update_gps")
func minimap_update_gps(_ lat: Double, _ lon: Double, _ heading: Float, _ speed_kmh: Float, _ accuracy_meters: Double)

@_silgen_name("minimap_set_gps_status")
func minimap_set_gps_status(_ status: Int32)

// GPS status constants (matches Rust)
let GPS_STATUS_NONE: Int32 = 0
let GPS_STATUS_INACCURATE: Int32 = 1
let GPS_STATUS_GOOD: Int32 = 2

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
    private var retryTimer: Timer?
    private var consecutiveErrors = 0
    private let maxRetries = 3

    override init() {
        super.init()
        manager.delegate = self
        manager.desiredAccuracy = kCLLocationAccuracyBestForNavigation
        manager.distanceFilter = 2  // More frequent updates for smoother movement
        manager.allowsBackgroundLocationUpdates = false
        manager.pausesLocationUpdatesAutomatically = false
    }

    func startUpdating() {
        manager.requestWhenInUseAuthorization()

        // Start updates immediately if already authorized
        if manager.authorizationStatus == .authorizedWhenInUse ||
           manager.authorizationStatus == .authorizedAlways {
            startLocationServices()
        }
    }

    private func startLocationServices() {
        consecutiveErrors = 0
        manager.startUpdatingLocation()
        manager.startUpdatingHeading()
    }

    private func scheduleRetry() {
        guard consecutiveErrors < maxRetries else {
            // Too many errors, stop retrying
            minimap_set_gps_status(GPS_STATUS_NONE)
            return
        }

        // Exponential backoff: 2s, 4s, 8s
        let delay = TimeInterval(2 << consecutiveErrors)
        retryTimer?.invalidate()
        retryTimer = Timer.scheduledTimer(withTimeInterval: delay, repeats: false) { [weak self] _ in
            self?.startLocationServices()
        }
    }

    func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        switch manager.authorizationStatus {
        case .authorizedWhenInUse, .authorizedAlways:
            startLocationServices()
        case .notDetermined:
            // Wait for user response
            minimap_set_gps_status(GPS_STATUS_NONE)
        case .denied, .restricted:
            minimap_set_gps_status(GPS_STATUS_NONE)
        @unknown default:
            break
        }
    }

    func locationManager(_ manager: CLLocationManager, didUpdateLocations locations: [CLLocation]) {
        guard let location = locations.last else { return }

        // Reset error counter on successful update
        consecutiveErrors = 0
        retryTimer?.invalidate()

        lastLocation = location

        let heading = lastHeading?.trueHeading ?? location.course
        let speed = location.speed >= 0 ? location.speed * 3.6 : 0
        let accuracy = location.horizontalAccuracy

        minimap_update_gps(
            location.coordinate.latitude,
            location.coordinate.longitude,
            Float(heading >= 0 ? heading : 0),
            Float(speed),
            accuracy
        )
    }

    func locationManager(_ manager: CLLocationManager, didUpdateHeading newHeading: CLHeading) {
        lastHeading = newHeading

        // Only send heading-only update if we have a recent location
        if let location = lastLocation,
           location.timestamp.timeIntervalSinceNow > -5 {
            let speed = location.speed >= 0 ? location.speed * 3.6 : 0
            let accuracy = location.horizontalAccuracy

            minimap_update_gps(
                location.coordinate.latitude,
                location.coordinate.longitude,
                Float(newHeading.trueHeading),
                Float(speed),
                accuracy
            )
        }
    }

    func locationManager(_ manager: CLLocationManager, didFailWithError error: Error) {
        consecutiveErrors += 1

        // Check if it's a temporary error
        if let clError = error as? CLError {
            switch clError.code {
            case .locationUnknown:
                // Temporary - GPS signal lost (tunnel, etc.)
                minimap_set_gps_status(GPS_STATUS_NONE)
                scheduleRetry()
            case .denied:
                // User denied - don't retry
                minimap_set_gps_status(GPS_STATUS_NONE)
            case .network:
                // Network error - retry
                minimap_set_gps_status(GPS_STATUS_INACCURATE)
                scheduleRetry()
            default:
                minimap_set_gps_status(GPS_STATUS_NONE)
                scheduleRetry()
            }
        } else {
            minimap_set_gps_status(GPS_STATUS_NONE)
            scheduleRetry()
        }
    }
}
