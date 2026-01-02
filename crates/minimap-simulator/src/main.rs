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

use minimap_core::{
    map_data::{self, BoundingBox},
    AreaType, MapRenderer, MinimapConfig, PoiType, RoadType, VehicleState, WaterwayType,
    WorldArea, WorldPoi, WorldRoad, WorldWaterway,
};
use minimap_tiles::loader::TileLoader;

slint::include_modules!();

use slint::{ComponentHandle, ModelRc, VecModel};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

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

    // Set up animation timer
    let ui_weak = ui.as_weak();
    let vehicle_clone = vehicle.clone();
    let renderer_clone = renderer.clone();
    let map_source_clone = map_source.clone();
    let keys_clone = pressed_keys.clone();
    let last_update = Rc::new(RefCell::new(Instant::now()));
    let last_tile_update = Rc::new(RefCell::new(Instant::now()));

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

    log::info!("Simulator ready. WASD=move, Q/E=zoom");
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
    let segments = renderer.render(vehicle);
    let pois = renderer.render_pois(vehicle);
    let area_triangles = renderer.render_area_triangles(vehicle);
    let waterways = renderer.render_waterways(vehicle);

    static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !LOGGED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        log::info!(
            "Total road segments: {}, POIs: {}, area triangles: {}, waterways: {}",
            segments.len(),
            pois.len(),
            area_triangles.len(),
            waterways.len()
        );
    }

    let road_model: Vec<RoadSegment> = segments
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
}
