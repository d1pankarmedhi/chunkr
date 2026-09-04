#!/usr/bin/env bash
set -euo pipefail

echo "⚡ Building Chunkr WebAssembly release..."

# 1. Compile wasm32-unknown-unknown cdylib
cargo build --target wasm32-unknown-unknown --features wasm --no-default-features --release

WASM_PATH="target/wasm32-unknown-unknown/release/chunkr.wasm"

if [ ! -f "$WASM_PATH" ]; then
    echo "Error: Failed to locate compiled wasm at $WASM_PATH" >&2
    exit 1
fi

# 2. Generate web bindings (Browsers & Cloudflare Workers)
echo "📦 Generating Web/Cloudflare Worker bindings (wasm/web)..."
wasm-bindgen --target web --out-dir wasm/web "$WASM_PATH"

# 3. Generate bundler bindings (Vite, Webpack, Rollup)
echo "📦 Generating Bundler bindings (wasm/bundler)..."
wasm-bindgen --target bundler --out-dir wasm/bundler "$WASM_PATH"

# 4. Generate Node.js bindings (CommonJS/Node)
echo "📦 Generating Node.js bindings (wasm/nodejs)..."
wasm-bindgen --target nodejs --out-dir wasm/nodejs "$WASM_PATH"

echo "✅ WebAssembly builds completed successfully!"
echo "   - Browser/Workers: wasm/web/"
echo "   - Bundlers:        wasm/bundler/"
echo "   - Node.js:         wasm/nodejs/"
