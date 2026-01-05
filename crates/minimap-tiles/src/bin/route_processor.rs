//! OSM PBF to routing tile processor
//!
//! Usage: route-processor <input.osm.pbf> <output-dir>
//!
//! Creates hierarchical routing tiles and search index:
//! - level0/ (Highway: 1.0° tiles) - motorway, trunk, primary
//! - level1/ (Arterial: 0.1° tiles) - secondary, tertiary
//! - level2/ (Local: 0.01° tiles) - residential, unclassified, service
//! - search/ - FST-based place search index

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use minimap_routing::{
    search::{PlaceCategory, SearchIndexBuilder},
    HierarchyLevel, RoadClass, RoutingEdge, RoutingNode, RoutingTile,
};
use osmpbf::{Element, ElementReader};
use rayon::prelude::*;

/// Tile sizes in degrees for each hierarchy level.
const TILE_SIZE_HIGHWAY: f64 = 1.0;
const TILE_SIZE_ARTERIAL: f64 = 0.1;
const TILE_SIZE_LOCAL: f64 = 0.01;

fn main() {
    env_logger::init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <input.osm.pbf> <output-dir>", args[0]);
        eprintln!();
        eprintln!("Creates hierarchical routing tiles from OSM data.");
        eprintln!();
        eprintln!("Download PBF files from:");
        eprintln!("  https://download.geofabrik.de/");
        std::process::exit(1);
    }

    let input_path = &args[1];
    let output_dir = &args[2];

    // Create output directories
    let routing_dir = Path::new(output_dir).join("routing");
    let search_dir = routing_dir.join("search");
    fs::create_dir_all(&routing_dir).expect("Failed to create routing directory");
    fs::create_dir_all(routing_dir.join("level0")).expect("Failed to create level0 directory");
    fs::create_dir_all(routing_dir.join("level1")).expect("Failed to create level1 directory");
    fs::create_dir_all(routing_dir.join("level2")).expect("Failed to create level2 directory");
    fs::create_dir_all(&search_dir).expect("Failed to create search directory");

    log::info!("Processing {} -> {}", input_path, routing_dir.display());

    // Pass 1: Collect all routable way node IDs
    log::info!("Pass 1: Collecting routable way node IDs...");
    let (needed_nodes, way_info) = collect_routable_ways(input_path);
    log::info!(
        "Found {} routable ways needing {} nodes",
        way_info.len(),
        needed_nodes.len()
    );

    // Pass 2: Collect node coordinates
    log::info!("Pass 2: Reading node coordinates...");
    let node_coords = collect_node_coords(input_path, &needed_nodes);
    log::info!("Collected {} node coordinates", node_coords.len());

    // Pass 3: Build routing graph
    log::info!("Pass 3: Building routing graph...");
    let graph = build_routing_graph(input_path, &node_coords, &way_info);
    log::info!(
        "Built graph with {} nodes and {} edges",
        graph.nodes.len(),
        graph.edges.len()
    );

    // Partition into tiles and write
    log::info!("Pass 4: Partitioning into tiles...");

    // Level 0: Highway (motorway, trunk, primary)
    let highway_tiles = partition_by_level(&graph, HierarchyLevel::Highway);
    log::info!("Created {} highway tiles (level 0)", highway_tiles.len());
    write_tiles(&highway_tiles, &routing_dir.join("level0"));

    // Level 1: Arterial (secondary, tertiary)
    let arterial_tiles = partition_by_level(&graph, HierarchyLevel::Arterial);
    log::info!("Created {} arterial tiles (level 1)", arterial_tiles.len());
    write_tiles(&arterial_tiles, &routing_dir.join("level1"));

    // Level 2: Local (residential, unclassified, service)
    let local_tiles = partition_by_level(&graph, HierarchyLevel::Local);
    log::info!("Created {} local tiles (level 2)", local_tiles.len());
    write_tiles(&local_tiles, &routing_dir.join("level2"));

    // Write summary
    let total_tiles = highway_tiles.len() + arterial_tiles.len() + local_tiles.len();
    log::info!(
        "Created {} total routing tiles ({} highway, {} arterial, {} local)",
        total_tiles,
        highway_tiles.len(),
        arterial_tiles.len(),
        local_tiles.len()
    );

    // Pass 5: Build search index
    log::info!("Pass 5: Building search index...");
    let search_builder = build_search_index(input_path);
    log::info!("Collected {} searchable places", search_builder.len());

    search_builder
        .write(&search_dir)
        .expect("Failed to write search index");

    log::info!("Done! Routing tiles and search index created.");
}

