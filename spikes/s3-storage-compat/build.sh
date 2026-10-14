#!/usr/bin/env bash
# Builds the three wasm variants (needs wasm-bindgen-cli matching the pinned wasm-bindgen crate).
set -e
cd "$(dirname "$0")/rust"
CARGO_TARGET_DIR=target     cargo build --target wasm32-unknown-unknown --release
CARGO_TARGET_DIR=target-arb cargo build --target wasm32-unknown-unknown --release --features arbprec
CARGO_TARGET_DIR=target-frt cargo build --target wasm32-unknown-unknown --release --features frt
wasm-bindgen --target web --out-dir ../pkg     target/wasm32-unknown-unknown/release/s3_compat.wasm
wasm-bindgen --target web --out-dir ../pkg-arb target-arb/wasm32-unknown-unknown/release/s3_compat.wasm
wasm-bindgen --target web --out-dir ../pkg-frt target-frt/wasm32-unknown-unknown/release/s3_compat.wasm
