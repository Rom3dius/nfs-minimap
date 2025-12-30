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

@main
struct MinimapApp: App {
    @StateObject private var locationManager = LocationManager()

    init() {
        // Get bundle path for tiles
        let tilePath = Bundle.main.path(forResource: "tiles", ofType: nil)

        // Initialize Rust library
        if let path = tilePath {
            _ = path.withCString { ptr in
                minimap_init(ptr)
            }
        } else {
            _ = minimap_init(nil)
        }

        // Start location updates
        locationManager.startUpdating()
    }

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(locationManager)
        }
    }
}

struct ContentView: View {
    var body: some View {
        // The Slint UI is rendered by the Rust library
        // This SwiftUI view is just a container
        Color.black
            .edgesIgnoringSafeArea(.all)
    }
}

class LocationManager: NSObject, ObservableObject, CLLocationManagerDelegate {
    private let manager = CLLocationManager()
    @Published var lastLocation: CLLocation?
    @Published var lastHeading: CLHeading?

    override init() {
        super.init()
        manager.delegate = self
        manager.desiredAccuracy = kCLLocationAccuracyBest
        manager.distanceFilter = 5 // Update every 5 meters
    }

    func startUpdating() {
        manager.requestWhenInUseAuthorization()
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
            manager.requestWhenInUseAuthorization()
        default:
            minimap_set_gps_active(false)
            "GPS Denied".withCString { ptr in
                minimap_set_status(ptr)
            }
        }
    }

    func locationManager(_ manager: CLLocationManager, didUpdateLocations locations: [CLLocation]) {
        guard let location = locations.last else { return }
        lastLocation = location

        let heading = lastHeading?.trueHeading ?? 0
        let speed = location.speed >= 0 ? location.speed * 3.6 : 0 // m/s to km/h

        minimap_update_gps(
            location.coordinate.latitude,
            location.coordinate.longitude,
            Float(heading),
            Float(speed)
        )
    }

    func locationManager(_ manager: CLLocationManager, didUpdateHeading newHeading: CLHeading) {
        lastHeading = newHeading

        // Update with current location and new heading
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
        "GPS Error".withCString { ptr in
            minimap_set_status(ptr)
        }
    }
}
