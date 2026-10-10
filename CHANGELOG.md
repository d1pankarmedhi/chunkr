# Changelog

All notable changes to `chunkr` (Rust crate) and `chunkr-rs` (PyPI package) are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added
- **Optional high-fidelity PDF parsing — `chunkr-rs[pdf]` / `chunkr-pdf`**: a separate extension package (and the `pdf` Cargo feature for Rust) that extracts PDFs with layout awareness — reading order, headings, lists, tables, figures, page labels, per-page complexity signals and optional OCR — then maps the result to `Document`s inside chunkr, so both paths share one implementation.
- **`chunkr::parser` module (base crate, no new dependencies)**: `ParserConfig` with presets (`retrieval`, `faithful`, `structure`) and every knob user-selectable (output `text`/`markdown`/`both`, granularity `page`/`block`/`document`, `extract` toggles, `sanitize` rules + thresholds, OCR mode, page scope, error policy), plus `pages_to_documents`, `render_markdown`, `sanitize_pages`, typed `PagePayload`/`BlockPayload` and `PdfParser` backend dispatch (`auto`/`fast`/`liteparse`).
- **Configurable sanitizer** for backend layout blocks: table rejection (`require_header`, `header_missing` `demote`/`synthesize`/`keep`, `max_words_per_cell`, `prose_cell_ratio`, `min_rows`, `min_columns`, `demote_to`) and heading rejection (`max_len`, `drop_truncated`, levels `auto`/`as_is`/`flat`/`derive`), glyph repair (`repair`/`report`/`off`) and a junk-glyph guard (`flag`/`fallback_fast`/`off`). Every change is reported in `sanitize_report` metadata; `sanitize.enabled = false` is a byte-for-byte passthrough.
- **Python plugin API**: `chunkr.ParserConfig`, `chunkr.pages_to_documents`, `chunkr.register_pdf_backend` / `unregister_pdf_backend` / `pdf_backends`, and `chunkr.PDFLoader(backend=..., config=...)` — `backend="auto"` uses an installed backend automatically, a name resolves a registered one, a callable is used inline.
- **`chunkr-pdf` package**: `PDFParser` with presets and full config passthrough, `load`/`load_pages`/`load_document`/`text`/`blocks`/`stream` (bounded-memory page batches)/`is_complex`/`payload`, and a `chunkr.pdf_backends` entry point so `pip install "chunkr-rs[pdf]"` is enough. Requires Python 3.10+ (bumped by the optional dependency, not by the base package).