/// Information about a routable way.
#[derive(Debug, Clone)]
struct WayInfo {
    nodes: Vec<i64>,
    road_class: RoadClass,
    oneway: bool,
    speed_kmh: u8,
    name: Option<String>,
}

/// Routing graph built from OSM data.
struct RoutingGraph {
    /// Node ID -> (lat, lon)
    nodes: HashMap<i64, (f64, f64)>,
    /// Edges: (from_node_id, to_node_id, distance_m, road_class, speed_kmh, name)
    edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone)]
struct GraphEdge {
    from: i64,
    to: i64,
    distance_m: f64,
    road_class: RoadClass,
    speed_kmh: u8,
    oneway: bool,
    name: Option<String>,
}

/// Collect all node IDs referenced by routable ways.
fn collect_routable_ways(path: &str) -> (HashSet<i64>, HashMap<i64, WayInfo>) {
    let reader = ElementReader::from_path(path).expect("Failed to open PBF");
    let mut needed_nodes = HashSet::new();
    let mut way_info = HashMap::new();

    reader
        .for_each(|element| {
            if let Element::Way(way) = element {
                let tags: Vec<(&str, &str)> = way.tags().collect();

                // Check if it's a routable highway
                let highway = tags.iter().find(|(k, _)| *k == "highway").map(|(_, v)| *v);
                let road_class = match highway {
                    Some("motorway") | Some("motorway_link") => Some(RoadClass::Motorway),
                    Some("trunk") | Some("trunk_link") => Some(RoadClass::Trunk),
                    Some("primary") | Some("primary_link") => Some(RoadClass::Primary),
                    Some("secondary") | Some("secondary_link") => Some(RoadClass::Secondary),
                    Some("tertiary") | Some("tertiary_link") => Some(RoadClass::Tertiary),
                    Some("residential") => Some(RoadClass::Residential),
                    Some("unclassified") | Some("living_street") => Some(RoadClass::Other),
                    Some("service") => Some(RoadClass::Service),
                    _ => None,
                };

                if let Some(rc) = road_class {
                    // Check for oneway
                    let oneway = tags.iter().any(|(k, v)| {
                        (*k == "oneway" && (*v == "yes" || *v == "1" || *v == "true"))
                            || (*k == "highway" && *v == "motorway")
                            || (*k == "junction" && *v == "roundabout")
                    });

                    // Get speed limit
                    let speed_kmh = tags
                        .iter()
                        .find(|(k, _)| *k == "maxspeed")
                        .and_then(|(_, v)| v.trim_end_matches(" km/h").parse::<u8>().ok())
                        .unwrap_or_else(|| default_speed(&rc));

                    // Get road name
                    let name = tags
                        .iter()
                        .find(|(k, _)| *k == "name")
                        .map(|(_, v)| v.to_string());

                    let nodes: Vec<i64> = way.refs().collect();
                    for node_id in &nodes {
                        needed_nodes.insert(*node_id);
                    }

                    way_info.insert(
                        way.id(),
                        WayInfo {
                            nodes,
                            road_class: rc,
                            oneway,
                            speed_kmh,
                            name,
                        },
                    );
                }
            }
        })
        .expect("Failed to read PBF");

    (needed_nodes, way_info)
}

/// Get default speed for a road class.
fn default_speed(road_class: &RoadClass) -> u8 {
    match road_class {
        RoadClass::Motorway => 120,
        RoadClass::Trunk => 100,
        RoadClass::Primary => 80,
        RoadClass::Secondary => 60,
        RoadClass::Tertiary => 50,
        RoadClass::Residential => 30,
        RoadClass::Service => 20,
        RoadClass::Other => 30,
    }
}

