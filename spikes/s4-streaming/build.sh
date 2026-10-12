#!/bin/sh
# needs: rustup target wasm32-unknown-unknown, wasm-bindgen-cli 0.2.128 (must equal the crate's pin)
set -e
cargo test
cargo build --target wasm32-unknown-unknown --release
wasm-bindgen --target nodejs --out-dir pkg-node target/wasm32-unknown-unknown/release/vishion_s4.wasm
wasm-bindgen --target web --out-dir pkg-web target/wasm32-unknown-unknown/release/vishion_s4.wasm
echo "now: node compare.js ; node fuzz.js 6000 ; node browser.js"