- **Pluggable OCR backends**: `ocr.backend` picks the engine — `tesseract` (bundled), `ppocr` (reserved for the in-process ONNX build), `server` (any service speaking liteparse's OCR API), or the name of an engine registered with `chunkr.register_ocr_backend(name, engine)`. Python engines are served to the backend through a loopback HTTP server, so reading order, rotation handling and text-layer merging stay in one place; `chunkr_pdf` ships ready adapters for RapidOCR (`chunkr-pdf[ocr-rapid]`), PaddleOCR (`[ocr-paddle]`), EasyOCR (`[ocr-easyocr]`) and Surya (`[ocr-surya]`), plus `ocr.server_url`/`ocr.headers`/`ocr.hedge_delays_ms` for remote services. Rust embedders can inject any engine with `PdfParser::with_ocr_engine(Arc<dyn OcrEngine>)`.
- **`ocr.mode="auto"` is now a real per-page gate**: instead of OCRing every page, the document is parsed once without OCR, `chunkr.pages_needing_ocr`/`chunkr::parser::select_ocr_pages` selects the pages whose text layer is broken (`scanned`, `no-text`, `garbled` by default; `ocr.auto_reasons`/`ocr.auto_min_chars` to override), and only those are re-parsed with OCR and spliced back in. On a 16-page digital deck this performs zero OCR work; on the textbook fixture it OCRs only the pages that need it.
- **OCR config validation and language handling**: `ParserConfig::validate()` / `ParserConfig(...)` now reject unusable setups at construction (a `server` backend without `server_url`, an unknown PP-OCR preset or device, a zero DPI, a malformed `scope.target_pages`, `concurrency`/`timeout_ms` of 0) and `ocr_language`/`chunkr.ocr_language` convert between Tesseract (`eng`) and ISO 639-1 (`en`) codes per engine.

### Changed
- **Python package layout**: the native extension now ships as `chunkr._core` behind a thin `chunkr/__init__.py` that re-exports it and discovers PDF backends. `import chunkr` and every existing API are unchanged; `chunkr.pyi`/`py.typed` moved to `python/chunkr/`.
- **Cargo feature `pdf`** (and `pdf-ocr` for the bundled Tesseract engine) — off by default, non-wasm; enabling it adds the `liteparse` dependency.

### Fixed
- Nothing yet.

---

## [1.5.0] - 2026-10-07

### Changed
- **`RecursiveChunker` defaults are now tuned for retrieval accuracy**: the sentence-aware hierarchy (`SENTENCE_SEPARATORS`, paragraph -> line -> sentence -> word, also exposed as `chunkr.SENTENCE_SEPARATORS`) with overlap 120 instead of 200. On Chroma's token-level benchmark that is 0.792 recall and 0.255 `prec_Ω` versus 0.784 and 0.213 before — better on every metric at once, with 6% fewer chunks. `MarkdownChunker` inherits the same sub-chunking. Pass `separators=["\n\n", "\n", " ", ""]` and `overlap=200` for the previous boundaries.

### Fixed
- **`RecursiveChunker` could emit chunks larger than `chunk_size`**: the overlap carried into the next chunk was not re-checked against the cap, so chunks could reach `chunk_size + overlap` bytes (for example 1126 bytes with `chunk_size=1000` and `overlap=200`). The carried overlap is now trimmed until the next piece fits.

---

## [1.4.0] - 2026-09-05

### Added
- **Lenient batch loading**: `DirectoryLoader::load_files_lenient` and `load_and_chunk_lenient` (Rust + Python) return `(documents, errors)` instead of failing the whole batch on one bad file; `DirectoryLoader` skips symlinks and reports path-qualified IO errors.
- **Semantic context window**: `SemanticChunker::with_buffer_size` builder (Rust) and `buffer_size` parameter (Python, default `1`) — each sentence is embedded together with neighboring-sentence context for stabler breakpoints (`0` restores legacy isolated embedding).
- **`ChunkSpans`**: public type alias for the `(document, token-span)` pairs returned by `LateChunker::chunk_spans`, re-exported in the prelude.

### Changed
- `tiktoken-rs` 0.6 → 0.12: identical BPE tables, faster encoding (TokenChunker BPE throughput up, `to_vec` clone removed).

### Fixed & Improved (chunking efficiency + robustness)
- **Efficiency**:
  - `TokenChunker` now shares BPE tables via `Arc` — `clone()` and legacy `chunk_text()` no longer rebuild rank tables from embedded data.
  - `LateChunker` caches per-token decoded byte lengths by token id (distinct tokens << total tokens on natural text).
  - `RecursiveChunker` separator probing uses a single `memmem::Finder::find` scan per candidate; char fallback streams `char_indices` without a per-char index `Vec`.
  - `CharacterChunker` resolves byte offsets only at chunk boundaries (O(chunks) memory, two allocation-free passes).
  - `SentenceChunker` abbreviation guard is allocation-free and bounded to the last token (was `split_whitespace` + `to_lowercase` over the whole prefix per `.`).
  - `FastLexicalEmbedder` embeds sentences in parallel via Rayon and hashes words allocation-free; cosine distance is now true normalized cosine, safe for custom unnormalized embedders.
  - `StreamChunker` tracks a read offset with bulk compaction instead of `String::drain` per chunk (was O(n²) memmoves on multi-GB streams).
- **Robustness**:
  - `TokenChunker`, `HFTokenChunker`, `PropositionChunker`, `QueryAwareChunker` re-validate `chunk_size`/`overlap` in `chunk()` so post-construction mutation of public fields returns `InvalidChunkSize`/`InvalidOverlap` instead of underflow panic/hang.
  - `MarkdownChunker` honors `include_header_in_content = false` (previously a dead flag) and detects headers under CRLF line endings.
  - CLI sentence/paragraph overlap values are clamped instead of erroring on defaults; `Contextual`/`Hierarchical`/`AstCode` legacy-path configs preserved.
  - Research grounding: late-chunking flow follows Günther et al. (2024, arXiv:2409.04701) — full-text token stream + mean-pool over spans; delimiter fast-path rationale per memchr/SIMD backward-search analysis.

---

## [1.3.0] - 2026-09-04

### Added
- **WebAssembly Support (`wasm32-unknown-unknown`)**:
  - Full first-class compatibility for Browsers, Cloudflare Workers, Node.js, Deno, and Bun.
  - Pure Rust implementation on Wasm target with C-based tree-sitter gated to native platforms.
  - Sequential fallbacks on `wasm32` for Rayon parallel chunking (`par_chunk_documents`, `par_chunk_texts`, `par_enrich`).
- **WebAssembly Bindings (`chunkr-wasm`)**:
  - `wasm-bindgen` bindings for all chunking strategies: `RecursiveChunker`, `MarkdownChunker`, `TokenChunker` (OpenAI BPE `cl100k_base`, `o200k_base`, `p50k_base`, `r50k_base`), `CodeChunker`, `HtmlChunker`, `JsonChunker`, `TableChunker`, `SentenceChunker`, `ParagraphChunker`, `CharacterChunker`, `WordChunker`, `SemanticChunker`, `LateChunker`, `PropositionChunker`, `HierarchicalChunker`, `QueryAwareChunker`, and `StreamChunker`.
  - In-memory `PDFLoader`: `loadTextFromBytes`, `loadDocumentFromBytes`, and `loadPagesFromBytes` for zero-server in-memory PDF parsing and extraction directly at the edge.
  - `ChunkPipeline`: Composable post-chunking filtering, deduplication, packing, and SHA-256 metadata enrichment.
  - Native JavaScript objects serialization via `serde_wasm_bindgen::Serializer::json_compatible()`.
  - Top-level `chunk()` and `countTokens()` helper functions.
- **Universal NPM Package**:
  - Multi-target packaging in `wasm/`: `web` (Browsers & Cloudflare Workers), `bundler` (Vite, Webpack, Rollup), `nodejs` (Node.js CommonJS & ESM).
- **Examples**:
  - `examples/cloudflare-worker/`: Ready-to-deploy Cloudflare Worker demonstrating synchronous Wasm loading, text chunking, and in-memory binary PDF parsing.
  - `examples/browser/`: Interactive client-side HTML demo showing multi-strategy live chunking and performance benchmarks.
- **CI/CD Wasm Publishing**:
  - Automated Wasm compilation, testing, and NPM release jobs in `.github/workflows/publish.yml`.

---

## [1.2.1] - 2026-09-04

### Added
- **Automated GitHub Releases**: Integrated automated GitHub Release publishing with compiled platform wheels (`.whl`) and source distributions (`.tar.gz`) attached directly to releases.
- **Annotated Tag Title & Release Notes Extraction**: Workflow automatically extracts release titles and custom markdown notes directly from git annotated tag messages, with fallback to GitHub's auto-generated release notes.
- **Comprehensive Rustdoc Documentation**: Added crate-level (`src/lib.rs`) and module-level documentation across all chunker families (`src/chunker/mod.rs`), pipelines (`src/pipeline/mod.rs`), loaders (`src/loader/mod.rs`), and core structures (`src/structures/mod.rs`).
- **Standardized Changelog**: Added `CHANGELOG.md` tracking all release milestones.

### Changed
- Improved release guide in `PUBLISHING.md` with examples for annotated tags, release notes, and multi-platform publishing.

---

## [1.2.0] - 2026-09-04

### Added
- **StreamingChunker**: Constant-memory sliding-window chunking for processing multi-GB files, network sockets, and UNIX STDIN.
- **Ecosystem Bridges**: Zero-copy adapter functions `to_langchain`, `to_llamaindex`, `from_langchain`, and `from_llamaindex` for seamless integration into AI agent frameworks.
- **CLI Stream Ingestion**: `chunkr-cli` support for streaming large files and UNIX piping with `--strategy stream`.
- **Post-Chunking Pipeline**: Composable `ChunkPipeline` providing:
  - `ChunkFilter`: Threshold filtering by minimum/maximum character length, word count, and alphanumeric ratio.
  - `ChunkDeduplicator`: Exact content hashing deduplication.
  - `MetadataEnricher`: Deterministic SHA-256 chunk IDs, length metrics, and timestamps.
- **ChunkPacker**: Greedy bin-packing optimizer to merge small chunk fragments into token budget windows.
- **AstCodeChunker**: Tree-sitter AST syntax chunker splitting along function and class definitions for Rust and Python.
- **DirectoryLoader**: Recursive multi-threaded folder scanning with extension filtering and auto-routing to optimal chunkers.
- **HFTokenChunker**: Universal token-based chunking supporting any Hugging Face tokenizer (Llama 3, Mistral, Qwen, BERT, BGE).
- **TableChunker**: Structure-aware tabular chunking for Markdown, CSV, and TSV tables with automatic header duplication across chunks.
- **LateChunker**: Full-document context chunking with token span snapping and embedding mean-pooling.

---

## [1.1.0] - 2026-09-03

### Added
- **Multi-Core Parallelism**: Rayon-backed `par_chunk_documents` and `par_chunk_texts` executing parallel chunking across all available CPU cores.
- **PDF Document Loading**: `PDFLoader` with page-by-page extraction into structured `Document` instances.
- **HierarchicalChunker**: Multi-level tree generation and parent-child chunk pairings.
- **QueryAwareChunker**: Hotspot detection and adaptive chunk sizing around search queries.
- **AgenticChunker**: Discourse transition and topic segmentation.
- **SemanticChunker**: Distance threshold breakpoint clustering using sentence embeddings.
- **PropositionChunker**: Syntactic proposition decomposition into atomic factual claims.
- **ContextualChunker**: Anthropic-style situational document preface injection.

### Fixed
- Multi-byte UTF-8 boundary safety across all sliding window chunkers.
- Strict abbreviation and decimal guards in sentence splitting.

---

## [1.0.1] - 2026-09-02

### Added
- PyPI release configuration under package name `chunkr-rs`.
- Multi-platform GitHub Actions CI matrix (Linux x86_64/aarch64/armv7, Windows x64/x86, macOS x86_64/arm64).

---

## [1.0.0] - 2026-09-02

### Added
- Initial production release with core chunking strategies (`RecursiveChunker`, `TokenChunker`, `SentenceChunker`, `ParagraphChunker`, `MarkdownChunker`, `CodeChunker`, `JsonChunker`, `HtmlChunker`, `CharacterChunker`, `WordChunker`).
- PyO3 native Python extension bindings with `uv` and `maturin` build system support.
- Crates.io publication under `chunkr`.
