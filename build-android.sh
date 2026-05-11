#!/bin/bash
set -e

# Configuration
ANDROID_API=34
TARGET_ARCH="aarch64-linux-android"

# Resolve paths
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Build Rust library using cargo-ndk
echo "Building docgen_core for $TARGET_ARCH (API $ANDROID_API)..."
cargo ndk -t arm64-v8a -P $ANDROID_API build --release

echo "Generating Kotlin bindings..."

# Generate Kotlin bindings
# Note: cargo-ndk puts output in target/aarch64-linux-android/release/
cargo run --release --bin uniffi-bindgen generate --library "target/$TARGET_ARCH/release/libdocgen_core.so" --language kotlin --out-dir "$PROJECT_ROOT/core/docgen/src/main/kotlin" --config "$SCRIPT_DIR/uniffi.toml"

# Copy .so to jniLibs
echo "Copying library to jniLibs..."
mkdir -p "$PROJECT_ROOT/core/docgen/src/main/jniLibs/arm64-v8a"
cp "target/$TARGET_ARCH/release/libdocgen_core.so" "$PROJECT_ROOT/core/docgen/src/main/jniLibs/arm64-v8a/"

echo "Android build and binding generation complete."
