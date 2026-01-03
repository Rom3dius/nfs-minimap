//! NFS2 Minimap Simulator
//!
//! Desktop application for testing and developing the minimap UI
//! without needing the actual ESP32-P4 hardware.
//!
//! Usage:
//!   simulator [tile-directory]
//!
//! If tile-directory is provided, loads map data from tiles.
//! Otherwise, fetches from Overpass API.
//!
//! Controls:
//! - W/S: Move forward/backward
//! - A/D: Rotate left/right
//! - Q/E: Zoom in/out
//! - T: Cycle theme
//!
//! With 'navigation' feature:
//! - Click: Navigate to clicked location
//! - 1-4: Navigate to pre-programmed destination
//! - C: Clear route

use minimap_core::{
    map_data::{self, BoundingBox},
    AreaType, MapRenderer, MinimapConfig, PoiType, RoadType, VehicleState, WaterwayType,
    WorldArea, WorldPoi, WorldRoad, WorldWaterway,
};
#[cfg(feature = "navigation")]
use minimap_routing::{Router, RoutingConfig, VehiclePosition};
use minimap_tiles::loader::TileLoader;

slint::include_modules!();

use slint::{ComponentHandle, ModelRc, VecModel};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

// ============================================================================
// Pre-programmed Destinations (navigation feature only)
// ============================================================================

#[cfg(feature = "navigation")]
#[derive(Clone)]
struct Destination {
    name: &'static str,
    lat: f64,
    lon: f64,
}

// Test destinations within loaded tile range (~1.5km from Rotkreuz center)
// For actual navigation, tiles need to be loaded along the route
#[cfg(feature = "navigation")]
const DESTINATIONS: &[Destination] = &[
    Destination {
        name: "Rotkreuz Center",
        lat: 47.1420,
        lon: 8.4310,
    },
    Destination {
        name: "Rotkreuz North",
        lat: 47.1480,
        lon: 8.4350,
    },
    Destination {
        name: "Rotkreuz East",
        lat: 47.1400,
        lon: 8.4420,
    },
    Destination {
        name: "Rotkreuz South",
        lat: 47.1350,
        lon: 8.4280,
    },
];

// ============================================================================
// Navigation State (navigation feature only)
// ============================================================================

#[cfg(feature = "navigation")]
struct NavigationState {
    is_navigating: bool,
    destination_name: String,
    destination_lat: f64,
    destination_lon: f64,
}

#[cfg(feature = "navigation")]
impl NavigationState {
    fn new() -> Self {
        Self {
            is_navigating: false,
            destination_name: String::new(),
            destination_lat: 0.0,
            destination_lon: 0.0,
        }
    }

    fn start(&mut self, name: &str, lat: f64, lon: f64) {
        self.is_navigating = true;
        self.destination_name = name.to_string();
        self.destination_lat = lat;
        self.destination_lon = lon;
    }

    fn stop(&mut self) {
        self.is_navigating = false;
        self.destination_name.clear();
    }
}

/// Calculate haversine distance between two lat/lon points in meters
#[cfg(feature = "navigation")]
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

/// Map data source - either tiles or static (from Overpass)
enum MapSource {
    Tiles(TileLoader),
    Static,
}

