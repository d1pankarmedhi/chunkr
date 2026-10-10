# chunkr-pdf

High-fidelity PDF parsing for [chunkr](https://github.com/d1pankarmedhi/chunkr).

`chunkr-rs` ships a lean, `lopdf`-based PDF loader. This package is the optional
extension that swaps in layout-aware extraction — reading order, headings,
lists, tables, page labels, figures, complexity signals and OCR — powered by
[liteparse](https://developers.llamaindex.ai/liteparse/). Extraction happens
here; every mapping and sanitizing rule still runs inside chunkr's Rust core, so
`chunkr-rs[pdf]` and the native Rust `pdf` feature behave identically.

```bash
pip install "chunkr-rs[pdf]"      # or: pip install chunkr-pdf
```

## Quick start

```python
import chunkr
from chunkr_pdf import PDFParser

parser = PDFParser()                      # retrieval-shaped defaults
documents = parser.load_pages("report.pdf")   # one chunkr.Document per page
chunks = chunkr.MarkdownChunker(1000, 120).chunk_documents(documents)
```

`chunkr.PDFLoader()` picks the backend up automatically once the extra is
installed (`backend="auto"` → liteparse when available, else the fast extractor):

```python
loader = chunkr.PDFLoader()                    # uses liteparse
loader = chunkr.PDFLoader(backend="fast")      # forces lopdf
loader = chunkr.PDFLoader(config=chunkr.ParserConfig("structure"))
```

## Configuration

Anything chunkr's parser config exposes is settable here; nothing is hidden and
nothing is forced. `PDFParser(preset=...)` fills in a base and every nested group
can be overridden.

| Preset | What it does |
| --- | --- |
| `retrieval` (default) | Sanitized structure, page documents, tables kept only when they look like tables |
| `faithful` | Backend output untouched (`sanitize.enabled=False`, raw markdown, raw tables) |
| `structure` | Markdown + one document per block + complexity signals |

```python
parser = PDFParser(
    preset="structure",
    output="markdown",              # "text" | "markdown" | "both"
    granularity="block",            # "page" | "block" | "document"
    keep_blocks=True,               # attach typed blocks to metadata
    include_bboxes=True,
    include_page_labels=True,
    include_complexity=False,
    extract={
        "headings": True,
        "tables": "sanitized",      # "sanitized" | "raw" | "off"
        "lists": True,
        "figures": "placeholder",   # "placeholder" | "skip" | "link"
        "links": True,
        "images": False,
        "form_fields": False,
        "annotations": False,
        "structure_tree": False,
        "vector_graphics": False,
        "text_metadata": False,
        "screenshots": False,
    },
    sanitize={
        "enabled": True,
        "tables": {
            "require_header": True,
            "header_missing": "demote",   # "demote" | "synthesize" | "keep"
            "max_words_per_cell": 3,
            "prose_cell_ratio": 0.33,
            "min_rows": 1,
            "min_columns": 2,
            "demote_to": "paragraph",     # "paragraph" | "list" | "drop"
        },
        "headings": {"max_len": 80, "drop_truncated": True, "levels": "auto"},
        "glyphs": "report",               # "repair" | "report" | "off"
        "junk_guard": "flag",             # "flag" | "fallback_fast" | "off"
    },
    ocr={
        "mode": "off",                    # "off" | "auto" | "always" | "server"
        "language": "eng",
        "server_url": None,
        "headers": [],
        "tessdata_path": None,
        "num_workers": 0,
        "failure_fatal": False,
    },
    scope={
        "max_pages": None,                # None = every page
        "target_pages": None,             # e.g. "1-5,10"
        "password": None,
        "dpi": 150.0,
        "preserve_very_small_text": False,
        "skip_diagonal_text": False,
        "keep_headers_footers": False,
        "quiet": True,
    },
    on_error="fail",                      # "fail" | "page_error"
)
```

Unknown keys and unknown enum values raise at construction time, so typos never
silently change behavior.

## API

| Call | Result |
| --- | --- |
| `load(source)` / `documents(source)` | `list[chunkr.Document]` honoring `granularity` |
| `load_pages(source)` | one document per page with `page_number` / `page_label` |
| `load_document(source)` | single document with every page |
| `text(source)` | plain text |
| `blocks(source)` | every layout block with `page_number`, `kind`, `level`, `bbox` |
| `stream(source, batch_size=25)` | iterator of document lists (bounded memory) |
| `is_complex(source)` | per-page `needs_ocr` / layout signals, no full parse |
| `payload(source)` | raw backend payload JSON, before chunkr maps it |
| `config` / `to_parser_config()` | resolved dict / `chunkr.ParserConfig` |

`source` is a path or `bytes`.

Block metadata in every produced document: `block_type`, `heading_level`,
`list_marker`, `ordered`, `bold`, `bbox`, and for tables `table_header` /
`table_rows`. Page metadata adds `page_number`, `page_label`, `pdf_complexity`,
plus `sanitize_report` whenever the sanitizer changed something.

## OCR

Off by default. `ocr.mode` decides *when* to OCR (`off`, `auto`, `always`,
`server`); `ocr.backend` decides *what* OCRs.

| `ocr.backend` | Engine | Install |
| --- | --- | --- |
| `tesseract` (default) | liteparse's bundled Tesseract | nothing |
| `server` | any service speaking the liteparse OCR API (`POST /ocr`, multipart `file` + `language`) | run the service |
| `rapidocr` | RapidOCR / PP-OCRv6 ONNX, CPU-friendly (verified end-to-end) | `pip install "chunkr-pdf[ocr-rapid]"` |
| `paddleocr` | PaddleOCR 3.x, best CJK (verified end-to-end) | `pip install "chunkr-pdf[ocr-paddle]"` |
| `easyocr` | EasyOCR, 80+ languages (verified end-to-end) | `pip install "chunkr-pdf[ocr-easyocr]"` |
| `surya` | Surya OCR 2 (VLM; needs a `llama.cpp`/`vLLM` inference backend, GPU advised) | `pip install "chunkr-pdf[ocr-surya]"` |
| anything you register | your own Python callable | — |

```python
parser = PDFParser(ocr={"mode": "always", "backend": "rapidocr"})
parser = PDFParser(ocr={"mode": "auto", "backend": "paddleocr", "language": "zh"})
parser = PDFParser(ocr={"mode": "server", "server_url": "http://localhost:8829/ocr"})
parser = PDFParser(ocr={"mode": "server", "server_url": "https://api.example/ocr",
                        "headers": [["Authorization", "Bearer …"]],
                        "hedge_delays_ms": [0, 250]})   # hedged duplicates
```

`mode="auto"` is a real per-page gate: the document is parsed once without OCR,
`chunkr.pages_needing_ocr` selects the pages whose text layer is broken, and only
those are re-parsed with the engine. The default gate is `scanned`, `no-text` and
`garbled`; `sparse-text`, `embedded-images` and `vector-text` are deliberately
excluded because digital slides carry them on healthy pages. Add your own with
`ocr.auto_reasons` and a minimum length with `ocr.auto_min_chars`:

```python
PDFParser(ocr={"mode": "auto", "backend": "rapidocr",
               "auto_reasons": ["scanned", "garbled", "sparse-text"],
               "auto_min_chars": 50})
```

### Your own engine

Register any callable (or object with `.recognize`) and use it by name. chunkr
serves it to liteparse through a loopback HTTP server, so reading order, rotation
handling and text-layer merging stay inside liteparse:

```python
import chunkr

def my_engine(image_png: bytes, *, language: str = "en", options: dict | None = None):
    # ... run your model over the page image ...
    return [{"text": "Detected line", "bbox": [10, 20, 200, 40], "confidence": 0.98},
            # optional 4-point polygon (TL, TR, BR, BL) for rotated text
            {"text": "Sidebar", "bbox": [5, 5, 30, 90], "confidence": 0.9,
             "polygon": [[5, 5], [30, 5], [30, 90], [5, 90]]}]

chunkr.register_ocr_backend("house-model", my_engine)
documents = PDFParser(ocr={"mode": "always", "backend": "house-model"}).load_pages("scan.pdf")
```

Return a plain string for a single full-page block (layout is lost — prefer
boxes). Cloud APIs work the same way: wrap the request in a callable that maps
the vendor's response to `text`/`bbox`/`confidence`.

Adapter notes: RapidOCR/PaddleOCR/EasyOCR are exercised in CI against a
rasterised fixture page (they recover the page's known text through the proxy).
Surya's adapter matches its released API (`RecognitionPredictor` over
`SuryaInferenceManager`, block HTML + polygons) but its inference runs in a
separate backend, so it is not exercised by the test suite — it also sets the
upstream-recommended env defaults (`SURYA_INFERENCE_BACKEND=llamacpp`,
`LLAMA_CPP_NGL=0`, `SURYA_GUIDED_LAYOUT=false`) before importing, and downloads
its GGUF weights on first use. These engines download models on first run
(PaddleOCR into `~/.paddlex`, EasyOCR into `~/.EasyOCR`, Surya next to its
inference backend).

Engine lifecycle: one engine instance per parser, started on first use and kept
warm (`ocr.plugin.warmup` loads it up front so a missing package fails
immediately), a lock serializes it unless `ocr.plugin.concurrency` is raised, and
`parser.close()` (or the context manager) stops the server. `parser.ocr_servers`
exposes the running servers for inspection.

## Options worth knowing

- `sanitize.enabled=False` is a true passthrough: backend markdown is returned
  byte for byte and nothing is demoted.
- `sanitize.tables.header_missing="synthesize"` promotes the first data row of a
  headerless table (useful for numeric grids whose header the backend missed).
- `sanitize.headings.levels="derive"` ranks heading block heights into `h1..h6`
  when a backend reports a single flat level.
- `sanitize.glyphs="repair"` rewrites ligature/CJK-substituted glyphs
  (`Arti昀椀cially` → `Artificially`); `"report"` only counts them.
- `sanitize.junk_guard="fallback_fast"` marks the result for a fast-backend retry
  when the text is full of unmapped-glyph markers.
- Native Rust users get the same gate and the same engines: `ocr.backend`,
  `ocr.auto_reasons`, `PdfParser::with_ocr_engine(Arc<dyn OcrEngine>)`, Tesseract
  via `pdf-ocr`, in-process ONNX PP-OCR via `pdf-ocr-ppocr` (plus `-coreml`,
  `-cuda`, `-directml`, `-openvino`, `-tensorrt`, `-webgpu` accelerators, with
  `ocr.ppocr.preset`/`models_dir`/`device`), and any HTTP OCR server via
  `ocr.server_url`. PP-OCR ignores `ocr.language`: its recognizer language is
  fixed by the model and character dictionary.

## License

MIT. Bundled transitively: liteparse (Apache-2.0) and PDFium (BSD-3-Clause).
