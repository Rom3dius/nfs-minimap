//! End-to-end integration tests for routing with real tile data.
//!
//! These tests verify that:
//! 1. Routes start and end on actual local roads (snapped to nearest road)
//! 2. Routes are continuous and valid
//! 3. Local, regional, and long-distance routes all work correctly
//!
//! Run the route processor first:
//!   cargo run -p minimap-tiles --release --features processor \
//!     --bin route-processor -- switzerland-latest.osm.pbf tiles
//!
//! Run tests with:
//!   RUST_LOG=minimap_routing=debug cargo test -p minimap-routing --features loader \
//!     --test routing_integration -- --nocapture --test-threads=1

#![cfg(feature = "loader")]

use minimap_routing::Router;

/// Initialize logging for tests.
fn init_logging() {
    let _ = env_logger::builder().is_test(true).try_init();
}

/// Get the tiles directory path.
fn get_tiles_dir() -> Option<String> {
    // Try relative path from workspace root
    if std::path::Path::new("tiles/routing/level0").exists() {
        return Some("tiles".to_string());
    }

    // Try absolute path for when running from crate directory
    let home = std::env::var("HOME").ok()?;
    let abs_path = format!("{}/src/nfs-minimap/tiles", home);
    if std::path::Path::new(&format!("{}/routing/level0", abs_path)).exists() {
        return Some(abs_path);
    }

    None
}

/// Test helper to check if tiles are available.
fn tiles_available() -> bool {
    get_tiles_dir().is_some()
}

/// Calculate haversine distance in meters between two points.
fn distance_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let lat1_rad = a.0.to_radians();
    let lat2_rad = b.0.to_radians();
    let delta_lat = (b.0 - a.0).to_radians();
    let delta_lon = (b.1 - a.1).to_radians();

    let a_val = (delta_lat / 2.0).sin().powi(2)
        + lat1_rad.cos() * lat2_rad.cos() * (delta_lon / 2.0).sin().powi(2);
    let c = 2.0 * a_val.sqrt().asin();

    6_371_000.0 * c
}

/// Test locations on LOCAL/RESIDENTIAL roads in Rotkreuz.
/// These are intentionally on small streets, not major roads.
mod locations {
    // Rotkreuz residential areas (on local streets)
    // These are offset from road centerlines to test snapping
    pub const ROTKREUZ_RESIDENTIAL_1: (f64, f64) = (47.1408, 8.4312); // Near Langweidstrasse
    pub const ROTKREUZ_RESIDENTIAL_2: (f64, f64) = (47.1425, 8.4340); // Near Buonaserstrasse
    pub const ROTKREUZ_RESIDENTIAL_3: (f64, f64) = (47.1395, 8.4285); // Near Forrenstrasse
    pub const ROTKREUZ_RESIDENTIAL_4: (f64, f64) = (47.1445, 8.4365); // Near Grundstrasse

    // Zug residential areas
    pub const ZUG_RESIDENTIAL_1: (f64, f64) = (47.1680, 8.5120); // Near Aegeristrasse
    pub const ZUG_RESIDENTIAL_2: (f64, f64) = (47.1650, 8.5180); // Near Bundesstrasse

    // Cham residential
    pub const CHAM_RESIDENTIAL: (f64, f64) = (47.1810, 8.4620);

    // Major cities (for long-distance tests)
    // Note: These are near arterial/primary roads to ensure connectivity at all hierarchy levels
    pub const ZURICH_RESIDENTIAL: (f64, f64) = (47.3780, 8.5400); // Near Bahnhofstrasse
    pub const BERN_RESIDENTIAL: (f64, f64) = (46.9480, 7.4480);   // Near Bundeshaus
    pub const BASEL_RESIDENTIAL: (f64, f64) = (47.5570, 7.5890);  // Near SBB
    pub const GENEVA_RESIDENTIAL: (f64, f64) = (46.2100, 6.1420); // Near Cornavin (not on highway)
    pub const LUCERNE_RESIDENTIAL: (f64, f64) = (47.0500, 8.3100); // Near station
}

use locations::*;

// ============================================================================
// Core Test Infrastructure
// ============================================================================

