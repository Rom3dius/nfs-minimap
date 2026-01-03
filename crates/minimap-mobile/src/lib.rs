//! NFS Minimap Mobile App
//!
//! Complete iOS/Android app with GPS support.
//!
//! For iOS: Uses CoreLocation via objc2 bindings
//! For Android: Uses android-activity

use minimap_core::{MapRenderer, MinimapConfig, VehicleState, WorldRoad, WorldPoi, WorldArea, WorldWaterway, RoadType, PoiType, AreaType, WaterwayType};
use minimap_routing::{Router, RoutingConfig, VehiclePosition};
use minimap_tiles::loader::TileLoader;
use std::ffi::{c_char, CString};
use std::sync::{Arc, Mutex, OnceLock};

slint::include_modules!();

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

// ============================================================================
// Shared App State
// ============================================================================

/// Global app state
static APP: OnceLock<Arc<Mutex<App>>> = OnceLock::new();

struct App {
    renderer: MapRenderer,
    tile_loader: Option<TileLoader>,
    router: Router,
    vehicle: VehicleState,
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

        let router = Router::new(RoutingConfig::default());
        let ui = MobileApp::new()?;

        Ok((Self {
            renderer,
            tile_loader,
            router,
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

    fn zoom_in(&mut self) {
        self.renderer.zoom_in(0.8); // 20% closer
        self.refresh_ui();
    }

    fn zoom_out(&mut self) {
        self.renderer.zoom_out(1.25); // 25% farther
        self.refresh_ui();
    }

    /// Set navigation destination and calculate route
    fn set_destination(&mut self, lat: f64, lon: f64) -> bool {
        let vehicle_pos = VehiclePosition {
            lat: self.vehicle.latitude,
            lon: self.vehicle.longitude,
            heading: self.vehicle.heading,
        };

        log::info!("Setting navigation destination: ({:.6}, {:.6})", lat, lon);

        if let Some(route) = self.router.calculate_route(vehicle_pos, (lat, lon)) {
            log::info!(
                "Route found: {} waypoints, {:.0}m total",
                route.waypoints.len(),
                route.total_distance_m
            );
            self.renderer.set_route_from_routing(route);
            self.refresh_ui();
            true
        } else {
            log::warn!("No route found to destination");
            // Create a simple straight-line route for demo purposes
            let simple_route = minimap_core::ActiveRoute {
                waypoints: vec![
                    (self.vehicle.latitude, self.vehicle.longitude),
                    (lat, lon),
                ],
                lane_guidance: None,
                next_maneuver_distance: haversine_distance_m(
                    self.vehicle.latitude,
                    self.vehicle.longitude,
                    lat,
                    lon,
                ),
                next_maneuver_type: 3, // straight
            };
            self.renderer.set_route(Some(simple_route));
            self.refresh_ui();
            true
        }
    }

    /// Clear navigation route
    fn clear_route(&mut self) {
        log::info!("Clearing navigation route");
        self.renderer.clear_route();
        self.router.clear_route();
        self.refresh_ui();
    }

    /// Check if lane guidance is available
    fn has_lane_guidance(&self) -> bool {
        self.renderer.lane_guidance().is_some()
    }

    /// Get distance to next junction
    fn junction_distance(&self) -> f32 {
        self.renderer.junction_distance()
    }

    /// Get lane guidance as JSON for native UI rendering
    fn lane_guidance_json(&self) -> Option<String> {
        let guidance = self.renderer.lane_guidance()?;
        let lanes: Vec<serde_json::Value> = guidance
            .lanes
            .iter()
            .map(|(turn_type, is_recommended)| {
                serde_json::json!({
                    "turn": *turn_type,
                    "recommended": *is_recommended
                })
            })
            .collect();

        let json = serde_json::json!({
            "lanes": lanes,
            "destination": self.renderer.junction_destination(),
            "distance_m": self.renderer.junction_distance(),
            "turn_type": self.renderer.junction_turn_type()
        });

        Some(json.to_string())
    }

    fn refresh_ui(&self) {
        if let Some(ui) = self.ui.upgrade() {
            let segments = self.renderer.render(&self.vehicle);
            let pois = self.renderer.render_pois(&self.vehicle);
            let area_triangles = self.renderer.render_area_triangles(&self.vehicle);
            let waterways = self.renderer.render_waterways(&self.vehicle);
            let route_segments = self.renderer.render_route(&self.vehicle);

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

            // Update junction view properties
            ui.set_junction_visible(self.renderer.should_show_junction());
            ui.set_junction_distance_m(self.renderer.junction_distance());
            ui.set_junction_destination(self.renderer.junction_destination().into());
            ui.set_junction_turn_type(self.renderer.junction_turn_type());

            // Update lane guidance if available
            if let Some(guidance) = self.renderer.lane_guidance() {
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
// Navigation FFI Interface
// ============================================================================

/// Set navigation destination and calculate route
///
/// # Arguments
/// * `lat` - Destination latitude
/// * `lon` - Destination longitude
///
/// # Returns
/// * `true` if route was found or fallback created, `false` on error
#[no_mangle]
pub extern "C" fn minimap_set_destination(lat: f64, lon: f64) -> bool {
    if let Some(app) = APP.get() {
        if let Ok(mut app) = app.lock() {
            return app.set_destination(lat, lon);
        }
    }
    false
}

/// Clear the current navigation route
#[no_mangle]
pub extern "C" fn minimap_clear_route() {
    if let Some(app) = APP.get() {
        if let Ok(mut app) = app.lock() {
            app.clear_route();
        }
    }
}

/// Check if lane guidance is available for the current route
///
/// # Returns
/// * `true` if lane guidance is available
#[no_mangle]
pub extern "C" fn minimap_has_lane_guidance() -> bool {
    if let Some(app) = APP.get() {
        if let Ok(app) = app.lock() {
            return app.has_lane_guidance();
        }
    }
    false
}

/// Get the distance to the next junction in meters
///
/// # Returns
/// * Distance in meters, or 0.0 if no junction is upcoming
#[no_mangle]
pub extern "C" fn minimap_get_junction_distance() -> f32 {
    if let Some(app) = APP.get() {
        if let Ok(app) = app.lock() {
            return app.junction_distance();
        }
    }
    0.0
}

/// Get lane guidance as JSON string
///
/// Returns a JSON object with the following structure:
/// ```json
/// {
///   "lanes": [{"turn": 3, "recommended": true}, ...],
///   "destination": "City Center",
///   "distance_m": 300.0,
///   "turn_type": 5
/// }
/// ```
///
/// Turn types: 0=none, 1=left, 2=slight-left, 3=through, 4=slight-right, 5=right, 6=uturn
///
/// # Returns
/// * Pointer to a C string that must be freed with `minimap_free_string()`, or null if no guidance
#[no_mangle]
pub extern "C" fn minimap_get_lane_guidance_json() -> *mut c_char {
    if let Some(app) = APP.get() {
        if let Ok(app) = app.lock() {
            if let Some(json) = app.lane_guidance_json() {
                if let Ok(c_string) = CString::new(json) {
                    return c_string.into_raw();
                }
            }
        }
    }
    std::ptr::null_mut()
}

/// Free a string returned by `minimap_get_lane_guidance_json()`
///
/// # Safety
/// The pointer must be a valid pointer returned by `minimap_get_lane_guidance_json()`
/// or null (which is safely ignored).
#[no_mangle]
pub unsafe extern "C" fn minimap_free_string(s: *mut c_char) {
    if !s.is_null() {
        let _ = CString::from_raw(s);
    }
}

// iOS entry point is handled by Swift's @main attribute
// The Swift app calls minimap_init() to initialize the Rust library
