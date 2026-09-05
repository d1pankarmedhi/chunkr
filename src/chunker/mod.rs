//! # Chunking Strategies
//!
//! This module contains all chunking algorithms supported by Chunkr, implementing the
//! [`Chunker`](base::Chunker) trait.
//!
//! ## Strategy Taxonomy
//!
//! ### 1. Natural Boundary Chunkers
//! - [`RecursiveChunker`](recursive::RecursiveChunker): Hierarchical splitting on paragraphs, sentences, and words.
//! - [`SentenceChunker`](sentence::SentenceChunker): Sentence boundary splitting with abbreviation preservation.
//! - [`ParagraphChunker`](sentence::ParagraphChunker): Grouping paragraphs across double newlines.
//! - [`CharacterChunker`](char::CharacterChunker) & [`WordChunker`](word::WordChunker): Fixed character and word sliding windows.
//!
//! ### 2. Token-Aware Chunkers
//! - [`TokenChunker`](token::TokenChunker): OpenAI BPE tokenization (`cl100k_base`, `o200k_base`, `p50k_base`, `r50k_base`).
//! - [`HFTokenChunker`](hf_token::HFTokenChunker): Hugging Face tokenizers (Llama 3, Mistral, Qwen, BERT, BGE).
//!
//! ### 3. Structured & Code Syntax Chunkers
//! - [`AstCodeChunker`](ast_code::AstCodeChunker): AST tree-sitter chunking along function and class definitions.
//! - [`CodeChunker`](code::CodeChunker): Regex syntax-aware code chunking across multiple programming languages.
//! - [`MarkdownChunker`](markdown::MarkdownChunker): Heading hierarchy (`#` through `######`) with breadcrumb headers.
//! - [`TableChunker`](table::TableChunker): Row-based tabular chunking (Markdown, CSV, TSV) preserving column headers.
//! - [`JsonChunker`](json::JsonChunker): Structural JSON tree chunking preserving valid objects.
//! - [`HtmlChunker`](html::HtmlChunker): HTML DOM element boundary chunking.
//!
//! ### 4. Advanced AI, Semantic & Agentic Chunkers
//! - [`LateChunker`](late::LateChunker): Long-context token span snapping and embedding pooling.
//! - [`SemanticChunker`](semantic::SemanticChunker): Distance-threshold breakpoint clustering on embedding vectors.
//! - [`PropositionChunker`](proposition::PropositionChunker): Atomic factual proposition decomposition.
//! - [`ContextualChunker`](contextual::ContextualChunker): Anthropic-style document preface injection.
//! - [`QueryAwareChunker`](query_aware::QueryAwareChunker): Hotspot detection and adaptive sizing around search queries.
//! - [`AgenticChunker`](agentic::AgenticChunker): Topic transition and discourse segmentation.
//! - [`HierarchicalChunker`](hierarchical::HierarchicalChunker): Multi-level parent-child tree representations.
//!
//! ### 5. Streaming & Optimization
//! - [`StreamChunker`](stream::StreamChunker): Constant-memory sliding-window streaming for multi-GB files and STDIN.
//! - [`ChunkPacker`](packer::ChunkPacker): Bin-packing optimizer merging small fragments into target budgets.

pub mod agentic;
#[cfg(not(target_arch = "wasm32"))]
pub mod ast_code;
pub mod base;
pub mod char;
pub mod code;
pub mod contextual;
pub mod hf_token;
pub mod hierarchical;
pub mod html;
pub mod json;
pub mod late;
pub mod markdown;
pub mod packer;
pub mod proposition;
pub mod query_aware;
pub mod recursive;
pub mod semantic;
pub mod sentence;
pub mod stream;
pub mod table;
pub mod token;
pub mod word;
