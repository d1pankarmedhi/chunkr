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

Off by default. `"auto"`/`"always"` use liteparse's bundled Tesseract;
`"server"` posts page images to an external OCR service (per liteparse's OCR HTTP
API) which is faster and usually more accurate for tables.

```python
parser = PDFParser(ocr={"mode": "server", "server_url": "http://localhost:8828/ocr"})
documents = PDFParser(ocr={"mode": "auto"}).load_pages("scanned.pdf")
```

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

## License

MIT. Bundled transitively: liteparse (Apache-2.0) and PDFium (BSD-3-Clause).
