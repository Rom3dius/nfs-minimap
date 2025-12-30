#!/bin/bash
# Setup iOS cross-compilation on Linux
#
# Two approaches:
# 1. Darling + Xcode (more complete, heavier)
# 2. SDK + cctools only (lighter, may have limitations)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
IOS_TOOLCHAIN_DIR="$HOME/.ios-toolchain"

echo "=== iOS Cross-Compilation Setup ==="
echo ""

# Check what we have
check_deps() {
    echo "Checking dependencies..."

    if command -v darling &> /dev/null; then
        echo "  ✓ Darling installed"
        HAVE_DARLING=1
    else
        echo "  ✗ Darling not installed"
        HAVE_DARLING=0
    fi

    if [ -d "$IOS_TOOLCHAIN_DIR/sdk" ]; then
        echo "  ✓ iOS SDK found at $IOS_TOOLCHAIN_DIR/sdk"
        HAVE_SDK=1
    else
        echo "  ✗ iOS SDK not found"
        HAVE_SDK=0
    fi

    if command -v ldid &> /dev/null; then
        echo "  ✓ ldid installed (for pseudo-signing)"
        HAVE_LDID=1
    else
        echo "  ✗ ldid not installed"
        HAVE_LDID=0
    fi
}

# Install ldid (pseudo-signer for jailbroken devices)
install_ldid() {
    echo ""
    echo "=== Installing ldid ==="

    if command -v ldid &> /dev/null; then
        echo "ldid already installed"
        return
    fi

    mkdir -p "$IOS_TOOLCHAIN_DIR/bin"
    cd "$IOS_TOOLCHAIN_DIR"

    # Clone and build ldid
    if [ ! -d "ldid" ]; then
        git clone https://github.com/ProcursusTeam/ldid.git
    fi

    cd ldid
    make
    cp ldid "$IOS_TOOLCHAIN_DIR/bin/"

    echo "ldid installed to $IOS_TOOLCHAIN_DIR/bin/ldid"
    echo "Add to PATH: export PATH=\"$IOS_TOOLCHAIN_DIR/bin:\$PATH\""
}

# Install cctools (Apple linker/tools for Linux)
install_cctools() {
    echo ""
    echo "=== Installing cctools-port ==="

    mkdir -p "$IOS_TOOLCHAIN_DIR"
    cd "$IOS_TOOLCHAIN_DIR"

    if [ ! -d "cctools-port" ]; then
        git clone https://github.com/nickirk/cctools-port.git
        cd cctools-port/usage_examples/ios_toolchain
        # This needs the iOS SDK
        echo "cctools needs the iOS SDK to complete setup"
    fi
}

# Create fake xcrun that uses our SDK
create_xcrun_wrapper() {
    echo ""
    echo "=== Creating xcrun wrapper ==="

    mkdir -p "$IOS_TOOLCHAIN_DIR/bin"

    cat > "$IOS_TOOLCHAIN_DIR/bin/xcrun" << 'XCRUN_EOF'
#!/bin/bash
# Fake xcrun for cross-compilation

SDK_PATH="${SDKROOT:-$HOME/.ios-toolchain/sdk}"

case "$1" in
    --show-sdk-path)
        echo "$SDK_PATH"
        ;;
    --sdk)
        shift
        shift  # skip sdk name (iphoneos)
        exec "$@"
        ;;
    -sdk)
        shift
        shift
        exec "$@"
        ;;
    --find)
        shift
        # Return path to tool
        tool="$1"
        if [ -x "$HOME/.ios-toolchain/bin/$tool" ]; then
            echo "$HOME/.ios-toolchain/bin/$tool"
        else
            echo "/usr/bin/$tool"
        fi
        ;;
    *)
        # Try to execute directly
        exec "$@"
        ;;
esac
XCRUN_EOF

    chmod +x "$IOS_TOOLCHAIN_DIR/bin/xcrun"
    echo "Created xcrun wrapper at $IOS_TOOLCHAIN_DIR/bin/xcrun"
}

# Setup cargo config for iOS
setup_cargo_config() {
    echo ""
    echo "=== Setting up Cargo config ==="

    mkdir -p "$PROJECT_DIR/.cargo"

    cat > "$PROJECT_DIR/.cargo/config.toml" << CARGO_EOF
[target.aarch64-apple-ios]
linker = "$IOS_TOOLCHAIN_DIR/bin/aarch64-apple-darwin-ld"
ar = "$IOS_TOOLCHAIN_DIR/bin/aarch64-apple-darwin-ar"

[env]
SDKROOT = "$IOS_TOOLCHAIN_DIR/sdk"
IPHONEOS_DEPLOYMENT_TARGET = "15.0"
PATH = "$IOS_TOOLCHAIN_DIR/bin:\$PATH"
CARGO_EOF

    echo "Created .cargo/config.toml"
}

# Print instructions
print_instructions() {
    echo ""
    echo "=== Next Steps ==="
    echo ""
    echo "1. Get the iOS SDK:"
    echo "   - Extract from Xcode.app/Contents/Developer/Platforms/iPhoneOS.platform/Developer/SDKs/iPhoneOS*.sdk"
    echo "   - Copy to: $IOS_TOOLCHAIN_DIR/sdk"
    echo ""
    echo "2. Add toolchain to PATH:"
    echo "   export PATH=\"$IOS_TOOLCHAIN_DIR/bin:\$PATH\""
    echo ""
    echo "3. Build the project:"
    echo "   cargo build -p minimap-mobile --target aarch64-apple-ios --release"
    echo ""
    echo "4. The compiled library will be at:"
    echo "   target/aarch64-apple-ios/release/libminimap_mobile.a"
    echo ""
    echo "=== Alternative: Use Darling ==="
    echo ""
    echo "For full Xcode tooling, install Darling:"
    echo "  # Fedora:"
    echo "  sudo dnf install darling"
    echo "  # Or build from source:"
    echo "  git clone --recursive https://github.com/nickirk/darling.git"
    echo "  cd darling && mkdir build && cd build"
    echo "  cmake .. && make -j\$(nproc) && sudo make install"
    echo ""
    echo "Then install Xcode command line tools inside Darling:"
    echo "  darling shell"
    echo "  xcode-select --install"
}

# Main
check_deps
echo ""

case "${1:-help}" in
    ldid)
        install_ldid
        ;;
    cctools)
        install_cctools
        ;;
    xcrun)
        create_xcrun_wrapper
        ;;
    cargo)
        setup_cargo_config
        ;;
    all)
        install_ldid
        create_xcrun_wrapper
        setup_cargo_config
        print_instructions
        ;;
    *)
        echo "Usage: $0 {ldid|cctools|xcrun|cargo|all|help}"
        echo ""
        echo "  ldid    - Install ldid (pseudo code signing)"
        echo "  cctools - Install Apple cctools (linker, ar)"
        echo "  xcrun   - Create xcrun wrapper script"
        echo "  cargo   - Setup .cargo/config.toml"
        echo "  all     - Do all of the above"
        echo "  help    - Show this message"
        print_instructions
        ;;
esac
