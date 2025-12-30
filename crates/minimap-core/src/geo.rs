//! Geographic coordinate utilities
//! 
//! Handles conversion between lat/lon and local coordinate systems

use nalgebra::Point2;

/// Earth radius in meters (WGS84 approximation)
const EARTH_RADIUS_M: f64 = 6_371_000.0;

/// Convert lat/lon to local meters relative to a reference point
/// Uses equirectangular approximation (accurate enough for small areas)
pub fn lat_lon_to_local_meters(
    lat: f64,
    lon: f64,
    ref_lat: f64,
    ref_lon: f64,
) -> Point2<f32> {
    let lat_rad = ref_lat.to_radians();
    
    // X = longitude difference * earth radius * cos(latitude)
    let x = (lon - ref_lon).to_radians() * EARTH_RADIUS_M * lat_rad.cos();
    
    // Y = latitude difference * earth radius
    let y = (lat - ref_lat).to_radians() * EARTH_RADIUS_M;
    
    Point2::new(x as f32, y as f32)
}

/// Convert local meters back to lat/lon
pub fn local_meters_to_lat_lon(
    x: f32,
    y: f32,
    ref_lat: f64,
    ref_lon: f64,
) -> (f64, f64) {
    let lat_rad = ref_lat.to_radians();
    
    let lon = ref_lon + (x as f64 / (EARTH_RADIUS_M * lat_rad.cos())).to_degrees();
    let lat = ref_lat + (y as f64 / EARTH_RADIUS_M).to_degrees();
    
    (lat, lon)
}

/// Calculate distance between two lat/lon points in meters (Haversine formula)
pub fn haversine_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();
    let delta_lat = (lat2 - lat1).to_radians();
    let delta_lon = (lon2 - lon1).to_radians();
    
    let a = (delta_lat / 2.0).sin().powi(2)
        + lat1_rad.cos() * lat2_rad.cos() * (delta_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    
    EARTH_RADIUS_M * c
}

/// Calculate bearing from point 1 to point 2 in degrees (0 = North)
pub fn bearing(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();
    let delta_lon = (lon2 - lon1).to_radians();
    
    let y = delta_lon.sin() * lat2_rad.cos();
    let x = lat1_rad.cos() * lat2_rad.sin()
        - lat1_rad.sin() * lat2_rad.cos() * delta_lon.cos();
    
    let bearing_rad = y.atan2(x);
    
    // Convert to 0-360 degrees
    (bearing_rad.to_degrees() + 360.0) % 360.0
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_lat_lon_roundtrip() {
        let ref_lat = 47.3769;  // Zurich
        let ref_lon = 8.5417;
        
        let target_lat = 47.3800;
        let target_lon = 8.5450;
        
        let local = lat_lon_to_local_meters(target_lat, target_lon, ref_lat, ref_lon);
        let (back_lat, back_lon) = local_meters_to_lat_lon(local.x, local.y, ref_lat, ref_lon);
        
        assert!((back_lat - target_lat).abs() < 0.0001);
        assert!((back_lon - target_lon).abs() < 0.0001);
    }
    
    #[test]
    fn test_haversine_known_distance() {
        // Zurich to Bern is roughly 95km
        let zurich_lat = 47.3769;
        let zurich_lon = 8.5417;
        let bern_lat = 46.9480;
        let bern_lon = 7.4474;
        
        let distance = haversine_distance(zurich_lat, zurich_lon, bern_lat, bern_lon);
        
        // Should be approximately 95km (95000m)
        assert!((distance - 95000.0).abs() < 5000.0);
    }
}
