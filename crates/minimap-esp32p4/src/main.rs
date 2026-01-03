//! NFS2 Minimap - ESP32-P4 Firmware
//!
//! This is the device firmware for the Waveshare ESP32-P4-WIFI6-Touch-LCD-3.4C
//!
//! Hardware setup:
//! - 800x800 round MIPI-DSI display
//! - GPS module connected via UART (configurable pins)
//! - Optional: CAN bus for vehicle data
//!
//! Build with ESP-IDF toolchain:
//!   cargo build --release --target riscv32imafc-esp-espidf

// Needed for ESP-IDF
use esp_idf_sys as _;

use esp_idf_hal::prelude::*;
use esp_idf_hal::uart::UartDriver;
use log::*;

use minimap_core::{
    MapRenderer, MinimapConfig, VehicleState,
    WorldRoad, WorldPoi, WorldArea, WorldWaterway,
    RoadType, PoiType, AreaType, WaterwayType,
};
use minimap_routing::{Router, RoutingConfig, VehiclePosition};
use minimap_tiles::loader::TileLoader;

use std::time::{Duration, Instant};

slint::include_modules!();

// ============================================================================
// Pre-programmed Destinations
// ============================================================================

/// Pre-programmed navigation destinations stored in flash
/// These can be selected via touch UI or physical buttons
#[derive(Clone, Copy)]
struct Destination {
    name: &'static str,
    lat: f64,
    lon: f64,
}

/// List of pre-programmed destinations
/// Edit this list to customize for your use case
const DESTINATIONS: &[Destination] = &[
    Destination {
        name: "Home",
        lat: 47.1415,
        lon: 8.4320,
    },
    Destination {
        name: "Work",
        lat: 47.3769,
        lon: 8.5417,
    },
    Destination {
        name: "Gas Station",
        lat: 47.1450,
        lon: 8.4380,
    },
    Destination {
        name: "Shopping",
        lat: 47.1380,
        lon: 8.4250,
    },
];

// ============================================================================
// Navigation State
// ============================================================================

struct NavigationState {
    router: Router,
    selected_destination: Option<usize>,
    is_navigating: bool,
    last_reroute_check: Instant,
}

impl NavigationState {
    fn new() -> Self {
        Self {
            router: Router::new(RoutingConfig::default()),
            selected_destination: None,
            is_navigating: false,
            last_reroute_check: Instant::now(),
        }
    }

    /// Start navigation to a pre-programmed destination
    fn start_navigation(&mut self, dest_idx: usize, vehicle: &VehicleState, renderer: &mut MapRenderer) -> bool {
        if dest_idx >= DESTINATIONS.len() {
            warn!("Invalid destination index: {}", dest_idx);
            return false;
        }

        let dest = &DESTINATIONS[dest_idx];
        info!("Starting navigation to: {} ({:.6}, {:.6})", dest.name, dest.lat, dest.lon);

        let vehicle_pos = VehiclePosition {
            lat: vehicle.latitude,
            lon: vehicle.longitude,
            heading: vehicle.heading,
        };

        if let Some(route) = self.router.calculate_route(vehicle_pos, (dest.lat, dest.lon)) {
            info!("Route found: {} waypoints, {:.0}m total", route.waypoints.len(), route.total_distance_m);
            renderer.set_route_from_routing(route);
            self.selected_destination = Some(dest_idx);
            self.is_navigating = true;
            true
        } else {
            warn!("No route found to {}", dest.name);
            // Create simple straight-line route as fallback
            let simple_route = minimap_core::ActiveRoute {
                waypoints: vec![
                    (vehicle.latitude, vehicle.longitude),
                    (dest.lat, dest.lon),
                ],
                lane_guidance: None,
                next_maneuver_distance: haversine_distance_m(
                    vehicle.latitude, vehicle.longitude,
                    dest.lat, dest.lon,
                ),
                next_maneuver_type: 3, // straight
            };
            renderer.set_route(Some(simple_route));
            self.selected_destination = Some(dest_idx);
            self.is_navigating = true;
            true
        }
    }

    /// Stop current navigation
    fn stop_navigation(&mut self, renderer: &mut MapRenderer) {
        info!("Stopping navigation");
        self.router.clear_route();
        renderer.clear_route();
        self.selected_destination = None;
        self.is_navigating = false;
    }

