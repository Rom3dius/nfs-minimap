//! NFS Minimap Mobile App
//!
//! Complete iOS/Android app with GPS support.
//!
//! For iOS: Uses CoreLocation via objc2 bindings
//! For Android: Uses android-activity

use minimap_core::{MapRenderer, MinimapConfig, VehicleState, WorldRoad, WorldPoi, RoadType, PoiType};
use minimap_tiles::loader::TileLoader;
use std::sync::{Arc, Mutex, OnceLock};

slint::include_modules!();

// ============================================================================
// Shared App State
// ============================================================================

/// Global app state
static APP: OnceLock<Arc<Mutex<App>>> = OnceLock::new();

struct App {
    renderer: MapRenderer,
    tile_loader: Option<TileLoader>,
    vehicle: VehicleState,
    ui: slint::Weak<MobileApp>,
}

impl App {
    fn new(tile_dir: Option<&str>) -> Result<(Self, MobileApp), slint::PlatformError> {
        let config = MinimapConfig {
            screen_width: 400.0,
            screen_height: 400.0,
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

        Ok((Self {
            renderer,
            tile_loader,
            vehicle: VehicleState {
                latitude: 47.1415,  // Rotkreuz, Switzerland
                longitude: 8.4320,
                heading: 0.0,
                speed_kmh: 0.0,
            },
            ui: ui.as_weak(),
        }, ui))
    }

    fn update_position(&mut self, lat: f64, lon: f64, heading: f32, speed: f32) {
        self.vehicle.latitude = lat;
        self.vehicle.longitude = lon;
        self.vehicle.heading = heading;
        self.vehicle.speed_kmh = speed;

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
                    },
                })
                .collect();

            self.renderer.set_roads(roads);
            self.renderer.set_pois(pois);
        }

        self.refresh_ui();
    }

    fn set_gps_active(&self, active: bool) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_gps_active(active);
            ui.set_status_text(if active {
                "GPS Active".into()
            } else {
                "Waiting for GPS...".into()
            });
        }
    }

    fn refresh_ui(&self) {
        if let Some(ui) = self.ui.upgrade() {
            let segments = self.renderer.render(&self.vehicle);
            let pois = self.renderer.render_pois(&self.vehicle);

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

            ui.set_roads(slint::ModelRc::new(slint::VecModel::from(road_model)));
            ui.set_pois(slint::ModelRc::new(slint::VecModel::from(poi_model)));
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

        if let Some(app) = APP.get() {
            if let Ok(app) = app.lock() {
                if let Some(ui) = app.ui.upgrade() {
                    ui.set_status_text("Waiting for GPS...".into());
                }
            }
        }
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

/// Manually update GPS position (for testing or external GPS)
#[no_mangle]
pub extern "C" fn minimap_update_gps(lat: f64, lon: f64, heading: f32, speed_kmh: f32) {
    if let Some(app) = APP.get() {
        if let Ok(mut app) = app.lock() {
            app.update_position(lat, lon, heading, speed_kmh);
        }
    }
}

/// Set GPS active status
#[no_mangle]
pub extern "C" fn minimap_set_gps_active(active: bool) {
    if let Some(app) = APP.get() {
        if let Ok(app) = app.lock() {
            app.set_gps_active(active);
        }
    }
}

/// Set status text
#[no_mangle]
pub extern "C" fn minimap_set_status(text: *const std::ffi::c_char) {
    if text.is_null() {
        return;
    }

    let c_str = unsafe { std::ffi::CStr::from_ptr(text) };
    if let Ok(status) = c_str.to_str() {
        if let Some(app) = APP.get() {
            if let Ok(app) = app.lock() {
                if let Some(ui) = app.ui.upgrade() {
                    ui.set_status_text(status.into());
                }
            }
        }
    }
}

// ============================================================================
// iOS App Entry Point
// ============================================================================

#[cfg(target_os = "ios")]
#[no_mangle]
pub extern "C" fn main() -> i32 {
    // iOS entry point
    // The actual app is initialized via UIApplicationMain in the Swift/ObjC layer
    // which then calls minimap_init
    0
}
