# Backends: parsers and OCR engines

Research + design for letting users choose or replace the extraction backends
behind `chunkr-rs[pdf]` / the `pdf` Cargo feature.

Status: **P1 implemented** (config, registry, loopback proxy, adapters, gated
`auto` mode, docs + tests); P2–P4 still open, see [Phasing](#5-phasing).
Everything labelled *verified* below was checked against source or upstream docs
in this repository's worktree; see [Sources](#sources).

---

## 1. TL;DR

1. There are **two different plug points**, and mixing them up is the main
   design risk:
   - **OCR engines** — word-level: `page image -> [{text, bbox, confidence}]`.
     They *augment* a parse; layout, reading order and table reconstruction stay
     in the parser.
   - **Parser backends** — page-level: `document -> [{text | markdown, blocks}]`.
     They *replace* the parse (VLM OCR, docling, MinerU, Mistral OCR, …).
2. `chunkr` already has the parser-backend registry (`register_pdf_backend`
   + `pages_to_documents`). The missing half is the **OCR engine registry**.
3. Four ways to plug an OCR engine already work or are one small step away:
   | Route | Status today | Needs |
   | --- | --- | --- |
   | Tesseract (in-process) | works (`pdf-ocr` / liteparse wheel) | nothing |
   | PP-OCR v5/v6 (in-process, ONNX) | one Cargo feature away | `liteparse/oar-ocr*` |
   | HTTP OCR server (any language) | works (`ocr.mode="server"`) | a conforming server |
   | Python OCR engine / cloud API | needs a ~120-line loopback proxy | `chunkr-pdf` |
4. Upstream already ships **PaddleOCR, EasyOCR and Surya reference servers**
   (`run-llama/liteparse/ocr/`) plus a documented HTTP contract
   (`OCR_API_SPEC.md`), so "choose your OCR backend" is a configuration problem,
   not a research project.
5. Recommended P1: `ocr.backend` config + `register_ocr_backend` +
   stdlib loopback proxy + adapters for those three engines + real per-page
   `ocr.mode="auto"` gating. No new required dependencies for the base package.

---

## 2. What upstream gives us (verified)

### 2.1 liteparse `OcrEngine` trait (Rust, compile time)

`liteparse-2.15.1/src/ocr/mod.rs`:

```rust
pub trait OcrEngine: Send + Sync {
    fn name(&self) -> &str;
    fn prefers_grayscale(&self) -> bool { false }  // Tesseract wants luma
    fn recognize(&self, image: &[u8], width: u32, height: u32,
                 options: &OcrOptions) -> Future<Output = Vec<OcrResult>>;
}

pub struct OcrResult { text: String, bbox: [f32; 4], confidence: f32,
                       polygon: Option<[[f32; 2]; 4]> }
```

Overridden per parse with `LiteParse::new(cfg)?.with_ocr_engine(Arc<dyn OcrEngine>)`
(`src/parser.rs:252`). Engine resolution order (`src/parser.rs:398`):
`ocr_engine_override` → `ocr_server_url` (HTTP) → Tesseract when the `tesseract`
feature is on → error. **We never set the override today**, so our Rust feature
can only ever reach Tesseract or an HTTP server.

Built-in engines in the crate:

| Engine | Feature (crate default) | Notes |
| --- | --- | --- |
| `tesseract` | `tesseract` (default on in the crate) | `tesseract-rs`, 100+ languages |
| `oar-ocr` (ONNX, PP-OCR) | `oar-ocr`, off by default | acceleration via `coreml` / `cuda` / `directml` / `openvino` / `tensorrt` / `webgpu`, model download via `oar-ocr-auto-download` |
| `http_simple` | always (native) | any service speaking the spec below |

`oar-ocr` is `greatv/oar-ocr`, Apache-2.0, Rust OCR + layout library on ONNX
Runtime, presets `ppocr_v6_tiny` / `ppocr_v6_small` / `ppocr_v6_medium`
(also PP-OCRv4 with per-language dictionaries). Presets download det/rec/dict
models from ModelScope, SHA-256 verified, cached in `$OAR_HOME` (`~/.oar`).
**Pair the recognizer with its matching dictionary** — a mismatch silently
produces garbled text (upstream README).

### 2.2 liteparse HTTP OCR protocol (language-independent, runtime)

`OCR_API_SPEC.md` in the upstream repo; implemented by
`src/ocr/http_simple.rs`:

- `POST /ocr`, `multipart/form-data`, fields `file` (PNG bytes) + `language`
  (ISO 639-1, e.g. `en`, `zh`).
- Response: `{"results": [{"text", "bbox": [x1,y1,x2,y2], "confidence": 0..1,
  "polygon": [[x,y]x4]?}]}`; the prod-style `{"result": [[poly], text, conf]}`
  (EasyOCR/PaddleOCR tuple shape) is also accepted.
- Pixels, origin top-left, coordinates in the **rendered image**, which
  liteparse rasterizes at `dpi` (default 150). Ordering: reading order.
- Optional `polygon` (TL→TR→BR→BL in the upright frame) lets liteparse rotate
  sideways/vertical text instead of flattening it into body lines.
- Client side: retries with backoff on transient failures, optional hedged
  duplicates (`ocr_hedge_delays_ms`, e.g. `[0, 250]` = send two, take the first
  reply) — useful for slow cloud APIs; `ocr_failure_fatal` decides whether an
  OCR failure fails the page or the whole parse.
- OCR words are merged into the text layer (`src/ocr_merge.rs`), preferring the
  embedded text where it exists; OCR is what fills scanned/garbled regions.

Upstream reference servers (ports 8828 / 8829 / 8830):

| Server | Engine | Notes from upstream |
| --- | --- | --- |
| `ocr/easyocr/` | EasyOCR | general purpose, 80+ languages |
| `ocr/paddleocr/` | PaddleOCR 3.x `predict()` | best CJK, "2-3x faster than EasyOCR"; env knobs `PADDLE_DET_LIMIT_SIDE_LEN` (1600 default, caps detector input), `PADDLE_TEXTLINE_ORIENTATION`, `PADDLE_CPU_THREADS`; maps `rec_texts`/`rec_scores`/`rec_boxes`/`rec_polys` |
| `ocr/suryaocr/` | Surya OCR 2 (VLM via llama.cpp or vLLM) | 90+ languages, block-level output, GPU recommended |

### 2.3 The Python wheel (what we can reach from Python)

`liteparse` 2.15.1 wheel exposes OCR config only through these constructor
kwargs: `ocr_enabled`, `ocr_server_url`, `ocr_server_headers`, `ocr_language`,
`tessdata_path`, `ocr_failure_fatal`, `ocr_hedge_delays_ms` (plus `dpi`,
`num_workers`, `pool_size`, `parse_timeout`). **No callable OCR engine** (only
wasm has an `ocrEngine` callback), and the wheel bundles Tesseract only — no
PP-OCR/ONNX engine (checked with `strings` on `_liteparse.abi3.so`).

Two capabilities make a Python-side engine plug-in possible anyway:

- `extract_screenshots=True` → `ParseResult.screenshots[].image_bytes` (PNG per
  page) if we ever want to drive OCR ourselves.
- `ocr_server_url` is just HTTP → a **local proxy we control** can expose any
  Python engine to liteparse, keeping liteparse's merge/reading-order logic.

### 2.4 What `chunkr` has today

- Parser registry: `chunkr.register_pdf_backend(name, callable)`,
  `pdf_backends()`, contract `callable(source, config_json) -> payload JSON`,
  mapped by `chunkr.pages_to_documents` (shared with the Rust feature).
- Config `ocr.*`: `mode` (`off|auto|always|server`), `language`, `server_url`,
  `headers`, `tessdata_path`, `num_workers`, `failure_fatal`.
- `include_complexity` + `is_complex()` returning per-page `needs_ocr` and
  `reasons`: `Scanned`, `NoText`, `SparseText`, `EmbeddedImages`, `Garbled`,
  `VectorText`, `AnnotationText`.

Gaps found while researching:

- `ocr.mode="auto"` is currently equivalent to `always`: it maps to
  `ocr_enabled=true`, which runs OCR over every page. A real "auto" needs
  complexity-gated page selection on our side.
- `ocr.language` defaults to Tesseract's `"eng"`, but the HTTP/plugin contract
  wants ISO 639-1 (`en`). The upstream PaddleOCR server normalizes with an alias
  map, but we should normalize per backend instead of relying on that.

---

## 3. Landscape (what else could be plugged in)

Word-level engines (fit the OCR contract):

| Engine | License | Install / runtime | Output | Notes |
| --- | --- | --- | --- | --- |
| Tesseract 5 | Apache-2.0 | already in liteparse | words + boxes | weakest on CJK, handwriting, noisy scans |
| PP-OCRv5 / PP-OCRv6 (PaddleOCR) | Apache-2.0 | `paddleocr` + `paddlepaddle` (heavy), or ONNX via `oar-ocr` / RapidOCR | polygons + scores | best CJK, strong general accuracy, 2-3x faster than EasyOCR per upstream |
| RapidOCR | Apache-2.0 | `rapidocr` (ONNX, ~27 MB wheel incl. small models), CPU-friendly | polygons + scores | PaddleOCR models without the Paddle runtime — best "light local" option |
| EasyOCR | Apache-2.0 | `easyocr` (torch) | polygons + scores | 80+ languages, torch dependency |
| Surya OCR 2 | code Apache-2.0, **weights OpenRAIL** | 650M VLM; llama.cpp CPU or vLLM GPU | HTML/blocks + boxes | strong multilingual; check weight terms before bundling |
| Azure Document Intelligence / Google Document AI / AWS Textract | commercial | HTTPS API | words + polygons (+ markdown) | conform to the OCR spec via an adapter; per-page cost |
| Mistral OCR 4 | commercial | HTTPS API | per-page markdown + bboxes/HTML | bbox granularity is coarser; better as a *parser* backend |
| VLM OCR (olmOCR 2 Apache-2.0, dots.ocr, PaddleOCR-VL, Qwen3-VL, Gemini/OpenAI vision) | mixed | vLLM / API | markdown/HTML | best on hard scans; use parser registry, not word fusion |

Page-level parsers (fit the parser-backend contract):

| Tool | License | Notes |
| --- | --- | --- |
| docling (+ `docling-serve`) | MIT | OCR choice built in (RapidOCR/EasyOCR/Tesseract/tesserocr), layout + tables; HTTP API `POST /v1/convert/file` |
| MinerU 2 | repo license is AGPL-style ("Other" per GitHub) | VLM pipeline, strong tables; verify terms for commercial use |
| marker | Apache-2.0 (code) | markdown + tables; model weights have separate terms |
| Mistral OCR / Azure layout markdown | commercial | page markdown with layout; simplest high-quality path |
| liteparse (ours) | Apache-2.0 | the default backend |

---

## 4. Design

### 4.1 Two contracts, one config

```
PDF bytes
   │
   ├─ parser backend ────► pages: text/markdown + typed blocks ──► pages_to_documents
   │      (liteparse, docling, MinerU, Mistral OCR, VLM endpoint)
   │
   └─ liteparse ──────────────────────────────────────────────► pages: blocks
             └─ ocr engine ◄── page image ── tesseract | ppocr | http server | python plugin
```

`ocr.backend` selects the engine inside liteparse; a parser backend does not use
`ocr.*` at all. A user replacing *everything* (rare, and only correct for
markdown/HTML-producing engines) registers a parser backend.

### 4.2 Config surface (additions to `OcrConfig`)

```jsonc
{
  "ocr": {
    "mode": "off",                 // off | auto | always | server
    "backend": "tesseract",        // tesseract | ppocr | server | <registered-name>
    "language": "en",              // ISO 639-1; normalized per backend
    "server_url": null,
    "headers": [],
    "hedge_delays_ms": [],         // e.g. [0, 250] for cloud APIs
    "failure_fatal": false,
    "tessdata_path": null,         // tesseract only
    "ppocr": {                     // native ONNX engine (Rust feature)
      "preset": "small",           // tiny | small | medium | custom
      "models_dir": null,          // cache/download dir ($OAR_HOME default)
      "det_model": null, "rec_model": null, "dict": null,
      "device": "cpu"              // cpu | coreml | cuda | directml | openvino | tensorrt | webgpu
    },
    "plugin": {                    // python-side engine via loopback proxy
      "options": {},               // passed to the engine factory
      "timeout_ms": 60000,
      "concurrency": 1,            // threads the engine can serve at once
      "warmup": true,              // fail fast at construction, not mid-parse
      "port": 0                    // 0 = ephemeral loopback port
    },
    "fallback": "none"             // none | fast | server  (on OCR failure)
  }
}
```

Validation rules (all enforced in Rust core so Rust and Python agree):

- `backend="ppocr"` without the compiled feature → actionable error naming the
  feature, not an empty parse.
- `backend="tesseract"` on a `pdf`-only build (no `pdf-ocr`) → same treatment;
  today that case surfaces late as liteparse's
  `"OCR enabled but no --ocr-server-url provided and tesseract feature is disabled"`.
- `backend="server"` requires `server_url`; `mode="off"` ignores `backend`.
- Unknown backend names error with the list of available backends.
- `language` is normalized (`eng`→`en`, `zh-cn`→`zh`, `ja`→`ja`, …) before it
  reaches a server/plugin, because the spec is ISO 639-1 while Tesseract codes
  differ.

### 4.3 OCR engine registry (Python)

```python
chunkr.register_ocr_backend(
    "paddleocr",
    engine,                       # callable or object with .recognize()
    capabilities={                # used for validation + later auto-routing
        "languages": ["en", "zh", "ja", "ko", "fr", "de", "es"],
        "polygon": True, "confidence": True,
        "local": True, "gpu": False, "page_images": True,
    },
)
chunkr.ocr_backends()             # names
chunkr.unregister_ocr_backend(name) -> bool
```

Engine contract (one page image per call, reading order, image pixel space):

```python
def engine(page_image_png: bytes, *,
           language: str = "en", page_number: int = 1,
           dpi: float = 150, options: dict | None = None
           ) -> list[dict]:           # [{"text", "bbox", "confidence", "polygon"?}]
```

A plain `str` return is accepted as shorthand (single full-page block); it is
documented as lossy and discouraged, because layout is thrown away.

### 4.4 How a Python engine reaches liteparse: loopback proxy

liteparse only accepts a URL in Python, so `chunkr_pdf` starts a **stdlib-only**
HTTP server on `127.0.0.1:<ephemeral>` that implements `POST /ocr` (+ `GET
/health`), parses the multipart body with
`email.parser.BytesParser` (no FastAPI/uvicorn dependency), calls the engine and
returns `{"results": [...]}`. liteparse then does reading order, rotation
handling, merge and table reconstruction exactly as for a remote server.

Why this and not our own merge:

- Reuses `ocr_merge.rs` (rotation, reading order, text-layer preference, table
  interaction) — reimplementing it in Python would fork the intelligence.
- Works for cloud APIs (the adapter is the "engine"), including auth and retry.
- No new dependency, no port to configure, loopback-only, thread-safe by lock.

Lifecycle: created lazily on first parse with `ocr.backend != tesseract`, kept
alive while the parser object lives, closed on `close()`/context exit and by
`__del__` as a backstop; `warmup=True` imports/loads the engine up front so a
missing `paddleocr` install fails immediately with a clear message instead of
five minutes into a parse.

Adapters shipped (`chunkr_pdf._ocr`), each ~15-40 lines:

| Adapter | Source | Extra |
| --- | --- | --- |
| `rapidocr()` | `rapidocr` (ONNX, CPU) | `chunkr-pdf[ocr-rapid]` |
| `paddleocr()` | `paddleocr` (Paddle) | `chunkr-pdf[ocr-paddle]` |
| `easyocr()` | `easyocr` (torch) | `chunkr-pdf[ocr-easyocr]` |
| `surya()` | `surya-ocr` (llama.cpp/vLLM) | `chunkr-pdf[ocr-surya]` |
| `mistral(api_key=...)`, `azure(...)`, `textract(...)`, `documentai(...)` | vendor HTTPS APIs | none (stdlib `urllib`) — **not written yet (P3)** |
| `vllm(base_url, model)` | OpenAI-compatible VLM endpoint | none |

Usage:

```python
parser = PDFParser(ocr={"mode": "always", "backend": "paddleocr"})
parser = PDFParser(ocr={"mode": "auto", "backend": "rapidocr"})

chunkr.register_ocr_backend("house", my_engine)
parser = PDFParser(ocr={"mode": "always", "backend": "house"})
```

For engines that only emit markdown/HTML (Mistral OCR, olmOCR, MinerU,
docling), use the parser registry instead:

```python
chunkr.register_pdf_backend("olmocr", olm_engine)     # page-level payload
PDFLoader(backend="olmocr")
```

### 4.5 Real `ocr.mode="auto"`

Today `auto` == `always`. Proposed implementation in the core:

1. Cheap pass: `is_complex()` → per-page `needs_ocr` + `reasons`.
2. Gate on reasons, not the boolean — on our fixtures every page is flagged
   (`EmbeddedImages`/`VectorText` fire on digital pages), so the default gate is
   `{Scanned, NoText, SparseText, Garbled}` with a configurable override:
   `ocr.auto_reasons`, `ocr.auto_min_chars`.
3. Parse without OCR (all pages) and with OCR restricted to the flagged pages
   (`scope.target_pages = "3,7-9"`), then take the OCR result for those pages.
4. Merge is per page number, so this stays a pure mapping-layer operation.

This turns OCR from "costs what the whole document costs" into "costs what the
broken pages cost". Acceptance: on a mixed fixture (digital pages + scanned
pages) chunks from flagged pages improve and unflagged pages stay byte-identical;
`mode="auto"` on a fully digital PDF performs zero OCR calls.

