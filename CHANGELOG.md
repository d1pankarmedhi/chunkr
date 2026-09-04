# Changelog

All notable changes to `chunkr` (Rust crate) and `chunkr-rs` (PyPI package) are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
