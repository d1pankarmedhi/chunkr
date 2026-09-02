pub mod chunker;
pub mod error;
pub mod loader;
pub mod structures;

#[cfg(feature = "python")]
pub mod python;

pub mod prelude {
    pub use crate::chunker::agentic::{
        AgenticChunker, AgenticDecision, AgenticDecisionMaker, CustomAgenticDecisionMaker,
        HeuristicAgenticDecisionMaker,
    };
    pub use crate::chunker::base::{BaseChunker, Chunker};
    pub use crate::chunker::char::CharacterChunker;
    pub use crate::chunker::code::{CodeChunker, CodeLanguage};
    pub use crate::chunker::contextual::{
        ContextFormat, ContextGenerator, ContextualChunker, CustomContextGenerator,
        ExtractiveContextGenerator,
    };
    pub use crate::chunker::hierarchical::{
        HierarchicalChunkPair, HierarchicalChunker, HierarchyNode,
    };
    pub use crate::chunker::html::HtmlChunker;
    pub use crate::chunker::json::JsonChunker;
    pub use crate::chunker::late::LateChunker;
    pub use crate::chunker::markdown::MarkdownChunker;
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
    pub use crate::chunker::table::{TableChunker, TableFormat};
    pub use crate::chunker::token::{TokenChunker, TokenEncoding};
    pub use crate::chunker::word::WordChunker;
    pub use crate::error::ChunkrError;
    pub use crate::loader::base::BaseLoader;
    pub use crate::loader::pdf::PDFLoader;
    pub use crate::structures::document::Document;
}

pub use prelude::*;