### 4.6 Rust-side parity

| Capability | Implementation |
| --- | --- |
| Tesseract | existing `pdf-ocr` (`liteparse/tesseract`) |
| PP-OCR (ONNX, in-process) | new `pdf-ocr-ppocr = ["pdf", "liteparse/oar-ocr", "liteparse/oar-ocr-auto-download"]`; platform acceleration features `pdf-ocr-ppocr-coreml`, `-cuda`, `-directml`, `-openvino`, `-tensorrt`, `-webgpu` |
| Custom engine | `PdfParser::with_ocr_engine(Arc<dyn OcrEngine>)` (re-export liteparse's trait), so embedders can inject anything without a new feature |
| HTTP server | existing |

Device selection is a **compile-time** feature in `oar-ocr`, so `ppocr.device`
can only *validate* that the compiled backend matches; a mismatch is a config
error, not a silent CPU fallback. Document that tradeoff in the README matrix.

### 4.7 Operational notes

- Concurrency: PDFium FFI is serialized process-wide inside liteparse, and OCR
  runs after rasterization; `num_workers` bounds concurrent rasters/OCR calls.
  A Python engine that is not thread-safe should declare `concurrency=1` and the
  proxy will serialize with a lock (a queue if we later add warming pools).
- Failures: `failure_fatal=false` + `fallback="fast"` should mean "OCR failed,
  fall back to the fast extractor for that page" — the existing
  `junk_guard="fallback_fast"` already flags that condition; wire them together
  so a dead OCR server cannot silently produce an empty document.
- Cost/latency: `hedge_delays_ms` cuts cloud tail latency; `ocr.backend` +
  `mode` are the only two knobs a user needs for the common cases.

---

## 5. Phasing

| Phase | Scope | Verification |
| --- | --- | --- |
| --- | --- | --- |
| ~~**P1**~~ **done** | `ocr.backend` + validation + `register_ocr_backend` registry + loopback proxy + `rapidocr`/`paddleocr`/`easyocr`/`surya` adapters + reason-gated `mode="auto"` + docs | 28 extension tests (proxy conformance incl. 400/500/504 paths, engine-shape normalisation, gating with zero OCR calls on a digital document, real RapidOCR over a rasterised page) and 7 Rust tests (`select_ocr_pages`, engine call counts per mode, `with_ocr_engine`, validation errors) |
| ~~**P2**~~ **done (engine + hook)** | Rust `pdf-ocr-ppocr*` features + `PdfParser::with_ocr_engine`. Still open: CLI `--ocr-backend`. | `pdf-ocr-ppocr` compiles; `ocr.ppocr.device` is checked against the compiled accelerator; an opt-in test (`CHUNKR_PPOCR_E2E=<page.png>`) runs the real ONNX engine over a rasterised page and recovers the page's text — 0.20 s warm in release, models cached in `$OAR_HOME` (6.2 MB for `tiny`), which is also why `mode="auto"` builds the engine only after the gate selects pages. Upstream detail: `oar-ocr-core` 0.8.1 only builds against `ort` rc.12, so the feature carries a documented `ort = "=2.0.0-rc.12"` version pin; PP-OCR also ignores `ocr.language` (the model + dictionary fix the language). |
| **P3** *in progress* | ~~page-level parser adapters~~ **done**: `chunkr_pdf.parsers` ships `docling` (docling-serve `/v1/convert/file`, structured `json_content` grouped by `prov.page_no`), `mistral` (`/v1/ocr`, page markdown) and `vlm` (any OpenAI-compatible vision endpoint, pages rasterised through liteparse screenshots), plus a markdown→blocks converter so structured output and raw passthrough both work. Still open: word-level cloud OCR adapters (Azure/Textract/DocAI). | mock-service tests cover request shape, page grouping, level preservation, raw-markdown passthrough, HTTP errors and unreachability; the default VLM renderer is exercised against a real PDF. Live services are **not** exercised (they need credentials/containers), which the README states. |
| **P4** | capability metadata + `backend="auto"` routing (language/device/cost hints). ~~OCR quality benchmark~~ **done**: `benchmarks/ocr_accuracy.py` rasterises fixture pages, OCRs them, and reports `char_sim`/`token_recall`/latency per engine against the page's own text layer (numbers in `benchmarks/README.md`). | benchmark script + README matrix |

Existing fixtures are digital PDFs, so P1 needs one **scanned** fixture
(rasterize a page to PNG via `extract_screenshots`, wrap into a 1-2 page PDF
with a dev-only dep) checked in next to `finance.pdf`.

---

## 6. Decisions needed

1. **Naming**: `ocr.backend` vs `ocr.engine` (upstream calls it an engine). The
   registry would be `register_ocr_backend` either way.
2. **P1 scope**: ship the Python loopback proxy (my recommendation — it is what
   makes "any OCR engine" true for Python users), or only expose the existing
   HTTP-server route and document the reference servers?
3. **Native PP-OCR**: does the Rust feature set (`pdf-ocr-ppocr` +
   6 acceleration features, ONNX Runtime download at build/first run) earn its
   place, or should PP-OCR stay a Python/HTTP concern?
4. **`mode="auto"` semantics**: ~~change `auto` to the reason-gated behaviour
   (fixes a silent "auto == always"), or add `mode="adaptive"` and leave `auto`
   alone?~~ **Decided**: `auto` is reason-gated, `always`/`server` unchanged.
   The default gate is `scanned`/`no-text`/`garbled` only — `sparse-text`,
   `embedded-images` and `vector-text` fired on healthy digital pages on both
   fixtures. Note the reason strings arrive hyphenated (`sparse-text`) while
   Tesseract-era docs may say `sparse_text`; both normalise to the same signal.
5. **Adapters to ship in P1**: RapidOCR (lightest, recommended default for a
   real engine) + PaddleOCR (best CJK) + EasyOCR, or start with one?
6. **Extras layout**: per-engine extras inside `chunkr-pdf`
   (`chunkr-pdf[ocr-paddle]`, `[ocr-rapid]`, …) — confirm the naming scheme.
7. **Definition of done for P1**: which fixture set (scanned + mixed-language?)
   and what accuracy bar (e.g. ≥99% of reference text on a clean scan) counts as
   passing.

---

## Sources

- liteparse OCR API spec — `https://github.com/run-llama/liteparse/blob/main/OCR_API_SPEC.md`
- liteparse reference servers — `https://github.com/run-llama/liteparse/tree/main/ocr` (`easyocr`, `paddleocr`, `suryaocr`)
- liteparse crate source examined locally: `src/ocr/{mod,tesseract,oar,http_simple}.rs`, `src/parser.rs`, `README.md` (Custom OCR Engine, Document Complexity)
- `oar-ocr` crate metadata (Apache-2.0, ONNX, accel features): `https://crates.io/crates/oar-ocr`
- PaddleOCR 3.x quick start / PP-StructureV3: `https://www.paddleocr.ai/latest/en/quick_start.html`, `https://www.paddleocr.ai/latest/en/version3.x/pipeline_usage/PP-StructureV3.html`
- RapidOCR: `https://github.com/RapidAI/RapidOCR` (Apache-2.0, ONNX conversions of PP-OCR models)
- Surya: `https://github.com/datalab-to/surya` (code Apache-2.0); weights `datalab-to/surya-ocr-2` are OpenRAIL licensed
- docling OCR options: `https://docling-project.github.io/docling/concepts/OCR/`; `docling-serve` API: `https://pypi.org/project/docling-serve/`
- MinerU: `https://github.com/opendatalab/MinerU` (AGPL-style license); marker: `https://github.com/datalab-to/marker` (Apache-2.0); olmOCR: `https://github.com/allenai/olmocr` (Apache-2.0)
- Mistral OCR: `https://docs.mistral.ai/studio/document-processing/basic_ocr`, `https://mistral.ai/news/ocr-4/`
- License data pulled from the GitHub license API for each repository (see the tables above; dots.ocr has no detected license — treat as unverified)