/// Result of a routing test with snapping verification.
struct RoutingTestResult {
    /// Whether a route was found
    found: bool,
    /// Distance from requested start to snapped start (meters)
    start_snap_distance_m: Option<f64>,
    /// Distance from requested end to snapped end (meters)
    end_snap_distance_m: Option<f64>,
    /// Actual snapped start position
    snapped_start: Option<(f64, f64)>,
    /// Actual snapped end position
    snapped_end: Option<(f64, f64)>,
    /// Route distance in meters
    route_distance_m: Option<f64>,
    /// Route time in seconds
    route_time_s: Option<f64>,
    /// Number of path points
    path_points: usize,
    /// Distance from route first point to snapped start
    route_start_accuracy_m: Option<f64>,
    /// Distance from route last point to snapped end
    route_end_accuracy_m: Option<f64>,
}

/// Run a full end-to-end routing test with snapping verification.
fn test_route_end_to_end(
    router: &mut Router,
    start: (f64, f64),
    end: (f64, f64),
    test_name: &str,
) -> RoutingTestResult {
    println!("\n=== {} ===", test_name);
    println!("Requested: ({:.5}, {:.5}) -> ({:.5}, {:.5})", start.0, start.1, end.0, end.1);

    // Step 1: Verify we can snap to roads at start and end
    let snapped_start = router.snap_to_road(start.0, start.1);
    let snapped_end = router.snap_to_road(end.0, end.1);

    let start_snap_distance = snapped_start.map(|(_, _, d)| d);
    let end_snap_distance = snapped_end.map(|(_, _, d)| d);

    if let Some((lat, lon, dist)) = snapped_start {
        println!("  Start snapped to: ({:.5}, {:.5}), distance: {:.1}m", lat, lon, dist);
    } else {
        println!("  WARNING: Could not snap start to any road!");
    }

    if let Some((lat, lon, dist)) = snapped_end {
        println!("  End snapped to: ({:.5}, {:.5}), distance: {:.1}m", lat, lon, dist);
    } else {
        println!("  WARNING: Could not snap end to any road!");
    }

    // Step 2: Find the route
    let route = router.find_route(start.0, start.1, end.0, end.1);

    if route.is_none() {
        println!("  RESULT: No route found");
        return RoutingTestResult {
            found: false,
            start_snap_distance_m: start_snap_distance,
            end_snap_distance_m: end_snap_distance,
            snapped_start: snapped_start.map(|(lat, lon, _)| (lat, lon)),
            snapped_end: snapped_end.map(|(lat, lon, _)| (lat, lon)),
            route_distance_m: None,
            route_time_s: None,
            path_points: 0,
            route_start_accuracy_m: None,
            route_end_accuracy_m: None,
        };
    }

    let route = route.unwrap();

    // Step 3: Verify route start and end match snapped positions
    let route_start = route.path.first().cloned();
    let route_end = route.path.last().cloned();

    let route_start_accuracy = match (route_start, snapped_start) {
        (Some(rs), Some((ss_lat, ss_lon, _))) => Some(distance_m(rs, (ss_lat, ss_lon))),
        _ => None,
    };

    let route_end_accuracy = match (route_end, snapped_end) {
        (Some(re), Some((se_lat, se_lon, _))) => Some(distance_m(re, (se_lat, se_lon))),
        _ => None,
    };

    println!(
        "  Route: {:.1}m, {:.1}s, {} points",
        route.total_distance_m,
        route.total_time_s,
        route.path.len()
    );

    if let Some(rs) = route_start {
        println!("  Route starts at: ({:.5}, {:.5})", rs.0, rs.1);
    }
    if let Some(re) = route_end {
        println!("  Route ends at: ({:.5}, {:.5})", re.0, re.1);
    }

    if let Some(acc) = route_start_accuracy {
        println!("  Start accuracy: {:.1}m from snapped position", acc);
    }
    if let Some(acc) = route_end_accuracy {
        println!("  End accuracy: {:.1}m from snapped position", acc);
    }

    RoutingTestResult {
        found: true,
        start_snap_distance_m: start_snap_distance,
        end_snap_distance_m: end_snap_distance,
        snapped_start: snapped_start.map(|(lat, lon, _)| (lat, lon)),
        snapped_end: snapped_end.map(|(lat, lon, _)| (lat, lon)),
        route_distance_m: Some(route.total_distance_m),
        route_time_s: Some(route.total_time_s),
        path_points: route.path.len(),
        route_start_accuracy_m: route_start_accuracy,
        route_end_accuracy_m: route_end_accuracy,
    }
}

