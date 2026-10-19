#!/usr/bin/env bash
# Builds the core staticlib for iOS device and simulator (arm64) and packages ios/build/ShanjieCore.xcframework
# (core/include/shanjie.h + a module map named CShanjie). Run from anywhere; set CARGO_TARGET_DIR to keep the
# build output out of the repo's target/.
set -euo pipefail
cd "$(dirname "$0")/../.."
ROOT="$PWD"
OUT="$ROOT/ios/build"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
export IPHONEOS_DEPLOYMENT_TARGET=18.0

for t in aarch64-apple-ios aarch64-apple-ios-sim; do
  CARGO_TARGET_DIR="$TARGET_DIR" cargo build --release --locked -p core --target "$t"
done

rm -rf "$OUT/ShanjieCore.xcframework" "$OUT/headers"
mkdir -p "$OUT/headers"
cp core/include/shanjie.h "$OUT/headers/"
cat > "$OUT/headers/module.modulemap" <<'MAP'
module CShanjie [system] {
  header "shanjie.h"
  export *
}
MAP
xcodebuild -create-xcframework \
  -library "$TARGET_DIR/aarch64-apple-ios/release/libcore.a" -headers "$OUT/headers" \
  -library "$TARGET_DIR/aarch64-apple-ios-sim/release/libcore.a" -headers "$OUT/headers" \
  -output "$OUT/ShanjieCore.xcframework"
echo "wrote $OUT/ShanjieCore.xcframework"
