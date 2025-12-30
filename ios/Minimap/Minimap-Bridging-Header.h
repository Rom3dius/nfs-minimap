//
//  Minimap-Bridging-Header.h
//  NFS Minimap iOS App
//
//  Rust FFI functions from libminimap_mobile.a
//

#ifndef Minimap_Bridging_Header_h
#define Minimap_Bridging_Header_h

#include <stdbool.h>

// Initialize the app with optional tile directory path
// Returns true on success, false on failure
bool minimap_init(const char* tile_dir);

// Update GPS position from native location manager
void minimap_update_gps(double lat, double lon, float heading, float speed_kmh);

// Set GPS active status
void minimap_set_gps_active(bool active);

// Set status text
void minimap_set_status(const char* text);

#endif /* Minimap_Bridging_Header_h */
