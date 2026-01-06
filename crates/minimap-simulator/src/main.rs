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
//! - W: Move forward
//! - S: Move backward
//! - A: Rotate left
//! - D: Rotate right
//! - Q: Zoom in
//! - E: Zoom out
//! - C: Clear route
//! - H: Cycle color theme
//! - Tap screen: Show UI buttons (search, settings, etc.)

use minimap_core::{
    map_data::{self, BoundingBox},
    AreaType, MapRenderer, MinimapConfig, PoiType, RoadType, Route, VehicleState, WaterwayType,
    WorldArea, WorldPoi, WorldRoad, WorldWaterway,
};
use minimap_routing::{Router, SearchIndex};
use minimap_tiles::loader::TileLoader;

slint::include_modules!();

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::collections::HashSet;
use std::error::Error;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Map data source - either tiles or static (from Overpass)
enum MapSource {
    Tiles(TileLoader),
    Static,
}

/// Helper function to show an error in the UI
fn show_error(ui: &SimulatorWindow, message: &str) {
    log::error!("{}", message);
    ui.set_error_message(SharedString::from(message));
    ui.set_error_visible(true);
}

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();
    log::info!("Starting NFS2 Minimap Simulator");

    let args: Vec<String> = std::env::args().collect();
    let tile_dir = args.get(1);

    // Create the Slint UI (simulator wrapper with keyboard support)
    let ui = SimulatorWindow::new()?;

    // Set GPS status to "good" for simulator (no red/yellow ring)
    ui.set_gps_status(2);

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

    // Initialize map source, optional router, and search index
    let (map_source, router, search_index) = if let Some(dir) = tile_dir {
        log::info!("Using tile-based loading from: {}", dir);
        let tile_loader = match TileLoader::new(dir) {
            Ok(loader) => Some(loader),
            Err(e) => {
                log::error!("Failed to load tiles: {}. Falling back to Overpass.", e);
                load_from_overpass(&mut renderer, center_lat, center_lon);
                None
            }
        };

        // Try to initialize router if routing tiles exist
        let routing_dir = Path::new(dir).join("routing");
        let router = if routing_dir.exists() {
            log::info!("Initializing router from: {}", dir);
            // Router::new expects the base tile directory, not the routing subdirectory
            match Router::new(dir) {
                Ok(r) => {
                    log::info!("Router initialized successfully");
                    Some(r)
                }
                Err(e) => {
                    log::warn!("Failed to initialize router: {}. Routing disabled.", e);
                    None
                }
            }
        } else {
            log::info!("No routing tiles found. Routing disabled.");
            None
        };

        // Try to initialize search index if it exists
        let search_dir = routing_dir.join("search");
        let search_index = if search_dir.exists() {
            log::info!("Initializing search index from: {}", search_dir.display());
            match SearchIndex::open(&search_dir) {
                Ok(idx) => {
                    log::info!("Search index loaded: {} entries", idx.entry_count());
                    Some(idx)
                }
                Err(e) => {
                    log::warn!("Failed to load search index: {}. Search disabled.", e);
                    None
                }
            }
        } else {
            log::info!("No search index found. Search disabled.");
            None
        };

        let source = match tile_loader {
            Some(loader) => MapSource::Tiles(loader),
            None => MapSource::Static,
        };
        (source, router, search_index)
    } else {
        log::info!("Using Overpass API for map data");
        load_from_overpass(&mut renderer, center_lat, center_lon);
        (MapSource::Static, None, None)
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
    let router = Rc::new(RefCell::new(router));
    let search_index = Rc::new(RefCell::new(search_index));


    // Initial render
    {
        let mut source = map_source.borrow_mut();
        let veh = vehicle.borrow();
        update_map_from_source(
            &mut source,
            &mut renderer.borrow_mut(),
            veh.latitude,
            veh.longitude,
        );
    }
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

    // Set up search callback
    let search_index_for_search = search_index.clone();
    let vehicle_for_search = vehicle.clone();
    let ui_weak_for_search = ui.as_weak();
    ui.on_search(move |query| {
        let query_str: String = query.into();
        log::debug!("Search query: '{}'", query_str);

        let veh = vehicle_for_search.borrow();
        let veh_lat = veh.latitude;
        let veh_lon = veh.longitude;
        drop(veh);

        if let Some(ref idx) = *search_index_for_search.borrow() {
            let mut results = idx.prefix_search(&query_str, 20);

            // Sort by distance from current position
            results.sort_by(|a, b| {
                let dist_a = haversine_km(veh_lat, veh_lon, a.lat, a.lon);
                let dist_b = haversine_km(veh_lat, veh_lon, b.lat, b.lon);
                dist_a.partial_cmp(&dist_b).unwrap_or(std::cmp::Ordering::Equal)
            });

            log::debug!("Found {} results", results.len());

            // Convert to Slint model with user-friendly category names and distance
            let slint_results: Vec<SearchResultItem> = results
                .into_iter()
                .take(10)
                .map(|r| {
                    let distance_km = haversine_km(veh_lat, veh_lon, r.lat, r.lon);
                    let category_name = match r.category {
                        minimap_routing::search::PlaceCategory::Settlement => "City/Town",
                        minimap_routing::search::PlaceCategory::Street => "Street",
                        minimap_routing::search::PlaceCategory::Poi => "Point of Interest",
                        minimap_routing::search::PlaceCategory::Transport => "Transport",
                        minimap_routing::search::PlaceCategory::Natural => "Natural Feature",
                        minimap_routing::search::PlaceCategory::Other => "Place",
                    };
                    SearchResultItem {
                        name: r.name.into(),
                        lat: r.lat as f32,
                        lon: r.lon as f32,
                        category: category_name.into(),
                        distance_km: distance_km as f32,
                    }
                })
                .collect();

            if let Some(ui) = ui_weak_for_search.upgrade() {
                ui.set_search_results(slint::ModelRc::new(slint::VecModel::from(slint_results)));
            }
        } else {
            log::warn!("Search index not available");
        }
    });

    // Set up route-to callback
    let router_for_route = router.clone();
    let renderer_for_route = renderer.clone();
    let vehicle_for_route = vehicle.clone();
    ui.on_route_to(move |dest_lat, dest_lon| {
        let veh = vehicle_for_route.borrow();
        let start_lat = veh.latitude;
        let start_lon = veh.longitude;
        log::info!(
            "Routing from ({:.5}, {:.5}) to ({:.5}, {:.5})",
            start_lat, start_lon, dest_lat, dest_lon
        );

        if let Some(ref mut router) = *router_for_route.borrow_mut() {
            let distance_km = haversine_km(start_lat, start_lon, dest_lat as f64, dest_lon as f64);
            log::info!("Route distance: {:.1} km", distance_km);

            match router.find_route(start_lat, start_lon, dest_lat as f64, dest_lon as f64) {
                Some(route) => {
                    log::info!(
                        "Route found: {:.0}m, {:.0}s, {} points",
                        route.total_distance_m,
                        route.total_time_s,
                        route.path.len()
                    );

                    // Prepend the car's current position and append destination
                    // so the route visually starts from the car and ends at destination
                    let mut full_path = Vec::with_capacity(route.path.len() + 2);
                    full_path.push((start_lat, start_lon));
                    full_path.extend(route.path);
                    full_path.push((dest_lat as f64, dest_lon as f64));

                    renderer_for_route.borrow_mut().set_route(Route::new(
                        full_path,
                        route.total_distance_m,
                        route.total_time_s,
                    ));
                }
                None => {
                    log::warn!("No route found. Try a closer destination (routing may have coverage gaps).");
                }
            }
        } else {
            log::warn!("Router not available. Generate routing tiles first.");
        }
    });

    // Set up clear-route callback
    let renderer_for_clear = renderer.clone();
    ui.on_clear_route(move || {
        renderer_for_clear.borrow_mut().clear_route();
        log::info!("Route cleared");
    });

    // Set up animation timer
    let ui_weak = ui.as_weak();
    let vehicle_clone = vehicle.clone();
    let renderer_clone = renderer.clone();
    let map_source_clone = map_source.clone();
    let keys_clone = pressed_keys.clone();
    let last_update = Rc::new(RefCell::new(Instant::now()));
    let last_tile_update = Rc::new(RefCell::new(Instant::now()));
    let theme_cycled = Rc::new(RefCell::new(false));
    let clear_pressed = Rc::new(RefCell::new(false));

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

            // Handle clear route (C = clear route)
            if keys.contains("c") && !*clear_pressed.borrow() {
                *clear_pressed.borrow_mut() = true;
                renderer_clone.borrow_mut().clear_route();
                log::info!("Route cleared");
            }
            if !keys.contains("c") {
                *clear_pressed.borrow_mut() = false;
            }

            // Handle theme cycling (H = cycle to next theme)
            if keys.contains("h") && !*theme_cycled.borrow() {
                *theme_cycled.borrow_mut() = true;
                ui.invoke_cycle_theme();
                let theme_name: String = ui.get_current_theme_name().into();
                log::info!("Theme changed to: {}", theme_name);
            }
            if !keys.contains("h") {
                *theme_cycled.borrow_mut() = false;
            }

            drop(keys);

            // Update tiles periodically (every 500ms)
            let tile_update_interval = Duration::from_millis(500);
            if now.duration_since(*last_tile_update.borrow()) >= tile_update_interval {
                *last_tile_update.borrow_mut() = now;
                update_map_from_source(
                    &mut map_source_clone.borrow_mut(),
                    &mut renderer_clone.borrow_mut(),
                    veh.latitude,
                    veh.longitude,
                );
            }

            drop(veh);

            update_ui(&ui, &renderer_clone.borrow(), &vehicle_clone.borrow());
        },
    );

    // Set up dismiss-error callback
    ui.on_dismiss_error(|| {
        log::debug!("Error dismissed");
    });

    log::info!("Simulator ready. WASD=move, Q/E=zoom, C=clear route, H=theme. Tap screen for search.");
    ui.run()?;

    Ok(())
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