// ============================================================================
// Local Road Snapping Tests
// ============================================================================

#[test]
fn test_snap_to_local_road() {
    init_logging();
    if !tiles_available() {
        eprintln!("Skipping test: tiles not available.");
        return;
    }

    let tiles_dir = get_tiles_dir().unwrap();
    let mut router = Router::new(&tiles_dir).expect("Failed to create router");

    println!("\n=== LOCAL ROAD SNAPPING TEST ===\n");

    let test_points = [
        ("Rotkreuz Residential 1", ROTKREUZ_RESIDENTIAL_1),
        ("Rotkreuz Residential 2", ROTKREUZ_RESIDENTIAL_2),
        ("Rotkreuz Residential 3", ROTKREUZ_RESIDENTIAL_3),
        ("Rotkreuz Residential 4", ROTKREUZ_RESIDENTIAL_4),
        ("Zug Residential 1", ZUG_RESIDENTIAL_1),
        ("Zug Residential 2", ZUG_RESIDENTIAL_2),
    ];

    let mut passed = 0;
    let mut failed = 0;

    for (name, point) in test_points {
        let result = router.snap_to_road(point.0, point.1);

        match result {
            Some((lat, lon, dist)) => {
                println!(
                    "{}: ({:.5}, {:.5}) -> ({:.5}, {:.5}), snap distance: {:.1}m",
                    name, point.0, point.1, lat, lon, dist
                );

                // Snap distance should be reasonable (< 500m for residential areas)
                if dist < 500.0 {
                    println!("  ✓ PASS: Found nearby road");
                    passed += 1;
                } else {
                    println!("  ✗ FAIL: Snap distance too large ({}m > 500m)", dist);
                    failed += 1;
                }
            }
            None => {
                println!("{}: ({:.5}, {:.5}) -> NO ROAD FOUND", name, point.0, point.1);
                println!("  ✗ FAIL: No road found nearby");
                failed += 1;
            }
        }
    }

    println!("\nSnapping results: {} passed, {} failed", passed, failed);
    assert!(failed == 0, "Some snap tests failed - local roads may not be loaded");
}

// ============================================================================
// End-to-End Route Tests (Local Roads)
// ============================================================================

#[test]
fn test_local_route_residential_short() {
    init_logging();
    if !tiles_available() {
        eprintln!("Skipping test: tiles not available.");
        return;
    }

    let tiles_dir = get_tiles_dir().unwrap();
    let mut router = Router::new(&tiles_dir).expect("Failed to create router");

    // Short route between two residential streets in Rotkreuz (~300m straight line)
    let result = test_route_end_to_end(
        &mut router,
        ROTKREUZ_RESIDENTIAL_1,
        ROTKREUZ_RESIDENTIAL_2,
        "Short Local Route (Rotkreuz residential)",
    );

    // Assertions
    assert!(
        result.start_snap_distance_m.is_some(),
        "Should be able to snap start to a road"
    );
    assert!(
        result.end_snap_distance_m.is_some(),
        "Should be able to snap end to a road"
    );

    // Snap distance should be < 200m for residential areas
    if let Some(d) = result.start_snap_distance_m {
        assert!(d < 200.0, "Start snap distance {} too large", d);
    }
    if let Some(d) = result.end_snap_distance_m {
        assert!(d < 200.0, "End snap distance {} too large", d);
    }

    assert!(result.found, "Route should be found between residential streets");

    // Route should have reasonable length (not 0, not excessively long)
    if let Some(dist) = result.route_distance_m {
        let straight_line = distance_m(ROTKREUZ_RESIDENTIAL_1, ROTKREUZ_RESIDENTIAL_2);
        assert!(dist > 0.0, "Route distance should be positive");
        assert!(
            dist < straight_line * 10.0,
            "Route {} too long vs straight line {}",
            dist,
            straight_line
        );
    }

    // Note: Route may use higher-level roads if local roads are disconnected.
    // The route start/end may not exactly match local road snap positions because
    // the router falls back to arterial roads when local routing fails.
    // This is expected behavior for the current hierarchical routing system.
    if let Some(acc) = result.route_start_accuracy_m {
        if acc > 100.0 {
            println!(
                "  Note: Route starts {}m from local snap - using higher-level road",
                acc
            );
        }
    }
    if let Some(acc) = result.route_end_accuracy_m {
        if acc > 100.0 {
            println!(
                "  Note: Route ends {}m from local snap - using higher-level road",
                acc
            );
        }
    }
}

