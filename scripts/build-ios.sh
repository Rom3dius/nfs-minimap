#!/bin/bash
# Build iOS app using Darling
#
# Prerequisites:
# 1. Darling installed and working
# 2. Xcode Command Line Tools installed in Darling
#
# Usage: ./scripts/build-ios.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$PROJECT_DIR/target/ios-build"
IPA_DIR="$PROJECT_DIR/target/ipa"

echo "=== NFS Minimap iOS Build ==="
echo ""

# Check for Darling
if ! command -v darling &> /dev/null; then
    echo "Error: Darling not found. Please install it first:"
    echo "  cd /tmp/darling && mkdir build && cd build"
    echo "  cmake .. && make -j\$(nproc)"
    echo "  sudo make install"
    exit 1
fi

# Check if we can run Darling
if ! darling shell -c "echo test" &> /dev/null; then
    echo "Error: Darling not working. You may need to:"
    echo "  sudo darling shutdown"
    echo "  sudo modprobe darling-mach"
    exit 1
fi

# Check for Xcode tools in Darling
echo "Checking for Xcode tools in Darling..."
if ! darling shell -c "which clang" &> /dev/null; then
    echo "Xcode tools not installed. Installing..."
    darling shell -c "xcode-select --install" || true
    echo ""
    echo "Please wait for Xcode Command Line Tools to install, then run this script again."
    exit 1
fi

# Get SDK path from Darling
SDK_PATH=$(darling shell -c "xcrun --sdk iphoneos --show-sdk-path" 2>/dev/null || echo "")
if [ -z "$SDK_PATH" ]; then
    echo "Error: iOS SDK not found in Darling."
    echo "You may need to install Xcode in Darling or copy the SDK manually."
    exit 1
fi

echo "Found iOS SDK at: $SDK_PATH"

# Export SDK for Rust build
export SDKROOT="$SDK_PATH"
export IPHONEOS_DEPLOYMENT_TARGET="15.0"
export PATH="$HOME/.ios-toolchain/bin:$PATH"

# Create xcrun wrapper that uses Darling
mkdir -p "$HOME/.ios-toolchain/bin"
cat > "$HOME/.ios-toolchain/bin/xcrun" << 'EOF'
#!/bin/bash
darling shell -c "xcrun $*"
EOF
chmod +x "$HOME/.ios-toolchain/bin/xcrun"

# Build the Rust library
echo ""
echo "Building Rust library for iOS..."
cd "$PROJECT_DIR"

cargo build -p minimap-mobile --target aarch64-apple-ios --release

if [ $? -ne 0 ]; then
    echo "Error: Rust build failed"
    exit 1
fi

echo "Rust library built successfully!"

# Create app bundle structure
echo ""
echo "Creating app bundle..."
mkdir -p "$BUILD_DIR/Payload/Minimap.app"

APP_DIR="$BUILD_DIR/Payload/Minimap.app"

# Copy the library
cp "$PROJECT_DIR/target/aarch64-apple-ios/release/libminimap_mobile.a" "$APP_DIR/"

# Create Info.plist
cat > "$APP_DIR/Info.plist" << 'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDisplayName</key>
    <string>NFS Minimap</string>
    <key>CFBundleExecutable</key>
    <string>Minimap</string>
    <key>CFBundleIdentifier</key>
    <string>com.nfs.minimap</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>Minimap</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>LSRequiresIPhoneOS</key>
    <true/>
    <key>MinimumOSVersion</key>
    <string>15.0</string>
    <key>UIDeviceFamily</key>
    <array>
        <integer>1</integer>
        <integer>2</integer>
    </array>
    <key>UILaunchStoryboardName</key>
    <string>LaunchScreen</string>
    <key>UIRequiredDeviceCapabilities</key>
    <array>
        <string>arm64</string>
    </array>
    <key>UISupportedInterfaceOrientations</key>
    <array>
        <string>UIInterfaceOrientationPortrait</string>
        <string>UIInterfaceOrientationLandscapeLeft</string>
        <string>UIInterfaceOrientationLandscapeRight</string>
    </array>
    <key>NSLocationWhenInUseUsageDescription</key>
    <string>NFS Minimap needs your location to show your position on the map</string>
    <key>NSLocationAlwaysAndWhenInUseUsageDescription</key>
    <string>NFS Minimap needs your location to show your position on the map while driving</string>
</dict>
</plist>
PLIST

# Copy tiles if they exist
if [ -d "$PROJECT_DIR/tiles" ]; then
    echo "Copying map tiles..."
    cp -r "$PROJECT_DIR/tiles" "$APP_DIR/"
fi

# Create the IPA
echo ""
echo "Creating IPA..."
mkdir -p "$IPA_DIR"
cd "$BUILD_DIR"
zip -r "$IPA_DIR/Minimap-unsigned.ipa" Payload

# Sign with ldid if available (for jailbroken devices)
if command -v ldid &> /dev/null; then
    echo ""
    echo "Pseudo-signing with ldid..."
    ldid -S "$APP_DIR/Minimap" 2>/dev/null || true

    # Recreate IPA with signed binary
    zip -r "$IPA_DIR/Minimap-signed.ipa" Payload
fi

echo ""
echo "=== Build Complete ==="
echo ""
echo "Unsigned IPA: $IPA_DIR/Minimap-unsigned.ipa"
if [ -f "$IPA_DIR/Minimap-signed.ipa" ]; then
    echo "Signed IPA:   $IPA_DIR/Minimap-signed.ipa"
fi
echo ""
echo "To install on jailbroken device:"
echo "  1. Transfer IPA to device"
echo "  2. Install via Filza or AppSync"
echo "  3. Or use: scp $IPA_DIR/Minimap-signed.ipa root@<device>:/var/mobile/"
echo "     Then on device: appinst /var/mobile/Minimap-signed.ipa"