fn update_map_from_source(source: &mut MapSource, renderer: &mut MapRenderer, lat: f64, lon: f64) {
    if let MapSource::Tiles(loader) = source {
        // Load visible tiles (roughly 1.2km radius = 0.015 degrees)
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
                    minimap_tiles::PoiType::FastFood => PoiType::FastFood,
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
}

fn update_ui(ui: &SimulatorWindow, renderer: &MapRenderer, vehicle: &VehicleState) {
    // Use shared render_all() for consistent rendering across platforms
    let frame = renderer.render_all(vehicle);

    let road_model: Vec<RoadSegment> = frame.segments
        .iter()
        .map(|seg| RoadSegment {
            x1: seg.x1,
            y1: seg.y1,
            x2: seg.x2,
            y2: seg.y2,
            road_type: seg.road_type,
        })
        .collect();

    let poi_model: Vec<Poi> = frame.pois
        .iter()
        .map(|p| Poi {
            x: p.x,
            y: p.y,
            poi_type: p.poi_type,
        })
        .collect();

    let area_model: Vec<AreaTriangle> = frame.areas
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

    let waterway_model: Vec<WaterwaySegment> = frame.waterways
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
}

/// Calculate distance in kilometers using haversine formula
fn haversine_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6371.0; // Earth radius in km

    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();

    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);

    let c = 2.0 * a.sqrt().asin();
    R * c
}