/// Collect coordinates for needed nodes.
fn collect_node_coords(path: &str, needed: &HashSet<i64>) -> HashMap<i64, (f64, f64)> {
    let reader = ElementReader::from_path(path).expect("Failed to open PBF");
    let mut coords = HashMap::new();

    reader
        .for_each(|element| {
            match element {
                Element::DenseNode(node) => {
                    if needed.contains(&node.id()) {
                        coords.insert(node.id(), (node.lat(), node.lon()));
                    }
                }
                Element::Node(node) => {
                    if needed.contains(&node.id()) {
                        coords.insert(node.id(), (node.lat(), node.lon()));
                    }
                }
                _ => {}
            }
        })
        .expect("Failed to read PBF");

    coords
}

/// Build routing graph from ways.
fn build_routing_graph(
    path: &str,
    node_coords: &HashMap<i64, (f64, f64)>,
    way_info: &HashMap<i64, WayInfo>,
) -> RoutingGraph {
    let reader = ElementReader::from_path(path).expect("Failed to open PBF");
    let mut edges = Vec::new();
    let mut used_nodes = HashSet::new();

    reader
        .for_each(|element| {
            if let Element::Way(way) = element {
                if let Some(info) = way_info.get(&way.id()) {
                    // Create edges for each segment
                    for window in info.nodes.windows(2) {
                        let from_id = window[0];
                        let to_id = window[1];

                        let from_coord = match node_coords.get(&from_id) {
                            Some(c) => c,
                            None => continue,
                        };
                        let to_coord = match node_coords.get(&to_id) {
                            Some(c) => c,
                            None => continue,
                        };

                        let distance_m =
                            haversine_distance(from_coord.0, from_coord.1, to_coord.0, to_coord.1);

                        // Add forward edge
                        edges.push(GraphEdge {
                            from: from_id,
                            to: to_id,
                            distance_m,
                            road_class: info.road_class,
                            speed_kmh: info.speed_kmh,
                            oneway: info.oneway,
                            name: info.name.clone(),
                        });

                        // Add reverse edge if not oneway
                        if !info.oneway {
                            edges.push(GraphEdge {
                                from: to_id,
                                to: from_id,
                                distance_m,
                                road_class: info.road_class,
                                speed_kmh: info.speed_kmh,
                                oneway: false,
                                name: info.name.clone(),
                            });
                        }

                        used_nodes.insert(from_id);
                        used_nodes.insert(to_id);
                    }
                }
            }
        })
        .expect("Failed to read PBF");

    // Only keep nodes that are used in edges
    let nodes: HashMap<i64, (f64, f64)> = node_coords
        .iter()
        .filter(|(id, _)| used_nodes.contains(id))
        .map(|(id, coord)| (*id, *coord))
        .collect();

    RoutingGraph { nodes, edges }
}

/// Calculate haversine distance between two points.
fn haversine_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const EARTH_RADIUS_M: f64 = 6_371_000.0;

    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();
    let delta_lat = (lat2 - lat1).to_radians();
    let delta_lon = (lon2 - lon1).to_radians();

    let a = (delta_lat / 2.0).sin().powi(2)
        + lat1_rad.cos() * lat2_rad.cos() * (delta_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();

    EARTH_RADIUS_M * c
}

/// Check if a road class belongs to a hierarchy level.
fn road_class_for_level(road_class: RoadClass, level: HierarchyLevel) -> bool {
    match level {
        HierarchyLevel::Highway => matches!(
            road_class,
            RoadClass::Motorway | RoadClass::Trunk | RoadClass::Primary
        ),
        HierarchyLevel::Arterial => matches!(road_class, RoadClass::Secondary | RoadClass::Tertiary),
        HierarchyLevel::Local => matches!(
            road_class,
            RoadClass::Residential | RoadClass::Service | RoadClass::Other
        ),
    }
}

/// Get tile size for a hierarchy level.
fn tile_size_for_level(level: HierarchyLevel) -> f64 {
    match level {
        HierarchyLevel::Highway => TILE_SIZE_HIGHWAY,
        HierarchyLevel::Arterial => TILE_SIZE_ARTERIAL,
        HierarchyLevel::Local => TILE_SIZE_LOCAL,
    }
}

