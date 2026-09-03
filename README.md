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

## 💡 Contributing

Contributions are welcome! Please check out the [Contribution Guide](CONTRIBUTION.md) to get started.

## 📝 License

Licensed under the MIT License - see the [LICENSE](LICENSE) file for details.