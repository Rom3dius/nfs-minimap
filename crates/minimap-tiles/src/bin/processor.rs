//! OSM PBF to tile processor
//!
//! Usage: tile-processor <input.osm.pbf> <output-dir>
//!
//! Downloads Switzerland PBF from:
//! https://download.geofabrik.de/europe/switzerland-latest.osm.pbf

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use minimap_tiles::*;
use osmpbf::{Element, ElementReader};

fn main() {
    env_logger::init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <input.osm.pbf> <output-dir>", args[0]);
        eprintln!();
        eprintln!("Download Switzerland PBF from:");
        eprintln!("  https://download.geofabrik.de/europe/switzerland-latest.osm.pbf");
        std::process::exit(1);
    }

    let input_path = &args[1];
    let output_dir = &args[2];

    // Create output directory
    fs::create_dir_all(output_dir).expect("Failed to create output directory");

    log::info!("Processing {} -> {}", input_path, output_dir);

    // First pass: collect all nodes we need (for ways)
    log::info!("Pass 1: Collecting node IDs from ways...");
    let needed_nodes = collect_needed_nodes(input_path);
    log::info!("Found {} needed nodes", needed_nodes.len());

    // Second pass: collect node coordinates
    log::info!("Pass 2: Reading node coordinates...");
    let node_coords = collect_node_coords(input_path, &needed_nodes);
    log::info!("Collected {} node coordinates", node_coords.len());

    // Third pass: process ways and build tiles
    log::info!("Pass 3: Processing ways and building tiles...");
    let tiles = process_ways(input_path, &node_coords);
    log::info!("Created {} tiles", tiles.len());

    // Write tiles to disk
    log::info!("Writing tiles to disk...");
    let mut total_roads = 0;
    let mut total_pois = 0;

    for ((tx, ty), tile) in &tiles {
        total_roads += tile.roads.len();
        total_pois += tile.pois.len();

        let filename = tile_filename(*tx, *ty);
        let path = Path::new(output_dir).join(&filename);

        let bytes = tile.to_bytes().expect("Failed to serialize tile");
        fs::write(&path, &bytes).expect("Failed to write tile");
    }

    log::info!(
        "Done! Wrote {} tiles with {} roads and {} POIs",
        tiles.len(),
        total_roads,
        total_pois
    );

    // Write index file
    let index_path = Path::new(output_dir).join("index.txt");
    let mut index_content = String::new();
    for (tx, ty) in tiles.keys() {
        index_content.push_str(&format!("{},{}\n", tx, ty));
    }
    fs::write(&index_path, &index_content).expect("Failed to write index");
    log::info!("Wrote tile index to {}", index_path.display());
}

/// Collect node IDs that are referenced by ways we care about
fn collect_needed_nodes(path: &str) -> std::collections::HashSet<i64> {
    let reader = ElementReader::from_path(path).expect("Failed to open PBF");
    let mut needed = std::collections::HashSet::new();

    reader
        .for_each(|element| {
            if let Element::Way(way) = element {
                let dominated_tags: Vec<(&str, &str)> = way.tags().collect();

                // Check if it's a road we want
                let dominated_is_road = dominated_tags.iter().any(|(k, v)| {
                    *k == "highway"
                        && matches!(
                            *v,
                            "motorway"
                                | "motorway_link"
                                | "trunk"
                                | "trunk_link"
                                | "primary"
                                | "primary_link"
                                | "secondary"
                                | "secondary_link"
                                | "tertiary"
                                | "tertiary_link"
                                | "residential"
                                | "unclassified"
                                | "living_street"
                                | "service"
                        )
                });

                // Check if it's a POI we want
                let is_fuel = dominated_tags.iter().any(|(k, v)| *k == "amenity" && *v == "fuel");
                let is_parking = dominated_tags.iter().any(|(k, v)| *k == "amenity" && *v == "parking")
                    && dominated_tags
                        .iter()
                        .any(|(k, v)| *k == "parking" && (*v == "multi-storey" || *v == "underground"));
                let is_mall = dominated_tags.iter().any(|(k, v)| *k == "shop" && *v == "mall");
                let is_car_wash = dominated_tags.iter().any(|(k, v)| *k == "amenity" && *v == "car_wash");

                if dominated_is_road || is_fuel || is_parking || is_mall || is_car_wash {
                    for node_id in way.refs() {
                        needed.insert(node_id);
                    }
                }
            }
        })
        .expect("Failed to read PBF");

    needed
}

