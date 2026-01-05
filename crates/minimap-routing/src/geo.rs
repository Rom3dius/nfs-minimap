//! Geographic utilities for routing.

/// Earth radius in meters (WGS84 approximation).
pub const EARTH_RADIUS_M: f64 = 6_371_000.0;

/// Calculate distance between two lat/lon points in meters (Haversine formula).
///
/// This is used as the heuristic for A* routing.
#[inline]
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

/// Calculate distance between two points using microdegree coordinates.
///
/// More efficient for routing since nodes store microdegrees.
#[inline]
pub fn haversine_distance_micro(lat1: i32, lon1: i32, lat2: i32, lon2: i32) -> f64 {
    haversine_distance(
        lat1 as f64 / 1_000_000.0,
        lon1 as f64 / 1_000_000.0,
        lat2 as f64 / 1_000_000.0,
        lon2 as f64 / 1_000_000.0,
    )
}

/// Fast approximate distance using equirectangular projection.
///
/// Less accurate than haversine but faster. Good for short distances.
#[inline]
pub fn approx_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let lat_avg = ((lat1 + lat2) / 2.0).to_radians();
    let x = (lon2 - lon1).to_radians() * lat_avg.cos();
    let y = (lat2 - lat1).to_radians();
    (x * x + y * y).sqrt() * EARTH_RADIUS_M
}

/// Fast approximate distance using microdegree coordinates.
#[inline]
pub fn approx_distance_micro(lat1: i32, lon1: i32, lat2: i32, lon2: i32) -> f64 {
    approx_distance(
        lat1 as f64 / 1_000_000.0,
        lon1 as f64 / 1_000_000.0,
        lat2 as f64 / 1_000_000.0,
        lon2 as f64 / 1_000_000.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_haversine_short_distance() {
        // ~100m distance
        let d = haversine_distance(47.3769, 8.5417, 47.3779, 8.5417);
        assert!((d - 111.0).abs() < 5.0); // ~111m per 0.001 degree latitude
    }

    #[test]
    fn test_haversine_micro() {
        let d1 = haversine_distance(47.3769, 8.5417, 47.3869, 8.5517);
        let d2 = haversine_distance_micro(47_376_900, 8_541_700, 47_386_900, 8_551_700);
        assert!((d1 - d2).abs() < 0.1);
    }

    #[test]
    fn test_approx_vs_haversine() {
        // For short distances, approx should be close to haversine
        let d1 = haversine_distance(47.3769, 8.5417, 47.3869, 8.5517);
        let d2 = approx_distance(47.3769, 8.5417, 47.3869, 8.5517);
        // Should be within 1% for ~1km distances
        assert!((d1 - d2).abs() / d1 < 0.01);
    }
}
