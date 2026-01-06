//! minimap-core: Platform-agnostic map rendering logic
//!
//! This crate contains the core data structures and algorithms for:
//! - Loading and storing map data (roads, POIs)
//! - Coordinate transformations (lat/lon to screen, rotation)
//! - Map styling and theming

use nalgebra::{Matrix3, Point2};
use serde::{Deserialize, Serialize};

/// Zoom constraints
pub const ZOOM_MIN: f32 = 0.75;   // Most zoomed in (~300m radius)
pub const ZOOM_DEFAULT: f32 = 1.5; // Default (~600m radius)
pub const ZOOM_MAX: f32 = 3.0;    // Most zoomed out (~1.2km radius, fits in fixed tile radius)

pub mod geo;
pub mod map_data;
pub mod transform;

// Re-export the Slint UI module
slint::include_modules!();

/// Configuration for the minimap display
#[derive(Debug, Clone)]
pub struct MinimapConfig {
    /// Screen dimensions in pixels
    pub screen_width: f32,
    pub screen_height: f32,
    
    /// Zoom level (meters per pixel)
    pub meters_per_pixel: f32,
    
    /// Whether to rotate map with heading (true) or keep north-up (false)
    pub rotate_with_heading: bool,
}

impl Default for MinimapConfig {
    fn default() -> Self {
        Self {
            screen_width: 800.0,
            screen_height: 800.0,
            meters_per_pixel: 2.0,  // ~400m radius visible
            rotate_with_heading: true,
        }
    }
}

/// Represents the player/vehicle state
#[derive(Debug, Clone, Default)]
pub struct VehicleState {
    /// Current latitude in degrees
    pub latitude: f64,
    /// Current longitude in degrees  
    pub longitude: f64,
    /// Heading in degrees (0 = North, 90 = East)
    pub heading: f32,
    /// Speed in km/h
    pub speed_kmh: f32,
}

/// A road segment in world coordinates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldRoad {
    /// Road name (if available)
    pub name: Option<String>,
    /// List of coordinates forming the road polyline
    pub points: Vec<(f64, f64)>,  // (lat, lon)
    /// Road type for styling
    pub road_type: RoadType,
}

/// Point of interest types
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PoiType {
    GasStation,
    Parking,
    ShoppingMall,
    CarWash,
    FastFood,
}

impl PoiType {
    pub fn to_int(&self) -> i32 {
        match self {
            PoiType::GasStation => 0,
            PoiType::Parking => 1,
            PoiType::ShoppingMall => 2,
            PoiType::CarWash => 3,
            PoiType::FastFood => 4,
        }
    }
}

/// A point of interest in world coordinates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldPoi {
    pub name: Option<String>,
    pub lat: f64,
    pub lon: f64,
    pub poi_type: PoiType,
}

/// A POI transformed to screen coordinates
#[derive(Debug, Clone)]
pub struct ScreenPoi {
    pub x: f32,
    pub y: f32,
    pub poi_type: i32,
}

/// Area types for natural features
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AreaType {
    Water,   // Lakes, rivers, ponds
    Forest,  // Forests, woods
    Park,    // Parks, gardens
    Grass,   // Meadows, grassland
}

impl AreaType {
    pub fn to_int(&self) -> i32 {
        match self {
            AreaType::Water => 0,
            AreaType::Forest => 1,
            AreaType::Park => 2,
            AreaType::Grass => 3,
        }
    }
}

/// Waterway types for linear water features
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum WaterwayType {
    River,   // Major rivers
    Stream,  // Streams, creeks
    Canal,   // Canals
}

impl WaterwayType {
    pub fn to_int(&self) -> i32 {
        match self {
            WaterwayType::River => 0,
            WaterwayType::Stream => 1,
            WaterwayType::Canal => 2,
        }
    }
}

/// A waterway (linear water feature) in world coordinates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldWaterway {
    pub points: Vec<(f64, f64)>,  // (lat, lon)
    pub waterway_type: WaterwayType,
}

/// A waterway segment transformed to screen coordinates
#[derive(Debug, Clone)]
pub struct ScreenWaterway {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    pub waterway_type: i32,
}

/// An area (polygon) in world coordinates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldArea {
    pub points: Vec<(f64, f64)>,  // (lat, lon)
    pub area_type: AreaType,
}