/// Collect coordinates for needed nodes
fn collect_node_coords(
    path: &str,
    needed: &std::collections::HashSet<i64>,
) -> HashMap<i64, (f64, f64)> {
    let reader = ElementReader::from_path(path).expect("Failed to open PBF");
    let mut coords = HashMap::new();

    reader
        .for_each(|element| {
            if let Element::DenseNode(node) = element {
                if needed.contains(&node.id()) {
                    coords.insert(node.id(), (node.lat(), node.lon()));
                }
            } else if let Element::Node(node) = element {
                if needed.contains(&node.id()) {
                    coords.insert(node.id(), (node.lat(), node.lon()));
                }
            }
        })
        .expect("Failed to read PBF");

    coords
}

/// Process ways and build tiles
fn process_ways(path: &str, node_coords: &HashMap<i64, (f64, f64)>) -> HashMap<(i32, i32), Tile> {
    let reader = ElementReader::from_path(path).expect("Failed to open PBF");
    let mut tiles: HashMap<(i32, i32), Tile> = HashMap::new();

    reader
        .for_each(|element| {
            if let Element::Way(way) = element {
                let tags: Vec<(&str, &str)> = way.tags().collect();

                // Get node coordinates for this way
                let points: Vec<(f64, f64)> = way
                    .refs()
                    .filter_map(|id| node_coords.get(&id).copied())
                    .collect();

                if points.is_empty() {
                    return;
                }

                // Check if it's a road
                let highway = tags.iter().find(|(k, _)| *k == "highway").map(|(_, v)| *v);
                if let Some(hw) = highway {
                    let road_type = match hw {
                        "motorway" | "motorway_link" | "trunk" | "trunk_link" | "primary"
                        | "primary_link" => RoadType::Primary,
                        "secondary" | "secondary_link" | "tertiary" | "tertiary_link"
                        | "residential" | "unclassified" | "living_street" | "service" => {
                            RoadType::Secondary
                        }
                        _ => return,
                    };

                    // Add road to all tiles it passes through
                    add_road_to_tiles(&mut tiles, &points, road_type);
                    return;
                }

                // Check if it's a POI
                let amenity = tags.iter().find(|(k, _)| *k == "amenity").map(|(_, v)| *v);
                let shop = tags.iter().find(|(k, _)| *k == "shop").map(|(_, v)| *v);

                let poi_type = match amenity {
                    Some("fuel") => Some(PoiType::GasStation),
                    Some("car_wash") => Some(PoiType::CarWash),
                    Some("parking") => {
                        let parking_type =
                            tags.iter().find(|(k, _)| *k == "parking").map(|(_, v)| *v);
                        if parking_type == Some("multi-storey")
                            || parking_type == Some("underground")
                        {
                            Some(PoiType::Parking)
                        } else {
                            None
                        }
                    }
                    _ => None,
                }.or_else(|| match shop {
                    Some("mall") => Some(PoiType::ShoppingMall),
                    _ => None,
                });

                if let Some(pt) = poi_type {
                    // Calculate centroid
                    let (sum_lat, sum_lon) = points.iter().fold((0.0, 0.0), |(la, lo), (lat, lon)| {
                        (la + lat, lo + lon)
                    });
                    let count = points.len() as f64;
                    let center_lat = sum_lat / count;
                    let center_lon = sum_lon / count;

                    add_poi_to_tiles(&mut tiles, center_lat, center_lon, pt);
                }
            }
        })
        .expect("Failed to read PBF");

    // Also check for node-based POIs
    let reader = ElementReader::from_path(path).expect("Failed to open PBF");
    reader
        .for_each(|element| {
            let (_id, lat, lon, tags): (i64, f64, f64, Vec<(&str, &str)>) = match element {
                Element::DenseNode(node) => {
                    (node.id(), node.lat(), node.lon(), node.tags().collect())
                }
                Element::Node(node) => (node.id(), node.lat(), node.lon(), node.tags().collect()),
                _ => return,
            };

            let amenity = tags.iter().find(|(k, _)| *k == "amenity").map(|(_, v)| *v);
            let shop = tags.iter().find(|(k, _)| *k == "shop").map(|(_, v)| *v);

            let poi_type = match amenity {
                Some("fuel") => Some(PoiType::GasStation),
                Some("car_wash") => Some(PoiType::CarWash),
                _ => None,
            }.or_else(|| match shop {
                Some("mall") => Some(PoiType::ShoppingMall),
                _ => None,
            });

            if let Some(pt) = poi_type {
                add_poi_to_tiles(&mut tiles, lat, lon, pt);
            }
        })
        .expect("Failed to read PBF");

    tiles
}

