# benchmarks

Reproducible benchmarks for `chunkr` vs. the mainstream Python/Rust chunking libraries.
Nothing here is part of the published crate or wheel — it is a dev-only harness.

## OCR accuracy (`ocr_accuracy.py`, `examples/ocr_bench.rs`)

Ground truth here is the page's own text layer: a digital page is rasterised with
liteparse (so the image carries no text), the backend OCRs it, and the result is
compared with the text liteparse extracted from that page. No scanned sample
files are needed and nothing is hand-scored.

```bash
# Python engines: tesseract (liteparse's wheel) plus whatever adapters are installed
python benchmarks/ocr_accuracy.py --dpi 200 --markdown

# Native Rust engines: in-process ONNX PP-OCR (Tesseract too, with pdf-ocr)
cargo run --release --features pdf-ocr-ppocr --example ocr_bench -- \
    tests/test_files/27-page textbook --preset medium
```

| metric | meaning |
| --- | --- |
| `char_sim` | 1 − normalised Levenshtein / max length. Order-sensitive: interleaved columns, duplicated lines or reflowed tables all cost it. |
| `token_recall` | share of the page's alphanumeric tokens that came back. Bag of words, so it measures "did the words survive" independently of order. |
| `chars_ratio` | OCR characters / ground-truth characters, to expose missing or duplicated text. |
| `ms/page` | render + OCR + merge per page, cold start included in the mean. |

**All 43 pages of both fixtures, 200 dpi, one measurement session**, Apple
Silicon laptop (CPU only), macOS 15, Python 3.14 for the Python rows (PaddleOCR
needs ≤3.13 in practice, so that venv used 3.12), release builds for the Rust
rows. Quality metrics are deterministic; latencies are single-session and move
with disk cache and thermals (see the last bullet below).

Python harness — `tesseract` is liteparse's bundled engine, the other three are
adapter engines served to liteparse over the loopback OCR proxy:

| PDF | Engine | Pages | char_sim | token_recall | chars_ratio | ms/page |
| :--- | :--- | ---: | ---: | ---: | ---: | ---: |
| 16-page deck | `rapidocr` | 16 | 0.929 | 0.913 | 0.940 | 746 |
| 16-page deck | `paddleocr` | 16 | 0.945 | 0.915 | 0.968 | 8592 |
| 16-page deck | `tesseract` | 16 | 0.876 | 0.915 | 0.915 | 400 |
| 16-page deck | `easyocr` | 16 | 0.848 | 0.914 | 0.950 | 2865 |
| 27-page textbook | `paddleocr` | 27 | 0.722 | 0.986 | 1.013 | 20169 |
| 27-page textbook | `tesseract` | 27 | 0.674 | 0.982 | 1.003 | 1925 |
| 27-page textbook | `rapidocr` | 27 | 0.609 | 0.981 | 0.992 | 2435 |
| 27-page textbook | `easyocr` | 27 | 0.464 | 0.979 | 1.009 | 4565 |

Native harness — in-process ONNX PP-OCR, no Python and no server:

| PDF | Engine | Pages | char_sim | token_recall | chars_ratio | ms/page |
| :--- | :--- | ---: | ---: | ---: | ---: | ---: |
| 16-page deck | `ppocr-tiny` | 16 | 0.897 | 0.909 | 0.958 | 169 |
| 16-page deck | `ppocr-small` | 16 | 0.900 | 0.915 | 0.961 | 291 |
| 16-page deck | `ppocr-medium` | 16 | 0.917 | 0.916 | 0.952 | 936 |
| 27-page textbook | `ppocr-tiny` | 27 | 0.643 | 0.980 | 1.008 | 339 |
| 27-page textbook | `ppocr-small` | 27 | 0.622 | 0.984 | 1.010 | 1010 |
| 27-page textbook | `ppocr-medium` | 27 | 0.817 | 0.985 | 1.010 | 4000 |

### Per-engine aggregate (43 pages)

