#!/bin/bash
set -e

# Configuration
ANDROID_API=34
ANDROID_ABI="arm64-v8a"
TARGET_ARCH="aarch64-linux-android"

# Resolve paths
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
KOTLIN_OUT_DIR="$PROJECT_ROOT/core/docgen/src/main/kotlin"
JNI_OUT_DIR="$PROJECT_ROOT/core/docgen/src/main/jniLibs/$ANDROID_ABI"
LIB_PATH="$SCRIPT_DIR/target/$TARGET_ARCH/release/libdocgen_core.so"
BINDGEN="$SCRIPT_DIR/target/release/uniffi-bindgen"

cd "$SCRIPT_DIR"

# Build Rust library using cargo-ndk
echo "Building docgen_core for $TARGET_ARCH (API $ANDROID_API)..."
cargo ndk -t "$ANDROID_ABI" -P "$ANDROID_API" build --release

if [[ ! -f "$LIB_PATH" ]]; then
    echo "Expected Android library was not produced: $LIB_PATH" >&2
    exit 1
fi

echo "Generating Kotlin bindings..."

# Build and run the host bindgen directly. `--no-format` avoids requiring a
# system `ktlint` binary just to regenerate bindings.
cargo build --release --bin uniffi-bindgen
mkdir -p "$KOTLIN_OUT_DIR"
"$BINDGEN" generate --library --language kotlin --no-format \
    --out-dir "$KOTLIN_OUT_DIR" \
    --config "$SCRIPT_DIR/uniffi.toml" \
    "$LIB_PATH"

# Copy .so to jniLibs
echo "Copying library to jniLibs..."
mkdir -p "$JNI_OUT_DIR"
cp "$LIB_PATH" "$JNI_OUT_DIR/"

echo "Android build and binding generation complete."
