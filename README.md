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
chunkr = "1.0"
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
| **Recursive** | `RecursiveChunker` | SIMD recursive separator splitting (**~600+ MB/s**) |
| **Token BPE** | `TokenChunker` | OpenAI BPE token splitting (`cl100k_base`, `o200k_base`) |
| **Sentence** | `SentenceChunker` | Abbreviation-safe sentence boundary splitting |
| **Paragraph** | `ParagraphChunker` | Multi-paragraph grouping across `\n\n` |
| **Semantic** | `SemanticChunker` | Distance threshold breakpoint clustering |
| **Proposition** | `PropositionChunker` | Atomic factual claim extraction & subject propagation |
| **Contextual** | `ContextualChunker` | Anthropic-style situational document preface injection |
| **Query-Aware** | `QueryAwareChunker` | Search query hotspot detection & adaptive sizing |
| **Agentic** | `AgenticChunker` | Discourse transition & topic segmentation |
| **Hierarchical** | `HierarchicalChunker` | Parent-child pairs & multi-level tree generation |
| **Markdown** | `MarkdownChunker` | Header hierarchy (`#`–`######`) & breadcrumb paths |
| **Code** | `CodeChunker` | Syntax-aware chunking (Rust, Python, JS, Go, etc.) |

---

## Python Quickstart

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

# 5. Markdown Structure Chunking (with header breadcrumbs)
md_chunker = chunkr.MarkdownChunker(chunk_size=1000, overlap=100)
md_docs = md_chunker.chunk("# Title\n## Section\nContent...")

# 6. PDF Document Loading & Chunking
loader = chunkr.PDFLoader()
pages = loader.load_pages("path/to/document.pdf")
pdf_chunks = recursive_chunker.chunk(pages[0].content)
```

---

## Rust Quickstart

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

    Ok(())
}
```

---

## 📊 In-Memory Speed Benchmark: Chunkr (Rust/Python) vs. LangChain

Pure in-memory Python runtime comparison (`import chunkr` vs. `langchain-text-splitters`):

<p align="center">
  <img src="https://raw.githubusercontent.com/d1pankarmedhi/chunkr/main/assets/benchmark.svg" alt="Chunkr vs LangChain Speed Benchmark" width="100%" />
</p>

---

## 💡 Contributing

Contributions are welcome! Please check out the [Contribution Guide](CONTRIBUTION.md) to get started.

## 📝 License

Licensed under the MIT License - see the [LICENSE](LICENSE) file for details.