fn main() {
    env_logger::init();
    log::info!("Starting NFS2 Minimap Simulator");

    let args: Vec<String> = std::env::args().collect();
    let tile_dir = args.get(1);

    // Create the Slint UI (simulator wrapper with keyboard support)
    let ui = SimulatorWindow::new().unwrap();

    // Set up configuration for 800x800 display
    let config = MinimapConfig {
        screen_width: 800.0,
        screen_height: 800.0,
        meters_per_pixel: 1.5, // ~600m view radius
        rotate_with_heading: true,
    };

    // Initialize map renderer
    let mut renderer = MapRenderer::new(config);

    // Rotkreuz, ZG, Switzerland
    let center_lat = 47.1415;
    let center_lon = 8.4320;

    // Initialize map source
    let map_source = if let Some(dir) = tile_dir {
        log::info!("Using tile-based loading from: {}", dir);
        match TileLoader::new(dir) {
            Ok(loader) => MapSource::Tiles(loader),
            Err(e) => {
                log::error!("Failed to load tiles: {}. Falling back to Overpass.", e);
                load_from_overpass(&mut renderer, center_lat, center_lon);
                MapSource::Static
            }
        }
    } else {
        log::info!("Using Overpass API for map data");
        load_from_overpass(&mut renderer, center_lat, center_lon);
        MapSource::Static
    };

    // Vehicle state
    let vehicle = Rc::new(RefCell::new(VehicleState {
        latitude: center_lat,
        longitude: center_lon,
        heading: 0.0,
        speed_kmh: 0.0,
    }));

    // Track pressed keys
    let pressed_keys: Rc<RefCell<HashSet<String>>> = Rc::new(RefCell::new(HashSet::new()));

    // Wrap in RefCells
    let renderer = Rc::new(RefCell::new(renderer));
    let map_source = Rc::new(RefCell::new(map_source));

    // Navigation state (only with navigation feature)
    #[cfg(feature = "navigation")]
    let router = Rc::new(RefCell::new(Router::new(RoutingConfig::default())));
    #[cfg(feature = "navigation")]
    let nav_state = Rc::new(RefCell::new(NavigationState::new()));

    // Initial render
    {
        let mut source = map_source.borrow_mut();
        let veh = vehicle.borrow();
        #[cfg(feature = "navigation")]
        update_map_from_source(
            &mut source,
            &mut renderer.borrow_mut(),
            &mut router.borrow_mut(),
            veh.latitude,
            veh.longitude,
        );
        #[cfg(not(feature = "navigation"))]
        update_map_from_source(
            &mut source,
            &mut renderer.borrow_mut(),
            veh.latitude,
            veh.longitude,
        );
    }
    #[cfg(feature = "navigation")]
    update_ui(&ui, &renderer.borrow(), &vehicle.borrow(), &nav_state.borrow());
    #[cfg(not(feature = "navigation"))]
    update_ui(&ui, &renderer.borrow(), &vehicle.borrow());

    // Set up keyboard handlers
    let keys_clone = pressed_keys.clone();
    ui.on_key_pressed(move |key| {
        keys_clone.borrow_mut().insert(key.to_lowercase());
    });

    let keys_clone = pressed_keys.clone();
    ui.on_key_released(move |key| {
        keys_clone.borrow_mut().remove(&key.to_lowercase());
    });

    // Set up zoom button handlers
    let renderer_zoom_in = renderer.clone();
    ui.on_zoom_in(move || {
        renderer_zoom_in.borrow_mut().zoom_in(0.8);
    });

    let renderer_zoom_out = renderer.clone();
    ui.on_zoom_out(move || {
        renderer_zoom_out.borrow_mut().zoom_out(1.25);
    });

    // Set up click-to-navigate handler (navigation feature only)
    #[cfg(feature = "navigation")]
    {
        let renderer_click = renderer.clone();
        let router_click = router.clone();
        let vehicle_click = vehicle.clone();
        let nav_state_click = nav_state.clone();
        ui.on_map_clicked(move |screen_x, screen_y| {
            let renderer = renderer_click.borrow();
            let veh = vehicle_click.borrow();

            // Convert screen coordinates to world coordinates
            let center_x = 400.0;
            let center_y = 400.0;
            let dx = screen_x - center_x;
            let dy = screen_y - center_y;

            let heading_rad = -veh.heading.to_radians() as f64;
            let world_dx = dx as f64 * heading_rad.cos() - dy as f64 * heading_rad.sin();
            let world_dy = dx as f64 * heading_rad.sin() + dy as f64 * heading_rad.cos();

            let meters_per_pixel = renderer.zoom() as f64;
            let offset_meters_x = world_dx * meters_per_pixel;
            let offset_meters_y = -world_dy * meters_per_pixel;

            let meters_per_degree_lat = 111_320.0;
            let meters_per_degree_lon = 111_320.0 * veh.latitude.to_radians().cos();

            let dest_lat = veh.latitude + offset_meters_y / meters_per_degree_lat;
            let dest_lon = veh.longitude + offset_meters_x / meters_per_degree_lon;

            drop(renderer);
            drop(veh);

            let veh = vehicle_click.borrow();
            let vehicle_pos = VehiclePosition {
                lat: veh.latitude,
                lon: veh.longitude,
                heading: veh.heading,
            };

            log::info!("Click navigation to: ({:.6}, {:.6})", dest_lat, dest_lon);

            let mut router = router_click.borrow_mut();
            let mut renderer = renderer_click.borrow_mut();
            let mut nav = nav_state_click.borrow_mut();

            if let Some(route) = router.calculate_route(vehicle_pos, (dest_lat, dest_lon)) {
                log::info!("Route found: {} waypoints, {:.0}m", route.waypoints.len(), route.total_distance_m);
                renderer.set_route_from_routing(route);
                nav.start("Clicked Location", dest_lat, dest_lon);
            } else {
                log::info!("No route found, creating straight line");
                let simple_route = minimap_core::ActiveRoute {
                    waypoints: vec![
                        (veh.latitude, veh.longitude),
                        (dest_lat, dest_lon),
                    ],
                    lane_guidance: None,
                    next_maneuver_distance: haversine_distance_m(veh.latitude, veh.longitude, dest_lat, dest_lon),
                    next_maneuver_type: 3,
                };
                renderer.set_route(Some(simple_route));
                nav.start("Clicked Location", dest_lat, dest_lon);
            }
        });
    }
    #[cfg(not(feature = "navigation"))]
    ui.on_map_clicked(|_, _| {});

    // Set up animation timer
    let ui_weak = ui.as_weak();
    let vehicle_clone = vehicle.clone();
    let renderer_clone = renderer.clone();
    let map_source_clone = map_source.clone();
    #[cfg(feature = "navigation")]
    let router_clone = router.clone();
    #[cfg(feature = "navigation")]
    let nav_state_clone = nav_state.clone();
    let keys_clone = pressed_keys.clone();
    let last_update = Rc::new(RefCell::new(Instant::now()));
    let last_tile_update = Rc::new(RefCell::new(Instant::now()));
    #[cfg(feature = "navigation")]
    let nav_key_debounce = Rc::new(RefCell::new(Instant::now()));

    const MOVE_SPEED: f64 = 200.0;
    const ROTATE_SPEED: f32 = 90.0;

    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(16),
        move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };

            let now = Instant::now();
            let dt = now.duration_since(*last_update.borrow()).as_secs_f64();
            *last_update.borrow_mut() = now;

            // Handle WASD movement
            let keys = keys_clone.borrow();
            let mut veh = vehicle_clone.borrow_mut();

            if keys.contains("a") {
                veh.heading -= ROTATE_SPEED * dt as f32;
            }
            if keys.contains("d") {
                veh.heading += ROTATE_SPEED * dt as f32;
            }
            veh.heading = veh.heading.rem_euclid(360.0);

            let heading_rad = veh.heading.to_radians() as f64;
            let meters_per_degree_lat = 111_320.0;
            let meters_per_degree_lon = 111_320.0 * veh.latitude.to_radians().cos();

            if keys.contains("w") {
                let distance = MOVE_SPEED * dt;
                veh.latitude += heading_rad.cos() * distance / meters_per_degree_lat;
                veh.longitude += heading_rad.sin() * distance / meters_per_degree_lon;
            }
            if keys.contains("s") {
                let distance = MOVE_SPEED * dt;
                veh.latitude -= heading_rad.cos() * distance / meters_per_degree_lat;
                veh.longitude -= heading_rad.sin() * distance / meters_per_degree_lon;
            }

            // Handle zoom (Q = zoom in, E = zoom out)
            // Zoom factor per second: 2x
            let zoom_speed = 2.0_f32.powf(dt as f32);
            if keys.contains("q") {
                renderer_clone.borrow_mut().zoom_in(1.0 / zoom_speed);
            }
            if keys.contains("e") {
                renderer_clone.borrow_mut().zoom_out(zoom_speed);
            }

            // Handle navigation keys (navigation feature only)
            #[cfg(feature = "navigation")]
            {
                let debounce_duration = Duration::from_millis(500);
                let can_press_nav_key = now.duration_since(*nav_key_debounce.borrow()) >= debounce_duration;

                // Helper to start navigation to a destination
                let start_nav = |dest_idx: usize, veh: &VehicleState, renderer: &mut MapRenderer, router: &mut Router, nav: &mut NavigationState| {
                    if dest_idx >= DESTINATIONS.len() {
                        return;
                    }
                    let dest = &DESTINATIONS[dest_idx];
                    let vehicle_pos = VehiclePosition {
                        lat: veh.latitude,
                        lon: veh.longitude,
                        heading: veh.heading,
                    };

                    log::info!("Navigating to [{}]: {} ({:.4}, {:.4})", dest_idx + 1, dest.name, dest.lat, dest.lon);

                    if let Some(route) = router.calculate_route(vehicle_pos, (dest.lat, dest.lon)) {
                        log::info!("Route found: {} waypoints, {:.0}m total", route.waypoints.len(), route.total_distance_m);
                        renderer.set_route_from_routing(route);
                    } else {
                        log::info!("No route found, creating straight line");
                        let simple_route = minimap_core::ActiveRoute {
                            waypoints: vec![
                                (veh.latitude, veh.longitude),
                                (dest.lat, dest.lon),
                            ],
                            lane_guidance: None,
                            next_maneuver_distance: haversine_distance_m(veh.latitude, veh.longitude, dest.lat, dest.lon),
                            next_maneuver_type: 3,
                        };
                        renderer.set_route(Some(simple_route));
                    }
                    nav.start(dest.name, dest.lat, dest.lon);
                };

                // Destination keys 1-4
                if can_press_nav_key {
                    let mut handled = false;
                    for (i, key) in ["1", "2", "3", "4"].iter().enumerate() {
                        if keys.contains(*key) {
                            *nav_key_debounce.borrow_mut() = now;
                            start_nav(i, &veh, &mut renderer_clone.borrow_mut(), &mut router_clone.borrow_mut(), &mut nav_state_clone.borrow_mut());
                            handled = true;
                            break;
                        }
                    }

                    // C = clear route
                    if !handled && keys.contains("c") {
                        *nav_key_debounce.borrow_mut() = now;
                        log::info!("Clearing navigation route");
                        renderer_clone.borrow_mut().clear_route();
                        router_clone.borrow_mut().clear_route();
                        nav_state_clone.borrow_mut().stop();
                    }
                }
            }

            drop(keys);

            // Update tiles periodically (every 500ms)
            let tile_update_interval = Duration::from_millis(500);
            if now.duration_since(*last_tile_update.borrow()) >= tile_update_interval {
                *last_tile_update.borrow_mut() = now;
                #[cfg(feature = "navigation")]
                update_map_from_source(
                    &mut map_source_clone.borrow_mut(),
                    &mut renderer_clone.borrow_mut(),
                    &mut router_clone.borrow_mut(),
                    veh.latitude,
                    veh.longitude,
                );
                #[cfg(not(feature = "navigation"))]
                update_map_from_source(
                    &mut map_source_clone.borrow_mut(),
                    &mut renderer_clone.borrow_mut(),
                    veh.latitude,
                    veh.longitude,
                );
            }

            drop(veh);

            #[cfg(feature = "navigation")]
            update_ui(&ui, &renderer_clone.borrow(), &vehicle_clone.borrow(), &nav_state_clone.borrow());
            #[cfg(not(feature = "navigation"))]
            update_ui(&ui, &renderer_clone.borrow(), &vehicle_clone.borrow());
        },
    );

    // Log controls
    log::info!("Simulator ready. Controls:");
    log::info!("  WASD = move, Q/E = zoom, T = cycle theme");
    #[cfg(feature = "navigation")]
    {
        log::info!("  Click = navigate to point, C = clear route");
        log::info!("Pre-programmed destinations:");
        for (i, dest) in DESTINATIONS.iter().enumerate() {
            log::info!("  [{}] {} ({:.4}, {:.4})", i + 1, dest.name, dest.lat, dest.lon);
        }
    }
    ui.run().unwrap();
}

