#!/bin/sh
# needs: rustup target wasm32-unknown-unknown, wasm-bindgen-cli 0.2.128 (must equal the crate's pin)
set -e
cargo test
cargo build --target wasm32-unknown-unknown --release
wasm-bindgen --target nodejs --out-dir pkg-node target/wasm32-unknown-unknown/release/vishion_s5.wasm
wasm-bindgen --target web --out-dir pkg-web target/wasm32-unknown-unknown/release/vishion_s5.wasm
echo "now: node run.js ; cargo test --test pulldown -- --nocapture"
