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
pip install "chunkr-rs[audio]" chunkr-audio   # + Whisper transcripts of audio files

cargo add chunkr                   # Rust crate
cargo add chunkr --features pdf    # + liteparse PDF backend (features: pdf, pdf-ocr, pdf-ocr-ppocr)
cargo add chunkr --features audio  # + pure-Rust audio decoding and speech timing

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
docs   = chunkr.load_documents("sales.xlsx") # any format: xlsx, docx, epub, eml, html, …
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
| Captions, meetings | `SubtitleChunker` (never splits a cue, tags `start_time_s`/`end_time_s`) |
| Production cleanup and token packing | `ChunkPipeline`, `ChunkPacker` |

Also available: `SentenceChunker`, `ParagraphChunker`, `SemanticChunker`, `ContextualChunker`, `AgenticChunker`, `JsonChunker`, `HtmlChunker`, `CharacterChunker`, `WordChunker`. Full API: [docs](docs/index.html) · [examples](examples/).

## Supported formats

`load_documents()` — from Rust, Python and the CLI — dispatches on the file extension and returns
the natural unit of each format:

| Family | Extensions | One `Document` per |
| :--- | :--- | :--- |
| Documents | `pdf` | page |
| Spreadsheets | `xlsx` `xlsm` `xlsb` `xls` `ods` `csv` `tsv` | worksheet (as a Markdown table) |
| Office | `docx` `docm` `pptx` `odt` | document / slide |
| Books & mail | `epub` `eml` `mbox` | chapter / message |
| Markup & notebooks | `html` `xml` `rtf` `ipynb` | document |
| Captions | `srt` `vtt` `webvtt` | cue |
| Text | `txt` `md` `log` `json` `jsonl` `ndjson`, any other extension | document |
| Archives | `zip` | member (dispatched by its own format) |
| Columnar (`pip install chunkr-tabular`) | `parquet` `feather` `arrow` `ipc` | batch of rows (as a Markdown table) |
| Audio (`--features audio`, `pip install chunkr-audio`) | `wav` `mp3` `flac` `ogg` `oga` `m4a` `m4b` `aac` | transcript segment (with `start_time_s`/`end_time_s`) |

```python
subs   = chunkr.load_documents("talk.vtt")       # one Document per cue, with time spans
windows = chunkr.SubtitleChunker(max_seconds=30).chunk_documents(subs)  # start_time_s/end_time_s

records = chunkr.JsonChunker().chunk(open("corpus.jsonl").read())  # whole records, start_record/end_record

docs = chunkr.load_documents("sales.xlsx")            # one Document per sheet
pages = chunkr.load_documents_from_bytes(data, "docx", source="report.docx")
chunkr.supported_extensions()                          # every extension the build handles
```

```bash
chunkr sales.xlsx --strategy recursive        # spreadsheets, office docs and EPUBs too
```

Extracted metadata rides along into every chunk (`sheet_name`, `slide_number`, `chapter_href`,
`page_number`, `source`, `format`, …), so retrieval can filter and cite where a chunk came from.
`DirectoryLoader` routes each file to the chunker its format deserves: spreadsheets and CSV to
`TableChunker` (headers repeat on every chunk), Markdown to `MarkdownChunker`, code to
`CodeChunker`, everything else to `RecursiveChunker`.

Audio is decoded in pure Rust and transcribed through a registered transcriber, so a recording chunks
into the time windows a person actually spoke — and every chunk cites its timestamp:

```python
docs = chunkr.load_documents("episode.mp3")     # one Document per transcript segment
chunks = chunkr.SubtitleChunker(max_seconds=60).chunk_documents(docs)  # start_time_s/end_time_s
```

`chunkr-audio` registers Whisper on install; anything that returns
`{"start", "end", "text", "speaker"?}` can be registered with
`chunkr.register_transcriber(name, fn)` — no Rust changes. Without a transcriber, audio fails with
instructions instead of guessing, and a file with no detectable speech fails before a model is
called. `CHUNKR_TRANSCRIBER=<name>` picks between several registered backends.