fn load_from_overpass(renderer: &mut MapRenderer, lat: f64, lon: f64) {
    let bbox = BoundingBox::from_center(lat, lon, 1000.0);
    match map_data::fetch_from_overpass(&bbox) {
        Ok((roads, pois)) => {
            log::info!(
                "Loaded {} roads and {} POIs from Overpass API",
                roads.len(),
                pois.len()
            );
            renderer.set_roads(roads);
            renderer.set_pois(pois);
        }
        Err(e) => {
            log::warn!("Failed to fetch from Overpass: {}. Using sample roads.", e);
            renderer.set_roads(map_data::generate_sample_roads(lat, lon));
        }
    }
}

// Navigation-enabled version: updates router graph
#[cfg(feature = "navigation")]
fn update_map_from_source(source: &mut MapSource, renderer: &mut MapRenderer, router: &mut Router, lat: f64, lon: f64) {
    if let MapSource::Tiles(loader) = source {
        // Load visible tiles (roughly 2.5km radius = 0.025 degrees for better routing coverage)
        let prev_count = loader.loaded_count();
        loader.load_visible(lat, lon, 0.025);
        let new_count = loader.loaded_count();

        // Only rebuild routing graph if tile count changed
        if new_count != prev_count {
            let tiles = loader.get_tiles();
            router.build_graph(&tiles);
        }

        update_renderer_from_tiles(loader, renderer);
    }
}