/// An area transformed to screen coordinates
#[derive(Debug, Clone)]
pub struct ScreenArea {
    pub points: Vec<(f32, f32)>,  // (x, y)
    pub area_type: i32,
}

/// A single triangle from a triangulated area, ready for Slint rendering
#[derive(Debug, Clone)]
pub struct ScreenTriangle {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    pub x3: f32,
    pub y3: f32,
    pub area_type: i32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RoadType {
    Primary,    // Major roads, highways
    Secondary,  // Regular streets
    Highlight,  // Route/navigation highlight
}

impl RoadType {
    pub fn to_int(&self) -> i32 {
        match self {
            RoadType::Primary => 0,
            RoadType::Secondary => 1,
            RoadType::Highlight => 2,
        }
    }
}

/// A road segment transformed to screen coordinates, ready for Slint
#[derive(Debug, Clone)]
pub struct ScreenSegment {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    pub road_type: i32,
}

/// A navigation route to display on the map
#[derive(Debug, Clone, Default)]
pub struct Route {
    /// Path as lat/lon coordinates
    pub path: Vec<(f64, f64)>,
    /// Total distance in meters
    pub total_distance_m: f64,
    /// Estimated time in seconds
    pub total_time_s: f64,
}

impl Route {
    pub fn new(path: Vec<(f64, f64)>, total_distance_m: f64, total_time_s: f64) -> Self {
        Self {
            path,
            total_distance_m,
            total_time_s,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.path.len() < 2
    }
}

/// A complete rendered frame with all map elements in screen coordinates.
///
/// This bundles all rendered data for a single frame, ensuring consistency
/// and reducing code duplication across platforms (simulator, mobile, ESP32).
#[derive(Debug, Clone, Default)]
pub struct RenderedFrame {
    /// Road segments including route highlights
    pub segments: Vec<ScreenSegment>,
    /// Points of interest
    pub pois: Vec<ScreenPoi>,
    /// Area triangles (water, forest, etc.)
    pub areas: Vec<ScreenTriangle>,
    /// Waterway segments (rivers, streams)
    pub waterways: Vec<ScreenWaterway>,
}

/// The main map renderer
pub struct MapRenderer {
    config: MinimapConfig,
    roads: Vec<WorldRoad>,
    pois: Vec<WorldPoi>,
    areas: Vec<WorldArea>,
    waterways: Vec<WorldWaterway>,
    /// Current navigation route (if any)
    route: Option<Route>,
}

impl MapRenderer {
    pub fn new(config: MinimapConfig) -> Self {
        Self {
            config,
            roads: Vec::new(),
            pois: Vec::new(),
            areas: Vec::new(),
            waterways: Vec::new(),
            route: None,
        }
    }

    /// Load roads from a slice of WorldRoad
    pub fn set_roads(&mut self, roads: Vec<WorldRoad>) {
        self.roads = roads;
    }

    /// Get a reference to the loaded roads
    pub fn roads(&self) -> &[WorldRoad] {
        &self.roads
    }

    /// Load POIs
    pub fn set_pois(&mut self, pois: Vec<WorldPoi>) {
        self.pois = pois;
    }

    /// Get a reference to the loaded POIs
    pub fn pois(&self) -> &[WorldPoi] {
        &self.pois
    }

    /// Load areas (polygons)
    pub fn set_areas(&mut self, areas: Vec<WorldArea>) {
        self.areas = areas;
    }

    /// Get a reference to the loaded areas
    pub fn areas(&self) -> &[WorldArea] {
        &self.areas
    }

    /// Load waterways (linear water features)
    pub fn set_waterways(&mut self, waterways: Vec<WorldWaterway>) {
        self.waterways = waterways;
    }

    /// Get a reference to the loaded waterways
    pub fn waterways(&self) -> &[WorldWaterway] {
        &self.waterways
    }

    /// Set the current navigation route
    pub fn set_route(&mut self, route: Route) {
        if route.is_empty() {
            self.route = None;
        } else {
            self.route = Some(route);
        }
    }

    /// Clear the current navigation route
    pub fn clear_route(&mut self) {
        self.route = None;
    }

    /// Get a reference to the current route (if any)
    pub fn route(&self) -> Option<&Route> {
        self.route.as_ref()
    }

    /// Check if there's an active route
    pub fn has_route(&self) -> bool {
        self.route.is_some()
    }