A zip is expanded rather than treated as one blob: every member is dispatched by its own
extension — a `.csv` inside it is chunked as a table with headers repeated, a `.md` as Markdown — and
each document carries `archive_entry` (its path inside the archive) plus `archive_entries` /
`archive_skipped` counts, so a chunk still cites the exact member it came from. Nested archives
expand two levels deep by default. `ArchiveLimits` guards the reads before they happen: 1000 entries,
512 MiB uncompressed, 100:1 per-entry compression ratio above a 1 MiB floor, depth 2 — a breach
reports which limit was hit instead of quietly eating memory:

```python
chunkr.load_documents("bundle.zip")                   # members, dispatched by format
chunkr.expand_zip?  # Rust: expand_zip(bytes, source, &ArchiveLimits::default(), 0)
```

Members that are not readable documents (images, binaries) are skipped and counted in
`archive_skipped`; a corrupt archive or a breached limit is an error naming the entry.

Payloads that do not name themselves are not guessed at: with an empty or unknown extension chunkr
identifies PDF and RTF by prefix and zip containers (`docx`, `xlsx`, `pptx`, `odt`, `epub`) by their
entries, then falls back to text — while a known extension always wins over the content.

Encoding rules are explicit: byte-order marks are always honoured, `.txt`/`.log`/`.csv`/`.tsv`
also decode legacy single-byte encodings, and formats that are UTF-8 by convention (`md`, `json`,
`xml`, source code) report undecodable bytes as errors instead of indexing mojibake.

Caption tracks are the first *time-indexed* format: `SubtitleChunker` merges cues into windows that
never split a cue, and every window carries `start_time_s`, `end_time_s`, `duration_s`, `cue_count`
and (for WebVTT voice tags) `speaker`, so retrieval can cite an exact moment:

```python
for window in chunkr.SubtitleChunker(max_seconds=30).chunk_documents(chunkr.load_documents("talk.vtt")):
    print(window.metadata["start_time_s"], window.metadata["end_time_s"], window.content[:60])
```

Line-delimited JSON is the *record* unit: `JsonChunker` batches whole records and tags each chunk
with `path` (`$[0..250]`), `start_record` and `end_record`, so a chunk cites the exact record range it
covers. Records are never split — a record larger than the budget is descended into by field instead,
and the chunk keeps its record index. Malformed lines make the whole file fall back to plain text
chunking rather than producing plausible-looking record spans.

Columnar files are the *table* unit, added by an extension package rather than by core: install
[`chunkr-tabular`](packages/chunkr-tabular) and Parquet/Arrow files dispatch like any other format,
with `row_offset`/`row_count` recording which rows a chunk covers.

```bash
pip install chunkr-tabular
```

```python
import chunkr, chunkr_tabular          # registers the loaders on import
chunkr.load_documents("events.parquet") # one Document per batch, Markdown table + row spans
```

Extensions register extractors, not chunkers: chunkr renders their tables, escapes cells, attaches
metadata and keeps `DirectoryLoader` routing, so an extension's format behaves exactly like a
built-in one. `chunkr.register_table_loader(name, extensions, loader)` is the same hook in plain
Python — no compilation needed to add a columnar or custom tabular format:

```python
def load(source, extension, source_name=None):
    yield {"columns": ["city", "pop"], "rows": [["Paris", "2.1M"]], "name": "rows_0_0", "row_offset": 0}

chunkr.register_table_loader("mine", ["mytab"], load)
```

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
chunkr report.pdf --format text             # PDFs, spreadsheets, office docs and EPUBs
```

Every chunk keeps the unit it came from, and text output names it:

```
=== report.xlsx · sheet Budget 1/2 ===
--- [CHUNK 0] ---
| Item | Rate | Revenue |
| --- | --- | --- |
| Widget | 15% | $1,234.50 |
```

## Benchmarks

Every number is reproducible with [`benchmarks/`](benchmarks/README.md) (raw samples in `benchmarks/results/`). Apple M4 (10 threads), Python 3.12, byte-identical corpora, matched chunk sizes, chunker construction excluded from timing; chunking figures are medians of 15 runs, PDF figures medians of 5 runs.

**Chunking throughput** — MB/s, higher is better, `—` = no equivalent splitter.

| Library | Recursive 1 MB | Fixed width 1 MB | Markdown 500 KB | Python 200 KB | BPE tokens 200 KB |
| :--- | ---: | ---: | ---: | ---: | ---: |
| **chunkr** | **2,245** | **760** | **861** | **3,265** | 37 |
| LangChain | 747 | 1.5 | 67 | 624 | 43 |
| Chonkie | 222 | 23 | — | — | **153** |
| text-splitter | 172 | — | 40 | 5.7 | 7.3 |
| semchunk | 41 | — | — | — | — |
| LlamaIndex | 9.8 | — | 19 | — | 2.0 |

How many times faster that is, per competitor (chunkr ÷ competitor, same runs):

| Library | Recursive 1 MB | Fixed width 1 MB | Markdown 500 KB | Python 200 KB | BPE tokens 200 KB |
| :--- | ---: | ---: | ---: | ---: | ---: |
| LangChain | 3.0× | 507× | 12.9× | 5.2× | 0.86× |
| Chonkie | 10.1× | 33.0× | — | — | 0.24× |
| text-splitter | 13.1× | — | 21.5× | 573× | 5.1× |
| semchunk | 54.8× | — | — | — | — |
| LlamaIndex | 229× | — | 45.3× | — | 18.5× |

Read it as "chunkr is N× faster"; figures below 1× are losses, left visible. LangChain's 507× on fixed-width is its character-by-character scanner, and the Python column sets regex `CodeChunker` against tree-sitter `CodeSplitter` — same boundaries, far more machinery. Multi-core batch chunking (`par_chunk_texts`) is 3,083 MB/s: 4.6× LangChain's loop and 14.6× Chonkie's. A cold chunker costs 22 ms — **2.2× faster** to first chunk than LangChain's and **15.7×** Chonkie's. The one strategy where chunkr does *not* win is BPE token chunking, where tiktoken encoding dominates the run: prefer `RecursiveChunker` unless you need exact token bounds.

**PDF extraction speed** (median of 5 runs, `python benchmarks/bench_pdf.py --big`; bracketed figures are speedups over the two Python extractors)

| Extractor | 10-page sample | 2,066-page textbook (19.9 MB) |
| :--- | :--- | :--- |
| **chunkr fast (`lopdf`)** — no extra installed | **1.03 ms · 9,661 pgs/s** (23× pypdf, 11× PyMuPDF) | **703 ms · 2,940 pgs/s** (17× pypdf, 3.5× PyMuPDF) |
| **chunkr `[pdf]`** (liteparse) — layout-aware | 10.7 ms · 939 pgs/s (2.2× pypdf, 1.06× PyMuPDF) | 2,280 ms · 906 pgs/s (5.2× pypdf, 1.09× PyMuPDF) |
| PyMuPDF (`fitz`) | 11.3 ms · 886 pgs/s | 2,489 ms · 830 pgs/s |
| pypdf (pure Python) | 23.5 ms · 426 pgs/s | 11,827 ms · 175 pgs/s |

End to end (extract + recursive chunk) the textbook takes 764 ms on the fast path — **15.3× pypdf + LangChain, 3.3× PyMuPDF + LangChain** — and 2,293 ms with the extension (**5.1×** and **1.11×**). Those fast-path multiples hold only *when it can read the document*: on a PDF whose fonts it cannot map it is the slowest reader in the table, because almost no text reaches you (0.8% — next table).

**PDF extraction quality** — what actually reaches your chunks (2,066-page textbook, 120 sampled pages, `benchmarks/pdf_quality.py`)

| Extractor | Pages returned | Words recovered | Precision | `?` chars | Most frequent word | Structure found (whole document) |
| :--- | ---: | ---: | ---: | ---: | :--- | :--- |
| **chunkr `[pdf]`** (liteparse) | 2,033 of 2,066 | **99.6%** | 99.3% | 0.0% | `the` (4%) | 9,915 headings, 580 tables |
| PyMuPDF (`fitz`) | 2,066 | 99.9% | 100.0% | 0.6% | `the` (4%) | — |
| pypdf | 2,066 | 99.5% | 98.3% | 0.6% | `the` (4%) | — |
| **chunkr fast (`lopdf`)** | 2,066 | **0.8%** | 42.7% | 7.6% | `identity-h` (50%) | — |

This is the speed/quality tradeoff in one table. On the textbook every font is `/Type0` + `/Identity-H`, and the built-in extractor (lopdf 0.32) answers that encoding with a hardcoded placeholder string — `"Identity-H" => "?Identity-H Unimplemented?"` in its `document.rs`, once per text-show operator, and it has no `/ToUnicode` support at all, so the mapping sitting in the file is never read (see [docs/backends.md](docs/backends.md#23b-why-the-built-in-fast-backend-is-not-a-fallback-for-real-documents)). So the fast path emits `?Identity-H Unimplemented?` instead of text: half of all its tokens are that one string and it recovers 0.8% of the document's words, so chunks built from it are mostly noise for retrieval. The extension recovers **99.6%** of them (words at least two independent extractors agree on) — **124× the fast path's 0.8%** — with no `?` noise, matching PyMuPDF and pypdf, and adds the 9,915 headings, 580 tables and 2,127 list items that become chunk `header_path` metadata and keep tables out of prose chunks. On a clean single-column PDF (the 10-page sample) the fast path is fine — 98.6% of words, no `?` characters — so it remains the right default when the extra is not installed.

The `pages` column is a page count, not a word count: the extension returns 2,033 `Document`s for this book because 33 of its 2,066 pages have no text at all — verified against pypdf and PyMuPDF, which extract nothing from any of them, and none of them contain images either. Every `Document` carries its real `page_number`, so page alignment with the source stays exact.

**Retrieval quality** — Chroma's token-level benchmark (472 questions, global retrieval, `all-MiniLM-L6-v2`); `R@k` = gold answer chars covered by the top-k chunks, `P@k` = share of retrieved chars that is gold, `prec_Ω` = purity at perfect recall.

| Library | R@5 | R@10 | R@15 | P@5 | P@10 | P@15 | prec_Ω | Avg chunk chars |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **chunkr `RecursiveChunker` (defaults)** | **0.792** | **0.878** | **0.913** | 0.057 | 0.034 | 0.026 | 0.255 | 854 |
| LangChain | 0.762 | 0.862 | 0.893 | 0.060 | 0.038 | 0.028 | 0.251 | 745 |
| text-splitter | 0.762 | 0.846 | 0.892 | 0.060 | 0.036 | 0.027 | 0.262 | 790 |
| Chonkie | 0.752 | 0.842 | 0.892 | 0.062 | 0.037 | 0.028 | **0.292** | 701 |
| semchunk | 0.736 | 0.851 | 0.883 | **0.070** | **0.045** | **0.033** | 0.260 | 644 |
| LlamaIndex | 0.654 | 0.752 | 0.805 | 0.013 | 0.010 | 0.009 | 0.054 | 4,136 |

chunkr leads recall at every k (+0.030 / +0.016 / +0.020 over the next best) and stays first with per-corpus retrieval. `P@k` tracks chunk length — chunkr's 854-char chunks carry more non-answer text than the 644-790-char fields, and that ordering is the whole gap. Interpolated to *the same* chunk length (500-900 chars) chunkr leads recall *and* purity at every budget. Full sweep, recursive + markdown: [`benchmarks/README.md`](benchmarks/README.md). OCR accuracy per engine: [`benchmarks/README.md`](benchmarks/README.md#ocr-accuracy-ocr_accuracypy).

## FAQ

- **Python 3.13 / 3.14?** Wheels cover 3.8–3.13; the `[pdf]` extra needs 3.10+.
- **Threads or processes?** `par_chunk_documents`/`par_chunk_texts` use Rayon and release the GIL; chunkers are cheap to construct and reusable.
- **Custom separators or tokenizer?** `RecursiveChunker(separators=[...])`, `HFTokenChunker.from_file(...)`; `chunkr.SENTENCE_SEPARATORS` is the retrieval-tuned default hierarchy.
- **Metadata?** `Document(content, metadata)`; chunkers preserve input metadata and add chunker-specific keys.

## Contributing

Issues and PRs welcome — see [`PUBLISHING.md`](PUBLISHING.md) for the release process and run `cargo test`, `pytest tests`, `pytest packages/chunkr-pdf/tests` and
`pytest packages/chunkr-tabular/tests` before opening one.

## License

MIT.
