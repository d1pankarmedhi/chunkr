#!/usr/bin/env pwsh
# Build script for Chunkr WebAssembly targets

$ErrorActionPreference = "Stop"

Write-Host "⚡ Building Chunkr WebAssembly release..." -ForegroundColor Cyan

# 1. Compile wasm32-unknown-unknown cdylib
cargo build --target wasm32-unknown-unknown --features wasm --no-default-features --release

$wasmPath = "target/wasm32-unknown-unknown/release/chunkr.wasm"

if (-not (Test-Path $wasmPath)) {
    Write-Error "Failed to locate compiled wasm at $wasmPath"
    exit 1
}

# 2. Generate web bindings (Browsers & Cloudflare Workers)
Write-Host "📦 Generating Web/Cloudflare Worker bindings (wasm/web)..." -ForegroundColor Cyan
wasm-bindgen --target web --out-dir wasm/web $wasmPath

# 3. Generate bundler bindings (Vite, Webpack, Rollup)
Write-Host "📦 Generating Bundler bindings (wasm/bundler)..." -ForegroundColor Cyan
wasm-bindgen --target bundler --out-dir wasm/bundler $wasmPath

# 4. Generate Node.js bindings (CommonJS/Node)
Write-Host "📦 Generating Node.js bindings (wasm/nodejs)..." -ForegroundColor Cyan
wasm-bindgen --target nodejs --out-dir wasm/nodejs $wasmPath

Write-Host "✅ WebAssembly builds completed successfully!" -ForegroundColor Green
Write-Host "   - Browser/Workers: wasm/web/"
Write-Host "   - Bundlers:        wasm/bundler/"
Write-Host "   - Node.js:         wasm/nodejs/"
