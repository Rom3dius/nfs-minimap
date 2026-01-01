//! Map data loading and management
//!
//! Handles loading road data from various sources:
//! - GeoJSON files
//! - Pre-processed binary formats
//! - (Future) PMTiles/MBTiles

use crate::{RoadType, WorldRoad, WorldPoi, PoiType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A bounding box in lat/lon coordinates
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BoundingBox {
    pub min_lat: f64,
    pub max_lat: f64,
    pub min_lon: f64,
    pub max_lon: f64,
}

impl BoundingBox {
    pub fn new(min_lat: f64, max_lat: f64, min_lon: f64, max_lon: f64) -> Self {
        Self { min_lat, max_lat, min_lon, max_lon }
    }
    
    /// Check if a point is within the bounding box
    pub fn contains(&self, lat: f64, lon: f64) -> bool {
        lat >= self.min_lat && lat <= self.max_lat &&
        lon >= self.min_lon && lon <= self.max_lon
    }
    
    /// Expand the bounding box by a margin in degrees
    pub fn expand(&self, margin: f64) -> Self {
        Self {
            min_lat: self.min_lat - margin,
            max_lat: self.max_lat + margin,
            min_lon: self.min_lon - margin,
            max_lon: self.max_lon + margin,
        }
    }
    
    /// Create a bounding box centered on a point with given radius in meters
    pub fn from_center(lat: f64, lon: f64, radius_m: f64) -> Self {
        // Approximate degrees per meter at this latitude
        let meters_per_degree_lat = 111_320.0;
        let meters_per_degree_lon = 111_320.0 * lat.to_radians().cos();
        
        let lat_delta = radius_m / meters_per_degree_lat;
        let lon_delta = radius_m / meters_per_degree_lon;
        
        Self {
            min_lat: lat - lat_delta,
            max_lat: lat + lat_delta,
            min_lon: lon - lon_delta,
            max_lon: lon + lon_delta,
        }
    }
}

/// Simplified GeoJSON structures for loading road data
#[derive(Debug, Deserialize)]
pub struct GeoJson {
    pub features: Vec<Feature>,
}

#[derive(Debug, Deserialize)]
pub struct Feature {
    pub geometry: Geometry,
    pub properties: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum Geometry {
    LineString { coordinates: Vec<[f64; 2]> },
    MultiLineString { coordinates: Vec<Vec<[f64; 2]>> },
    #[serde(other)]
    Other,
}

/// Load roads from a GeoJSON string
pub fn load_from_geojson(json: &str) -> Result<Vec<WorldRoad>, String> {
    let geojson: GeoJson = serde_json::from_str(json)
        .map_err(|e| format!("Failed to parse GeoJSON: {}", e))?;
    
    let mut roads = Vec::new();
    
    for feature in geojson.features {
        let road_type = classify_road(&feature.properties);
        let name = extract_name(&feature.properties);
        
        match feature.geometry {
            Geometry::LineString { coordinates } => {
                if coordinates.len() >= 2 {
                    roads.push(WorldRoad {
                        name,
                        points: coordinates.iter()
                            .map(|[lon, lat]| (*lat, *lon))  // GeoJSON is [lon, lat]
                            .collect(),
                        road_type,
                    });
                }
            }
            Geometry::MultiLineString { coordinates } => {
                for line in coordinates {
                    if line.len() >= 2 {
                        roads.push(WorldRoad {
                            name: name.clone(),
                            points: line.iter()
                                .map(|[lon, lat]| (*lat, *lon))
                                .collect(),
                            road_type,
                        });
                    }
                }
            }
            Geometry::Other => {
                // Skip non-line geometries (points, polygons)
            }
        }
    }
    
    Ok(roads)
}

/// Fetch roads and POIs from OpenStreetMap Overpass API
///
/// Fetches highway features and POIs (gas stations, parking) within the given bounding box.
pub fn fetch_from_overpass(bbox: &BoundingBox) -> Result<(Vec<WorldRoad>, Vec<WorldPoi>), String> {
    let query = format!(
        r#"[out:json][timeout:30];
        (
          way["highway"~"^(motorway|trunk|primary|secondary|tertiary|residential|unclassified|living_street)$"]({0},{1},{2},{3});
          node["amenity"="fuel"]({0},{1},{2},{3});
          way["amenity"="fuel"]({0},{1},{2},{3});
          way["amenity"="parking"]["parking"="multi-storey"]({0},{1},{2},{3});
          way["amenity"="parking"]["parking"="underground"]({0},{1},{2},{3});
        );
        out body;
        >;
        out skel qt;"#,
        bbox.min_lat, bbox.min_lon, bbox.max_lat, bbox.max_lon
    );

    log::info!("Fetching roads and POIs from Overpass API for bbox: {:?}", bbox);

    let client = reqwest::blocking::Client::new();
    let response = client
        .post("https://overpass-api.de/api/interpreter")
        .body(query)
        .send()
        .map_err(|e| format!("Overpass request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Overpass returned status: {}", response.status()));
    }

    let data: OverpassResponse = response
        .json()
        .map_err(|e| format!("Failed to parse Overpass response: {}", e))?;

    parse_overpass_response(&data)
}

/// Overpass API response structures
#[derive(Debug, Deserialize)]
struct OverpassResponse {
    elements: Vec<OverpassElement>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum OverpassElement {
    #[serde(rename = "node")]
    Node {
        id: i64,
        lat: f64,
        lon: f64,
        tags: Option<HashMap<String, String>>,
    },
    #[serde(rename = "way")]
    Way {
        #[allow(dead_code)]
        id: i64,
        nodes: Vec<i64>,
        tags: Option<HashMap<String, String>>,
    },
    #[serde(other)]
    Other,
}

/// Parse Overpass response into WorldRoad and WorldPoi formats
fn parse_overpass_response(response: &OverpassResponse) -> Result<(Vec<WorldRoad>, Vec<WorldPoi>), String> {
    // First, build a map of node ID -> (coordinates, tags)
    let mut nodes: HashMap<i64, (f64, f64)> = HashMap::new();
    let mut node_tags: HashMap<i64, Option<HashMap<String, String>>> = HashMap::new();

    for element in &response.elements {
        if let OverpassElement::Node { id, lat, lon, tags } = element {
            nodes.insert(*id, (*lat, *lon));
            node_tags.insert(*id, tags.clone());
        }
    }

    let mut roads = Vec::new();
    let mut pois = Vec::new();

    // Process ways (roads and POI areas)
    for element in &response.elements {
        if let OverpassElement::Way { id: _, nodes: node_ids, tags } = element {
            // Check if it's a POI area (gas station or parking)
            if let Some(tags) = tags {
                let amenity = tags.get("amenity").map(|s| s.as_str());
                let poi_type = match amenity {
                    Some("fuel") => Some(PoiType::GasStation),
                    Some("parking") => Some(PoiType::Parking),
                    _ => None,
                };

                if let Some(poi_type) = poi_type {
                    // Calculate centroid of the area
                    let coords: Vec<(f64, f64)> = node_ids
                        .iter()
                        .filter_map(|id| nodes.get(id).copied())
                        .collect();
                    if !coords.is_empty() {
                        let (sum_lat, sum_lon) = coords.iter().fold((0.0, 0.0), |(lat, lon), (la, lo)| (lat + la, lon + lo));
                        let count = coords.len() as f64;
                        pois.push(WorldPoi {
                            name: tags.get("name").cloned(),
                            lat: sum_lat / count,
                            lon: sum_lon / count,
                            poi_type,
                        });
                    }
                    continue;
                }
            }

            // Convert node IDs to coordinates for roads
            let points: Vec<(f64, f64)> = node_ids
                .iter()
                .filter_map(|id| nodes.get(id).copied())
                .collect();

            if points.len() >= 2 {
                let road_type = classify_road_from_tags(tags);
                let name = tags.as_ref().and_then(|t| t.get("name").cloned());

                roads.push(WorldRoad {
                    name,
                    points,
                    road_type,
                });
            }
        }
    }

    // Process POI nodes (gas stations only - parking is way-based for large structures)
    for (id, tags) in &node_tags {
        if let Some(tags) = tags {
            let amenity = tags.get("amenity").map(|s| s.as_str());
            if amenity == Some("fuel") {
                if let Some((lat, lon)) = nodes.get(id) {
                    pois.push(WorldPoi {
                        name: tags.get("name").cloned(),
                        lat: *lat,
                        lon: *lon,
                        poi_type: PoiType::GasStation,
                    });
                }
            }
        }
    }

    log::info!("Parsed {} roads and {} POIs from Overpass response", roads.len(), pois.len());
    Ok((roads, pois))
}

/// Classify road type from OSM tags (HashMap version for Overpass)
fn classify_road_from_tags(tags: &Option<HashMap<String, String>>) -> RoadType {
    let Some(tags) = tags else {
        return RoadType::Secondary;
    };

    let highway = tags.get("highway").map(|s| s.as_str()).unwrap_or("");

    match highway {
        "motorway" | "trunk" | "primary" |
        "motorway_link" | "trunk_link" | "primary_link" => RoadType::Primary,
        _ => RoadType::Secondary,
    }
}

/// Classify road type based on OSM highway tag
fn classify_road(properties: &Option<HashMap<String, serde_json::Value>>) -> RoadType {
    let Some(props) = properties else {
        return RoadType::Secondary;
    };

    let highway = props.get("highway")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match highway {
        // Major roads - Primary styling
        "motorway" | "trunk" | "primary" |
        "motorway_link" | "trunk_link" | "primary_link" => RoadType::Primary,

        // Medium roads - Secondary styling
        "secondary" | "tertiary" | "secondary_link" | "tertiary_link" |
        "unclassified" | "residential" | "living_street" => RoadType::Secondary,

        // Everything else (paths, service roads, etc.) - also Secondary
        _ => RoadType::Secondary,
    }
}

/// Extract road name from properties
fn extract_name(properties: &Option<HashMap<String, serde_json::Value>>) -> Option<String> {
    properties.as_ref()?
        .get("name")?
        .as_str()
        .map(|s| s.to_string())
}

/// Generate sample road data for testing (creates a grid around a center point)
pub fn generate_sample_roads(center_lat: f64, center_lon: f64) -> Vec<WorldRoad> {
    let mut roads = Vec::new();
    
    // Approximate grid spacing in degrees (roughly 100m)
    let spacing = 0.001;
    let grid_size = 10;
    
    // Create horizontal roads
    for i in -grid_size..=grid_size {
        let lat = center_lat + (i as f64 * spacing);
        let road_type = if i == 0 { RoadType::Primary } else { RoadType::Secondary };
        
        roads.push(WorldRoad {
            name: Some(format!("Horizontal {}", i)),
            points: vec![
                (lat, center_lon - (grid_size as f64 * spacing)),
                (lat, center_lon + (grid_size as f64 * spacing)),
            ],
            road_type,
        });
    }
    
    // Create vertical roads
    for i in -grid_size..=grid_size {
        let lon = center_lon + (i as f64 * spacing);
        let road_type = if i == 0 { RoadType::Primary } else { RoadType::Secondary };
        
        roads.push(WorldRoad {
            name: Some(format!("Vertical {}", i)),
            points: vec![
                (center_lat - (grid_size as f64 * spacing), lon),
                (center_lat + (grid_size as f64 * spacing), lon),
            ],
            road_type,
        });
    }
    
    // Add a diagonal "highway" for visual interest
    roads.push(WorldRoad {
        name: Some("Main Highway".to_string()),
        points: vec![
            (center_lat - (grid_size as f64 * spacing), center_lon - (grid_size as f64 * spacing)),
            (center_lat + (grid_size as f64 * spacing), center_lon + (grid_size as f64 * spacing)),
        ],
        road_type: RoadType::Highlight,
    });
    
    roads
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_bounding_box_contains() {
        let bbox = BoundingBox::new(47.0, 48.0, 8.0, 9.0);
        
        assert!(bbox.contains(47.5, 8.5));
        assert!(!bbox.contains(46.0, 8.5));
        assert!(!bbox.contains(47.5, 10.0));
    }
    
    #[test]
    fn test_generate_sample_roads() {
        let roads = generate_sample_roads(47.3769, 8.5417);
        
        // Should have horizontal + vertical + diagonal roads
        assert!(roads.len() > 40);
        
        // Should have at least one highlight road
        assert!(roads.iter().any(|r| r.road_type == RoadType::Highlight));
    }
    
    #[test]
    fn test_geojson_parsing() {
        let json = r#"{
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature",
                "geometry": {
                    "type": "LineString",
                    "coordinates": [[8.54, 47.37], [8.55, 47.38]]
                },
                "properties": {
                    "highway": "primary",
                    "name": "Test Road"
                }
            }]
        }"#;
        
        let roads = load_from_geojson(json).unwrap();
        
        assert_eq!(roads.len(), 1);
        assert_eq!(roads[0].name, Some("Test Road".to_string()));
        assert_eq!(roads[0].road_type, RoadType::Primary);
        assert_eq!(roads[0].points.len(), 2);
    }
}