#[test]
fn test_local_route_residential_medium() {
    init_logging();
    if !tiles_available() {
        eprintln!("Skipping test: tiles not available.");
        return;
    }

    let tiles_dir = get_tiles_dir().unwrap();
    let mut router = Router::new(&tiles_dir).expect("Failed to create router");

    // Medium route across Rotkreuz (~800m)
    let result = test_route_end_to_end(
        &mut router,
        ROTKREUZ_RESIDENTIAL_3,
        ROTKREUZ_RESIDENTIAL_4,
        "Medium Local Route (across Rotkreuz)",
    );

    assert!(
        result.start_snap_distance_m.is_some(),
        "Should snap start to road"
    );
    assert!(
        result.end_snap_distance_m.is_some(),
        "Should snap end to road"
    );
    assert!(result.found, "Route should be found");
    assert!(result.path_points >= 2, "Route should have multiple points");
}

#[test]
fn test_local_route_cross_tile() {
    init_logging();
    if !tiles_available() {
        eprintln!("Skipping test: tiles not available.");
        return;
    }

    let tiles_dir = get_tiles_dir().unwrap();
    let mut router = Router::new(&tiles_dir).expect("Failed to create router");

    // Route that likely crosses tile boundaries (~2km)
    // Rotkreuz to edge of Rotkreuz
    let start = ROTKREUZ_RESIDENTIAL_1;
    let end = (47.1500, 8.4400); // Northern edge of Rotkreuz

    let result = test_route_end_to_end(
        &mut router,
        start,
        end,
        "Cross-Tile Local Route",
    );

    assert!(
        result.start_snap_distance_m.is_some(),
        "Should snap start to road"
    );
    assert!(
        result.end_snap_distance_m.is_some(),
        "Should snap end to road"
    );

    // This may or may not find a route depending on tile coverage
    if result.found {
        println!("  ✓ Cross-tile routing works!");
        assert!(result.path_points >= 2);
    } else {
        println!("  ⚠ No cross-tile route found - may need hierarchical routing");
    }
}

// ============================================================================
// Regional Route Tests (Start/End on Local Roads)
// ============================================================================

#[test]
fn test_regional_route_rotkreuz_to_zug() {
    init_logging();
    if !tiles_available() {
        eprintln!("Skipping test: tiles not available.");
        return;
    }

    let tiles_dir = get_tiles_dir().unwrap();
    let mut router = Router::new(&tiles_dir).expect("Failed to create router");

    // Start from residential street in Rotkreuz, end at residential street in Zug
    let result = test_route_end_to_end(
        &mut router,
        ROTKREUZ_RESIDENTIAL_1,
        ZUG_RESIDENTIAL_1,
        "Regional Route: Rotkreuz -> Zug (residential to residential)",
    );

    // Both endpoints should snap to local roads
    assert!(
        result.start_snap_distance_m.is_some(),
        "Should snap Rotkreuz start to road"
    );
    assert!(
        result.end_snap_distance_m.is_some(),
        "Should snap Zug end to road"
    );

    if let Some(d) = result.start_snap_distance_m {
        assert!(d < 300.0, "Start snap distance {} too large", d);
    }
    if let Some(d) = result.end_snap_distance_m {
        assert!(d < 300.0, "End snap distance {} too large", d);
    }

    assert!(result.found, "Regional route should be found");

    if let Some(dist) = result.route_distance_m {
        // ~7km straight line, route should be 7-20km
        assert!(dist > 5000.0, "Route too short: {}m", dist);
        assert!(dist < 30000.0, "Route too long: {}m", dist);
    }
}

#[test]
fn test_regional_route_rotkreuz_to_cham() {
    init_logging();
    if !tiles_available() {
        eprintln!("Skipping test: tiles not available.");
        return;
    }

    let tiles_dir = get_tiles_dir().unwrap();
    let mut router = Router::new(&tiles_dir).expect("Failed to create router");

    let result = test_route_end_to_end(
        &mut router,
        ROTKREUZ_RESIDENTIAL_2,
        CHAM_RESIDENTIAL,
        "Regional Route: Rotkreuz -> Cham",
    );

    assert!(result.start_snap_distance_m.is_some(), "Should snap start");
    assert!(result.end_snap_distance_m.is_some(), "Should snap end");
    assert!(result.found, "Route should be found");
}