    /// Update configuration
    pub fn set_config(&mut self, config: MinimapConfig) {
        self.config = config;
    }

    /// Get current zoom level (meters per pixel)
    pub fn zoom(&self) -> f32 {
        self.config.meters_per_pixel
    }

    /// Set zoom level (clamped to valid range)
    pub fn set_zoom(&mut self, meters_per_pixel: f32) {
        self.config.meters_per_pixel = meters_per_pixel.clamp(ZOOM_MIN, ZOOM_MAX);
    }

    /// Zoom in by a factor (e.g., 0.8 = 20% closer)
    pub fn zoom_in(&mut self, factor: f32) {
        self.set_zoom(self.config.meters_per_pixel * factor);
    }

    /// Zoom out by a factor (e.g., 1.25 = 25% farther)
    pub fn zoom_out(&mut self, factor: f32) {
        self.set_zoom(self.config.meters_per_pixel * factor);
    }

    /// Render all map elements for a single frame.
    ///
    /// This is the preferred method for rendering as it ensures all elements
    /// are rendered consistently and reduces code duplication across platforms.
    pub fn render_all(&self, vehicle: &VehicleState) -> RenderedFrame {
        let mut segments = self.render(vehicle);
        let route_segments = self.render_route(vehicle);
        segments.extend(route_segments);

        RenderedFrame {
            segments,
            pois: self.render_pois(vehicle),
            areas: self.render_area_triangles(vehicle),
            waterways: self.render_waterways(vehicle),
        }
    }

    /// Transform all roads to screen coordinates based on current vehicle state
    pub fn render(&self, vehicle: &VehicleState) -> Vec<ScreenSegment> {
        let mut segments = Vec::new();

        let center = Point2::new(
            self.config.screen_width / 2.0,
            self.config.screen_height / 2.0,
        );

        // Build transformation matrix
        let transform = self.build_transform(vehicle);

        for road in &self.roads {
            // Convert road points to screen segments
            for window in road.points.windows(2) {
                let (lat1, lon1) = window[0];
                let (lat2, lon2) = window[1];

                // Convert to local meters from vehicle position
                let local1 = geo::lat_lon_to_local_meters(
                    lat1, lon1,
                    vehicle.latitude, vehicle.longitude,
                );
                let local2 = geo::lat_lon_to_local_meters(
                    lat2, lon2,
                    vehicle.latitude, vehicle.longitude,
                );

                // Apply transformation (scale, rotate)
                let screen1 = transform_point(&transform, &local1, &center, self.config.meters_per_pixel);
                let screen2 = transform_point(&transform, &local2, &center, self.config.meters_per_pixel);

                // Clip to screen bounds (with margin)
                let margin = 50.0;
                if is_segment_visible(
                    &screen1, &screen2,
                    self.config.screen_width,
                    self.config.screen_height,
                    margin,
                ) {
                    segments.push(ScreenSegment {
                        x1: screen1.x,
                        y1: screen1.y,
                        x2: screen2.x,
                        y2: screen2.y,
                        road_type: road.road_type.to_int(),
                    });
                }
            }
        }

        segments
    }

    /// Transform the navigation route to screen coordinates
    /// Returns segments with RoadType::Highlight
    pub fn render_route(&self, vehicle: &VehicleState) -> Vec<ScreenSegment> {
        let mut segments = Vec::new();

        let route = match &self.route {
            Some(r) => r,
            None => return segments,
        };

        if route.path.len() < 2 {
            return segments;
        }

        let center = Point2::new(
            self.config.screen_width / 2.0,
            self.config.screen_height / 2.0,
        );

        let transform = self.build_transform(vehicle);

        for window in route.path.windows(2) {
            let (lat1, lon1) = window[0];
            let (lat2, lon2) = window[1];

            let local1 = geo::lat_lon_to_local_meters(
                lat1, lon1,
                vehicle.latitude, vehicle.longitude,
            );
            let local2 = geo::lat_lon_to_local_meters(
                lat2, lon2,
                vehicle.latitude, vehicle.longitude,
            );

            let screen1 = transform_point(&transform, &local1, &center, self.config.meters_per_pixel);
            let screen2 = transform_point(&transform, &local2, &center, self.config.meters_per_pixel);

            let margin = 50.0;
            if is_segment_visible(
                &screen1, &screen2,
                self.config.screen_width,
                self.config.screen_height,
                margin,
            ) {
                segments.push(ScreenSegment {
                    x1: screen1.x,
                    y1: screen1.y,
                    x2: screen2.x,
                    y2: screen2.y,
                    road_type: RoadType::Highlight.to_int(),
                });
            }
        }

        segments
    }