Page-weighted means across both fixtures, so the 27-page textbook counts for more
than the 16-page deck; the Python harness prints this table itself.

| Engine | Where it runs | Pages | char_sim | token_recall | chars_ratio | ms/page |
| :--- | :--- | ---: | ---: | ---: | ---: | ---: |
| `ppocr-medium` | Rust, ONNX in-process | 43 | 0.854 | 0.959 | 0.988 | 2860 |
| `paddleocr` | Python, Paddle runtime via proxy | 43 | 0.805 | 0.960 | 0.996 | 15861 |
| `tesseract` | liteparse wheel | 43 | 0.749 | 0.957 | 0.970 | 1358 |
| `ppocr-tiny` | Rust, ONNX in-process | 43 | 0.738 | 0.954 | 0.989 | 276 |
| `rapidocr` | Python, ONNX via proxy | 43 | 0.728 | 0.956 | 0.973 | 1807 |
| `ppocr-small` | Rust, ONNX in-process | 43 | 0.725 | 0.958 | 0.992 | 742 |
| `easyocr` | Python, torch via proxy | 43 | 0.607 | 0.955 | 0.987 | 3932 |

Native Tesseract is the same engine as the wheel row: the Rust `pdf-ocr` build
compiles Tesseract from source and needs `cmake`, which this machine does not
have, so that row is the Python-wheel measurement of the identical engine.

### What the numbers say

- **Layout-aware reading order is the whole story on the textbook**: every engine
  recovers ~98% of the words (`token_recall` 0.979–0.986) yet `char_sim` spans
  0.46–0.82 — the differences are segmentation and ordering, not recognition.
- **The runtime is not a footnote**: the same PP-OCRv6 medium models score 0.854
  `char_sim` at 2.9 s per page in-process (ONNX, Rust) and 0.805 at 15.9 s per
  page through the Python Paddle runtime. Want that accuracy without the runtime
  cost? Use the native `pdf-ocr-ppocr` feature or `rapidocr`.
- **`tiny` is the speed/quality sweet spot for the native path**: 0.738
  `char_sim` at 276 ms per page — it beats `small` on both axes here, and
  `medium` buys +0.12 `char_sim` for 10x the time.
- **Tesseract remains a sane default and a poor specialist**: fastest Python
  engine, decent on slides, mid-table on the textbook, and it never wins
  `char_sim` where the layout is hard — which is why OCR is pluggable.
- **A 2-page sample lied twice**: on the textbook's first two pages Tesseract scored
  0.441 `char_sim` (0.674 across all 27) and RapidOCR 0.948 (0.609 across 27).
  Run the whole document before drawing conclusions.
- **Latency is the noisy axis**: a disk-cold first session put `ppocr-small` at
  2.1 s per page on the deck, the clean session above at 0.29 s. Treat quality
  columns as results and latency columns as orders of magnitude.

## What is measured

`bench_chunking.py` — text chunking throughput (the core claim), in-process:

| case | corpus | parameters |
| --- | --- | --- |
| `recursive_1mb` / `recursive_5mb` | paragraph-structured prose | chunk_size 1000 chars, overlap 200 |
| `fixed_char_1mb` | prose | chunk_size 1000 chars, overlap 200 |
| `markdown_500kb` | markdown (headings, tables, code fences) | chunk_size 1000 chars, overlap 150 |
| `code_python_200kb` | synthetic Python source | chunk_size 1500 chars, overlap 200 |
| `token_cl100k_200kb` | prose | chunk_size 512 tokens (cl100k_base), overlap 50 |
| `sentence_500kb` | prose | 3 sentences/chunk (chunkr) vs 1000-char budget (others) |
| `batch_100x50kb` | 100 docs x 50 KB | chunk_size 1000, overlap 200 |

