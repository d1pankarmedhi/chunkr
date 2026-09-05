//! # ⚡ `chunkr`: High-Performance Document Chunking for LLMs & RAG
//!
//! `chunkr` is a blazingly fast document and text chunking library engineered in Rust
//! with native Python bindings for Large Language Models (LLMs), AI Agents, and
//! Retrieval-Augmented Generation (RAG) pipelines.
//!
//! ## Core Features
//!
//! - **Extreme Performance**: High throughput (up to 1,000+ MB/s) with minimal allocations,
//!   efficient string slicing, and multi-core Rayon parallelism.
//! - **20+ Chunking Strategies**: Natural language, token BPE (OpenAI), Hugging Face tokenizers,
//!   AST code syntax, Markdown, Tables, JSON, HTML, Late Chunking, and more.
//! - **Post-Processing Pipeline**: Composable filtering, deduplication, bin-packing, and SHA-256 metadata enrichment.
//! - **Streaming Processing**: Constant-memory chunking for multi-gigabyte files, network streams, and STDIN.
//! - **First-Class Ecosystem Bridges**: Seamless export to LangChain, LlamaIndex, Hugging Face, and Pandas.
//!
//! ## Quickstart
//!
//! ```rust
//! use chunkr::prelude::*;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let text = "Chunkr provides fast text chunking. It handles paragraphs and sentences cleanly.\n\nSecond section starts here.";
//!
//! // 1. Recursive semantic boundary chunker
//! let chunker = RecursiveChunker::new()
//!     .with_chunk_size(100)
//!     .with_overlap(20);
//!
//! let chunks = chunker.chunk(text)?;
//! for chunk in &chunks {
//!     println!("Chunk: {} (meta: {:?})", chunk.content, chunk.metadata);
//! }
//!
//! // 2. Post-processing pipeline (filter, dedup, pack, enrich)
//! let pipeline = ChunkPipeline::new()
//!     .filter_min_characters(10)
//!     .deduplicate_exact(true)
//!     .pack(250)
//!     .enrich_metadata();
//!
//! let optimized = pipeline.process(chunks);
//! # Ok(())
//! # }
//! ```
//!
//! ## Re-exports
//!
//! Most users will want to import [`prelude::*`](prelude), which brings all chunkers,
//! loaders, pipeline stages, and data structures into scope.

pub mod chunker;
pub mod error;
pub mod loader;
pub mod pipeline;
pub mod structures;

#[cfg(feature = "python")]
pub mod python;

#[cfg(feature = "wasm")]
pub mod wasm;

pub mod prelude {
    pub use crate::chunker::agentic::{
        AgenticChunker, AgenticDecision, AgenticDecisionMaker, CustomAgenticDecisionMaker,
        HeuristicAgenticDecisionMaker,
    };
    #[cfg(not(target_arch = "wasm32"))]
    pub use crate::chunker::ast_code::{AstCodeChunker, AstLanguage};
    pub use crate::chunker::base::{BaseChunker, Chunker};
    pub use crate::chunker::char::CharacterChunker;
    pub use crate::chunker::code::{CodeChunker, CodeLanguage};
    pub use crate::chunker::contextual::{
        ContextFormat, ContextGenerator, ContextualChunker, CustomContextGenerator,
        ExtractiveContextGenerator,
    };
    pub use crate::chunker::hf_token::HFTokenChunker;
    pub use crate::chunker::hierarchical::{
        HierarchicalChunkPair, HierarchicalChunker, HierarchyNode,
    };
    pub use crate::chunker::html::HtmlChunker;
    pub use crate::chunker::json::JsonChunker;
    pub use crate::chunker::late::LateChunker;
    pub use crate::chunker::markdown::MarkdownChunker;
    pub use crate::chunker::packer::ChunkPacker;
    pub use crate::chunker::proposition::{
        CustomPropositionExtractor, PropositionChunker, PropositionExtractor,
        SyntacticPropositionExtractor,
    };
    pub use crate::chunker::query_aware::QueryAwareChunker;
    pub use crate::chunker::recursive::{KeepSeparator, RecursiveChunker};
    pub use crate::chunker::semantic::{
        BreakpointThreshold, CustomEmbedder, Embedder, FastLexicalEmbedder, SemanticChunker,
    };
    pub use crate::chunker::sentence::{ParagraphChunker, SentenceChunker};
    pub use crate::chunker::stream::StreamChunker;
    pub use crate::chunker::table::{TableChunker, TableFormat};
    pub use crate::chunker::token::{TokenChunker, TokenEncoding};
    pub use crate::chunker::word::WordChunker;
    pub use crate::error::ChunkrError;
    pub use crate::loader::base::BaseLoader;
    #[cfg(not(target_arch = "wasm32"))]
    pub use crate::loader::directory::DirectoryLoader;
    pub use crate::loader::pdf::PDFLoader;
    pub use crate::pipeline::{ChunkDeduplicator, ChunkFilter, ChunkPipeline, MetadataEnricher};
    pub use crate::structures::document::Document;
}

pub use prelude::*;
