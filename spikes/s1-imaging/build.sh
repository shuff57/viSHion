#!/bin/sh
# Requires: rustup target wasm32-unknown-unknown, wasm-bindgen-cli =0.2.129
set -e
cd "$(dirname "$0")"
cargo build --release --target wasm32-unknown-unknown -p imaging-wasm
wasm-bindgen --target nodejs --out-dir pkg target/wasm32-unknown-unknown/release/imaging_wasm.wasm
ls -l pkg/*.wasm
