//! OSM PBF to tile processor
//!
//! Usage: tile-processor <input.osm.pbf> <output-dir>
//!
//! Downloads Switzerland PBF from:
//! https://download.geofabrik.de/europe/switzerland-latest.osm.pbf

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use minimap_tiles::*;
use osmpbf::{Element, ElementReader, RelMemberType};

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

    // Pass 1: Collect way IDs from water relations
    log::info!("Pass 1: Collecting way IDs from water relations...");
    let (relation_ways, water_relations) = collect_water_relation_ways(input_path);
    log::info!("Found {} water relations with {} member ways", water_relations.len(), relation_ways.len());

    // Pass 2: collect all nodes we need (for ways + relation member ways)
    log::info!("Pass 2: Collecting node IDs from ways...");
    let needed_nodes = collect_needed_nodes(input_path, &relation_ways);
    log::info!("Found {} needed nodes", needed_nodes.len());

    // Pass 3: collect node coordinates
    log::info!("Pass 3: Reading node coordinates...");
    let node_coords = collect_node_coords(input_path, &needed_nodes);
    log::info!("Collected {} node coordinates", node_coords.len());

    // Pass 4: collect way geometries for relation members
    log::info!("Pass 4: Collecting way geometries for relations...");
    let way_geometries = collect_way_geometries(input_path, &relation_ways, &node_coords);
    log::info!("Collected {} way geometries", way_geometries.len());

    // Pass 5: process ways and relations, build tiles
    log::info!("Pass 5: Processing ways/relations and building tiles...");
    let tiles = process_ways_and_relations(input_path, &node_coords, &water_relations, &way_geometries);
    log::info!("Created {} tiles", tiles.len());

    // Write tiles to disk
    log::info!("Writing tiles to disk...");
    let mut total_roads = 0;
    let mut total_pois = 0;
    let mut total_areas = 0;
    let mut total_waterways = 0;

    for ((tx, ty), tile) in &tiles {
        total_roads += tile.roads.len();
        total_pois += tile.pois.len();
        total_areas += tile.areas.len();
        total_waterways += tile.waterways.len();

        let filename = tile_filename(*tx, *ty);
        let path = Path::new(output_dir).join(&filename);

        let bytes = tile.to_bytes().expect("Failed to serialize tile");
        fs::write(&path, &bytes).expect("Failed to write tile");
    }

    log::info!(
        "Done! Wrote {} tiles with {} roads, {} POIs, {} areas, and {} waterways",
        tiles.len(),
        total_roads,
        total_pois,
        total_areas,
        total_waterways
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

/// Water relation info
struct WaterRelation {
    outer_ways: Vec<i64>,
}

/// Collect way IDs from water relations (lakes, rivers as multipolygons)
fn collect_water_relation_ways(path: &str) -> (HashSet<i64>, HashMap<i64, WaterRelation>) {
    let reader = ElementReader::from_path(path).expect("Failed to open PBF");
    let mut way_ids = HashSet::new();
    let mut relations = HashMap::new();

    reader
        .for_each(|element| {
            if let Element::Relation(rel) = element {
                let tags: Vec<(&str, &str)> = rel.tags().collect();

                // Check if it's a water multipolygon
                let is_multipolygon = tags.iter().any(|(k, v)| *k == "type" && *v == "multipolygon");
                let is_water = tags.iter().any(|(k, v)| {
                    (*k == "natural" && *v == "water")
                        || (*k == "waterway" && *v == "riverbank")
                        || (*k == "landuse" && *v == "reservoir")
                        || *k == "water"
                });

                if is_multipolygon && is_water {
                    let mut outer_ways = Vec::new();

                    for member in rel.members() {
                        if member.member_type == RelMemberType::Way {
                            // Include outer and unlabeled (default outer) roles
                            let role = member.role().unwrap_or("");
                            if role == "outer" || role.is_empty() {
                                way_ids.insert(member.member_id);
                                outer_ways.push(member.member_id);
                            }
                        }
                    }

                    if !outer_ways.is_empty() {
                        relations.insert(rel.id(), WaterRelation { outer_ways });
                    }
                }
            }
        })
        .expect("Failed to read PBF");

    (way_ids, relations)
}

/// Collect node IDs that are referenced by ways we care about
fn collect_needed_nodes(path: &str, relation_ways: &HashSet<i64>) -> HashSet<i64> {
    let reader = ElementReader::from_path(path).expect("Failed to open PBF");
    let mut needed = HashSet::new();

    reader
        .for_each(|element| {
            if let Element::Way(way) = element {
                let tags: Vec<(&str, &str)> = way.tags().collect();

                // Check if it's a road we want
                let is_road = tags.iter().any(|(k, v)| {
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
                let is_fuel = tags.iter().any(|(k, v)| *k == "amenity" && *v == "fuel");
                let is_parking = tags.iter().any(|(k, v)| *k == "amenity" && *v == "parking")
                    && tags
                        .iter()
                        .any(|(k, v)| *k == "parking" && (*v == "multi-storey" || *v == "underground"))
                    && !tags.iter().any(|(k, _)| *k == "bicycle");  // Exclude bicycle parking
                let is_mall = tags.iter().any(|(k, v)| *k == "shop" && *v == "mall");
                let is_car_wash = tags.iter().any(|(k, v)| *k == "amenity" && *v == "car_wash");
                let is_fast_food = tags.iter().any(|(k, v)| *k == "amenity" && *v == "fast_food");

                // Check if it's an area we want (water, forest, park)
                let is_water = tags.iter().any(|(k, v)| {
                    (*k == "natural" && *v == "water")
                        || (*k == "waterway" && *v == "riverbank")
                        || (*k == "landuse" && *v == "reservoir")
                        || *k == "water"
                });
                let is_forest = tags.iter().any(|(k, v)| {
                    (*k == "natural" && *v == "wood") || (*k == "landuse" && *v == "forest")
                });
                let is_park = tags.iter().any(|(k, v)| *k == "leisure" && *v == "park");
                let is_grass = tags.iter().any(|(k, v)| {
                    (*k == "landuse" && (*v == "grass" || *v == "meadow"))
                        || (*k == "natural" && *v == "grassland")
                });

                // Check if it's a linear waterway (river, stream, canal)
                let is_waterway = tags.iter().any(|(k, v)| {
                    *k == "waterway" && matches!(*v, "river" | "stream" | "canal")
                });

                // Also check if this way is part of a water relation
                let is_relation_member = relation_ways.contains(&way.id());

                if is_road || is_fuel || is_parking || is_mall || is_car_wash || is_fast_food
                    || is_water || is_forest || is_park || is_grass || is_waterway || is_relation_member
                {
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

/// Collect way geometries for relation member ways
fn collect_way_geometries(
    path: &str,
    relation_ways: &HashSet<i64>,
    node_coords: &HashMap<i64, (f64, f64)>,
) -> HashMap<i64, Vec<(f64, f64)>> {
    let reader = ElementReader::from_path(path).expect("Failed to open PBF");
    let mut geometries = HashMap::new();

    reader
        .for_each(|element| {
            if let Element::Way(way) = element {
                if relation_ways.contains(&way.id()) {
                    let points: Vec<(f64, f64)> = way
                        .refs()
                        .filter_map(|id| node_coords.get(&id).copied())
                        .collect();

                    if !points.is_empty() {
                        geometries.insert(way.id(), points);
                    }
                }
            }
        })
        .expect("Failed to read PBF");

    geometries
}

/// Process ways and relations, build tiles
fn process_ways_and_relations(
    path: &str,
    node_coords: &HashMap<i64, (f64, f64)>,
    water_relations: &HashMap<i64, WaterRelation>,
    way_geometries: &HashMap<i64, Vec<(f64, f64)>>,
) -> HashMap<(i32, i32), Tile> {
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

                // Check if it's a linear waterway (river, stream, canal)
                let waterway = tags.iter().find(|(k, _)| *k == "waterway").map(|(_, v)| *v);
                if let Some(ww) = waterway {
                    let waterway_type = match ww {
                        "river" => Some(WaterwayType::River),
                        "stream" => Some(WaterwayType::Stream),
                        "canal" => Some(WaterwayType::Canal),
                        _ => None,
                    };

                    if let Some(wt) = waterway_type {
                        add_waterway_to_tiles(&mut tiles, &points, wt);
                        return;
                    }
                }

                // Check if it's an area (water, forest, park, grass)
                // Only treat as area if the way is closed (first point ≈ last point)
                let is_closed = if points.len() >= 3 {
                    let first = points.first().unwrap();
                    let last = points.last().unwrap();
                    (first.0 - last.0).abs() < 0.0001 && (first.1 - last.1).abs() < 0.0001
                } else {
                    false
                };

                if is_closed {
                    let natural = tags.iter().find(|(k, _)| *k == "natural").map(|(_, v)| *v);
                    let landuse = tags.iter().find(|(k, _)| *k == "landuse").map(|(_, v)| *v);
                    let leisure = tags.iter().find(|(k, _)| *k == "leisure").map(|(_, v)| *v);
                    let has_water_tag = tags.iter().any(|(k, _)| *k == "water");

                    let area_type = if natural == Some("water") || waterway == Some("riverbank")
                        || landuse == Some("reservoir") || has_water_tag
                    {
                        Some(AreaType::Water)
                    } else if natural == Some("wood") || landuse == Some("forest") {
                        Some(AreaType::Forest)
                    } else if leisure == Some("park") {
                        Some(AreaType::Park)
                    } else if landuse == Some("grass") || landuse == Some("meadow")
                        || natural == Some("grassland")
                    {
                        Some(AreaType::Grass)
                    } else {
                        None
                    };

                    if let Some(at) = area_type {
                        add_area_to_tiles(&mut tiles, &points, at);
                        return;
                    }
                }

                // Check if it's a POI
                let amenity = tags.iter().find(|(k, _)| *k == "amenity").map(|(_, v)| *v);
                let shop = tags.iter().find(|(k, _)| *k == "shop").map(|(_, v)| *v);
                let has_bicycle_tag = tags.iter().any(|(k, _)| *k == "bicycle");

                let poi_type = match amenity {
                    Some("fuel") => Some(PoiType::GasStation),
                    Some("car_wash") => Some(PoiType::CarWash),
                    Some("fast_food") => Some(PoiType::FastFood),
                    Some("parking") => {
                        // Exclude bicycle parking
                        if has_bicycle_tag {
                            None
                        } else {
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
            let has_bicycle_tag = tags.iter().any(|(k, _)| *k == "bicycle");

            let poi_type = match amenity {
                Some("fuel") => Some(PoiType::GasStation),
                Some("car_wash") => Some(PoiType::CarWash),
                Some("fast_food") => Some(PoiType::FastFood),
                Some("parking") => {
                    // Exclude bicycle parking
                    if has_bicycle_tag {
                        None
                    } else {
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
                }
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

    // Process water relations (lakes, etc.)
    for (_rel_id, relation) in water_relations {
        // Try to join ways into closed polygons
        let mut remaining_ways: Vec<Vec<(f64, f64)>> = relation
            .outer_ways
            .iter()
            .filter_map(|way_id| way_geometries.get(way_id).cloned())
            .filter(|points| points.len() >= 2)
            .collect();

        while !remaining_ways.is_empty() {
            // Start a new polygon with the first remaining way
            let mut polygon = remaining_ways.remove(0);

            // Try to extend the polygon by finding connecting ways
            let mut made_progress = true;
            while made_progress {
                made_progress = false;

                let first = polygon.first().unwrap();
                let last = polygon.last().unwrap();

                // Check if polygon is already closed
                if polygon.len() >= 3
                    && (first.0 - last.0).abs() < 0.0001
                    && (first.1 - last.1).abs() < 0.0001
                {
                    break;
                }

                // Find a way that connects to the end of our polygon
                for i in 0..remaining_ways.len() {
                    let way = &remaining_ways[i];
                    let way_first = way.first().unwrap();
                    let way_last = way.last().unwrap();

                    // Check if way connects to end of polygon
                    if (last.0 - way_first.0).abs() < 0.0001 && (last.1 - way_first.1).abs() < 0.0001
                    {
                        // Append way (skip first point as it's the same as polygon's last)
                        let mut way = remaining_ways.remove(i);
                        polygon.extend(way.drain(1..));
                        made_progress = true;
                        break;
                    } else if (last.0 - way_last.0).abs() < 0.0001
                        && (last.1 - way_last.1).abs() < 0.0001
                    {
                        // Append reversed way
                        let mut way = remaining_ways.remove(i);
                        way.reverse();
                        polygon.extend(way.drain(1..));
                        made_progress = true;
                        break;
                    }
                }
            }

            // Only add if the polygon is closed
            if polygon.len() >= 3 {
                let first = polygon.first().unwrap();
                let last = polygon.last().unwrap();
                if (first.0 - last.0).abs() < 0.0001 && (first.1 - last.1).abs() < 0.0001 {
                    add_area_to_tiles(&mut tiles, &polygon, AreaType::Water);
                }
            }
        }
    }

    log::info!("Processed {} water relations", water_relations.len());

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

fn add_waterway_to_tiles(tiles: &mut HashMap<(i32, i32), Tile>, points: &[(f64, f64)], waterway_type: WaterwayType) {
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
                tile.waterways.push(TileWaterway {
                    waterway_type,
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

fn add_area_to_tiles(tiles: &mut HashMap<(i32, i32), Tile>, points: &[(f64, f64)], area_type: AreaType) {
    if points.len() < 3 {
        return; // Need at least 3 points for a polygon
    }

    // Find the bounding box of the polygon
    let min_lat = points.iter().map(|(lat, _)| *lat).fold(f64::INFINITY, f64::min);
    let max_lat = points.iter().map(|(lat, _)| *lat).fold(f64::NEG_INFINITY, f64::max);
    let min_lon = points.iter().map(|(_, lon)| *lon).fold(f64::INFINITY, f64::min);
    let max_lon = points.iter().map(|(_, lon)| *lon).fold(f64::NEG_INFINITY, f64::max);

    let (min_tx, min_ty) = lat_lon_to_tile(min_lat, min_lon);
    let (max_tx, max_ty) = lat_lon_to_tile(max_lat, max_lon);

    // Add the polygon to each tile it touches, CLIPPED to tile bounds
    for tx in min_tx..=max_tx {
        for ty in min_ty..=max_ty {
            // Tile bounds in lat/lon
            let tile_min_lat = ty as f64 * TILE_SIZE_DEG;
            let tile_max_lat = tile_min_lat + TILE_SIZE_DEG;
            let tile_min_lon = tx as f64 * TILE_SIZE_DEG;
            let tile_max_lon = tile_min_lon + TILE_SIZE_DEG;

            // Clip polygon to tile bounds using Sutherland-Hodgman algorithm
            let clipped = clip_polygon_to_rect(
                points,
                tile_min_lat, tile_max_lat,
                tile_min_lon, tile_max_lon,
            );

            if clipped.len() < 3 {
                continue; // Clipped polygon is degenerate
            }

            // Convert clipped points to tile-local coordinates
            let tile_points: Vec<(i16, i16)> = clipped
                .iter()
                .map(|(lat, lon)| lat_lon_to_tile_offset(*lat, *lon, tx, ty))
                .collect();

            let tile = tiles.entry((tx, ty)).or_insert_with(|| Tile::new(tx, ty));
            tile.areas.push(TileArea {
                area_type,
                points: tile_points,
            });
        }
    }
}

/// Sutherland-Hodgman polygon clipping algorithm
/// Clips a polygon to a rectangular boundary
fn clip_polygon_to_rect(
    points: &[(f64, f64)],
    min_lat: f64, max_lat: f64,
    min_lon: f64, max_lon: f64,
) -> Vec<(f64, f64)> {
    // Clip against each edge in turn: bottom, top, left, right
    let mut output = points.to_vec();

    // Clip against bottom edge (min_lat)
    output = clip_polygon_edge(&output, |p| p.0 >= min_lat, |p1, p2| {
        let t = (min_lat - p1.0) / (p2.0 - p1.0);
        (min_lat, p1.1 + t * (p2.1 - p1.1))
    });

    // Clip against top edge (max_lat)
    output = clip_polygon_edge(&output, |p| p.0 <= max_lat, |p1, p2| {
        let t = (max_lat - p1.0) / (p2.0 - p1.0);
        (max_lat, p1.1 + t * (p2.1 - p1.1))
    });

    // Clip against left edge (min_lon)
    output = clip_polygon_edge(&output, |p| p.1 >= min_lon, |p1, p2| {
        let t = (min_lon - p1.1) / (p2.1 - p1.1);
        (p1.0 + t * (p2.0 - p1.0), min_lon)
    });

    // Clip against right edge (max_lon)
    output = clip_polygon_edge(&output, |p| p.1 <= max_lon, |p1, p2| {
        let t = (max_lon - p1.1) / (p2.1 - p1.1);
        (p1.0 + t * (p2.0 - p1.0), max_lon)
    });

    output
}

/// Clip polygon against a single edge
fn clip_polygon_edge<F, I>(
    points: &[(f64, f64)],
    inside: F,
    intersect: I,
) -> Vec<(f64, f64)>
where
    F: Fn(&(f64, f64)) -> bool,
    I: Fn(&(f64, f64), &(f64, f64)) -> (f64, f64),
{
    if points.is_empty() {
        return Vec::new();
    }

    let mut output = Vec::new();

    for i in 0..points.len() {
        let current = &points[i];
        let next = &points[(i + 1) % points.len()];

        let current_inside = inside(current);
        let next_inside = inside(next);

        if current_inside {
            if next_inside {
                // Both inside: add next
                output.push(*next);
            } else {
                // Current inside, next outside: add intersection
                output.push(intersect(current, next));
            }
        } else if next_inside {
            // Current outside, next inside: add intersection and next
            output.push(intersect(current, next));
            output.push(*next);
        }
        // Both outside: add nothing
    }

    output
}