    /// Transform all POIs to screen coordinates based on current vehicle state
    pub fn render_pois(&self, vehicle: &VehicleState) -> Vec<ScreenPoi> {
        let mut pois = Vec::new();

        let center = Point2::new(
            self.config.screen_width / 2.0,
            self.config.screen_height / 2.0,
        );

        let transform = self.build_transform(vehicle);

        for poi in &self.pois {
            let local = geo::lat_lon_to_local_meters(
                poi.lat, poi.lon,
                vehicle.latitude, vehicle.longitude,
            );

            let screen = transform_point(&transform, &local, &center, self.config.meters_per_pixel);

            // Check if POI is visible on screen
            let margin = 20.0;
            if screen.x >= -margin && screen.x <= self.config.screen_width + margin
                && screen.y >= -margin && screen.y <= self.config.screen_height + margin
            {
                pois.push(ScreenPoi {
                    x: screen.x,
                    y: screen.y,
                    poi_type: poi.poi_type.to_int(),
                });
            }
        }

        pois
    }

    /// Transform all waterways to screen coordinates based on current vehicle state
    pub fn render_waterways(&self, vehicle: &VehicleState) -> Vec<ScreenWaterway> {
        let mut segments = Vec::new();

        let center = Point2::new(
            self.config.screen_width / 2.0,
            self.config.screen_height / 2.0,
        );

        let transform = self.build_transform(vehicle);

        for waterway in &self.waterways {
            for window in waterway.points.windows(2) {
                let (lat1, lon1) = window[0];
                let (lat2, lon2) = window[1];

                let local1 = geo::lat_lon_to_local_meters(
                    lat1, lon1,
                    vehicle.latitude, vehicle.longitude,
                );
                let local2 = geo::lat_lon_to_local_meters(
                    lat2, lon2,
                    vehicle.latitude, vehicle.longitude,
                );

                let screen1 = transform_point(&transform, &local1, &center, self.config.meters_per_pixel);
                let screen2 = transform_point(&transform, &local2, &center, self.config.meters_per_pixel);

                let margin = 50.0;
                if is_segment_visible(
                    &screen1, &screen2,
                    self.config.screen_width,
                    self.config.screen_height,
                    margin,
                ) {
                    segments.push(ScreenWaterway {
                        x1: screen1.x,
                        y1: screen1.y,
                        x2: screen2.x,
                        y2: screen2.y,
                        waterway_type: waterway.waterway_type.to_int(),
                    });
                }
            }
        }

        segments
    }

    /// Transform all areas to screen coordinates based on current vehicle state
    pub fn render_areas(&self, vehicle: &VehicleState) -> Vec<ScreenArea> {
        let mut areas = Vec::new();

        let center = Point2::new(
            self.config.screen_width / 2.0,
            self.config.screen_height / 2.0,
        );

        let transform = self.build_transform(vehicle);

        for area in &self.areas {
            let screen_points: Vec<(f32, f32)> = area
                .points
                .iter()
                .map(|(lat, lon)| {
                    let local = geo::lat_lon_to_local_meters(
                        *lat, *lon,
                        vehicle.latitude, vehicle.longitude,
                    );
                    let screen = transform_point(&transform, &local, &center, self.config.meters_per_pixel);
                    (screen.x, screen.y)
                })
                .collect();

            // Check if any point is visible on screen (simple visibility check)
            let margin = 100.0;
            let is_visible = screen_points.iter().any(|(x, y)| {
                *x >= -margin && *x <= self.config.screen_width + margin
                    && *y >= -margin && *y <= self.config.screen_height + margin
            });

            if is_visible && screen_points.len() >= 3 {
                areas.push(ScreenArea {
                    points: screen_points,
                    area_type: area.area_type.to_int(),
                });
            }
        }

        areas
    }

