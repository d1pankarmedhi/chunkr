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

The base package ships a lean `lopdf` extractor (fastest text-only path, ~2,700 pgs/s; no layout, drops unmapped glyphs). The optional extension adds layout-aware parsing via liteparse and maps it into normal `Document`s, so sanitizing, granularity and metadata rules are identical in Python and Rust:

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

Median of 15 runs, Apple M4, 1 MB prose, matched chunk sizes. Full harness, corpora and caveats: [`benchmarks/README.md`](benchmarks/README.md).

| Case | Chunkr | LangChain | Chonkie | semchunk | text-splitter | LlamaIndex |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| Recursive 1000/200 | **2,219 MB/s** | 769 | 223 | 42 | 164 | 10 |
| Fixed char 1000/200 | **750 MB/s** | 1.7 | 22 | — | — | — |
| Markdown 1000/150 | **819 MB/s** | 67 | — | — | 40 | 19 |
| Python code 1500/200 | **3,232 MB/s** | 622 | — | — | 5.7 | — |
| BPE tokens 512/50 | 38 MB/s | 43 | **151** | — | 7.2 | 2.0 |

BPE is chunkr's weakest strategy (tiktoken encoding dominates; use `RecursiveChunker` unless exact token bounds matter). On Chroma's token-level retrieval benchmark, `RecursiveChunker` defaults reach **0.792 recall / 0.255 prec_Ω** — best recall and best chunk purity at every size from 500–900 chars when interpolated to equal chunk length. PDF: 9,300 pgs/s on a 10-page sample, 2,700 pgs/s on a 2,066-page textbook (15–23x PyPDF). OCR accuracy per engine: [`benchmarks/README.md`](benchmarks/README.md#ocr-accuracy-ocr_accuracypy).

## FAQ

- **Python 3.13 / 3.14?** Wheels cover 3.8–3.13; the `[pdf]` extra needs 3.10+.
- **Threads or processes?** `par_chunk_documents`/`par_chunk_texts` use Rayon and release the GIL; chunkers are cheap to construct and reusable.
- **Custom separators or tokenizer?** `RecursiveChunker(separators=[...])`, `HFTokenChunker.from_file(...)`; `chunkr.SENTENCE_SEPARATORS` is the retrieval-tuned default hierarchy.
- **Metadata?** `Document(content, metadata)`; chunkers preserve input metadata and add chunker-specific keys.

## Contributing

Issues and PRs welcome — see [`PUBLISHING.md`](PUBLISHING.md) for the release process and run `cargo test`, `pytest tests`, `pytest packages/chunkr-pdf/tests` before opening one.

## License

MIT.