    /// Cycle to next destination (for button-based selection)
    fn next_destination(&mut self) -> usize {
        match self.selected_destination {
            Some(idx) => (idx + 1) % DESTINATIONS.len(),
            None => 0,
        }
    }

    /// Check if vehicle is off-route and needs re-routing
    fn check_reroute(&mut self, vehicle: &VehicleState, renderer: &mut MapRenderer) {
        if !self.is_navigating {
            return;
        }

        // Only check every 2 seconds to save CPU
        let now = Instant::now();
        if now.duration_since(self.last_reroute_check) < Duration::from_secs(2) {
            return;
        }
        self.last_reroute_check = now;

        let vehicle_pos = VehiclePosition {
            lat: vehicle.latitude,
            lon: vehicle.longitude,
            heading: vehicle.heading,
        };

        if self.router.check_off_route(vehicle_pos) {
            info!("Vehicle off-route, recalculating...");
            if let Some(dest_idx) = self.selected_destination {
                self.start_navigation(dest_idx, vehicle, renderer);
            }
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Calculate haversine distance between two lat/lon points in meters
fn haversine_distance_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f32 {
    const EARTH_RADIUS_M: f64 = 6_371_000.0;

    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();

    let a = (dlat / 2.0).sin().powi(2)
        + lat1_rad.cos() * lat2_rad.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();

    (EARTH_RADIUS_M * c) as f32
}

/// Load map data from tiles for current position
fn load_tiles_for_position(
    loader: &mut TileLoader,
    renderer: &mut MapRenderer,
    lat: f64,
    lon: f64,
) {
    // Load visible tiles (roughly 1.2km radius)
    loader.load_visible(lat, lon, 0.015);

    // Convert tile data to renderer format
    let roads: Vec<WorldRoad> = loader
        .get_roads()
        .into_iter()
        .map(|(road_type, points)| WorldRoad {
            name: None,
            points,
            road_type: match road_type {
                minimap_tiles::RoadType::Primary => RoadType::Primary,
                minimap_tiles::RoadType::Secondary => RoadType::Secondary,
            },
        })
        .collect();

    let pois: Vec<WorldPoi> = loader
        .get_pois()
        .into_iter()
        .map(|(poi_type, lat, lon)| WorldPoi {
            name: None,
            lat,
            lon,
            poi_type: match poi_type {
                minimap_tiles::PoiType::GasStation => PoiType::GasStation,
                minimap_tiles::PoiType::Parking => PoiType::Parking,
                minimap_tiles::PoiType::ShoppingMall => PoiType::ShoppingMall,
                minimap_tiles::PoiType::CarWash => PoiType::CarWash,
            },
        })
        .collect();

    let areas: Vec<WorldArea> = loader
        .get_areas()
        .into_iter()
        .map(|(area_type, points)| WorldArea {
            points,
            area_type: match area_type {
                minimap_tiles::AreaType::Water => AreaType::Water,
                minimap_tiles::AreaType::Forest => AreaType::Forest,
                minimap_tiles::AreaType::Park => AreaType::Park,
                minimap_tiles::AreaType::Grass => AreaType::Grass,
            },
        })
        .collect();

    let waterways: Vec<WorldWaterway> = loader
        .get_waterways()
        .into_iter()
        .map(|(waterway_type, points)| WorldWaterway {
            points,
            waterway_type: match waterway_type {
                minimap_tiles::WaterwayType::River => WaterwayType::River,
                minimap_tiles::WaterwayType::Stream => WaterwayType::Stream,
                minimap_tiles::WaterwayType::Canal => WaterwayType::Canal,
            },
        })
        .collect();

    renderer.set_roads(roads);
    renderer.set_pois(pois);
    renderer.set_areas(areas);
    renderer.set_waterways(waterways);
}

// ============================================================================
// Main Entry Point
// ============================================================================

fn main() -> anyhow::Result<()> {
    // Initialize ESP-IDF
    esp_idf_sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("NFS2 Minimap starting on ESP32-P4");
    info!("Pre-programmed destinations: {}", DESTINATIONS.len());
    for (i, dest) in DESTINATIONS.iter().enumerate() {
        info!("  [{}] {} ({:.4}, {:.4})", i, dest.name, dest.lat, dest.lon);
    }

    // Get peripherals
    let _peripherals = Peripherals::take()?;

    // TODO: Initialize MIPI-DSI display
    info!("Initializing display...");

    // Initialize Slint platform for ESP32-P4
    init_slint_platform()?;

    // Create the UI
    let ui = Minimap::new()?;

    // Set up map renderer
    let config = MinimapConfig {
        screen_width: 800.0,
        screen_height: 800.0,
        meters_per_pixel: 1.5,
        rotate_with_heading: true,
    };
    let mut renderer = MapRenderer::new(config);

    // Initialize tile loader from SD card
    // Path depends on ESP-IDF VFS mount point
    let tile_path = "/sdcard/tiles";
    let mut tile_loader = match TileLoader::new(tile_path) {
        Ok(loader) => {
            info!("Loaded tiles from {}", tile_path);
            Some(loader)
        }
        Err(e) => {
            warn!("Failed to load tiles from {}: {}. Using sample roads.", tile_path, e);
            None
        }
    };

    // Initialize navigation state
    let mut nav_state = NavigationState::new();

    // Vehicle state
    let sample_lat = 47.1415;  // Rotkreuz, Switzerland
    let sample_lon = 8.4320;
    let mut vehicle = VehicleState {
        latitude: sample_lat,
        longitude: sample_lon,
        heading: 0.0,
        speed_kmh: 0.0,
    };

    // Initial tile load
    if let Some(ref mut loader) = tile_loader {
        load_tiles_for_position(loader, &mut renderer, vehicle.latitude, vehicle.longitude);
    }

    // Timing for tile updates
    let mut last_tile_update = Instant::now();
    let tile_update_interval = Duration::from_millis(500);

    // Navigation button state (for physical button debouncing)
    let mut nav_button_pressed = false;
    let mut last_button_time = Instant::now();
    let button_debounce = Duration::from_millis(300);

    info!("Entering main loop");
    info!("Navigation: Press NAV button to cycle destinations, long-press to start/stop");

    // Main loop
    loop {
        let now = Instant::now();

        // Read GPS data
        // TODO: Parse NMEA sentences from UART
        // if let Some(gps_data) = read_gps(&uart) {
        //     vehicle.latitude = gps_data.latitude;
        //     vehicle.longitude = gps_data.longitude;
        //     vehicle.heading = gps_data.heading;
        //     vehicle.speed_kmh = gps_data.speed_kmh;
        // }

        // Check navigation button (GPIO or touch)
        // TODO: Replace with actual button/touch reading
        let button_state = false; // read_nav_button();
        if button_state && !nav_button_pressed &&
           now.duration_since(last_button_time) > button_debounce {
            nav_button_pressed = true;
            last_button_time = now;

            if nav_state.is_navigating {
                // Stop navigation
                nav_state.stop_navigation(&mut renderer);
            } else {
                // Start navigation to next destination
                let dest_idx = nav_state.next_destination();
                nav_state.start_navigation(dest_idx, &vehicle, &mut renderer);
            }
        } else if !button_state {
            nav_button_pressed = false;
        }

        // Update tiles periodically
        if now.duration_since(last_tile_update) >= tile_update_interval {
            last_tile_update = now;
            if let Some(ref mut loader) = tile_loader {
                load_tiles_for_position(loader, &mut renderer, vehicle.latitude, vehicle.longitude);
            }
        }

        // Check for re-routing if navigating
        nav_state.check_reroute(&vehicle, &mut renderer);

        // Render map
        let segments = renderer.render(&vehicle);
        let pois = renderer.render_pois(&vehicle);
        let area_triangles = renderer.render_area_triangles(&vehicle);
        let waterways = renderer.render_waterways(&vehicle);
        let route_segments = renderer.render_route(&vehicle);

        // Combine road and route segments
        let mut all_segments = segments;
        all_segments.extend(route_segments);

        // Update Slint UI - Roads
        let road_model: Vec<RoadSegment> = all_segments
            .iter()
            .map(|seg| RoadSegment {
                x1: seg.x1,
                y1: seg.y1,
                x2: seg.x2,
                y2: seg.y2,
                road_type: seg.road_type,
            })
            .collect();

        // Update Slint UI - POIs
        let poi_model: Vec<Poi> = pois
            .iter()
            .map(|p| Poi {
                x: p.x,
                y: p.y,
                poi_type: p.poi_type,
            })
            .collect();

        // Update Slint UI - Areas
        let area_model: Vec<AreaTriangle> = area_triangles
            .iter()
            .map(|t| AreaTriangle {
                x1: t.x1,
                y1: t.y1,
                x2: t.x2,
                y2: t.y2,
                x3: t.x3,
                y3: t.y3,
                area_type: t.area_type,
            })
            .collect();

        // Update Slint UI - Waterways
        let waterway_model: Vec<WaterwaySegment> = waterways
            .iter()
            .map(|w| WaterwaySegment {
                x1: w.x1,
                y1: w.y1,
                x2: w.x2,
                y2: w.y2,
                waterway_type: w.waterway_type,
            })
            .collect();

        ui.set_roads(slint::ModelRc::new(slint::VecModel::from(road_model)));
        ui.set_pois(slint::ModelRc::new(slint::VecModel::from(poi_model)));
        ui.set_areas(slint::ModelRc::new(slint::VecModel::from(area_model)));
        ui.set_waterways(slint::ModelRc::new(slint::VecModel::from(waterway_model)));
        ui.set_player(PlayerState {
            heading: vehicle.heading,
            speed_kmh: vehicle.speed_kmh,
            latitude: vehicle.latitude as f32,
            longitude: vehicle.longitude as f32,
        });

        // Update junction view
        ui.set_junction_visible(renderer.should_show_junction());
        ui.set_junction_distance_m(renderer.junction_distance());
        ui.set_junction_destination(renderer.junction_destination().into());
        ui.set_junction_turn_type(renderer.junction_turn_type());

        // Update lane guidance
        if let Some(guidance) = renderer.lane_guidance() {
            let lanes: Vec<LaneDisplay> = guidance
                .lanes
                .iter()
                .map(|(turn_type, is_recommended)| LaneDisplay {
                    turn_type: *turn_type,
                    is_recommended: *is_recommended,
                })
                .collect();
            ui.set_junction_lanes(slint::ModelRc::new(slint::VecModel::from(lanes)));
        } else {
            ui.set_junction_lanes(slint::ModelRc::new(slint::VecModel::from(Vec::<LaneDisplay>::new())));
        }

        // Process Slint events and render
        slint::platform::update_timers_and_animations();

        // Small delay to prevent busy-looping (~60 FPS)
        std::thread::sleep(Duration::from_millis(16));
    }
}

/// Initialize the Slint platform for ESP32-P4
///
/// This sets up the software renderer and display backend
fn init_slint_platform() -> anyhow::Result<()> {
    // TODO: Implement custom Platform trait
    // This involves:
    // 1. Creating a WindowAdapter that renders to a framebuffer
    // 2. Setting up the MIPI-DSI display driver
    // 3. Handling touch input from the FT6X36 driver
    //
    // For reference, see:
    // - https://github.com/slint-ui/slint/tree/master/examples/mcu-board-support
    // - https://slint.dev/docs/cpp/mcu/esp_idf.html

    info!("Slint platform initialization placeholder");
    info!("Full implementation requires MIPI-DSI driver integration");

    // When implementing, you'll need something like:
    // slint::platform::set_platform(Box::new(Esp32P4Platform::new(...)))?;

    Ok(())
}

/// Read and parse GPS NMEA data
#[allow(dead_code)]
fn read_gps(_uart: &UartDriver) -> Option<GpsData> {
    // TODO: Read UART buffer and parse NMEA sentences
    // Using the `nmea` crate for parsing
    //
    // Example NMEA parsing:
    // let sentence = nmea::parse_str(&buffer)?;
    // if let nmea::SentenceType::GGA(gga) = sentence {
    //     return Some(GpsData {
    //         latitude: gga.latitude?,
    //         longitude: gga.longitude?,
    //         ...
    //     });
    // }

    None
}

#[allow(dead_code)]
struct GpsData {
    latitude: f64,
    longitude: f64,
    heading: f32,
    speed_kmh: f32,
}
