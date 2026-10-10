<div align="center">
<h1>chunkr</h1>
<h3>⚡ Fast document &amp; text chunking for LLMs, agents and RAG</h3>

[![PyPI](https://img.shields.io/pypi/v/chunkr-rs.svg)](https://pypi.org/project/chunkr-rs/)
[![PyPI - Python Version](https://img.shields.io/pypi/pyversions/chunkr-rs.svg)](https://pypi.org/project/chunkr-rs/)
[![Crates.io](https://img.shields.io/crates/v/chunkr.svg)](https://crates.io/crates/chunkr)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

</div>

Rust chunking engine with native Python bindings: **2,000+ MB/s on prose, 3,200+ MB/s on code**, 2.9–33x faster than LangChain, Chonkie, `semchunk` and Rust `text-splitter` at matched chunk sizes. 20+ strategies, zero-copy slices, Rayon multi-core, no GIL.

| Install | Import |
| :--- | :--- |
| `pip install chunkr-rs` | `import chunkr` |
| `cargo add chunkr` | `use chunkr::prelude::*;` |
| `npm install chunkr-wasm` | `import { RecursiveChunker } from "chunkr-wasm"` |

Python wheels cover Linux, Windows and macOS (Intel + Apple Silicon), Python 3.8–3.13 — no Rust toolchain needed.

## Install

```bash
pip install chunkr-rs              # text & document chunking
pip install "chunkr-rs[pdf]"       # + layout-aware PDF parsing (headings, tables, OCR)

cargo add chunkr                   # Rust crate
cargo add chunkr --features pdf    # + liteparse PDF backend (features: pdf, pdf-ocr, pdf-ocr-ppocr)

npm install chunkr-wasm            # browsers, Cloudflare Workers, Node
```

## Quickstart (Python)

```python
import chunkr

# Prose, logs, docs — the default choice
docs = chunkr.RecursiveChunker(chunk_size=1000, overlap=120).chunk(text)

# Exact token bounds (OpenAI BPE, or any Hugging Face tokenizer.json)
docs = chunkr.TokenChunker(chunk_size=512, overlap=50, encoding="cl100k_base").chunk(text)
hf   = chunkr.HFTokenChunker.from_file("tokenizer.json", chunk_size=512, overlap=50)

# Structure-aware
md    = chunkr.MarkdownChunker(chunk_size=1000, overlap=100).chunk(markdown)  # # breadcrumbs
table = chunkr.TableChunker(rows_per_chunk=10).chunk(csv_text)  # repeats headers
code  = chunkr.AstCodeChunker(language="python").chunk(source)  # tree-sitter boundaries

# Retrieval patterns
pairs = chunkr.HierarchicalChunker(parent_size=1000, child_size=200).chunk_hierarchical(text)
late  = chunkr.LateChunker(chunk_size=300, overlap=30).chunk(text)   # full-doc context
stream = chunkr.StreamChunker(chunk_size=1000, overlap=150).chunk_text(huge_text)  # constant memory

# PDFs, directories, batches, bridges
pages  = chunkr.PDFLoader().load_pages("report.pdf")
chunks = chunkr.RecursiveChunker(1000, 120).chunk_documents(pages)
corpus = chunkr.DirectoryLoader(extensions=["pdf", "md", "py"]).load_and_chunk("docs/")
many   = chunkr.RecursiveChunker(1000, 120).par_chunk_documents(documents)   # Rayon
chain  = chunkr.to_langchain(docs)          # also to_llamaindex, to_dict_list

# Quality pass: filter, dedupe, pack, id
clean = (chunkr.ChunkPipeline()
         .filter_min_chars(30).deduplicate(exact=True)
         .pack(max_characters=1200).enrich(id_prefix="kb_")
         .process(docs))
```

## Which chunker?

| Document / goal | Chunker |
| :--- | :--- |
| Prose, articles, general text | `RecursiveChunker` (defaults tuned for retrieval recall + purity) |
| Exact token budgets (GPT, embeddings) | `TokenChunker`, `HFTokenChunker` |
| Small-to-big retrieval | `HierarchicalChunker` |
| Embeddings with document context | `LateChunker` (`pool_embeddings`) |
| Markdown, docs, specs | `MarkdownChunker` (header breadcrumbs, inherits sub-chunking) |
| CSV/TSV/markdown tables | `TableChunker` (repeats headers on every chunk) |
| Code | `CodeChunker` (fast) or `AstCodeChunker` (AST-correct, 2 languages) |
| Claims, contracts | `PropositionChunker` |
| Query-time dynamic sizing | `QueryAwareChunker` |
| Multi-GB files, stdin | `StreamChunker` |
| Production cleanup and token packing | `ChunkPipeline`, `ChunkPacker` |

Also available: `SentenceChunker`, `ParagraphChunker`, `SemanticChunker`, `ContextualChunker`, `AgenticChunker`, `JsonChunker`, `HtmlChunker`, `CharacterChunker`, `WordChunker`. Full API: [docs](docs/index.html) · [examples](examples/).

## PDF parsing and OCR — `chunkr-rs[pdf]`

The base package ships a lean `lopdf` extractor: the fastest text-only path (~2,900 pgs/s), no layout, and on PDFs whose fonts are Identity-H encoded — common in modern exports — it can only emit `?Identity-H Unimplemented?` text. The optional extension adds layout-aware parsing via liteparse and maps it into normal `Document`s, so sanitizing, granularity and metadata rules are identical in Python and Rust:

```bash
pip install "chunkr-rs[pdf]"                    # or: pip install chunkr-pdf
pip install "chunkr-pdf[ocr-rapid]"             # + an OCR engine (also: ocr-paddle, ocr-easyocr, ocr-surya)
```

```python
from chunkr_pdf import PDFParser

parser = PDFParser(preset="structure")           # retrieval (default) | faithful | structure
pages  = parser.load_pages("report.pdf")        # one Document per page; blocks, headings, tables
raw    = PDFParser(preset="faithful", sanitize={"enabled": False}).load("report.pdf")  # untouched
```

Everything is configurable: `output` (text/markdown/both), `granularity` (page/block/document), `extract` (tables, headings, lists, figures, links, images, annotations, structure tree…), `sanitize` (table/heading rules, glyph repair, junk guard), `scope`, `on_error`. Unknown keys raise instead of being ignored. `chunkr.PDFLoader()` picks the backend up automatically (`backend="auto"`).

OCR is pluggable and off by default. `ocr.mode="auto"` OCRs only pages whose text layer is broken; `ocr.backend` selects the engine:

| `ocr.backend` | Engine | Install |
| :--- | :--- | :--- |
| `tesseract` | liteparse's bundled engine | nothing |
| `server` | any service speaking the liteparse OCR API (`POST /ocr`) | run the service |
| `rapidocr` | RapidOCR / PP-OCRv6 ONNX, CPU-friendly | `chunkr-pdf[ocr-rapid]` |
| `paddleocr` | PaddleOCR 3.x, best CJK | `chunkr-pdf[ocr-paddle]` |
| `easyocr` | EasyOCR, 80+ languages | `chunkr-pdf[ocr-easyocr]` |
| `surya` | Surya OCR 2 (VLM) | `chunkr-pdf[ocr-surya]` + llama.cpp/vLLM |
| your callable | `chunkr.register_ocr_backend("name", engine)` | — |

Rust users get the same engines without Python: `features = ["pdf-ocr"]` (Tesseract), `["pdf-ocr-ppocr"]` (in-process ONNX PP-OCR, models auto-downloaded; `-coreml`/`-cuda`/`-directml`/`-openvino`/`-tensorrt`/`-webgpu` accelerators), any HTTP OCR server via `ocr.server_url`, and `PdfParser::with_ocr_engine()` for custom engines.

Whole-document services that return page markdown instead of word boxes plug in as parser backends — `docling`, `mistral` and any OpenAI-compatible VLM ship in `chunkr_pdf.parsers`:

```python
chunkr.register_pdf_backend("docling", chunkr_pdf.parsers.docling("http://localhost:5001"))
documents = chunkr.PDFLoader(backend="docling").load("report.pdf")
```

## JavaScript / wasm

```js
import { RecursiveChunker } from "chunkr-wasm";
const chunks = new RecursiveChunker(1000, 120).chunk(text);
```

Works in browsers, Cloudflare Workers and Node; PDFs can be chunked in memory at the edge. See [wasm/](wasm/).

## CLI

```bash
cargo install chunkr                        # installs the `chunkr` binary
chunkr notes.md --strategy recursive --chunk-size 1000 --overlap 120 --format jsonl
chunkr docs/ --strategy dir --out-file chunks.jsonl
chunkr report.pdf --format text             # PDFs read directly
```

## Benchmarks

Every number is reproducible with [`benchmarks/`](benchmarks/README.md) (raw samples in `benchmarks/results/`). Apple M4 (10 threads), Python 3.12, byte-identical corpora, matched chunk sizes, chunker construction excluded from timing; chunking figures are medians of 15 runs, PDF figures medians of 3-5 runs.

**Chunking throughput** — MB/s, higher is better, `—` = no equivalent splitter.

| Library | Recursive 1 MB | Fixed width 1 MB | Markdown 500 KB | Python 200 KB | BPE tokens 200 KB |
| :--- | ---: | ---: | ---: | ---: | ---: |
| **chunkr** | **2,219** | **750** | **819** | **3,232** | 38 |
| LangChain | 769 | 1.7 | 67 | 622 | 43 |
| Chonkie | 223 | 22 | — | — | **151** |
| text-splitter | 164 | — | 40 | 5.7 | 7.2 |
| semchunk | 42 | — | — | — | — |
| LlamaIndex | 10 | — | 19 | — | 2.0 |

On recursive prose that is **2.9× LangChain, 10× Chonkie, 53× semchunk, 222× LlamaIndex**; on fixed-width it is 34× the next best. Multi-core batch chunking (`par_chunk_texts`) reaches 3,224 MB/s (4.8× LangChain's loop), and a cold chunker costs 22 ms (LangChain 43 ms, Chonkie 457 ms). The one strategy where chunkr is *not* fastest is BPE token chunking — 38 MB/s against Chonkie's 151, because tiktoken encoding dominates the run — so prefer `RecursiveChunker` unless you need exact token bounds.

**PDF extraction speed** (ms and pgs/s; median of 5 runs, `python benchmarks/bench_pdf.py --big`)

| Extractor | 10-page sample | 2,066-page textbook (19.9 MB) |
| :--- | :--- | :--- |
| **chunkr fast (`lopdf`)** — no extra installed | **1.02 ms · 9,773 pgs/s** | **703 ms · 2,937 pgs/s** |
| **chunkr `[pdf]`** (liteparse) — layout-aware | 10.3 ms · 972 pgs/s | 2,264 ms · 913 pgs/s |
| PyMuPDF (`fitz`) | 11.2 ms · 884 pgs/s | 2,492 ms · 829 pgs/s |
| pypdf (pure Python) | 23.6 ms · 424 pgs/s | 11,700 ms · 177 pgs/s |

The fast path is 17-23× pypdf and 3.5-11× PyMuPDF *when it can read the document*. The extension is slower per page but still ahead of both: 2.3× pypdf on the small sample, 5.2× pypdf on the textbook, and 1.1× PyMuPDF on both. End to end (extract + recursive chunk) the textbook takes 760 ms on the fast path and 2,285 ms with the extension, against 2,519 ms for PyMuPDF + LangChain and 11,783 ms for pypdf + LangChain.

**PDF extraction quality** — what actually reaches your chunks (2,066-page textbook, 120 sampled pages, `benchmarks/pdf_quality.py`)

| Extractor | Pages returned | Words recovered | Precision | `?` chars | Most frequent word | Structure found (whole document) |
| :--- | ---: | ---: | ---: | ---: | :--- | :--- |
| **chunkr `[pdf]`** (liteparse) | 2,033 of 2,066 | **99.6%** | 99.3% | 0.0% | `the` (4%) | 9,915 headings, 580 tables |
| PyMuPDF (`fitz`) | 2,066 | 99.9% | 100.0% | 0.6% | `the` (4%) | — |
| pypdf | 2,066 | 99.5% | 98.3% | 0.6% | `the` (4%) | — |
| **chunkr fast (`lopdf`)** | 2,066 | **0.8%** | 42.7% | 7.6% | `identity-h` (50%) | — |

This is the speed/quality tradeoff in one table. On the textbook the fonts are Identity-H encoded, so the fast path cannot map glyphs and emits `?Identity-H Unimplemented?` instead of text: half of all its tokens are that one string and it recovers 0.8% of the document's words, so chunks built from it are mostly noise for retrieval. The extension recovers **99.6%** of them (words at least two independent extractors agree on) with no `?` noise, matching PyMuPDF and pypdf, and adds the 9,915 headings, 580 tables and 2,127 list items that become chunk `header_path` metadata and keep tables out of prose chunks. On a clean single-column PDF (the 10-page sample) the fast path is fine — 98.6% of words, no `?` characters — so it remains the right default when the extra is not installed.

The `pages` column is a page count, not a word count: the extension returns 2,033 `Document`s for this book because 33 of its 2,066 pages have no text at all — verified against pypdf and PyMuPDF, which extract nothing from any of them, and none of them contain images either. Every `Document` carries its real `page_number`, so page alignment with the source stays exact.

**Retrieval quality** — Chroma's token-level benchmark (472 questions); `prec_Ω` is chunk purity at perfect recall.

| Library | Recall | Purity (prec_Ω) | Avg chunk chars |
| :--- | ---: | ---: | ---: |
| **chunkr `RecursiveChunker` (defaults)** | **0.792** | 0.255 | 854 |
| LangChain | 0.762 | 0.251 | 745 |
| text-splitter | 0.762 | 0.262 | 790 |
| Chonkie | 0.752 | **0.292** | 701 |
| semchunk | 0.736 | 0.260 | 644 |
| LlamaIndex | 0.654 | 0.054 | 4,136 |

Purity tracks chunk length at least as much as boundary quality, which is why the libraries emitting smaller chunks score higher here. Interpolated to *the same* chunk length (500-900 chars) chunkr's sentence-aware defaults lead on both recall and purity at every budget. OCR accuracy per engine: [`benchmarks/README.md`](benchmarks/README.md#ocr-accuracy-ocr_accuracypy).

## FAQ

- **Python 3.13 / 3.14?** Wheels cover 3.8–3.13; the `[pdf]` extra needs 3.10+.
- **Threads or processes?** `par_chunk_documents`/`par_chunk_texts` use Rayon and release the GIL; chunkers are cheap to construct and reusable.
- **Custom separators or tokenizer?** `RecursiveChunker(separators=[...])`, `HFTokenChunker.from_file(...)`; `chunkr.SENTENCE_SEPARATORS` is the retrieval-tuned default hierarchy.
- **Metadata?** `Document(content, metadata)`; chunkers preserve input metadata and add chunker-specific keys.

## Contributing

Issues and PRs welcome — see [`PUBLISHING.md`](PUBLISHING.md) for the release process and run `cargo test`, `pytest tests`, `pytest packages/chunkr-pdf/tests` before opening one.

## License

MIT.