// Non-navigation version: no router
#[cfg(not(feature = "navigation"))]
fn update_map_from_source(source: &mut MapSource, renderer: &mut MapRenderer, lat: f64, lon: f64) {
    if let MapSource::Tiles(loader) = source {
        loader.load_visible(lat, lon, 0.015);
        update_renderer_from_tiles(loader, renderer);
    }
}

fn update_renderer_from_tiles(loader: &TileLoader, renderer: &mut MapRenderer) {
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

// Navigation-enabled version: includes nav state
#[cfg(feature = "navigation")]
fn update_ui(ui: &SimulatorWindow, renderer: &MapRenderer, vehicle: &VehicleState, nav: &NavigationState) {
    update_ui_common(ui, renderer, vehicle);

    // Update navigation status display
    ui.set_navigating(nav.is_navigating);
    ui.set_nav_destination_name(nav.destination_name.clone().into());
    if nav.is_navigating {
        let distance = haversine_distance_m(
            vehicle.latitude, vehicle.longitude,
            nav.destination_lat, nav.destination_lon,
        );
        ui.set_nav_distance_remaining(distance);
    } else {
        ui.set_nav_distance_remaining(0.0);
    }
}

// Non-navigation version
#[cfg(not(feature = "navigation"))]
fn update_ui(ui: &SimulatorWindow, renderer: &MapRenderer, vehicle: &VehicleState) {
    update_ui_common(ui, renderer, vehicle);
    ui.set_navigating(false);
    ui.set_nav_destination_name("".into());
    ui.set_nav_distance_remaining(0.0);
}

fn update_ui_common(ui: &SimulatorWindow, renderer: &MapRenderer, vehicle: &VehicleState) {
    let segments = renderer.render(vehicle);
    let pois = renderer.render_pois(vehicle);
    let area_triangles = renderer.render_area_triangles(vehicle);
    let waterways = renderer.render_waterways(vehicle);
    let route_segments = renderer.render_route(vehicle);

    static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !LOGGED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        log::info!(
            "Rendering: {} road segments, {} POIs, {} area triangles, {} waterways",
            segments.len(),
            pois.len(),
            area_triangles.len(),
            waterways.len()
        );
    }

    // Combine road segments with route overlay
    let mut all_segments = segments;
    all_segments.extend(route_segments);

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

    let poi_model: Vec<Poi> = pois
        .iter()
        .map(|p| Poi {
            x: p.x,
            y: p.y,
            poi_type: p.poi_type,
        })
        .collect();

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

    ui.set_roads(ModelRc::new(VecModel::from(road_model)));
    ui.set_pois(ModelRc::new(VecModel::from(poi_model)));
    ui.set_areas(ModelRc::new(VecModel::from(area_model)));
    ui.set_waterways(ModelRc::new(VecModel::from(waterway_model)));

    ui.set_player(PlayerState {
        heading: vehicle.heading,
        speed_kmh: vehicle.speed_kmh,
        latitude: vehicle.latitude as f32,
        longitude: vehicle.longitude as f32,
    });

    // Update junction view properties
    ui.set_junction_visible(renderer.should_show_junction());
    ui.set_junction_distance_m(renderer.junction_distance());
    ui.set_junction_destination(renderer.junction_destination().into());
    ui.set_junction_turn_type(renderer.junction_turn_type());

    // Update lane guidance if available
    if let Some(guidance) = renderer.lane_guidance() {
        // Convert minimap_core's lane data to simulator's Slint types
        let lanes: Vec<LaneDisplay> = guidance
            .lanes
            .iter()
            .map(|(turn_type, is_recommended)| LaneDisplay {
                turn_type: *turn_type,
                is_recommended: *is_recommended,
            })
            .collect();
        ui.set_junction_lanes(ModelRc::new(VecModel::from(lanes)));
    } else {
        ui.set_junction_lanes(ModelRc::new(VecModel::from(Vec::<LaneDisplay>::new())));
    }
}
