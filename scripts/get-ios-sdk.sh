#!/bin/bash
# Download/setup iOS SDK for cross-compilation
#
# Options:
# 1. Extract from Xcode (if you have access to a Mac)
# 2. Use a pre-extracted SDK (check licensing)

set -e

SDK_DIR="$HOME/.ios-toolchain/sdk"
CCTOOLS_DIR="$HOME/.ios-toolchain/cctools"

echo "=== iOS SDK Setup ==="
echo ""
echo "The iOS SDK is required for cross-compilation."
echo ""
echo "Options:"
echo "1. Copy SDK from a Mac with Xcode installed:"
echo "   scp -r user@mac:/Applications/Xcode.app/Contents/Developer/Platforms/iPhoneOS.platform/Developer/SDKs/iPhoneOS*.sdk $SDK_DIR"
echo ""
echo "2. If you have Xcode.xip, extract it:"
echo "   # On a Mac or with tools that can extract xip:"
echo "   xip --expand Xcode.xip"
echo "   # Then copy the SDK"
echo ""

mkdir -p "$HOME/.ios-toolchain/bin"

# Create xcrun wrapper
cat > "$HOME/.ios-toolchain/bin/xcrun" << 'EOF'
#!/bin/bash
# xcrun wrapper for cross-compilation
SDK_PATH="${SDKROOT:-$HOME/.ios-toolchain/sdk}"

case "$1" in
    --show-sdk-path|-show-sdk-path)
        echo "$SDK_PATH"
        exit 0
        ;;
    --show-sdk-version|-show-sdk-version)
        echo "17.0"
        exit 0
        ;;
    --find|-find)
        tool="${2:-$1}"
        if [ -f "$HOME/.ios-toolchain/bin/$tool" ]; then
            echo "$HOME/.ios-toolchain/bin/$tool"
        elif command -v "$tool" &>/dev/null; then
            command -v "$tool"
        else
            echo "$tool"
        fi
        exit 0
        ;;
    --sdk|-sdk)
        shift; shift  # skip -sdk iphoneos
        exec "$@"
        ;;
    *)
        # Just run the command
        exec "$@"
        ;;
esac
EOF
chmod +x "$HOME/.ios-toolchain/bin/xcrun"

# Create ar wrapper
cat > "$HOME/.ios-toolchain/bin/aarch64-apple-ios-ar" << 'EOF'
#!/bin/bash
exec llvm-ar "$@"
EOF
chmod +x "$HOME/.ios-toolchain/bin/aarch64-apple-ios-ar"

# Create ld wrapper using lld
cat > "$HOME/.ios-toolchain/bin/aarch64-apple-ios-ld" << 'EOF'
#!/bin/bash
# Use ld64.lld for Mach-O linking
SDK_PATH="${SDKROOT:-$HOME/.ios-toolchain/sdk}"
exec ld64.lld -arch arm64 -platform_version ios 15.0.0 17.0.0 \
    -syslibroot "$SDK_PATH" \
    "$@"
EOF
chmod +x "$HOME/.ios-toolchain/bin/aarch64-apple-ios-ld"

# Create clang wrapper
cat > "$HOME/.ios-toolchain/bin/aarch64-apple-ios-clang" << 'EOF'
#!/bin/bash
SDK_PATH="${SDKROOT:-$HOME/.ios-toolchain/sdk}"
exec clang -target aarch64-apple-ios15.0 \
    -isysroot "$SDK_PATH" \
    "$@"
EOF
chmod +x "$HOME/.ios-toolchain/bin/aarch64-apple-ios-clang"

cat > "$HOME/.ios-toolchain/bin/aarch64-apple-ios-clang++" << 'EOF'
#!/bin/bash
SDK_PATH="${SDKROOT:-$HOME/.ios-toolchain/sdk}"
exec clang++ -target aarch64-apple-ios15.0 \
    -isysroot "$SDK_PATH" \
    "$@"
EOF
chmod +x "$HOME/.ios-toolchain/bin/aarch64-apple-ios-clang++"

echo ""
echo "Created toolchain wrappers in $HOME/.ios-toolchain/bin/"
echo ""
echo "Next steps:"
echo "1. Get the iOS SDK and place it at: $SDK_DIR"
echo "2. Add to your shell:"
echo "   export PATH=\"\$HOME/.ios-toolchain/bin:\$PATH\""
echo "   export SDKROOT=\"$SDK_DIR\""
echo ""
echo "3. Build with:"
echo "   cargo build -p minimap-mobile --target aarch64-apple-ios --release"
