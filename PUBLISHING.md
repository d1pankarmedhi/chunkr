# Publishing & Maintenance Guide — `chunkr` / `chunkr-rs` / `chunkr-wasm`

This repo ships **three artifacts from one codebase**:

| Artifact | Registry | Install | Import |
|---|---|---|---|
| Rust crate `chunkr` | [crates.io](https://crates.io/crates/chunkr) | `cargo add chunkr` | `use chunkr::prelude::*;` |
| Python wheels `chunkr-rs` | [PyPI](https://pypi.org/project/chunkr-rs/) | `pip install chunkr-rs` | `import chunkr` |
| Wasm package `chunkr-wasm` | [npm](https://www.npmjs.com/package/chunkr-wasm) | `npm install chunkr-wasm` | `import { RecursiveChunker } from "chunkr-wasm"` |

Two workflows automate everything (see [.github/workflows/](.github/workflows/)):

| Workflow | Trigger | Job |
|---|---|---|
| `ci.yml` | every PR + push to `main`/`develop` | fmt, clippy, rustdoc, Rust tests (linux/win/mac), Python binding tests (3.9–3.13), Wasm build + smoke test, version-sync check |
| `release.yml` | **tag `vX.Y.Z` only** (or manual dispatch) | version gate → build wheels/sdist/wasm → wheel smoke test → GitHub Release → PyPI → crates.io → npm |

> **Golden rule: tags are the release.** Never publish to a registry by hand
> except to recover from an incident (and then say so in the release notes).
> Everything that reaches PyPI / crates.io / npm must first exist as a git tag
> with a green `release.yml` run attached.

---

## 1. One-time setup (maintainers only)

Publishing uses long-lived tokens stored as GitHub secrets. (Upgrade path:
[Trusted Publishing / OIDC](https://docs.pypi.org/trusted-publishers/) for
PyPI/crates.io and npm provenance — no rotatable secrets. Tracked as a TODO
in `release.yml`; migrate when convenient.)

Token hygiene rules (all three): scope each token to the single package,
store it as an **environment secret** (not repo-wide) so a leak elsewhere
can't publish your packages, and rotate yearly or on any suspected exposure.

### 1a. PyPI — `PYPI_API_TOKEN`

1. PyPI → Account settings → **API tokens → Add API token**, scope it to the
   `chunkr-rs` project (not your whole account).
2. In GitHub: **Settings → Environments → New environment** named `pypi`
   (no reviewers required; optionally restrict deployment branches to tags `v*`).
3. In that environment: **Add environment secret** → `PYPI_API_TOKEN`.
4. (Recommended) Mint a second token on **TestPyPI** first and practice with
a `v…-rc.1` prerelease tag before touching production PyPI.

### 1b. crates.io — `CARGO_REGISTRY_TOKEN`

1. crates.io → Account Settings → **API Tokens → New Token**, scope: publish
   (select the `chunkr` crate if scoping is offered).
2. In GitHub create environment `crates-io`, add secret `CARGO_REGISTRY_TOKEN`.

### 1c. npm — `NPM_TOKEN`

1. npmjs.com → Access Tokens → **Generate New Token → Automation** (classic).
   Automation tokens bypass 2FA by design — never use one locally, only in CI.
2. In GitHub create environment `npm`, add secret `NPM_TOKEN`.

### 1d. Branch protection (do this today)

**Settings → Branches → Add rule for `main`:**
- ✅ Require a pull request before merging (1 approval)
- ✅ Require status checks to pass: `Version sync check`, `fmt + clippy`,
  `Rust tests`, `Python bindings`, `Wasm build + smoke test`
- ✅ Require branches to be up to date before merging
- ✅ Do not allow bypassing the above settings (applies to admins too)
- ✅ Block force pushes

`develop` is your integration branch; `main` is always releasable.

---

## 2. Versioning policy

- **SemVer** (`MAJOR.MINOR.PATCH`), one version across all three manifests:
  `Cargo.toml` = `pyproject.toml` = `wasm/package.json`.
  The `version-sync` job (CI) and `validate` job (release) enforce this —
  a drifted version fails the build before anything publishes.
- Bump **before** tagging. The tag must equal the manifests exactly
  (`v1.3.0` ↔ `version = "1.3.0"`).
- Prereleases use `-rc.1`, `-beta.1` suffixes (`v1.4.0-rc.1`).
  They publish to GitHub as *prerelease* and are safe to test from TestPyPI;
  crates.io/npm accept them without polluting `latest`.
- `CHANGELOG.md` follows [Keep a Changelog](https://keepachangelog.com/).
  Every release PR moves entries from `[Unreleased]` to `[X.Y.Z] - YYYY-MM-DD`.
  The release workflow **fails if the changelog has no entry** for the tag.

---

## 3. Release checklist (the normal path, ~10 min)

```bash
# 0. Start from a green main
git checkout main && git pull

# 1. Prep PR: bump version in all three manifests + changelog
#    Cargo.toml, pyproject.toml, wasm/package.json  →  1.4.0
#    CHANGELOG.md: move [Unreleased] → [1.4.0] - 2026-09-..
#    Open PR → wait for ci.yml green → merge.

# 2. Tag (ANNOTATED — carries the release title + notes) and push
git checkout main && git pull
git tag -a v1.4.0 -m "Release v1.4.0: <short title>" -m "### Highlights
- ...
- ..."
git push origin v1.4.0

# 3. Watch Actions → release.yml:
#    validate → wheels (linux/win/mac) + sdist + wasm →
#    smoke test → GitHub Release → PyPI → crates.io → npm
#
# 4. Verify (links are in the workflow summary):
#    - GitHub Release shows wheels + sdist + wasm tarball + SHA256SUMS.txt
#    - pip install chunkr-rs==1.4.0 && python -c "import chunkr"
#    - cargo add chunkr@1.4.0 builds
#    - npm view chunkr-wasm version
```

**Tag conventions:**
- Always annotated (`-a` with two `-m`: first line = title, rest = notes).
  The workflow extracts the title + notes from the tag object. Lightweight tags
  are **rejected** by the `validate` job on purpose — they carry no message and
  can't be audited later.
- Never move or delete a published tag. A wrong release is fixed by a *new*
  patch version, not by rewriting history (registries are append-only).

**Manual dispatch** (`Actions → Release → Run workflow`) is for dry-runs
(`dry_run: true` builds everything, publishes nothing) and incident re-publishes
(`dry_run: false`, `target: pypi|crates|npm|github`). It never replaces tagging.

---

## 4. What the release pipeline guarantees

| Property | How |
|---|---|
| No partial releases | Publish jobs (`pypi`, `crates`, `npm`) all `need: [github]` — registries only get artifacts already attached to a GitHub Release |
| Reproducible-ish builds | `--locked` everywhere; `Cargo.lock` committed; `uv.lock` committed |
| Supply-chain integrity | `SHA256SUMS.txt` on every GitHub Release + Sigstore build attestation (`attest-build-provenance`) + npm `--provenance` |
| Broken wheels can't ship | `smoke-wheels` installs the real built wheel and runs the Python suite **before** any publish |
| Loud failures | No `continue-on-error` on any publish step. A failed publish fails the workflow so the release is never "half-green" |
| Concurrency safety | `concurrency: release-${{ ref }}` with `cancel-in-progress: false` — two pushes of the same tag queue instead of racing; a release is never cancelled mid-publish |
| Credentials | Scoped API tokens as environment secrets; per-job `permissions: {}`-least-privilege; OIDC/provenance tracked as upgrade TODOs in `release.yml` |

**Architecture matrix (deliberate, not maximal):** Linux `x86_64 + aarch64`,
Windows `x64`, macOS `x86_64 + aarch64`, plus sdist. The old matrix also built
`armv7 / s390x / ppc64le` (Linux) and 32-bit Windows — dropped because they had
zero test coverage, ~10x CI minutes, and no reported users. Re-add an arch only
with (a) a user request and (b) a smoke-test job for it. sdist covers the rest:
anyone on an exotic platform gets a source build via `pip install`.

---

## 5. Hotfixes, yanks & incident recovery

- **Hotfix:** branch from the tag (`git checkout -b hotfix/1.4.1 v1.4.0`),
  fix, bump to `1.4.1` in all three manifests + changelog, PR → merge → tag
  `v1.4.1`. Never commit directly on a tag.
- **Bad PyPI release:** `yank` it on PyPI (keeps the files but blocks new
  installs), then ship a patch version. Never delete files — deletion breaks
  lockfiles downstream.
- **Bad crate:** `cargo yank --vers 1.4.0` (same semantics), then patch release.
- **Bad npm:** `npm deprecate chunkr-wasm@1.4.0 "use >=1.4.1"` (npm has no real
  yank; deprecate + patch).
- **Manual publish** (only when CI is broken and users are blocked):
  ```bash
  uv build && uv publish            # PyPI (needs UV_PUBLISH_TOKEN)
  cargo publish --locked            # crates.io
  npm publish ./wasm --access public # npm
  ```
  Then open an issue titled `chore: manual publish of vX.Y.Z — backfill CI`
  and attach logs. Manual publishes must remain exceptional and auditable.

---

## 6. Maintaining the package (beyond releases)

The release pipeline is ~20% of open-source maintenance. The rest:

1. **Health files** — you have `LICENSE` (MIT) + `CONTRIBUTION.md`. Still missing
   and worth adding: `SECURITY.md` (how to report vulns, supported versions),
   issue templates (`.github/ISSUE_TEMPLATE/bug_report.yml`,
   `feature_request.yml`), a PR template (`.github/pull_request_template.md`
   with a changelog checkbox), and `CODEOWNERS`. These cut maintainer triage
   time more than any automation.
2. **Dependency hygiene** — `dependabot.yml` (added) opens weekly grouped PRs
   for Actions, Cargo, and pip. Merge green ones promptly; run `cargo audit`
   / `cargo deny` before each minor release to catch advisories + license drift
   (`copyleft` deps are incompatible with MIT consumers).
3. **MSRV & Python floor** — declare and test them. `requires-python = ">=3.8"`
   is only a claim until CI installs 3.8; the `test-python` matrix (3.9–3.13)
   is the enforcement. Same for Rust: pick an MSRV (e.g. 1.75), pin it in
   `rust-version` in `Cargo.toml`, and add one `*-msrv` job when the user base
   grows. Bumping either floor is a **minor** (or major) version decision —
   announce it in the changelog.
4. **Docs as a release gate** — `cargo doc` with `-D warnings` already runs in
   CI, so broken intra-doc links fail PRs. Keep the README's install matrix
   (`pip`/`cargo`/`npm`) and the `chunkr.pyi` stubs in sync with every API PR;
   stale `.pyi` is the #1 papercut for Python consumers of Rust extensions.
5. **Changelog discipline** — enforce "every user-facing PR updates
   `[Unreleased]`" in review. A release where the changelog is written
   after-the-fact from `git log` reads like an apology; one curated alongside
   the code reads like a product.
6. **Issue stewardship** — label (`bug`, `enhancement`, `good first issue`,
   `wontfix`), respond within a week even if only "reproduced, queued", and
   close the loop (comment the version that fixed it). Stale-but-acknowledged
   beats silent. `good first issue` + a clear `CONTRIBUTION.md` test command is
   your contributor funnel.
7. **Release cadence** — time-box, don't scope-box: small minors every few
   weeks beat rare "big bang" majors. Keep `main` releasable (that's what
   branch protection + CI buy you) so a security fix can ship as `1.4.1` the
   same day.
8. **Trust signals** — badges already in README (PyPI/crates/license) are good;
   add CI status + docs.rs. Respond to the Big Three questions every evaluator
   asks in 30 seconds: *is it alive?* (recent release + CI green),
   *is it safe?* (license, audit, provenance), *does it work for me?*
   (per-OS wheels listed on the GitHub Release).

---

## 7. Quick reference

| Task | Command |
|---|---|
| Local Rust check (mirror of CI) | `cargo fmt --check && cargo clippy --all-targets --features python -- -D warnings && cargo test` |
| Local Python bindings | `uv run maturin develop --features python && pytest tests/test_python_bindings.py -q` |
| Local Wasm | `./scripts/build_wasm.sh && node tests/test_wasm_smoke.js` |
| Dry-run a release build | `Actions → Release → Run workflow → dry_run: true` |
| Ship a release | bump 3 manifests + changelog → merge → `git tag -a vX.Y.Z … && git push origin vX.Y.Z` |
| Recover PyPI/crates/npm incident | §5 above |

*Last reviewed: 2026-09-05. If this doc and the workflows disagree, the
workflows win — then open a PR fixing the doc.*