fn add_road_to_tiles(tiles: &mut HashMap<(i32, i32), Tile>, points: &[(f64, f64)], road_type: RoadType) {
    // Process each segment (pair of consecutive points)
    for window in points.windows(2) {
        let (lat1, lon1) = window[0];
        let (lat2, lon2) = window[1];

        // Find all tiles this segment passes through
        let tiles_touched = get_tiles_for_segment(lat1, lon1, lat2, lon2);

        for (tx, ty) in tiles_touched {
            // Clip segment to tile bounds and convert to tile coordinates
            let tile_min_lat = ty as f64 * TILE_SIZE_DEG;
            let tile_max_lat = tile_min_lat + TILE_SIZE_DEG;
            let tile_min_lon = tx as f64 * TILE_SIZE_DEG;
            let tile_max_lon = tile_min_lon + TILE_SIZE_DEG;

            // Clip the segment to tile bounds
            if let Some((clipped_lat1, clipped_lon1, clipped_lat2, clipped_lon2)) =
                clip_segment_to_tile(lat1, lon1, lat2, lon2, tile_min_lat, tile_max_lat, tile_min_lon, tile_max_lon)
            {
                let (ox1, oy1) = lat_lon_to_tile_offset(clipped_lat1, clipped_lon1, tx, ty);
                let (ox2, oy2) = lat_lon_to_tile_offset(clipped_lat2, clipped_lon2, tx, ty);

                let tile = tiles.entry((tx, ty)).or_insert_with(|| Tile::new(tx, ty));
                tile.roads.push(TileRoad {
                    road_type,
                    points: vec![(ox1, oy1), (ox2, oy2)],
                });
            }
        }
    }
}

/// Get all tiles that a line segment passes through
fn get_tiles_for_segment(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> Vec<(i32, i32)> {
    let mut tiles = Vec::new();

    let (tx1, ty1) = lat_lon_to_tile(lat1, lon1);
    let (tx2, ty2) = lat_lon_to_tile(lat2, lon2);

    let min_tx = tx1.min(tx2);
    let max_tx = tx1.max(tx2);
    let min_ty = ty1.min(ty2);
    let max_ty = ty1.max(ty2);

    for tx in min_tx..=max_tx {
        for ty in min_ty..=max_ty {
            tiles.push((tx, ty));
        }
    }

    tiles
}

/// Clip a line segment to tile bounds using Cohen-Sutherland algorithm
fn clip_segment_to_tile(
    mut lat1: f64, mut lon1: f64,
    mut lat2: f64, mut lon2: f64,
    min_lat: f64, max_lat: f64,
    min_lon: f64, max_lon: f64,
) -> Option<(f64, f64, f64, f64)> {
    // Cohen-Sutherland outcodes
    const INSIDE: u8 = 0;
    const LEFT: u8 = 1;
    const RIGHT: u8 = 2;
    const BOTTOM: u8 = 4;
    const TOP: u8 = 8;

    let compute_outcode = |lat: f64, lon: f64| -> u8 {
        let mut code = INSIDE;
        if lon < min_lon { code |= LEFT; }
        else if lon > max_lon { code |= RIGHT; }
        if lat < min_lat { code |= BOTTOM; }
        else if lat > max_lat { code |= TOP; }
        code
    };

    let mut outcode1 = compute_outcode(lat1, lon1);
    let mut outcode2 = compute_outcode(lat2, lon2);

    loop {
        if (outcode1 | outcode2) == 0 {
            // Both points inside
            return Some((lat1, lon1, lat2, lon2));
        } else if (outcode1 & outcode2) != 0 {
            // Both points share an outside zone
            return None;
        }

        // Pick an outside point
        let outcode_out = if outcode1 != 0 { outcode1 } else { outcode2 };

        let (lat, lon) = if (outcode_out & TOP) != 0 {
            let lon = lon1 + (lon2 - lon1) * (max_lat - lat1) / (lat2 - lat1);
            (max_lat, lon)
        } else if (outcode_out & BOTTOM) != 0 {
            let lon = lon1 + (lon2 - lon1) * (min_lat - lat1) / (lat2 - lat1);
            (min_lat, lon)
        } else if (outcode_out & RIGHT) != 0 {
            let lat = lat1 + (lat2 - lat1) * (max_lon - lon1) / (lon2 - lon1);
            (lat, max_lon)
        } else {
            let lat = lat1 + (lat2 - lat1) * (min_lon - lon1) / (lon2 - lon1);
            (lat, min_lon)
        };

        if outcode_out == outcode1 {
            lat1 = lat;
            lon1 = lon;
            outcode1 = compute_outcode(lat1, lon1);
        } else {
            lat2 = lat;
            lon2 = lon;
            outcode2 = compute_outcode(lat2, lon2);
        }
    }
}

fn add_poi_to_tiles(tiles: &mut HashMap<(i32, i32), Tile>, lat: f64, lon: f64, poi_type: PoiType) {
    let (tx, ty) = lat_lon_to_tile(lat, lon);
    let (ox, oy) = lat_lon_to_tile_offset(lat, lon, tx, ty);

    let tile = tiles.entry((tx, ty)).or_insert_with(|| Tile::new(tx, ty));
    tile.pois.push(TilePoi {
        poi_type,
        x: ox,
        y: oy,
    });
}
