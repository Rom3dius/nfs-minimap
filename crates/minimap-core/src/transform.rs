//! Coordinate transformation utilities
//!
//! Additional transformation helpers beyond what's in the main lib

use nalgebra::{Matrix3, Point2};

/// Viewport transformation parameters
#[derive(Debug, Clone)]
pub struct Viewport {
    /// Screen width in pixels
    pub width: f32,
    /// Screen height in pixels  
    pub height: f32,
    /// Center X offset
    pub center_x: f32,
    /// Center Y offset
    pub center_y: f32,
    /// Scale factor (pixels per meter)
    pub scale: f32,
    /// Rotation angle in radians
    pub rotation: f32,
}

impl Viewport {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            center_x: width / 2.0,
            center_y: height / 2.0,
            scale: 1.0,
            rotation: 0.0,
        }
    }
    
    /// Set meters per pixel (inverse of scale)
    pub fn with_meters_per_pixel(mut self, mpp: f32) -> Self {
        self.scale = 1.0 / mpp;
        self
    }
    
    /// Set rotation in degrees
    pub fn with_rotation_degrees(mut self, degrees: f32) -> Self {
        self.rotation = degrees.to_radians();
        self
    }
    
    /// Build the full transformation matrix
    pub fn to_matrix(&self) -> Matrix3<f32> {
        let cos_r = self.rotation.cos();
        let sin_r = self.rotation.sin();
        
        // Combined: translate to origin, rotate, scale, translate to center
        Matrix3::new(
            self.scale * cos_r,  -self.scale * sin_r, self.center_x,
            self.scale * sin_r,   self.scale * cos_r, self.center_y,
            0.0,                  0.0,                1.0,
        )
    }
    
    /// Transform a point from world (meters) to screen coordinates
    pub fn world_to_screen(&self, world: &Point2<f32>) -> Point2<f32> {
        // Apply rotation
        let cos_r = self.rotation.cos();
        let sin_r = self.rotation.sin();
        
        let rotated_x = world.x * cos_r - world.y * sin_r;
        let rotated_y = world.x * sin_r + world.y * cos_r;
        
        // Apply scale and translate to center
        // Note: Y is flipped for screen coordinates
        Point2::new(
            self.center_x + rotated_x * self.scale,
            self.center_y - rotated_y * self.scale,
        )
    }
    
    /// Transform a point from screen to world coordinates
    pub fn screen_to_world(&self, screen: &Point2<f32>) -> Point2<f32> {
        // Translate from center
        let centered_x = screen.x - self.center_x;
        let centered_y = -(screen.y - self.center_y);  // Flip Y
        
        // Apply inverse scale
        let scaled_x = centered_x / self.scale;
        let scaled_y = centered_y / self.scale;
        
        // Apply inverse rotation
        let cos_r = self.rotation.cos();
        let sin_r = self.rotation.sin();
        
        Point2::new(
            scaled_x * cos_r + scaled_y * sin_r,
            -scaled_x * sin_r + scaled_y * cos_r,
        )
    }
}

/// Smooth interpolation for heading changes (avoids jarring jumps)
pub fn smooth_angle(current: f32, target: f32, smoothing: f32) -> f32 {
    // Handle wrap-around at 360 degrees
    let mut diff = target - current;
    
    if diff > 180.0 {
        diff -= 360.0;
    } else if diff < -180.0 {
        diff += 360.0;
    }
    
    let new_angle = current + diff * smoothing;
    
    // Normalize to 0-360
    ((new_angle % 360.0) + 360.0) % 360.0
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_viewport_no_transform() {
        let viewport = Viewport::new(800.0, 800.0)
            .with_meters_per_pixel(1.0);  // 1:1 scale
        
        let world = Point2::new(0.0, 0.0);
        let screen = viewport.world_to_screen(&world);
        
        // Should map to center
        assert!((screen.x - 400.0).abs() < 0.001);
        assert!((screen.y - 400.0).abs() < 0.001);
    }
    
    #[test]
    fn test_viewport_roundtrip() {
        let viewport = Viewport::new(800.0, 800.0)
            .with_meters_per_pixel(2.0)
            .with_rotation_degrees(45.0);
        
        let original = Point2::new(100.0, 50.0);
        let screen = viewport.world_to_screen(&original);
        let back = viewport.screen_to_world(&screen);
        
        assert!((back.x - original.x).abs() < 0.01);
        assert!((back.y - original.y).abs() < 0.01);
    }
    
    #[test]
    fn test_smooth_angle_no_wrap() {
        let result = smooth_angle(10.0, 20.0, 0.5);
        assert!((result - 15.0).abs() < 0.001);
    }
    
    #[test]
    fn test_smooth_angle_wrap_around() {
        // From 350 to 10 should go through 0, not backwards
        let result = smooth_angle(350.0, 10.0, 0.5);
        assert!(result > 350.0 || result < 20.0);
    }
}
