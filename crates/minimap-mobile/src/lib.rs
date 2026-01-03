//! NFS Minimap Mobile App
//!
//! Complete iOS/Android app with GPS support.
//!
//! For iOS: Uses CoreLocation via objc2 bindings
//! For Android: Uses android-activity

use minimap_core::{MapRenderer, MinimapConfig, VehicleState, WorldRoad, WorldPoi, WorldArea, WorldWaterway, RoadType, PoiType, AreaType, WaterwayType};
use minimap_core::transform::smooth_angle;
use minimap_tiles::loader::TileLoader;
use std::sync::{Arc, Mutex, OnceLock};

slint::include_modules!();

// ============================================================================
// Shared App State
// ============================================================================

/// Interpolation smoothing factor (0.0 = no movement, 1.0 = instant snap)
/// Lower values = smoother but more lag, higher = snappier but less smooth
const POSITION_SMOOTHING: f64 = 0.15;
const HEADING_SMOOTHING: f32 = 0.2;

/// GPS accuracy threshold in meters for "good" signal
const GPS_ACCURACY_GOOD: f64 = 20.0;
/// GPS accuracy threshold in meters for "inaccurate" (above this = no signal)
const GPS_ACCURACY_POOR: f64 = 100.0;

/// GPS status values (matches Slint UI)
pub const GPS_STATUS_NONE: i32 = 0;        // No GPS signal (red ring)
pub const GPS_STATUS_INACCURATE: i32 = 1;  // Inaccurate GPS (yellow ring)
pub const GPS_STATUS_GOOD: i32 = 2;        // Good GPS signal (no ring)

/// Global app state
static APP: OnceLock<Arc<Mutex<App>>> = OnceLock::new();

struct App {
    renderer: MapRenderer,
    tile_loader: Option<TileLoader>,
    /// Currently displayed vehicle state (smoothly interpolated)
    vehicle: VehicleState,
    /// Target vehicle state from GPS (where we're interpolating toward)
    target: VehicleState,
    /// Whether we've received at least one GPS update
    has_gps_fix: bool,
    /// Current GPS status (0 = none, 1 = inaccurate, 2 = good)
    gps_status: i32,
    ui: slint::Weak<MobileApp>,
}

impl App {
    fn new(tile_dir: Option<&str>) -> Result<(Self, MobileApp), slint::PlatformError> {
        let config = MinimapConfig {
            screen_width: 800.0,   // Must match Slint viewbox dimensions
            screen_height: 800.0,
            meters_per_pixel: 1.5,
            rotate_with_heading: true,
        };

        let renderer = MapRenderer::new(config);

        let tile_loader = tile_dir.and_then(|dir| {
            match TileLoader::new(dir) {
                Ok(loader) => {
                    log::info!("Loaded tiles from {}", dir);
                    Some(loader)
                }
                Err(e) => {
                    log::warn!("Failed to load tiles from {}: {}", dir, e);
                    None
                }
            }
        });

        let ui = MobileApp::new()?;

        let initial_state = VehicleState {
            latitude: 47.1415,  // Rotkreuz, Switzerland
            longitude: 8.4320,
            heading: 0.0,
            speed_kmh: 0.0,
        };

        Ok((Self {
            renderer,
            tile_loader,
            vehicle: initial_state.clone(),
            target: initial_state,
            has_gps_fix: false,
            gps_status: GPS_STATUS_NONE,
            ui: ui.as_weak(),
        }, ui))
    }

    fn update_position(&mut self, lat: f64, lon: f64, heading: f32, speed: f32) {
        // On first GPS fix, snap directly to position (no interpolation)
        if !self.has_gps_fix {
            self.vehicle.latitude = lat;
            self.vehicle.longitude = lon;
            self.vehicle.heading = heading;
            self.vehicle.speed_kmh = speed;
            self.has_gps_fix = true;
        }

        // Set target position for interpolation
        self.target.latitude = lat;
        self.target.longitude = lon;
        self.target.heading = heading;
        self.target.speed_kmh = speed;

        // Load tiles for new position
        if let Some(ref mut loader) = self.tile_loader {
            loader.load_visible(lat, lon, 0.015);

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

            self.renderer.set_roads(roads);
            self.renderer.set_pois(pois);
            self.renderer.set_areas(areas);
            self.renderer.set_waterways(waterways);
        }

        self.refresh_ui();
    }

    /// Set GPS status based on accuracy in meters
    /// accuracy < 0 means no GPS signal
    fn set_gps_status_from_accuracy(&mut self, accuracy_meters: f64) {
        let new_status = if accuracy_meters < 0.0 {
            GPS_STATUS_NONE
        } else if accuracy_meters <= GPS_ACCURACY_GOOD {
            GPS_STATUS_GOOD
        } else if accuracy_meters <= GPS_ACCURACY_POOR {
            GPS_STATUS_INACCURATE
        } else {
            GPS_STATUS_NONE  // Very poor accuracy treated as no signal
        };

        if new_status != self.gps_status {
            self.gps_status = new_status;
            if let Some(ui) = self.ui.upgrade() {
                ui.set_gps_status(new_status);
            }
        }
    }

    /// Set GPS status directly (0 = none, 1 = inaccurate, 2 = good)
    fn set_gps_status(&mut self, status: i32) {
        if status != self.gps_status {
            self.gps_status = status;
            if let Some(ui) = self.ui.upgrade() {
                ui.set_gps_status(status);
            }
        }
    }

