# Publishing Guide for `chunkr` / `chunkr-rs`

`chunkr` is configured to publish as:
- **Rust Crate**: [`chunkr`](https://crates.io/crates/chunkr) on [crates.io](https://crates.io/crates/chunkr)
- **Python Native Extension Wheel**: [`chunkr-rs`](https://pypi.org/project/chunkr-rs/) on [PyPI](https://pypi.org/project/chunkr-rs/) (imported in Python simply as `import chunkr`)

---

## 🚀 1. Automated Release via GitHub Actions (Recommended)

GitHub Actions will build multi-platform wheels for Linux, Windows, and macOS, compile the sdist, build and verify WebAssembly targets, create a **GitHub Release** with all wheel, wasm, and source assets attached, and publish to **PyPI**, **Crates.io**, and **npm** automatically.

### Setup GitHub Secrets (One-time)
In your GitHub repo: **Settings → Secrets and variables → Actions → New repository secret**:
1. `PYPI_API_TOKEN`: Your PyPI API Token (starts with `pypi-...` from [pypi.org/manage/account/token](https://pypi.org/manage/account/token/))
2. `CARGO_REGISTRY_TOKEN`: Your Crates.io API Token (from [crates.io/settings/tokens](https://crates.io/settings/tokens))
3. `NPM_TOKEN`: Your npm Access Token (from [npmjs.com/settings/tokens](https://www.npmjs.com/settings/tokens)) with publish permissions for `chunkr-wasm`

### Release Steps:

#### Option A: Release with Custom Title and Notes (Annotated Tag)
```bash
# First -m is the Title, second -m (or subsequent lines) is the Release Notes body
git tag -a v1.3.0 -m "Release v1.3.0: Streaming Chunker" -m "### Highlights
- Added StreamingChunker for real-time token streams.
- Added LangChain & LlamaIndex ecosystem bridges.
- Added chunkr-cli binary."

git push origin v1.3.0
```

#### Option B: Quick Release with Auto-Generated Notes (Lightweight Tag)
```bash
git tag v1.3.0
git push origin v1.3.0
```
*(GitHub Actions will automatically extract PRs and commits to generate the changelog).*

---

## ⚡ 2. PyPI Publishing with `uv`

`uv` natively builds Python extension packages using the `maturin` backend configured in `pyproject.toml` and uploads them to PyPI.

### Step 1: Build source distribution and wheel
```bash
uv build
```
This produces `dist/chunkr_rs-1.0.0.tar.gz` and `dist/chunkr_rs-1.0.0-...whl`.

### Step 2: Publish to PyPI
Set your PyPI API token (`pypi-...`) or pass it directly:
```bash
# Using token argument
uv publish --token <YOUR_PYPI_API_TOKEN>

# Or set environment variable
$env:UV_PUBLISH_TOKEN="<YOUR_PYPI_API_TOKEN>"
uv publish
```

---

## 🐍 3. Alternative: Manual PyPI Publishing (Maturin)

```bash
# Build & publish directly with maturin
maturin publish
```

---

## 🦀 4. Crates.io Publishing (Rust)

Make sure you are logged in to crates.io (`cargo login <CARGO_API_TOKEN>`):

```bash
# Verify the crate packages cleanly
cargo package

# Publish to crates.io
cargo publish
```

---

## 🌐 5. NPM Publishing (WebAssembly)

Build the WebAssembly artifacts and publish `chunkr-wasm` to npm:

```bash
# 1. Build Wasm targets (web, bundler, nodejs)
./scripts/build_wasm.sh  # or .\scripts\build_wasm.ps1 on Windows

# 2. Run smoke tests
node tests/test_wasm_smoke.js

# 3. Publish to npm
npm publish ./wasm --access public
```
