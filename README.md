<div align="center">
<h1>chunkr</h1>
<h3>⚡ Blazingly Fast Document & Text Chunking for LLMs, Agents and RAG</h3>

[![PyPI](https://img.shields.io/pypi/v/chunkr-rs.svg)](https://pypi.org/project/chunkr-rs/)
[![PyPI - Python Version](https://img.shields.io/pypi/pyversions/chunkr-rs.svg)](https://pypi.org/project/chunkr-rs/)
[![Crates.io](https://img.shields.io/crates/v/chunkr.svg)](https://crates.io/crates/chunkr)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

</div>

**Chunkr** (`chunkr-rs` on PyPI, `import chunkr`) is an ultra-high-performance document chunking and text-splitting engine written in Rust with native Python C-ABI bindings. Engineered specifically for Large Language Models (LLMs), Vector Databases (Chroma, Qdrant, Pinecone, Weaviate, Milvus), and Retrieval-Augmented Generation (RAG) pipelines, Chunkr delivers **up to 1,000+ MB/s throughput** with zero superfluous heap allocations — operating **2x to 20x faster** than pure-Python splitters like LangChain's `RecursiveCharacterTextSplitter` and LlamaIndex node parsers.

> [!TIP]
> **Quick Package Disambiguation**:
> - **Python (pip)**: `pip install chunkr-rs` ➔ `import chunkr`
> - **Rust (cargo)**: `cargo add chunkr` ➔ `use chunkr::prelude::*;`
> - **Core Architecture**: Zero heap churn, SIMD-accelerated separator scanning, and true multi-core Rayon execution that bypasses the Python GIL.

---

## 🥊 Chunkr vs. Alternatives

Why data engineers and AI developers choose Chunkr over pure-Python chunking libraries:

| Feature / Capability | Chunkr (`chunkr-rs`) | LangChain Splitters | LlamaIndex Node Parsers | Chonkie | Semchunk |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Core Architecture** | **Rust + PyO3 (Native)** | Pure Python | Pure Python | Python / Rust partial | Pure Python |
| **Recursive Split Throughput** | **500 – 1,000+ MB/s** | 200 – 350 MB/s | 150 – 300 MB/s | ~400 MB/s | ~100 MB/s |
| **Fixed Char Throughput** | **100 – 140 MB/s** | 7 – 10 MB/s | 8 – 12 MB/s | ~50 MB/s | ~10 MB/s |
| **Memory Strategy** | **Zero-Copy Slices** | String Duplication | Object Churn | String Duplication | String Duplication |
| **Multithreading** | **Rayon (True Multi-core)** | ThreadPool (GIL bound) | Async / GIL bound | None | ProcessPool |
| **Built-in Strategies** | **18+ Strategies** | ~5 Splitters | ~6 Node Parsers | 4 Chonkers | 1 Strategy |
| **Late Chunking Support** | **Built-in (Span Snapping)** | ❌ None | Experimental | ❌ None | ❌ None |
| **Tree-sitter AST Code Chunking** | **Built-in (Python & Rust)** | Regex-based | ❌ None | ❌ None | ❌ None |
| **Markdown Header Breadcrumbs** | **Full `#`–`######` Hierarchy** | Basic Split | Basic Markdown | ❌ None | ❌ None |
| **Table Chunking (CSV/TSV/MD)** | **Header Preserving/Repeating** | ❌ None | Limited | ❌ None | ❌ None |
| **Native PDF Extraction Engine** | **Built-in (600+ pgs/s)** | External (`pypdf`, `fitz`) | External (`pypdf`) | ❌ None | ❌ None |
| **Parent-Child / Hierarchical** | **Built-in (`HierarchicalChunker`)** | Multi-class setup | Class pipeline | ❌ None | ❌ None |
| **Post-Processing Pipeline** | **Built-in (`ChunkPipeline`)** | Manual code | IngestionPipeline | ❌ None | ❌ None |
| **Constant-Memory Streaming** | **Built-in (`StreamChunker`)** | ❌ None | ❌ None | ❌ None | ❌ None |
| **Ecosystem Bridges** | **`to_langchain`, `to_llamaindex`** | Native | Native | Conversion helper | ❌ None |

---

## 🧭 Which Chunker Should I Use? (Decision Guide)

| If your document or use-case is... | Recommended Chunker | Why? |
| :--- | :--- | :--- |
| **General prose, blog posts, articles** | `RecursiveChunker` | Blazing-fast SIMD separator splitting (`\n\n`, `\n`, ` `). |
| **OpenAI models (GPT-4o, text-embedding-3)** | `TokenChunker` | Exact BPE token bounds (`cl100k_base`, `o200k_base`) with zero token waste. |
| **Open-source LLMs (Llama 3, Mistral, Qwen, BGE)** | `HFTokenChunker` | Direct native integration with Hugging Face `tokenizer.json`. |
| **Small-to-Big RAG Architectures** | `HierarchicalChunker` | Matches high-relevance child chunks and retrieves full parent context. |
| **Preserving full-document context in embeddings** | `LateChunker` | Computes token spans across full text; pools embeddings with global context. |
| **Markdown documentation & technical specs** | `MarkdownChunker` | Preserves headers and tracks breadcrumb paths (`Guide > Setup > Auth`). |
| **Codebases (Python, Rust, JS, Go, C++, SQL)** | `AstCodeChunker` or `CodeChunker` | Splits cleanly at AST function/class boundaries without breaking logic. |
| **Financial tables, CSVs, TSVs, Markdown tables** | `TableChunker` | Automatically repeats table header rows on split chunks for LLM clarity. |
| **Factual claim verification & legal contracts** | `PropositionChunker` | Splits sentences into atomic, self-contained factual propositions. |
| **Isolated chunks needing context** | `ContextualChunker` | Injects Anthropic-style situational document prefaces into every chunk. |
| **Search-time dynamic sizing** | `QueryAwareChunker` | Tight high-resolution chunks around query hotspots, wide context elsewhere. |
| **Multi-gigabyte log files, dumps, stdin** | `StreamChunker` | Constant-memory sliding window streaming for arbitrarily large inputs. |
| **Production cleanup & token budget packing** | `ChunkPipeline` | Filters short noise, deduplicates, packs small chunks, and generates SHA-256 IDs. |

---

## 📦 Installation

### Python

Install `chunkr-rs` via `pip`:

```bash
pip install chunkr-rs
```

*Pre-compiled wheels are available for Linux, Windows, and macOS (Intel & Apple Silicon) on Python 3.8 through 3.13. No Rust compiler required!*

Or build from source with `maturin`:
```bash
maturin develop --release
```

### Rust

Add `chunkr` to your `Cargo.toml`:

```toml
[dependencies]
chunkr = "1.2"
```

Or via `cargo`:
```bash
cargo add chunkr
```

---

## 🚀 Supported Chunking Strategies

| Strategy | Chunker Class | Description |
| :--- | :--- | :--- |
| **Recursive** | `RecursiveChunker` | SIMD recursive separator splitting (**~1,000+ MB/s**) |
| **Token BPE** | `TokenChunker` | OpenAI BPE token splitting (`cl100k_base`, `o200k_base`) |
| **Universal HF Token** | `HFTokenChunker` | Hugging Face token splitting (Llama 3, Mistral, Qwen, BGE, BERT) |
| **Sentence** | `SentenceChunker` | Multi-byte UTF-8 safe sentence splitting with abbreviation guards |
| **Paragraph** | `ParagraphChunker` | Multi-paragraph grouping across `\n\n` |
| **Semantic** | `SemanticChunker` | Distance threshold breakpoint clustering |
| **Proposition** | `PropositionChunker` | Atomic factual claim extraction & subject propagation |
| **Contextual** | `ContextualChunker` | Anthropic-style situational document preface injection |
| **Query-Aware** | `QueryAwareChunker` | Search query hotspot detection & adaptive sizing |
| **Agentic** | `AgenticChunker` | Discourse transition & topic segmentation |
| **Hierarchical** | `HierarchicalChunker` | Parent-child pairs & multi-level tree generation |
| **Late Chunking** | `LateChunker` | Full-document token span snapping & embedding pooling |
| **Table** | `TableChunker` | Structure-aware tabular chunking (Markdown, CSV, TSV) with header duplication |
| **Markdown** | `MarkdownChunker` | Header hierarchy (`#`–`######`) & breadcrumb paths |
| **Code** | `CodeChunker` | Syntax-aware chunking (Rust, Python, JS, TS, Go, C++, SQL) |
| **AST Code** | `AstCodeChunker` | Tree-sitter AST syntax chunking (Rust & Python) along function/class boundaries |
| **Chunk Bin-Packing** | `ChunkPacker` | Post-processing optimizer bin-packing small chunks into token budget blocks |
| **Post-Chunking Pipeline** | `ChunkPipeline` | Composable quality filtering, deduplication, packing & SHA-256 metadata enrichment |
| **Streaming Chunker** | `StreamChunker` | Constant-memory sliding-window streaming for multi-GB files & stdin |
| **Ecosystem Bridges** | `to_langchain`, `to_llamaindex`, `to_dict_list` | Zero-copy adapters for LangChain, LlamaIndex, Hugging Face & Pandas |
| **JSON** | `JsonChunker` | Structure-aware JSON chunker preserving valid sub-trees |
| **HTML** | `HtmlChunker` | DOM element boundary chunking |
| **Character & Word** | `CharacterChunker`, `WordChunker` | High-throughput fixed character and word-count splitting |

---

## 🐍 Python Quickstart

```python
import chunkr

sample_text = (
    "Convolutional neural networks specialize in visual imagery. "
    "Recurrent networks process sequential text.\n\n"
    "In conclusion, deep learning powers modern vision systems."
)

# 1. Recursive Character Chunking
recursive_chunker = chunkr.RecursiveChunker(chunk_size=500, overlap=50)
docs = recursive_chunker.chunk(sample_text)
for doc in docs:
    print(doc.content, doc.metadata)

# 2. Token-Based Chunking (OpenAI cl100k_base / GPT-4)
token_chunker = chunkr.TokenChunker(chunk_size=100, overlap=20, encoding="cl100k_base")
token_docs = token_chunker.chunk(sample_text)

# 3. Query-Aware Adaptive Chunking
query_chunker = chunkr.QueryAwareChunker(query="neural networks", hotspot_sentences=1, context_sentences=2)
query_docs = query_chunker.chunk(sample_text)

# 4. Agentic Topic Chunking
agentic_chunker = chunkr.AgenticChunker(min_chars=100, max_chars=1000)
agentic_docs = agentic_chunker.chunk(sample_text)

# 5. Hierarchical Parent-Child Pairs & Tree Chunking
hier_chunker = chunkr.HierarchicalChunker(parent_size=1000, child_size=200)
pairs = hier_chunker.chunk_hierarchical(sample_text)  # List[{"parent": Document, "children": [Document, ...]}]
tree = hier_chunker.chunk_tree(sample_text)          # Nested hierarchy tree dict

# 6. Markdown Structure Chunking (with header breadcrumbs)
md_chunker = chunkr.MarkdownChunker(chunk_size=1000, overlap=100)
md_docs = md_chunker.chunk("# Title\n## Section\nContent...")

# 7. Table-Aware Chunking (Markdown / CSV / TSV with repeated headers)
table_chunker = chunkr.TableChunker(rows_per_chunk=10, overlap_rows=1)
table_docs = table_chunker.chunk("| Date | Metric | Value |\n|---|---|---|\n| 2024-01 | MRR | $50K |")

# 8. Late Chunking (Full-document context with token span snapping & pooling)
late_chunker = chunkr.LateChunker(chunk_size=300, overlap=30)
late_docs = late_chunker.chunk(sample_text)
# Pool token embeddings directly from your transformer model:
# pooled_embeddings = late_chunker.pool_embeddings(token_embeddings, late_docs)

# 9. Hugging Face Universal Token Chunking (Llama 3, Mistral, BGE, BERT)
# hf_chunker = chunkr.HFTokenChunker.from_file("path/to/tokenizer.json", chunk_size=512, overlap=50)
# hf_chunker = chunkr.HFTokenChunker.from_tokenizer(transformers_tokenizer, chunk_size=512, overlap=50)

# 10. Recursive Directory Ingestion & Auto-Routing
dir_loader = chunkr.DirectoryLoader(extensions=["pdf", "md", "csv", "py"])
dir_chunks = dir_loader.load_and_chunk("path/to/repo_or_folder")

# 11. PDF Document Loading & Chunking
loader = chunkr.PDFLoader()
pages = loader.load_pages("path/to/document.pdf")
pdf_chunks = recursive_chunker.chunk_documents(pages)

# 12. AST-Based Code Chunking (Tree-sitter syntax boundaries)
ast_chunker = chunkr.AstCodeChunker(language="python", max_chunk_size=1500)
code_chunks = ast_chunker.chunk("def calculate():\n    return 42\n\nclass Model:\n    pass")

# 13. Chunk Bin-Packing (Greedily merging small chunks into token budgets)
packer = chunkr.ChunkPacker(max_characters=1000)
packed_chunks = packer.pack(recursive_chunker.chunk(sample_text))

# 14. Post-Chunking Transformation Pipeline (Filter + Dedup + Pack + SHA-256 Enrich)
pipeline = (
    chunkr.ChunkPipeline()
    .filter_min_chars(30)
    .filter_min_alpha_ratio(0.5)
    .deduplicate(exact=True)
    .pack(max_characters=1200)
    .enrich(id_prefix="kb_doc_")
)
optimized_chunks = pipeline.process(recursive_chunker.chunk(sample_text))

# 15. Multi-Core Parallel Batch Chunking (Rayon-backed multi-threading)
batch_docs = [
    chunkr.Document(f"Document {i} content...", {"doc_id": i, "category": "AI", "score": 0.98})
    for i in range(100)
]
# Parallel processing across all available CPU cores
parallel_chunks = recursive_chunker.par_chunk_documents(batch_docs)

# 16. Streaming Chunker (Constant memory for multi-GB inputs)
streamer = chunkr.StreamChunker(chunk_size=1000, overlap=150)
stream_chunks = streamer.chunk_text(sample_text)

# 17. Ecosystem Bridges (LangChain, LlamaIndex, Hugging Face, Pandas)
langchain_docs = chunkr.to_langchain(stream_chunks)      # List[langchain_core.documents.Document]
llamaindex_nodes = chunkr.to_llamaindex(stream_chunks)  # List[llama_index.core.schema.TextNode]
records = chunkr.to_dict_list(stream_chunks)            # Direct DataFrame / Dataset input
```

---

## 🛠️ Production RAG Recipes

### Recipe 1: End-to-End LangChain + ChromaDB Ingestion
Combine Chunkr's high-speed PDF loading and recursive splitting with LangChain vector stores:

```python
import chunkr
from langchain_community.vectorstores import Chroma
from langchain_openai import OpenAIEmbeddings

# 1. High-speed native PDF parsing (17x faster than pypdf)
loader = chunkr.PDFLoader()
pages = loader.load_pages("annual_report.pdf")

# 2. Multi-threaded recursive chunking across all CPU cores
chunker = chunkr.RecursiveChunker(chunk_size=800, overlap=100)
chunks = chunker.par_chunk_documents(pages)

# 3. Zero-copy bridge to LangChain Document format
langchain_docs = chunkr.to_langchain(chunks)

# 4. Ingest into Chroma vector database
vectorstore = Chroma.from_documents(
    documents=langchain_docs,
    embedding=OpenAIEmbeddings(model="text-embedding-3-small")
)
```

### Recipe 2: Small-to-Big (Parent-Child) Retrieval with LlamaIndex
Match tight child chunks for high semantic precision, then return full parent context to the LLM:

```python
import chunkr

# 1. Generate parent-child pairs in a single native pass
hier = chunkr.HierarchicalChunker(parent_size=1500, parent_overlap=150, child_size=300, child_overlap=30)
pairs = hier.chunk_hierarchical(document_text)

# 2. Extract child chunks enriched with parent context metadata
child_docs = []
for pair in pairs:
    parent_doc = pair["parent"]
    for child in pair["children"]:
        child.metadata["parent_id"] = parent_doc.metadata.get("chunk_id")
        child.metadata["parent_content"] = parent_doc.content
        child_docs.append(child)

# 3. Export directly to LlamaIndex TextNodes
llamaindex_nodes = chunkr.to_llamaindex(child_docs)
```

### Recipe 3: Late Chunking with SentenceTransformers & Full Document Context
Late Chunking encodes the entire document first to retain bidirectional context across all chunks:

```python
import chunkr
import torch
from transformers import AutoTokenizer, AutoModel

# 1. Tokenize full document and obtain token embeddings
model_name = "jinaai/jina-embeddings-v2-base-en"
tokenizer = AutoTokenizer.from_pretrained(model_name)
model = AutoModel.from_pretrained(model_name)

inputs = tokenizer(full_text, return_tensors="pt")
with torch.no_grad():
    outputs = model(**inputs)
    token_embeddings = outputs.last_hidden_state[0].tolist()

# 2. Snap exact token spans and mean-pool chunk embeddings
late_chunker = chunkr.LateChunker(chunk_size=400, overlap=40)
chunks = late_chunker.chunk(full_text)
chunk_embeddings = late_chunker.pool_embeddings(token_embeddings, chunks)
```

### Recipe 4: Universal Hugging Face Tokenizer Chunking
Chunk documents directly with open-source model tokenizers (Llama 3, Mistral, Qwen, DeepSeek):

```python
import chunkr
from transformers import AutoTokenizer

tok = AutoTokenizer.from_pretrained("meta-llama/Meta-Llama-3-8B")
hf_chunker = chunkr.HFTokenChunker.from_tokenizer(tok, chunk_size=512, overlap=50)

# Chunks are guaranteed to fit within exact model token limits
token_bounded_chunks = hf_chunker.chunk(document_text)
```

---

## 🦀 Rust Quickstart

```rust
use chunkr::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let text = "Convolutional neural networks specialize in visual imagery. Recurrent networks process sequential text.\n\nIn conclusion, deep learning powers modern vision.";

    // 1. Recursive Chunker
    let recursive_chunker = RecursiveChunker::new()
        .with_chunk_size(500)
        .with_overlap(50);
    let chunks = recursive_chunker.chunk(text)?;

    // 2. Query-Aware Adaptive Chunker
    let query_chunker = QueryAwareChunker::new("convolutional neural networks")
        .with_hotspot_sizing(1, 0)
        .with_context_sizing(3, 1);
    let query_chunks = query_chunker.chunk(text)?;

    // 3. Hierarchical Parent-Child Tree Chunker
    let hier_chunker = HierarchicalChunker::with_sizes(150, 20, 50, 10)?;
    let tree = hier_chunker.chunk_tree(text)?;

    // 4. Table-Aware Chunker
    let table_chunker = TableChunker::new()
        .with_rows_per_chunk(Some(5))
        .with_overlap_rows(1);
    let table_chunks = table_chunker.chunk("| Col A | Col B |\n|---|---|\n| 1 | 2 |")?;

    // 5. Late Chunking (Span Snapping & Mean-Pooling)
    let late_chunker = LateChunker::new();
    let late_chunks = late_chunker.chunk(text)?;

    // 6. Tree-sitter AST Code Chunker
    let ast_chunker = AstCodeChunker::new(AstLanguage::Rust).with_max_chunk_size(1500);
    let rust_chunks = ast_chunker.chunk("fn main() { println!(\"Hello\"); }")?;

    // 7. Chunk Bin-Packing
    let packer = ChunkPacker::new(1000);
    let packed = packer.pack(&chunks);

    // 8. Directory Ingestion & Chunker Auto-Routing
    let dir_loader = DirectoryLoader::new()
        .with_extensions(vec!["md".into(), "csv".into(), "pdf".into()]);
    // 9. Post-Chunking Pipeline (Filter + Dedup + Packing + SHA-256 Enrichment)
    let pipeline = ChunkPipeline::new()
        .filter_min_characters(30)
        .filter_min_alpha_ratio(0.5)
        .deduplicate_exact(true)
        .pack(1200)
        .enrich_metadata()
        .with_id_prefix("rust_doc_");
    let clean_chunks = pipeline.process(chunks);

    // 10. PDF Document Loading & Chunking
    let loader = PDFLoader::new();
    let pdf_pages = loader.load_pages_from_file("tests/test_files/sample_doc.pdf")?;
    let pdf_chunks = recursive_chunker.chunk_documents(&pdf_pages)?;

    // 11. Multi-Threaded Parallel Document Batch Chunking
    let parallel_chunks = recursive_chunker.par_chunk_documents(&pdf_pages)?;

    // 12. Constant-Memory Streaming Chunker (Files, Sockets, STDIN)
    let streamer = StreamChunker::new(1000, 150)?;
    let stream_iter = streamer.chunk_file("large_document.txt")?;
    for chunk_result in stream_iter {
        let chunk = chunk_result?;
        println!("Streamed chunk: {}", chunk.content.len());
    }

    Ok(())
}
```

---

## ⚡ Command-Line Interface (`chunkr-cli`)

Install or run the standalone `chunkr` CLI binary for fast batch processing or UNIX piping:

```powershell
# Chunk any file using Markdown strategy to JSONL format
cargo run --bin chunkr -- README.md -s markdown -c 500 -f jsonl

# Stream massive multi-GB files with constant memory footprint
cargo run --bin chunkr -- large_file.txt -s stream --chunk-size 1000 -f jsonl

# Pipe from STDIN with post-chunking pipeline (dedup, filtering, packing, SHA-256 hash enrichment)
cat document.txt | chunkr -s recursive --chunk-size 800 --min-chars 30 --dedup --enrich --pack 1200 > output.jsonl

# Ingest and auto-route an entire directory
chunkr ./docs -s dir --format jsonl --out-file chunks.jsonl
```

---

## 📊 Performance Benchmarks

Direct in-memory Python runtime comparison (`import chunkr` vs. `langchain-text-splitters`, `pypdf`, and `PyMuPDF`):

### Text Chunking Throughput Comparison

| Strategy & Test Case | Document Size | LangChain (ms) | Chunkr (ms) | LangChain Throughput | Chunkr Throughput | Speedup Factor |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Fixed Char (100 KB)** | 100 KB | 14.12 ms | **0.69 ms** | 6.9 MB/s | **141.3 MB/s** | **20.5x Faster** |
| **Fixed Char (1 MB)** | 1 MB | 113.72 ms | **9.36 ms** | 8.8 MB/s | **106.8 MB/s** | **12.1x Faster** |
| **Recursive Char (100 KB)** | 100 KB | 0.27 ms | **0.09 ms** | 359.8 MB/s | **1,064.1 MB/s** | **3.0x Faster** |
| **Recursive Char (1 MB)** | 1 MB | 3.19 ms | **1.57 ms** | 312.9 MB/s | **635.0 MB/s** | **2.0x Faster** |
| **Recursive Char (5 MB)** | 5 MB | 23.75 ms | **9.78 ms** | 210.5 MB/s | **511.0 MB/s** | **2.4x Faster** |
| **Markdown Split (500 KB)** | 500 KB | 2.43 ms | **0.75 ms** | 200.9 MB/s | **654.3 MB/s** | **3.3x Faster** |
| **Markdown Header Parser** | 500 KB | 35.31 ms | **2.59 ms** | 13.8 MB/s | **188.6 MB/s** | **13.6x Faster** |
| **Python Code (200 KB)** | 200 KB | 0.44 ms | **0.18 ms** | 445.1 MB/s | **1,063.2 MB/s** | **2.4x Faster** |

### PDF Extraction & End-to-End Pipeline Latency

| Extractor / Pipeline | Latency | Throughput | Speedup vs PyPDF |
| :--- | :--- | :--- | :--- |
| **Chunkr PDFLoader (Full Text)** | **5.78 ms** | **1,730.5 pgs/s** | **16.7x Faster** |
| **Chunkr PDFLoader (Page Documents)** | **5.51 ms** | **1,816.5 pgs/s** | **17.5x Faster** |
| PyMuPDF (`fitz`) | 33.58 ms | 297.8 pgs/s | 2.9x Faster |
| pypdf (pure Python) | 96.27 ms | 103.9 pgs/s | 1.0x (baseline) |
| **Chunkr End-to-End (PDF + Recursive)** | **5.82 ms** | **1,718.8 pgs/s** | **18.7x Faster** |
| PyMuPDF + LangChain RecursiveTextSplitter | 27.33 ms | 365.9 pgs/s | 4.0x Faster |
| pypdf + LangChain RecursiveTextSplitter | 109.04 ms | 91.7 pgs/s | 1.0x (baseline) |

---

## ❓ Frequently Asked Questions (FAQ)

<details>
<summary><b>What is Chunkr?</b></summary>
<br>

**Chunkr** is an ultra-fast document chunking and text-splitting library written in Rust with native Python bindings (`chunkr-rs` on PyPI, `import chunkr`). It is engineered to replace slow, pure-Python text splitters in Retrieval-Augmented Generation (RAG) and LLM application pipelines.
</details>

<details>
<summary><b>Why use Chunkr instead of LangChain's <code>RecursiveCharacterTextSplitter</code>?</b></summary>
<br>

Chunkr provides a **2x to 3.5x speedup** on recursive text splitting and up to **20x speedup** on character splitting, with zero heap allocations. Furthermore, Chunkr includes 18+ specialized strategies (Late Chunking, Tree-sitter AST for code, table header preservation, parent-child trees) and a native PDF extractor that is **17x faster than `pypdf`**, all callable via `import chunkr` with zero-copy LangChain and LlamaIndex adapters.
</details>

<details>
<summary><b>Why is the package named <code>chunkr-rs</code> on PyPI but imported as <code>chunkr</code>?</b></summary>
<br>

On PyPI, the name `chunkr` was previously occupied by a legacy OCR service. To avoid naming collisions and provide a clean distribution channel, the package is registered as `chunkr-rs` on PyPI:
```bash
pip install chunkr-rs
```
In Python code, you import it directly:
```python
import chunkr
```
</details>

<details>
<summary><b>Does Chunkr require a local Rust compiler or toolchain to install?</b></summary>
<br>

No. Pre-compiled binary wheels (built with `maturin` and PyO3 C-ABI) are distributed on PyPI for Linux (x86_64, aarch64), Windows (x86_64), and macOS (Apple Silicon & Intel). When you run `pip install chunkr-rs`, pip downloads the pre-built native binary directly.
</details>

<details>
<summary><b>How do I use Chunkr with LangChain or LlamaIndex?</b></summary>
<br>

Chunkr features zero-copy bridges:
```python
import chunkr

chunks = chunkr.RecursiveChunker(chunk_size=500, overlap=50).chunk("Sample text...")

# Convert to LangChain Document objects:
langchain_docs = chunkr.to_langchain(chunks)

# Convert to LlamaIndex TextNode objects:
llamaindex_nodes = chunkr.to_llamaindex(chunks)
```
</details>

<details>
<summary><b>What is Late Chunking and how does it improve retrieval quality?</b></summary>
<br>

Traditional chunking splits text prior to embedding generation, causing each chunk to lose global document context. **Late Chunking** passes the entire document through a transformer embedding model first, then uses `chunkr.LateChunker` to snap exact token span boundaries and pool chunk embeddings directly from full-document hidden states.
</details>

<details>
<summary><b>Can Chunkr parse and chunk PDFs directly without external dependencies?</b></summary>
<br>

Yes. Chunkr includes a native `PDFLoader` built on `lopdf` in Rust. It extracts text and generates page documents at **over 600–1,800 pages per second**, running **12x–17x faster than `pypdf`** without requiring Poppler or PyMuPDF.
</details>

<details>
<summary><b>How does Markdown chunking preserve header breadcrumbs?</b></summary>
<br>

`chunkr.MarkdownChunker` inspects Markdown headers (`#` through `######`) and assigns a hierarchical `header_path` attribute to each chunk's metadata (e.g., `Guide > Setup > Configuration`), preventing LLMs and vector search engines from losing structural context.
</details>

---

## 💡 Contributing

Contributions are welcome! Please check out the [Contribution Guide](CONTRIBUTION.md) to get started.

## 📝 License

Licensed under the MIT License - see the [LICENSE](LICENSE) file for details.