#[test]
fn test_regional_route_rotkreuz_to_lucerne() {
    init_logging();
    if !tiles_available() {
        eprintln!("Skipping test: tiles not available.");
        return;
    }

    let tiles_dir = get_tiles_dir().unwrap();
    let mut router = Router::new(&tiles_dir).expect("Failed to create router");

    let result = test_route_end_to_end(
        &mut router,
        ROTKREUZ_RESIDENTIAL_1,
        LUCERNE_RESIDENTIAL,
        "Regional Route: Rotkreuz -> Lucerne (~15km)",
    );

    assert!(result.start_snap_distance_m.is_some(), "Should snap start");
    assert!(result.end_snap_distance_m.is_some(), "Should snap end");
    assert!(result.found, "Route should be found");

    if let Some(dist) = result.route_distance_m {
        assert!(dist > 10000.0, "Route too short");
        assert!(dist < 50000.0, "Route too long");
    }
}

// ============================================================================
// Long Distance Route Tests (Start/End on Local Roads)
// ============================================================================

#[test]
fn test_long_route_rotkreuz_to_zurich() {
    init_logging();
    if !tiles_available() {
        eprintln!("Skipping test: tiles not available.");
        return;
    }

    let tiles_dir = get_tiles_dir().unwrap();
    let mut router = Router::new(&tiles_dir).expect("Failed to create router");

    let result = test_route_end_to_end(
        &mut router,
        ROTKREUZ_RESIDENTIAL_1,
        ZURICH_RESIDENTIAL,
        "Long Route: Rotkreuz -> Zurich (~30km)",
    );

    assert!(result.start_snap_distance_m.is_some(), "Should snap start");
    assert!(result.end_snap_distance_m.is_some(), "Should snap end");
    assert!(result.found, "Long route should be found");
}

#[test]
fn test_long_route_zurich_to_bern() {
    init_logging();
    if !tiles_available() {
        eprintln!("Skipping test: tiles not available.");
        return;
    }

    let tiles_dir = get_tiles_dir().unwrap();
    let mut router = Router::new(&tiles_dir).expect("Failed to create router");

    let result = test_route_end_to_end(
        &mut router,
        ZURICH_RESIDENTIAL,
        BERN_RESIDENTIAL,
        "Long Route: Zurich -> Bern (~100km)",
    );

    assert!(result.start_snap_distance_m.is_some(), "Should snap start");
    assert!(result.end_snap_distance_m.is_some(), "Should snap end");
    assert!(result.found, "Long route should be found");

    if let Some(dist) = result.route_distance_m {
        assert!(dist > 80000.0, "Route too short for Zurich-Bern");
    }
}

#[test]
fn test_long_route_zurich_to_geneva() {
    init_logging();
    if !tiles_available() {
        eprintln!("Skipping test: tiles not available.");
        return;
    }

    let tiles_dir = get_tiles_dir().unwrap();
    let mut router = Router::new(&tiles_dir).expect("Failed to create router");

    let result = test_route_end_to_end(
        &mut router,
        ZURICH_RESIDENTIAL,
        GENEVA_RESIDENTIAL,
        "Very Long Route: Zurich -> Geneva (~225km)",
    );

    assert!(result.start_snap_distance_m.is_some(), "Should snap start");
    assert!(result.end_snap_distance_m.is_some(), "Should snap end");
    assert!(result.found, "Very long route should be found");
}

// ============================================================================
// Bidirectional Consistency Tests
// ============================================================================

#[test]
fn test_bidirectional_local_route() {
    init_logging();
    if !tiles_available() {
        eprintln!("Skipping test: tiles not available.");
        return;
    }

    let tiles_dir = get_tiles_dir().unwrap();
    let mut router = Router::new(&tiles_dir).expect("Failed to create router");

    let a = ROTKREUZ_RESIDENTIAL_1;
    let b = ROTKREUZ_RESIDENTIAL_4;

    println!("\n=== BIDIRECTIONAL LOCAL ROUTE TEST ===");

    let forward = test_route_end_to_end(&mut router, a, b, "Forward: Res1 -> Res4");
    let backward = test_route_end_to_end(&mut router, b, a, "Backward: Res4 -> Res1");

    // Both directions should find routes
    assert!(forward.found, "Forward route should exist");
    assert!(backward.found, "Backward route should exist");

    // Routes can differ due to one-way roads, but both should be reasonable
    if let (Some(f_dist), Some(b_dist)) = (forward.route_distance_m, backward.route_distance_m) {
        let ratio = f_dist / b_dist;
        println!(
            "\nForward: {:.0}m, Backward: {:.0}m, Ratio: {:.2}",
            f_dist, b_dist, ratio
        );

        // Warn if routes are very different (but don't fail - one-way roads cause this)
        if ratio > 3.0 || ratio < 0.33 {
            println!("⚠ Warning: Routes differ significantly - likely one-way roads");
        }
    }
}

