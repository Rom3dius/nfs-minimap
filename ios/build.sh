#!/bin/bash
# Build iOS app without Xcode project
# Requires: macOS with Xcode Command Line Tools

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$SCRIPT_DIR/build"
APP_DIR="$BUILD_DIR/Payload/Minimap.app"

echo "=== NFS Minimap iOS Build ==="
echo ""

# Check for Xcode tools
if ! command -v swiftc &> /dev/null; then
    echo "Error: swiftc not found. Install Xcode Command Line Tools."
    exit 1
fi

# Get iOS SDK
IOS_SDK=$(xcrun --sdk iphoneos --show-sdk-path)
echo "Using iOS SDK: $IOS_SDK"

# Check for Rust library
RUST_LIB="$PROJECT_ROOT/target/aarch64-apple-ios/release/libminimap_mobile.a"
if [ ! -f "$RUST_LIB" ]; then
    echo "Error: Rust library not found at $RUST_LIB"
    echo "Run: cargo build -p minimap-mobile --target aarch64-apple-ios --release"
    exit 1
fi

echo "Found Rust library: $RUST_LIB"

# Create build directories
mkdir -p "$APP_DIR"

# Find Skia libraries (built by cargo)
SKIA_DIR=$(find "$PROJECT_ROOT/target/aarch64-apple-ios/release/build" -name "libskia.a" -exec dirname {} \; | head -1)
if [ -z "$SKIA_DIR" ]; then
    echo "Warning: Skia libraries not found, linking may fail"
    SKIA_DIR="$PROJECT_ROOT/target/aarch64-apple-ios/release"
fi
echo "Skia libraries: $SKIA_DIR"

# Compile Swift code
echo ""
echo "Compiling Swift..."
swiftc \
    -target arm64-apple-ios16.0 \
    -sdk "$IOS_SDK" \
    -O \
    -parse-as-library \
    -emit-executable \
    -module-name Minimap \
    -import-objc-header "$SCRIPT_DIR/Minimap/Minimap-Bridging-Header.h" \
    "$SCRIPT_DIR/Minimap/MinimapApp.swift" \
    "$RUST_LIB" \
    -L "$SKIA_DIR" \
    -lskia \
    -lskia-bindings \
    -lskparagraph \
    -lskshaper \
    -lskunicode_core \
    -lskunicode_icu \
    -framework UIKit \
    -framework CoreLocation \
    -framework SwiftUI \
    -framework CoreFoundation \
    -framework CoreGraphics \
    -framework CoreText \
    -framework QuartzCore \
    -framework Metal \
    -framework MetalKit \
    -framework Security \
    -framework Foundation \
    -framework ImageIO \
    -framework MobileCoreServices \
    -lc++ \
    -lz \
    -o "$APP_DIR/Minimap"

echo "Compiled successfully!"

# Copy Info.plist
echo "Copying Info.plist..."
cp "$SCRIPT_DIR/Minimap/Info.plist" "$APP_DIR/"

# Process Info.plist (substitute variables)
/usr/libexec/PlistBuddy -c "Set :CFBundleExecutable Minimap" "$APP_DIR/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier com.nfs.minimap" "$APP_DIR/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleName Minimap" "$APP_DIR/Info.plist"

# Copy tiles if they exist
if [ -d "$PROJECT_ROOT/tiles" ]; then
    echo "Copying map tiles..."
    cp -r "$PROJECT_ROOT/tiles" "$APP_DIR/"
fi

# Create unsigned IPA
echo ""
echo "Creating IPA..."
cd "$BUILD_DIR"
zip -r Minimap-unsigned.ipa Payload

# Optionally sign with ldid if available
if command -v ldid &> /dev/null; then
    echo ""
    echo "Signing with ldid..."
    ldid -S "$APP_DIR/Minimap"
    zip -r Minimap-signed.ipa Payload
    echo "Created: $BUILD_DIR/Minimap-signed.ipa"
fi

echo ""
echo "=== Build Complete ==="
echo ""
echo "Unsigned IPA: $BUILD_DIR/Minimap-unsigned.ipa"
if [ -f "$BUILD_DIR/Minimap-signed.ipa" ]; then
    echo "Signed IPA:   $BUILD_DIR/Minimap-signed.ipa"
fi
echo ""
echo "To install on jailbroken device:"
echo "  1. Transfer IPA to device"
echo "  2. Install via Filza + AppSync Unified"
echo ""
