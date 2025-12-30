# Cross-compiling iOS from Linux

This is experimental and only produces a static library - you still need a Mac to:
- Run the iOS Simulator
- Code sign the app
- Deploy to devices

## Option 1: osxcross (Most Complete)

### Setup osxcross

```bash
# Clone osxcross
git clone https://github.com/tpoechtrager/osxcross
cd osxcross

# You need the Xcode Command Line Tools or SDK
# Extract from Xcode.xip (requires downloading from Apple)
# Or use: https://github.com/AcademySoftwareFoundation/OpenTimelineIO/wiki/MacOS-SDK-Instructions

# Package the SDK
./tools/gen_sdk_package_pbzx.sh /path/to/Xcode.app

# Build osxcross
SDK_VERSION=14.0 ./build.sh

# Add to PATH
export PATH="$PWD/target/bin:$PATH"
```

### Setup iOS SDK

```bash
# Extract iOS SDK from Xcode
# Located at: Xcode.app/Contents/Developer/Platforms/iPhoneOS.platform/Developer/SDKs/

# Set environment
export SDKROOT=/path/to/iPhoneOS.sdk
export IPHONEOS_DEPLOYMENT_TARGET=15.0
```

### Configure Rust

```bash
# Add iOS targets
rustup target add aarch64-apple-ios

# Create .cargo/config.toml
cat > .cargo/config.toml << 'EOF'
[target.aarch64-apple-ios]
linker = "aarch64-apple-darwin-clang"
ar = "aarch64-apple-darwin-ar"

[env]
SDKROOT = "/path/to/iPhoneOS.sdk"
IPHONEOS_DEPLOYMENT_TARGET = "15.0"
EOF
```

### Build

```bash
cargo build --target aarch64-apple-ios --release -p minimap-mobile
```

This produces `target/aarch64-apple-ios/release/libminimap_mobile.a`

## Option 2: Zig as Linker

Zig can cross-compile to iOS:

```bash
# Install zig
# Add to Cargo config:
[target.aarch64-apple-ios]
linker = "zig"
rustflags = ["-C", "link-arg=-target", "-C", "link-arg=aarch64-apple-ios"]
```

## Option 3: GitHub Actions (Recommended)

The easiest approach - use the workflow in `.github/workflows/ios.yml`:

1. Push to GitHub
2. GitHub runs the build on macOS
3. Download artifacts

## Option 4: Rent a Mac

- **MacStadium**: Cloud Mac instances
- **AWS EC2 Mac**: Mac instances on AWS
- **MacinCloud**: Mac rental service
- **GitHub Codespaces**: Can't run macOS, but Actions can

## What Works Without a Mac

| Task | Linux | Requires Mac |
|------|-------|--------------|
| Compile Rust to .a | ✅ (with SDK) | |
| Build .app bundle | | ✅ |
| Run Simulator | | ✅ |
| Code signing | | ✅ |
| TestFlight upload | | ✅ |
| App Store submission | | ✅ |

## Recommendation

For serious iOS development, either:
1. Use **GitHub Actions** for CI/CD (free for public repos)
2. Get a cheap **Mac Mini** (M1 starts ~$500 used)
3. Use **MacStadium** ($50-100/month) for cloud Mac