/// Convert lat/lon to tile coordinates.
fn lat_lon_to_tile(lat: f64, lon: f64, level: HierarchyLevel) -> (i32, i32) {
    let tile_size = tile_size_for_level(level);
    let tx = (lon / tile_size).floor() as i32;
    let ty = (lat / tile_size).floor() as i32;
    (tx, ty)
}

/// Node location information for canonical tile assignment.
#[derive(Debug, Clone, Copy)]
struct NodeLocation {
    /// Canonical tile coordinates.
    tile_x: i32,
    tile_y: i32,
    /// Local index within the canonical tile.
    local_idx: u32,
}

/// Partition the graph into tiles for a specific hierarchy level.
///
/// Uses canonical tile ownership: each node belongs to exactly one tile
/// (determined by its coordinates), and edges crossing tile boundaries
/// are stored as CrossTileEdge.
fn partition_by_level(graph: &RoutingGraph, level: HierarchyLevel) -> Vec<RoutingTile> {
    use minimap_routing::CrossTileEdge;

    // Step 1: Collect all edges for this level
    let level_edges: Vec<&GraphEdge> = graph
        .edges
        .iter()
        .filter(|e| road_class_for_level(e.road_class, level))
        .collect();

    if level_edges.is_empty() {
        return Vec::new();
    }

    // Step 2: Find all nodes referenced by edges at this level
    let mut referenced_nodes: HashSet<i64> = HashSet::new();
    for edge in &level_edges {
        referenced_nodes.insert(edge.from);
        referenced_nodes.insert(edge.to);
    }

    // Step 3: Assign each node to its canonical tile (based on coordinates)
    let mut node_locations: HashMap<i64, NodeLocation> = HashMap::new();
    let mut tile_nodes: HashMap<(i32, i32), Vec<(i64, f64, f64)>> = HashMap::new();

    for &osm_id in &referenced_nodes {
        if let Some(&(lat, lon)) = graph.nodes.get(&osm_id) {
            let (tx, ty) = lat_lon_to_tile(lat, lon, level);
            tile_nodes.entry((tx, ty)).or_default().push((osm_id, lat, lon));
        }
    }

    // Assign local indices within each tile
    for ((tx, ty), nodes) in &tile_nodes {
        for (idx, &(osm_id, _, _)) in nodes.iter().enumerate() {
            node_locations.insert(
                osm_id,
                NodeLocation {
                    tile_x: *tx,
                    tile_y: *ty,
                    local_idx: idx as u32,
                },
            );
        }
    }

    // Step 4: Group edges by source node's canonical tile
    let mut tile_edges: HashMap<(i32, i32), Vec<&GraphEdge>> = HashMap::new();
    for edge in &level_edges {
        if let Some(loc) = node_locations.get(&edge.from) {
            tile_edges
                .entry((loc.tile_x, loc.tile_y))
                .or_default()
                .push(edge);
        }
    }

    // Step 5: Build tiles in parallel
    let tile_coords: Vec<(i32, i32)> = tile_nodes.keys().cloned().collect();

    tile_coords
        .par_iter()
        .filter_map(|&(tx, ty)| {
            let nodes = tile_nodes.get(&(tx, ty))?;
            if nodes.is_empty() {
                return None;
            }

            // Build name table for this tile
            let mut name_table: Vec<String> = Vec::new();
            let mut name_to_id: HashMap<String, u8> = HashMap::new();

            let get_name_id = |name: &Option<String>,
                               name_table: &mut Vec<String>,
                               name_to_id: &mut HashMap<String, u8>|
             -> u8 {
                if let Some(ref name) = name {
                    if let Some(&id) = name_to_id.get(name) {
                        id
                    } else if name_table.len() < 255 {
                        let id = (name_table.len() + 1) as u8;
                        name_table.push(name.clone());
                        name_to_id.insert(name.clone(), id);
                        id
                    } else {
                        0
                    }
                } else {
                    0
                }
            };

            // Create routing nodes
            let mut routing_nodes: Vec<RoutingNode> = nodes
                .iter()
                .map(|&(_, lat, lon)| RoutingNode::new(lat, lon))
                .collect();

            // Separate edges into same-tile and cross-tile
            let edges = tile_edges.get(&(tx, ty)).map(|v| v.as_slice()).unwrap_or(&[]);

            // Group same-tile edges by source node, collect cross-tile edges
            let mut same_tile_edges: HashMap<u32, Vec<(u32, &GraphEdge)>> = HashMap::new();
            let mut cross_tile_edges: Vec<CrossTileEdge> = Vec::new();

            for edge in edges.iter() {
                let from_loc = match node_locations.get(&edge.from) {
                    Some(l) => l,
                    None => continue,
                };
                let to_loc = match node_locations.get(&edge.to) {
                    Some(l) => l,
                    None => continue,
                };

                if to_loc.tile_x == tx && to_loc.tile_y == ty {
                    // Same tile: store as regular edge
                    same_tile_edges
                        .entry(from_loc.local_idx)
                        .or_default()
                        .push((to_loc.local_idx, *edge));
                } else {
                    // Cross-tile: store as CrossTileEdge
                    let name_id = get_name_id(&edge.name, &mut name_table, &mut name_to_id);
                    let mut cross_edge = CrossTileEdge::new(
                        from_loc.local_idx,
                        to_loc.tile_x,
                        to_loc.tile_y,
                        to_loc.local_idx,
                        edge.distance_m,
                        edge.road_class,
                    );
                    cross_edge.speed_kmh = edge.speed_kmh;
                    cross_edge.name_id = name_id;
                    if edge.oneway {
                        cross_edge.flags.set_oneway(true);
                    }
                    cross_tile_edges.push(cross_edge);

                    // Mark source node as boundary
                    routing_nodes[from_loc.local_idx as usize]
                        .flags
                        .set_boundary(true);
                }
            }

            // Build edge array and update node edge indices
            let mut routing_edges = Vec::new();
            for (node_idx, node) in routing_nodes.iter_mut().enumerate() {
                let node_idx = node_idx as u32;
                node.first_edge = routing_edges.len() as u32;

                if let Some(node_edges) = same_tile_edges.get(&node_idx) {
                    let edge_count = node_edges.len().min(255);
                    node.edge_count = edge_count as u8;

                    for (target_idx, edge) in node_edges.iter().take(255) {
                        let name_id = get_name_id(&edge.name, &mut name_table, &mut name_to_id);
                        let mut routing_edge =
                            RoutingEdge::new(*target_idx, edge.distance_m, edge.road_class);
                        routing_edge.speed_kmh = edge.speed_kmh;
                        routing_edge.name_id = name_id;
                        if edge.oneway {
                            routing_edge.flags.set_oneway(true);
                        }
                        routing_edges.push(routing_edge);
                    }
                } else {
                    node.edge_count = 0;
                }
            }

            // Create tile
            let mut tile = RoutingTile::new(level, tx, ty);
            tile.nodes = routing_nodes;
            tile.edges = routing_edges;
            tile.cross_tile_edges = cross_tile_edges;
            tile.names = name_table;

            // Build boundary_nodes list
            tile.boundary_nodes = tile
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| n.flags.is_boundary())
                .map(|(i, _)| i as u16)
                .collect();

            Some(tile)
        })
        .collect()
}

