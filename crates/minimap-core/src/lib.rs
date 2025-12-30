//! minimap-core: Platform-agnostic map rendering logic
//! 
//! This crate contains the core data structures and algorithms for:
//! - Loading and storing map data (roads, POIs)
//! - Coordinate transformations (lat/lon to screen, rotation)
//! - Map styling and theming

use nalgebra::{Matrix3, Point2, Vector2};
use serde::{Deserialize, Serialize};

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
}

impl PoiType {
    pub fn to_int(&self) -> i32 {
        match self {
            PoiType::GasStation => 0,
            PoiType::Parking => 1,
            PoiType::ShoppingMall => 2,
            PoiType::CarWash => 3,
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

/// The main map renderer
pub struct MapRenderer {
    config: MinimapConfig,
    roads: Vec<WorldRoad>,
    pois: Vec<WorldPoi>,
}

impl MapRenderer {
    pub fn new(config: MinimapConfig) -> Self {
        Self {
            config,
            roads: Vec::new(),
            pois: Vec::new(),
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

    /// Update configuration
    pub fn set_config(&mut self, config: MinimapConfig) {
        self.config = config;
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
