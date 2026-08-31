# Publishing Guide for `chunkr`

`chunkr` is configured to publish as both a **Rust Crate** on [crates.io](https://crates.io/crates/chunkr) and a **Python Native Extension Wheel** on [PyPI](https://pypi.org/project/chunkr/).

---

## 🚀 1. Automated Release via GitHub Actions (Recommended)

When you push a git tag matching `v*` (e.g. `v1.0.0`), GitHub Actions will automatically:
1. Run all unit and integration tests.
2. Build multi-platform wheels for Linux (`x86_64`, `aarch64`, `armv7`), Windows (`x86_64`, `x86`), and macOS (`x86_64`, `Apple Silicon aarch64`).
3. Build the source distribution (`sdist`).
4. Publish the package directly to PyPI using PyPI Trusted Publishing.

### Steps:
```bash
git tag v1.0.0
git push origin v1.0.0
```

---

## ⚡ 2. PyPI Publishing with `uv`

`uv` natively builds Python extension packages using the `maturin` backend configured in `pyproject.toml` and uploads them to PyPI.

### Step 1: Build source distribution and wheel
```bash
uv build
```
This produces `dist/chunkr-1.0.0.tar.gz` and `dist/chunkr-1.0.0-...whl`.

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