Implementations compared: `chunkr` (this repo, built from source), LangChain
(`langchain-text-splitters`), LlamaIndex (`llama-index-core` node parsers), Chonkie
(with the `chonkie-core` Rust extension installed), `semchunk`, and `semantic-text-splitter`
(the Rust `text-splitter` crate's Python bindings).

`bench_pdf.py` — PDF extraction and end-to-end pipelines: `chunkr.PDFLoader` vs `pypdf` vs
PyMuPDF, and each followed by recursive chunking.

`bench_accuracy.py` — retrieval accuracy (not speed) of the same libraries, on Chroma's
[token-level chunking benchmark](https://research.trychroma.com/evaluating-chunking)
(MIT): 5 corpora / 472 questions whose gold answer spans are known character ranges.
Chunks are embedded, the top-5 per question are retrieved, and the gold characters inside
them give `recall`, `precision`, `IoU` and `prec_Ω` (chunk purity with perfect recall).
The dataset is downloaded and cached under `benchmarks/data/` on first run; no API keys
are needed (local `all-MiniLM-L6-v2` embeddings).

`tune_accuracy.py` — the same metric as a config sweep: chunk size x overlap x separator
hierarchy for every library, plus the best config at each recall level.

## How to run

```bash
# one-time setup
uv venv --python 3.12 .venv
uv pip install --python .venv/bin/python maturin langchain-text-splitters llama-index-core \
    "chonkie[all]" semchunk semantic-text-splitter tree-sitter-python tiktoken pypdf pymupdf \
    sentence-transformers
VIRTUAL_ENV=$PWD/.venv .venv/bin/maturin develop --release   # builds chunkr from source

# benchmarks
.venv/bin/python benchmarks/bench_chunking.py --reps 15
.venv/bin/python benchmarks/bench_pdf.py --big
.venv/bin/python benchmarks/bench_accuracy.py                 # all impls, k=5
.venv/bin/python benchmarks/bench_accuracy.py --impl chunkr --scoped
.venv/bin/python benchmarks/tune_accuracy.py --libs chunkr    # sweep one library's config space
```

Results are written to `benchmarks/results/` as both JSON (all raw samples) and Markdown.

## Methodology

* **Identical work.** All implementations run on the same in-memory corpus with the same
  chunk-size/overlap parameters. Chunk count, average chunk size and character coverage are
  recorded per implementation so unequal output is visible rather than hidden.
* **Round-robin repetitions.** One timed sample per implementation per round, `--reps`
  rounds, **median** reported. This cancels ordering and thermal drift that a
  measure-one-impl-to-completion schedule would bake in.
* **Timer noise control.** Inner iterations are chosen per implementation so every timed
  sample spans at least ~5 ms; the `inner_calls` column shows the multiplier. Raw per-rep
  timings and the spread are in the JSON.
* **Construction excluded.** Chunker/tokenizer construction happens before timing (realistic
  for reused chunkers). First-use costs are reported separately in the cold-start section.
* **GC disabled** for the whole measured region, for every implementation equally.
* **Deterministic corpora.** Fixed seeds, SHA-256 recorded in the JSON. Byte-identical
  corpora across runs and machines.
* **MB = 10^6 bytes.** Throughput is file bytes / wall time; MB/s is not MiB/s.
* **Pipeline cost is included** for `chunkr` (Rust -> Python object conversion) and for
  LlamaIndex (Document/Node construction), because that is what a user actually calls.

### Caveats (read before quoting numbers)

* `chunkr`'s `SentenceChunker` is **sentence-count** based; Chonkie/LlamaIndex sentence
  splitters are **size-budget** based. The `sentence_500kb` case compares pass speed over the
  same text, not identical output.
* LangChain's `MarkdownHeaderTextSplitter` and LlamaIndex's `MarkdownNodeParser` split only on
  headers and do not enforce a size budget, so their chunks are much larger (see
  `avg_chunk_chars`). Fewer, bigger chunks makes them look faster than they are on a
  per-output basis; the chunk counts are printed for this reason.
* `chunkr`'s `AstCodeChunker` and `text-splitter`'s `CodeSplitter` parse with tree-sitter;
  `chunkr`'s plain `CodeChunker` and LangChain's `RecursiveCharacterTextSplitter.from_language`
  use regex separators. They are different quality/perf points, not substitutes.
* Single-text throughput only exercises one core for `chunkr`; the `batch_100x50kb` case is the
  one that shows the Rayon/GIL-released multi-document path (LangChain has no parallel
  equivalent in `langchain-text-splitters`).
* Memory/allocations are **not** measured here. RSS deltas and `tracemalloc` do not capture
  Rust-side heap behaviour, so the README's zero-copy claims are not verified by this harness.

## Results

Latest results live in `benchmarks/results/`. Files are timestamped; the newest JSON contains
every raw sample and the machine/package metadata needed to interpret it.

### Headline run: Apple M4 (10 threads), macOS 15.7.9, Python 3.12.11

`chunkr` built from source at `c2fe03f` (`maturin develop --release`), 15 reps, median.
Full matrix: `benchmarks/results/chunking-20261001-000820.md` (raw samples in the matching
`.json`). The two recursive rows were re-measured after the separator defaults changed
(`benchmarks/results/chunking-20261007-123302.{md,json}`); the sentence-aware hierarchy costs
~2% on the 1 MB case, inside the run-to-run spread. No row was flagged noisy; the three full
runs made during this session agreed within ~10% on every case.

| case | chunkr | fastest alternative | chunkr vs best alt | chunk parity |
| --- | --- | --- | --- | --- |
| `recursive_1mb` (1000/200 chars) | **2219 MB/s** | langchain RecursiveCharacterTextSplitter 769 MB/s | **2.89x** | 1413 vs 1413 chunks |
| `recursive_5mb` (1000/200 chars) | **2028 MB/s** | langchain RecursiveCharacterTextSplitter 692 MB/s | **2.93x** | 7057 vs 7060 |
| `fixed_char_1mb` (1000/200 chars) | **750 MB/s** | chonkie TokenChunker(character) 22 MB/s | **33.4x** | 1250 vs 1250 |
| `markdown_500kb` (1000/150) | **819 MB/s** | langchain MarkdownHeaderTextSplitter 67 MB/s | **12.2x** | 715 vs 113 (alt has no size budget) |
| `code_python_200kb` (1500/200) | **3232 MB/s** | langchain Recursive(PYTHON) 622 MB/s | **5.19x** | 166 vs 166 |
| `token_cl100k_200kb` (512/50 tok) | 38 MB/s | chonkie TokenChunker(cl100k_base) 151 MB/s | **0.25x (loses)** | 62 vs 62 |
| `sentence_500kb` (3 sentences) | **622 MB/s** | chonkie SentenceChunker 20 MB/s | **30.9x** | different semantics |
| `batch_100x50kb` (Rayon path) | **3224 MB/s** | langchain loop 679 MB/s | **4.75x** | 7010 vs 7010 |

Other observations from the same run:

* `semantic-text-splitter` (the Rust `text-splitter` crate) and `semchunk` are far behind on
  recursive splitting: 175 MB/s and 42 MB/s at 1 MB, 46 MB/s and 40 MB/s at 5 MB. `text-splitter`
  scales superlinearly on this corpus (0.25/0.5/1/2/5 MB -> 0.65/1.99/5.47/28.9/113.8 ms, reproduced
  with `overlap=0`), so chunkr's lead over it grows from 12.9x at 1 MB to ~44x at 5 MB.
* LangChain's fixed-width splitter (`CharacterTextSplitter(separator='')`, and the common
  `RecursiveCharacterTextSplitter(separators=[''])` workaround) is pathological: 1.6-1.7 MB/s,
  ~450x slower than `chunkr.CharacterChunker` on 1 MB. Chonkie's character-mode `TokenChunker`
  (22 MB/s) is the fastest alternative there.
* tree-sitter paths are the slow ones by nature: `chunkr.AstCodeChunker` 12.4 MB/s and
  `text-splitter.CodeSplitter` 5.7 MB/s, vs 3232 MB/s for regex-based `chunkr.CodeChunker`.
  AST chunking costs ~260x the regex path inside chunkr itself; it buys syntax boundaries, not speed.
* Cold start (fresh interpreter, encoder build + first chunk, cl100k): chunkr 22.0 ms,
  text-splitter 22.0 ms, langchain+tiktoken 42.8 ms, chonkie 456.9 ms.
* LlamaIndex node parsers are the slowest tier throughout (2-20 MB/s): they construct
  `Document`/`Node` objects and token-count every candidate split.

### PDF extraction (same machine)

| corpus | chunkr PDFLoader.load | pypdf | PyMuPDF | chunkr vs pypdf | chunkr vs PyMuPDF |
| --- | --- | --- | --- | --- | --- |
| `tests/test_files/sample_doc.pdf` (10 pgs) | **1.07 ms** (9344 pgs/s) | 24.41 ms (410 pgs/s) | 11.16 ms (896 pgs/s) | 22.8x | 10.4x |
| 2066-page ML textbook (19.9 MB) | **748 ms** (2762 pgs/s) | 11901 ms (174 pgs/s) | 2617 ms (790 pgs/s) | 15.9x | 3.5x |

End-to-end (extract + recursive chunk) on the 2066-page corpus: chunkr 798 ms vs
pypdf+LangChain 12054 ms (15.1x) and PyMuPDF+LangChain 2659 ms (3.3x).
`chunkr.PDFLoader.load_pages` is the fastest extraction path (721 ms, 2865 pgs/s).
Raw data: `benchmarks/results/pdf-20261001-001557.json`.

### PDF extraction quality (`pdf_quality.py`)

Extraction quality is scored without a reference text: three or more independent
extractors read the same embedded text layer, and a word that at least two of them put
on the *same page* counts as really present (consensus). Each engine is then measured on
how much of that consensus it recovers, how much of its own output the consensus
supports (precision catches glyph-mapping bugs, which fabricate words rather than drop
them), how much junk it emits (control characters, U+FFFD, private-use glyphs, lone
surrogates, and engine error text such as `?Identity-H Unimplemented?`), and what the
chunker hands to the embedder: chunks polluted by junk and chunks with no sentence at
all. Pages are aligned by `page_number`, not by index: backends may skip blank pages
(the extension returns 2,033 of the textbook's 2,066) and printed page labels differ
from physical page numbers, so index-based comparison silently compares different pages.

```bash
python benchmarks/pdf_quality.py book.pdf --sample 120 --json out.json
```

2,066-page textbook (19.9 MB), 120 sampled pages:

| extractor | pages | words/kchar | consensus recall | precision | `?` chars | top word | junk chunks | layout |
| --- | ---: | ---: | ---: | ---: | ---: | --- | ---: | --- |
| chunkr `[pdf]` (liteparse) | 2,033 | 119.4 | **98.5%** | 98.2% | 0.0% | `the` (4%) | 1.3% | 9,915 headings, 580 tables |
| PyMuPDF (`fitz`) | 2,066 | 132.8 | 99.9% | 99.9% | 0.6% | `the` (4%) | 5.5% | — |
| pypdf | 2,066 | 132.4 | 99.6% | 98.3% | 0.6% | `the` (4%) | 5.5% | — |
| chunkr fast (`lopdf`) | 2,066 | 76.5 | 0.8% | 42.7% | 7.6% | `identity-h` (50%) | 0.0% | — |

The textbook's fonts are Identity-H encoded. lopdf cannot map them, so it emits
`?Identity-H Unimplemented?` for the whole document: half its tokens are that string and
it recovers 0.8% of the page words. That is the failure mode behind the
`chunkr-rs[pdf]` extension, and it is document-dependent: on the clean single-column
10-page sample every engine scores 97-100% with no `?` characters, so the fast path stays
a fine default on simple PDFs, where it is also 10x faster (see the table above).

### Accuracy (Apple M4, Python 3.12.11, `all-MiniLM-L6-v2`, k=5, global retrieval)

Chroma's 5 corpora (1.44 M chars, 472 questions, 132 k gold chars). Higher is better on every
column; `prec_Ω` is the chunker-intrinsic ceiling (no retrieval).
Full matrix and per-corpus IoU: `benchmarks/results/accuracy-20261007-064824.md`.

Recursive splitting. chunkr runs its own defaults (1000 chars / 120 overlap / sentence-aware
separators); the other libraries are on 1000 chars / 200 overlap, the closest equivalent they
offer:

| implementation | recall | precision | IoU | prec_Ω | chunks | avg chunk chars |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| chunkr RecursiveChunker (defaults) | **0.792** | 0.057 | 0.057 | 0.255 | 1745 | 854 |
| langchain RecursiveCharacterTextSplitter | 0.762 | 0.060 | 0.060 | 0.251 | 2184 | 745 |
| text-splitter TextSplitter | 0.762 | 0.060 | 0.060 | 0.262 | 2038 | 790 |
| chonkie RecursiveChunker | 0.752 | 0.062 | 0.061 | **0.292** | 2059 | 701 |
| semchunk chunk(token_counter=len) | 0.736 | **0.070** | **0.069** | 0.260 | 2798 | 644 |
| llama-index SentenceSplitter | 0.654 | 0.013 | 0.013 | 0.054 | 424 | 4136 |

chunkr has the best recall; the remaining purity gap is chunk length (854 avg chars vs 701-790
for the others), not boundary quality — see the size-matched table below, where it disappears.

Markdown splitting, chunkr defaults vs 1000 chars / 150 overlap:

| implementation | recall | precision | IoU | prec_Ω | chunks | avg chunk chars | unlocated |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| chunkr MarkdownChunker | **0.777** | 0.055 | 0.055 | 0.249 | 1792 | 853 | 0 |
| text-splitter MarkdownSplitter | 0.750 | **0.058** | **0.058** | **0.256** | 2042 | 749 | 0 |
| llama-index MarkdownNodeParser | 1.000 | 0.003 | 0.003 | 0.003 | 5 | 288865 | 0 |
| langchain MarkdownHeaderTextSplitter | 0.000 | 0.000 | 0.000 | 0.000 | 5 | 289076 | 5 |

Reading the table: `precision` and `IoU` move with chunk size, because 5 retrieved chunks of
~800 chars always carry far more text than the ~280 gold characters a question has. `recall`
says whether the answers are inside the retrieved chunks, `prec_Ω` says how concentrated the
answers are inside the chunker's own chunks. The two markdown rows show the size effect at the
extreme (a single 289 k-char chunk per corpus covers everything and is pure noise); the
langchain row is 0.000 because that splitter strips headers, so its chunks no longer exist
verbatim in the corpus and are excluded (see the `unlocated` column).

### Accuracy tuning (`tune_accuracy.py`)

Same metric, swept over chunk size, overlap and separator hierarchy for every library
(full grid: `benchmarks/results/tuning-20261007-070024.json`).

**The only fair comparison is at matched average chunk size.** Precision and IoU fall as
retrieved chunks get longer, so a library that emits bigger chunks always looks less pure unless
you hold chunk length fixed. Interpolating every config family to the same average chunk length:

| avg chunk chars | chonkie | chunkr paragraph | **chunkr sentence (default)** | langchain default | langchain sentence | semchunk | text-splitter |
| ---: | :-- | :-- | :-- | :-- | :-- | :-- | :-- |
| 500 | — | — | **R 0.725 / Ω 0.389** | — | — | — | — |
| 600 | 0.736 / 0.331 | — | **R 0.743 / Ω 0.338** | — | 0.738 / 0.327 | — | — |
| 700 | 0.752 / 0.293 | — | **R 0.784 / Ω 0.300** | — | 0.752 / 0.290 | 0.751 / 0.248 | 0.753 / 0.291 |
| 800 | 0.764 / 0.271 | 0.769 / 0.233 | **R 0.778 / Ω 0.271** | — | 0.766 / 0.268 | — | — |
| 900 | — | 0.782 / 0.216 | **R 0.787 / Ω 0.255** | — | — | — | — |

`R` marks the best recall at that size, `Ω` the best `prec_Ω`; chunkr's sentence recipe takes
both at every size. So at any context budget between 500 and 900 characters per chunk, chunkr
is ahead of every other library on recall *and* chunk purity at the same time — no metric is
bought with another. Config families without a point at that size are marked —.

What the sweep shows:

* **Sentence-aware separators are a free win.** Adding sentence breaks to the hierarchy
  (paragraph -> line -> sentence -> word) raises recall *and* purity at fixed chunk size and
  overlap: at (800, 0) recall 0.758 -> 0.785 and prec_Ω 0.270 -> 0.307, at (1000, 0)
  0.753 -> 0.772 and 0.236 -> 0.258. No throughput cost (2338 MB/s vs 2245 MB/s on the 1 MB
  prose corpus, same chunk count). Answers are whole sentences, so sentence-aligned chunks
  contain them more often and dilute them less.
* **What chunk length does is move you along the frontier; separators move the frontier.**
  Picking a smaller `chunk_size` buys purity and spends recall (both at ~50% lower retrieval
  cost). Picking sentence separators improves both curves at once.
* **Shipped defaults.** `RecursiveChunker` now defaults to 1000/120 with
  `SENTENCE_SEPARATORS` (`chunkr.SENTENCE_SEPARATORS` in Python). Against the previous defaults
  (1000/200, paragraph-only separators) at the same nominal size, that is
  0.792 / 0.057 / 0.057 / 0.255 versus 0.784 / 0.057 / 0.056 / 0.213 — better on every metric
  at once, with 6% fewer chunks to embed. `MarkdownChunker` inherits the same sub-chunking, so
  its markdown row above also improved (recall 0.770 -> 0.777, prec_Ω 0.220 -> 0.249)

### Bug found while running this: chunks could exceed `chunk_size`

The recursive merge carried an overlap window into the next chunk without re-checking it
against the cap, so chunks could be emitted at up to `chunk_size + overlap` bytes
(reproduced: paragraphs `[56, 63, 30, 41, 984]` at 1000/200 produced a 1126-byte chunk;
9 of 937 chunks were over the cap, max 1182). Fixed in `src/chunker/recursive.rs`
with a regression test in `tests/chunker_tests.rs`; chunk boundaries are unchanged apart
from the affected tail windows. `benchmarks/data/` is downloaded on first run and gitignored.

### How these compare to the numbers in the root README

| README claim | this harness |
| --- | --- |
| Recursive 1 MB: 631 MB/s, 1.8x vs LangChain | 2219 MB/s, 2.89x |
| Recursive 5 MB: 435 MB/s, 1.9x vs LangChain | 2028 MB/s, 2.93x |
| Fixed char 1 MB: 323 MB/s, 30.8x | 750 MB/s, 33.4x vs Chonkie / ~450x vs LangChain |
| Markdown 500 KB: 617 MB/s, 2.5x | 819 MB/s, 12.2x (vs LangChain's header-only splitter) |
| Python code 200 KB: 1186 MB/s, 2.5x | 3232 MB/s, 5.19x |
| Token BPE 200 KB: 10.4 MB/s, 0.62x vs LangChain | 38 MB/s, 0.29x vs LangChain (loses to Chonkie 4x) |
| PDF 10-page sample: 1730 pgs/s, 16.7x vs pypdf | 9344 pgs/s, 22.8x vs pypdf |
| PDF end-to-end: 1719 pgs/s, 18.7x vs pypdf | 2589 pgs/s, 15.1x vs pypdf (2066-pg corpus) |

Absolute throughput is higher than the README on this machine (M4 vs whatever the README was
measured on); the only claim that does not survive is the relative position on token/BPE
chunking, where chunkr sits behind both Chonkie and LangChain.
