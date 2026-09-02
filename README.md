<div align="center">
<h1>chunkr</h1>
<h3>⚡ Blazingly Fast Document & Text Chunking for LLMs, Agents and RAG</h3>

[![Crates.io](https://img.shields.io/crates/v/chunkr.svg)](https://crates.io/crates/chunkr)
[![PyPI](https://img.shields.io/pypi/v/chunkr-rs.svg)](https://pypi.org/project/chunkr-rs/)
![License](https://img.shields.io/crates/l/chunkr.svg)

</div>

**`chunkr`** is a high-performance document chunking library built in Rust with first-class Python native bindings for Large Language Models (LLMs) and Retrieval-Augmented Generation (RAG) applications. It delivers throughput up to **hundreds of MB/sec** with zero superfluous heap allocations, advanced structure awareness, OpenAI BPE tokenization, semantic clustering, proposition decomposition, query-adaptive sizing, agentic topic segmentation, and multi-core parallel processing.

---

## 📦 Installation

### Rust

Add `chunkr` to your `Cargo.toml`:

```toml
[dependencies]
chunkr = "1.1"
```

### Python

Install `chunkr-rs` via `pip`:

```bash
pip install chunkr-rs
```

Or build from source with `maturin`:
```bash
maturin develop --release
```

---

## 🚀 Supported Chunking Strategies

| Strategy | Chunker Class | Description |
| :--- | :--- | :--- |
| **Recursive** | `RecursiveChunker` | SIMD recursive separator splitting (**~1,000+ MB/s**) |
| **Token BPE** | `TokenChunker` | OpenAI BPE token splitting (`cl100k_base`, `o200k_base`) |
| **Sentence** | `SentenceChunker` | Multi-byte UTF-8 safe sentence splitting with abbreviation guards |
| **Paragraph** | `ParagraphChunker` | Multi-paragraph grouping across `\n\n` |
| **Semantic** | `SemanticChunker` | Distance threshold breakpoint clustering |
| **Proposition** | `PropositionChunker` | Atomic factual claim extraction & subject propagation |
| **Contextual** | `ContextualChunker` | Anthropic-style situational document preface injection |
| **Query-Aware** | `QueryAwareChunker` | Search query hotspot detection & adaptive sizing |
| **Agentic** | `AgenticChunker` | Discourse transition & topic segmentation |
| **Hierarchical** | `HierarchicalChunker` | Parent-child pairs & multi-level tree generation |
| **Markdown** | `MarkdownChunker` | Header hierarchy (`#`–`######`) & breadcrumb paths |
| **Code** | `CodeChunker` | Syntax-aware chunking (Rust, Python, JS, TS, Go, C++, SQL) |
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

# 7. PDF Document Loading & Chunking
loader = chunkr.PDFLoader()
pages = loader.load_pages("path/to/document.pdf")
pdf_chunks = recursive_chunker.chunk_documents(pages)

# 8. Multi-Core Parallel Batch Chunking (Rayon-backed multi-threading)
batch_docs = [
    chunkr.Document(f"Document {i} content...", {"doc_id": i, "category": "AI", "score": 0.98})
    for i in range(100)
]
# Parallel processing across all available CPU cores
parallel_chunks = recursive_chunker.par_chunk_documents(batch_docs)
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

    // 4. PDF Document Loading & Chunking
    let loader = PDFLoader::new();
    let pdf_pages = loader.load_pages_from_file("tests/test_files/sample_doc.pdf")?;
    let pdf_chunks = recursive_chunker.chunk_documents(&pdf_pages)?;

    // 5. Multi-Threaded Parallel Document Batch Chunking
    let parallel_chunks = recursive_chunker.par_chunk_documents(&pdf_pages)?;

    Ok(())
}
```

---

## 📊 Performance Benchmarks

Direct in-memory Python runtime comparison (`import chunkr` vs. `langchain-text-splitters`, `pypdf`, and `PyMuPDF`):

<p align="center">
  <img src="https://raw.githubusercontent.com/d1pankarmedhi/chunkr/main/assets/benchmark.svg" alt="Chunkr vs LangChain Speed Benchmark" width="100%" />
</p>

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

## 💡 Contributing

Contributions are welcome! Please check out the [Contribution Guide](CONTRIBUTION.md) to get started.

## 📝 License

Licensed under the MIT License - see the [LICENSE](LICENSE) file for details.