/// Write tiles to disk.
fn write_tiles(tiles: &[RoutingTile], output_dir: &Path) {
    for tile in tiles {
        let filename = format!("tile_{}_{}.bin", tile.tile_x, tile.tile_y);
        let path = output_dir.join(&filename);

        let bytes = tile.to_bytes();
        fs::write(&path, &bytes).expect("Failed to write tile");
    }

    // Write index file
    let index_path = output_dir.join("index.bin");
    let mut index_data = Vec::new();

    // Write tile count
    let count = tiles.len() as u32;
    index_data.extend_from_slice(&count.to_le_bytes());

    // Write tile coordinates
    for tile in tiles {
        index_data.extend_from_slice(&tile.tile_x.to_le_bytes());
        index_data.extend_from_slice(&tile.tile_y.to_le_bytes());
    }

    fs::write(&index_path, &index_data).expect("Failed to write index");
    log::info!(
        "Wrote {} tiles and index to {}",
        tiles.len(),
        output_dir.display()
    );
}

/// Build search index from OSM data.
fn build_search_index(path: &str) -> SearchIndexBuilder {
    let reader = ElementReader::from_path(path).expect("Failed to open PBF");
    let mut builder = SearchIndexBuilder::new();

    reader
        .for_each(|element| {
            match element {
                Element::Node(node) => {
                    let tags: Vec<(&str, &str)> = node.tags().collect();
                    if let Some(entry) = extract_place_from_tags(&tags, node.lat(), node.lon()) {
                        builder.add(&entry.0, entry.1, entry.2, entry.3);
                    }
                }
                Element::DenseNode(node) => {
                    let tags: Vec<(&str, &str)> = node.tags().collect();
                    if let Some(entry) = extract_place_from_tags(&tags, node.lat(), node.lon()) {
                        builder.add(&entry.0, entry.1, entry.2, entry.3);
                    }
                }
                Element::Way(_way) => {
                    // For ways, we could compute centroid for POIs/buildings
                    // For now, skip ways as most named places are nodes
                }
                _ => {}
            }
        })
        .expect("Failed to read PBF");

    builder
}