    /// Interpolation tick - called at ~60Hz to smoothly animate position
    fn interpolation_tick(&mut self) {
        if !self.has_gps_fix {
            return;
        }

        // Smoothly interpolate position toward target
        self.vehicle.latitude += (self.target.latitude - self.vehicle.latitude) * POSITION_SMOOTHING;
        self.vehicle.longitude += (self.target.longitude - self.vehicle.longitude) * POSITION_SMOOTHING;

        // Smoothly interpolate heading (with wrap-around handling)
        self.vehicle.heading = smooth_angle(self.vehicle.heading, self.target.heading, HEADING_SMOOTHING);

        // Speed doesn't need interpolation - use target directly
        self.vehicle.speed_kmh = self.target.speed_kmh;

        self.refresh_ui();
    }

    fn zoom_in(&mut self) {
        self.renderer.zoom_in(0.8); // 20% closer
        self.refresh_ui();
    }

    fn zoom_out(&mut self) {
        self.renderer.zoom_out(1.25); // 25% farther
        self.refresh_ui();
    }

    fn refresh_ui(&self) {
        if let Some(ui) = self.ui.upgrade() {
            let segments = self.renderer.render(&self.vehicle);
            let pois = self.renderer.render_pois(&self.vehicle);
            let area_triangles = self.renderer.render_area_triangles(&self.vehicle);
            let waterways = self.renderer.render_waterways(&self.vehicle);

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

            ui.set_roads(slint::ModelRc::new(slint::VecModel::from(road_model)));
            ui.set_pois(slint::ModelRc::new(slint::VecModel::from(poi_model)));
            ui.set_areas(slint::ModelRc::new(slint::VecModel::from(area_model)));
            ui.set_waterways(slint::ModelRc::new(slint::VecModel::from(waterway_model)));
            ui.set_player(PlayerState {
                heading: self.vehicle.heading,
                speed_kmh: self.vehicle.speed_kmh,
                latitude: self.vehicle.latitude as f32,
                longitude: self.vehicle.longitude as f32,
            });
        }
    }
}

// ============================================================================
// iOS Implementation
// ============================================================================

#[cfg(target_os = "ios")]
mod ios {
    use super::*;

    /// Start location updates
    ///
    /// Note: GPS is handled via FFI - the native iOS code (Swift/ObjC)
    /// should call minimap_update_gps() when location updates arrive.
    pub fn start_location_updates() {
        log::info!("iOS GPS: Use native code to call minimap_update_gps()");
        // GPS status is shown via gps_status property (ring indicator)
    }
}

// ============================================================================
// Android Implementation
// ============================================================================

#[cfg(target_os = "android")]
mod android {
    use super::*;

    pub fn start_location_updates() {
        log::info!("Starting Android location updates");
        // Android location is typically handled via JNI
        // For now, we'd need a Kotlin/Java layer
        // TODO: Implement via android-activity or JNI
    }
}

// ============================================================================
// FFI Interface (for external native code if needed)
// ============================================================================

/// Initialize the app
///
/// # Arguments
/// * `tile_dir` - Path to tile directory, or null for no tiles
///
/// # Returns
/// * `true` on success, `false` on failure
#[no_mangle]
pub extern "C" fn minimap_init(tile_dir: *const std::ffi::c_char) -> bool {
    // Get tile directory
    let tile_path = if tile_dir.is_null() {
        None
    } else {
        let c_str = unsafe { std::ffi::CStr::from_ptr(tile_dir) };
        c_str.to_str().ok()
    };

    // Create app
    let (app, ui) = match App::new(tile_path) {
        Ok(result) => result,
        Err(e) => {
            log::error!("Failed to create app: {}", e);
            return false;
        }
    };

    // Store globally
    if APP.set(Arc::new(Mutex::new(app))).is_err() {
        log::error!("App already initialized");
        return false;
    }

    // Set up zoom callbacks
    ui.on_zoom_in(|| {
        if let Some(app) = APP.get() {
            if let Ok(mut app) = app.lock() {
                app.zoom_in();
            }
        }
    });

    ui.on_zoom_out(|| {
        if let Some(app) = APP.get() {
            if let Ok(mut app) = app.lock() {
                app.zoom_out();
            }
        }
    });

    // Set up interpolation timer (~60 FPS for smooth animation)
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(16), // ~60 FPS
        move || {
            if let Some(app) = APP.get() {
                if let Ok(mut app) = app.lock() {
                    app.interpolation_tick();
                }
            }
        },
    );

    // Start location updates
    #[cfg(target_os = "ios")]
    ios::start_location_updates();

    #[cfg(target_os = "android")]
    android::start_location_updates();

    // Run UI (blocks until app exits)
    if let Err(e) = ui.run() {
        log::error!("UI error: {}", e);
        return false;
    }

    true
}

/// Update GPS position with accuracy
///
/// # Arguments
/// * `lat` - Latitude in degrees
/// * `lon` - Longitude in degrees
/// * `heading` - Heading in degrees (0-360)
/// * `speed_kmh` - Speed in km/h
/// * `accuracy_meters` - Horizontal accuracy in meters (negative = unknown/no signal)
#[no_mangle]
pub extern "C" fn minimap_update_gps(lat: f64, lon: f64, heading: f32, speed_kmh: f32, accuracy_meters: f64) {
    if let Some(app) = APP.get() {
        if let Ok(mut app) = app.lock() {
            app.update_position(lat, lon, heading, speed_kmh);
            app.set_gps_status_from_accuracy(accuracy_meters);
        }
    }
}

/// Set GPS status directly
///
/// # Arguments
/// * `status` - GPS status: 0 = no signal, 1 = inaccurate, 2 = good
#[no_mangle]
pub extern "C" fn minimap_set_gps_status(status: i32) {
    if let Some(app) = APP.get() {
        if let Ok(mut app) = app.lock() {
            app.set_gps_status(status);
        }
    }
}

// iOS entry point is handled by Swift's @main attribute
// The Swift app calls minimap_init() to initialize the Rust library