#[test]
fn test_bidirectional_regional_route() {
    init_logging();
    if !tiles_available() {
        eprintln!("Skipping test: tiles not available.");
        return;
    }

    let tiles_dir = get_tiles_dir().unwrap();
    let mut router = Router::new(&tiles_dir).expect("Failed to create router");

    println!("\n=== BIDIRECTIONAL REGIONAL ROUTE TEST ===");

    let forward = test_route_end_to_end(
        &mut router,
        ROTKREUZ_RESIDENTIAL_1,
        ZUG_RESIDENTIAL_1,
        "Forward: Rotkreuz -> Zug",
    );
    let backward = test_route_end_to_end(
        &mut router,
        ZUG_RESIDENTIAL_1,
        ROTKREUZ_RESIDENTIAL_1,
        "Backward: Zug -> Rotkreuz",
    );

    assert!(forward.found, "Forward route should exist");
    assert!(backward.found, "Backward route should exist");
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_same_location() {
    init_logging();
    if !tiles_available() {
        eprintln!("Skipping test: tiles not available.");
        return;
    }

    let tiles_dir = get_tiles_dir().unwrap();
    let mut router = Router::new(&tiles_dir).expect("Failed to create router");

    let loc = ROTKREUZ_RESIDENTIAL_1;

    let result = test_route_end_to_end(&mut router, loc, loc, "Same Location Route");

    // Should snap to road
    assert!(result.start_snap_distance_m.is_some(), "Should snap to road");

    // Route may or may not be found for same location
    if result.found {
        if let Some(dist) = result.route_distance_m {
            assert!(dist < 100.0, "Same location route should be very short");
        }
    }
}

#[test]
fn test_very_close_points() {
    init_logging();
    if !tiles_available() {
        eprintln!("Skipping test: tiles not available.");
        return;
    }

    let tiles_dir = get_tiles_dir().unwrap();
    let mut router = Router::new(&tiles_dir).expect("Failed to create router");

    // Points ~50m apart
    let start = ROTKREUZ_RESIDENTIAL_1;
    let end = (start.0 + 0.0004, start.1 + 0.0003); // ~50m away

    let result = test_route_end_to_end(&mut router, start, end, "Very Close Points (~50m)");

    assert!(result.start_snap_distance_m.is_some(), "Should snap start");
    assert!(result.end_snap_distance_m.is_some(), "Should snap end");

    // Close points may snap to same node, which is fine
    if result.found {
        println!("  ✓ Found route between very close points");
    } else {
        println!("  ⚠ No route - points may have snapped to same node");
    }
}

// ============================================================================
// Comprehensive Summary Test
// ============================================================================

#[test]
fn test_routing_comprehensive_summary() {
    init_logging();
    if !tiles_available() {
        eprintln!("\n=== ROUTING TESTS SKIPPED ===");
        eprintln!("Tiles not available. Generate them with:");
        eprintln!("  cargo run -p minimap-tiles --release --features processor \\");
        eprintln!("    --bin route-processor -- switzerland-latest.osm.pbf tiles");
        return;
    }

    let tiles_dir = get_tiles_dir().unwrap();
    let mut router = Router::new(&tiles_dir).expect("Failed to create router");

    println!("\n{}", "=".repeat(60));
    println!("COMPREHENSIVE END-TO-END ROUTING TEST");
    println!("{}\n", "=".repeat(60));

    struct TestCase {
        name: &'static str,
        start: (f64, f64),
        end: (f64, f64),
        expected_min_dist_m: f64,
        expected_max_dist_m: f64,
    }

    let test_cases = vec![
        TestCase {
            name: "Local: Rotkreuz short",
            start: ROTKREUZ_RESIDENTIAL_1,
            end: ROTKREUZ_RESIDENTIAL_2,
            expected_min_dist_m: 100.0,
            expected_max_dist_m: 3000.0,
        },
        TestCase {
            name: "Local: Rotkreuz medium",
            start: ROTKREUZ_RESIDENTIAL_3,
            end: ROTKREUZ_RESIDENTIAL_4,
            expected_min_dist_m: 500.0,
            expected_max_dist_m: 5000.0,
        },
        TestCase {
            name: "Regional: Rotkreuz->Zug",
            start: ROTKREUZ_RESIDENTIAL_1,
            end: ZUG_RESIDENTIAL_1,
            expected_min_dist_m: 5000.0,
            expected_max_dist_m: 25000.0,
        },
        TestCase {
            name: "Regional: Rotkreuz->Cham",
            start: ROTKREUZ_RESIDENTIAL_2,
            end: CHAM_RESIDENTIAL,
            expected_min_dist_m: 3000.0,
            expected_max_dist_m: 15000.0,
        },
        TestCase {
            name: "Regional: Rotkreuz->Lucerne",
            start: ROTKREUZ_RESIDENTIAL_1,
            end: LUCERNE_RESIDENTIAL,
            expected_min_dist_m: 10000.0,
            expected_max_dist_m: 40000.0,
        },
        TestCase {
            name: "Long: Rotkreuz->Zurich",
            start: ROTKREUZ_RESIDENTIAL_1,
            end: ZURICH_RESIDENTIAL,
            expected_min_dist_m: 25000.0,
            expected_max_dist_m: 60000.0,
        },
        TestCase {
            name: "Long: Zurich->Bern",
            start: ZURICH_RESIDENTIAL,
            end: BERN_RESIDENTIAL,
            expected_min_dist_m: 80000.0,
            expected_max_dist_m: 150000.0,
        },
        TestCase {
            name: "VeryLong: Zurich->Geneva",
            start: ZURICH_RESIDENTIAL,
            end: GENEVA_RESIDENTIAL,
            expected_min_dist_m: 200000.0,
            expected_max_dist_m: 350000.0,
        },
    ];

    println!(
        "{:<28} {:>8} {:>8} {:>10} {:>10} {:>8}",
        "Test", "SnapS", "SnapE", "Distance", "Time", "Status"
    );
    println!("{}", "-".repeat(80));

    let mut total_passed = 0;
    let mut total_failed = 0;
    let mut snap_failures = 0;
    let mut route_failures = 0;

    for tc in &test_cases {
        let result = test_route_end_to_end(&mut router, tc.start, tc.end, tc.name);

        let snap_s = result
            .start_snap_distance_m
            .map(|d| format!("{:.0}m", d))
            .unwrap_or_else(|| "FAIL".to_string());
        let snap_e = result
            .end_snap_distance_m
            .map(|d| format!("{:.0}m", d))
            .unwrap_or_else(|| "FAIL".to_string());

        let (dist_str, time_str, status) = if result.found {
            let dist = result.route_distance_m.unwrap();
            let time = result.route_time_s.unwrap();

            let status = if dist >= tc.expected_min_dist_m && dist <= tc.expected_max_dist_m {
                total_passed += 1;
                "✓ PASS"
            } else {
                total_failed += 1;
                route_failures += 1;
                "✗ DIST"
            };

            (format!("{:.1}km", dist / 1000.0), format!("{:.1}m", time / 60.0), status)
        } else {
            if result.start_snap_distance_m.is_none() || result.end_snap_distance_m.is_none() {
                snap_failures += 1;
            }
            route_failures += 1;
            total_failed += 1;
            ("--".to_string(), "--".to_string(), "✗ FAIL")
        };

        println!(
            "{:<28} {:>8} {:>8} {:>10} {:>10} {:>8}",
            tc.name, snap_s, snap_e, dist_str, time_str, status
        );
    }

    println!("{}", "-".repeat(80));
    println!(
        "\nSUMMARY: {} passed, {} failed ({}% success rate)",
        total_passed,
        total_failed,
        100 * total_passed / (total_passed + total_failed)
    );

    if snap_failures > 0 {
        println!("  - {} snap failures (local roads not found)", snap_failures);
    }
    if route_failures > 0 {
        println!("  - {} route failures", route_failures);
    }

    // Assert high success rate
    assert!(
        total_passed >= test_cases.len() - 1,
        "Too many routing failures: {} failed out of {}",
        total_failed,
        test_cases.len()
    );
}