/// Extract place info from OSM tags.
fn extract_place_from_tags(
    tags: &[(&str, &str)],
    lat: f64,
    lon: f64,
) -> Option<(String, f64, f64, PlaceCategory)> {
    // Get name
    let name = tags.iter().find(|(k, _)| *k == "name").map(|(_, v)| *v)?;
    if name.is_empty() {
        return None;
    }

    // Determine category based on tags
    let category = determine_category(tags);

    Some((name.to_string(), lat, lon, category))
}

/// Determine place category from OSM tags.
fn determine_category(tags: &[(&str, &str)]) -> PlaceCategory {
    // Check for place tag (cities, towns, villages)
    if let Some((_, value)) = tags.iter().find(|(k, _)| *k == "place") {
        return match *value {
            "city" | "town" | "village" | "hamlet" | "suburb" | "neighbourhood" => {
                PlaceCategory::Settlement
            }
            _ => PlaceCategory::Other,
        };
    }

    // Check for highway tag (streets)
    if tags.iter().any(|(k, _)| *k == "highway") {
        return PlaceCategory::Street;
    }

    // Check for amenity tag (POIs)
    if let Some((_, value)) = tags.iter().find(|(k, _)| *k == "amenity") {
        return match *value {
            "fuel" | "restaurant" | "cafe" | "fast_food" | "parking" | "hospital" | "pharmacy"
            | "bank" | "atm" | "police" | "post_office" | "school" | "university" | "library"
            | "cinema" | "theatre" => PlaceCategory::Poi,
            _ => PlaceCategory::Other,
        };
    }

    // Check for shop tag
    if tags.iter().any(|(k, _)| *k == "shop") {
        return PlaceCategory::Poi;
    }

    // Check for tourism tag
    if tags.iter().any(|(k, _)| *k == "tourism") {
        return PlaceCategory::Poi;
    }

    // Check for transport
    if let Some((_, value)) = tags.iter().find(|(k, _)| *k == "aeroway") {
        if *value == "aerodrome" {
            return PlaceCategory::Transport;
        }
    }
    if let Some((_, value)) = tags.iter().find(|(k, _)| *k == "railway") {
        if *value == "station" || *value == "halt" {
            return PlaceCategory::Transport;
        }
    }
    if let Some((_, value)) = tags.iter().find(|(k, _)| *k == "public_transport") {
        if *value == "station" {
            return PlaceCategory::Transport;
        }
    }

    // Check for natural features
    if let Some((_, value)) = tags.iter().find(|(k, _)| *k == "natural") {
        return match *value {
            "peak" | "volcano" | "water" | "bay" | "beach" | "glacier" => PlaceCategory::Natural,
            _ => PlaceCategory::Other,
        };
    }

    // Check for waterway
    if let Some((_, value)) = tags.iter().find(|(k, _)| *k == "waterway") {
        if *value == "river" || *value == "stream" {
            return PlaceCategory::Natural;
        }
    }

    PlaceCategory::Other
}