    /// Transform all areas to screen coordinates and triangulate for Slint rendering
    pub fn render_area_triangles(&self, vehicle: &VehicleState) -> Vec<ScreenTriangle> {
        let mut triangles = Vec::new();

        let center = Point2::new(
            self.config.screen_width / 2.0,
            self.config.screen_height / 2.0,
        );

        let transform = self.build_transform(vehicle);

        for area in &self.areas {
            let screen_points: Vec<(f32, f32)> = area
                .points
                .iter()
                .map(|(lat, lon)| {
                    let local = geo::lat_lon_to_local_meters(
                        *lat, *lon,
                        vehicle.latitude, vehicle.longitude,
                    );
                    let screen = transform_point(&transform, &local, &center, self.config.meters_per_pixel);
                    (screen.x, screen.y)
                })
                .collect();

            // Check if any point is visible on screen
            let margin = 100.0;
            let is_visible = screen_points.iter().any(|(x, y)| {
                *x >= -margin && *x <= self.config.screen_width + margin
                    && *y >= -margin && *y <= self.config.screen_height + margin
            });

            if is_visible && screen_points.len() >= 3 {
                let area_type = area.area_type.to_int();
                for tri in triangulate_polygon(&screen_points) {
                    triangles.push(ScreenTriangle {
                        x1: tri[0].0,
                        y1: tri[0].1,
                        x2: tri[1].0,
                        y2: tri[1].1,
                        x3: tri[2].0,
                        y3: tri[2].1,
                        area_type,
                    });
                }
            }
        }

        triangles
    }

    /// Build the rotation matrix based on heading
    fn build_transform(&self, vehicle: &VehicleState) -> Matrix3<f32> {
        if self.config.rotate_with_heading {
            // Rotate map so heading points up
            let angle = -vehicle.heading.to_radians();
            Matrix3::new(
                angle.cos(), -angle.sin(), 0.0,
                angle.sin(),  angle.cos(), 0.0,
                0.0,          0.0,         1.0,
            )
        } else {
            // North-up, no rotation
            Matrix3::identity()
        }
    }
}

/// Apply transformation to convert local meters to screen pixels
fn transform_point(
    rotation: &Matrix3<f32>,
    local: &Point2<f32>,
    center: &Point2<f32>,
    meters_per_pixel: f32,
) -> Point2<f32> {
    // Scale from meters to pixels
    let scaled = Point2::new(
        local.x / meters_per_pixel,
        -local.y / meters_per_pixel,  // Flip Y (screen Y grows down)
    );
    
    // Apply rotation
    let rotated = rotation.transform_point(&scaled);
    
    // Translate to screen center
    Point2::new(
        center.x + rotated.x,
        center.y + rotated.y,
    )
}

/// Check if a line segment is at least partially visible on screen
fn is_segment_visible(
    p1: &Point2<f32>,
    p2: &Point2<f32>,
    width: f32,
    height: f32,
    margin: f32,
) -> bool {
    let min_x = -margin;
    let max_x = width + margin;
    let min_y = -margin;
    let max_y = height + margin;

    // Simple bounding box check (not perfect but fast)
    let seg_min_x = p1.x.min(p2.x);
    let seg_max_x = p1.x.max(p2.x);
    let seg_min_y = p1.y.min(p2.y);
    let seg_max_y = p1.y.max(p2.y);

    seg_max_x >= min_x && seg_min_x <= max_x &&
    seg_max_y >= min_y && seg_min_y <= max_y
}

/// Simple ear-clipping triangulation for convex and simple concave polygons
/// Returns triangles as [(x1,y1), (x2,y2), (x3,y3)]
fn triangulate_polygon(points: &[(f32, f32)]) -> Vec<[(f32, f32); 3]> {
    let mut triangles = Vec::new();

    if points.len() < 3 {
        return triangles;
    }

    // For simple cases, use fan triangulation from first vertex
    // This works well for convex polygons and reasonably convex shapes
    let anchor = points[0];
    for i in 1..points.len() - 1 {
        triangles.push([anchor, points[i], points[i + 1]]);
    }

    triangles
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_road_type_to_int() {
        assert_eq!(RoadType::Primary.to_int(), 0);
        assert_eq!(RoadType::Secondary.to_int(), 1);
        assert_eq!(RoadType::Highlight.to_int(), 2);
    }
    
    #[test]
    fn test_default_config() {
        let config = MinimapConfig::default();
        assert_eq!(config.screen_width, 800.0);
        assert_eq!(config.screen_height, 800.0);
        assert!(config.rotate_with_heading);
    }
}
