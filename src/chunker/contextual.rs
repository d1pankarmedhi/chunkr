use std::sync::Arc;
use serde_json::Value;

use crate::chunker::base::{BaseChunker, Chunker};
use crate::chunker::recursive::RecursiveChunker;
use crate::error::ChunkrError;
use crate::structures::document::Document;

/// Context formatting mode for contextual chunking
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ContextFormat {
    /// Prepend context string to chunk body: "[Context: {context}]\n\n{content}"
    #[default]
    Prefix,
    /// Store context only in Document metadata without modifying chunk content
    MetadataOnly,
    /// Custom template using `{context}` and `{content}` placeholders
    Custom(String),
}

/// Interface for generating document-level situational context for individual chunks
pub trait ContextGenerator: Send + Sync {
    fn generate_context(
        &self,
        full_doc: &str,
        chunk: &Document,
        chunk_idx: usize,
        total_chunks: usize,
    ) -> Result<String, ChunkrError>;
}

/// Fast extractive context generator that infers document title, overview, and section breadcrumbs
#[derive(Debug, Clone, Default)]
pub struct ExtractiveContextGenerator {
    pub max_context_chars: usize,
}

impl ExtractiveContextGenerator {
    pub fn new() -> Self {
        Self {
            max_context_chars: 200,
        }
    }

    pub fn with_max_chars(mut self, max_chars: usize) -> Self {
        self.max_context_chars = max_chars;
        self
    }
}

impl ContextGenerator for ExtractiveContextGenerator {
    fn generate_context(
        &self,
        full_doc: &str,
        chunk: &Document,
        chunk_idx: usize,
        total_chunks: usize,
    ) -> Result<String, ChunkrError> {
        // 1. Extract document title / first non-empty line
        let first_line = full_doc
            .lines()
            .map(|l| l.trim().trim_start_matches('#').trim())
            .find(|l| !l.is_empty())
            .unwrap_or("Document");

        // 2. Check if chunk already contains breadcrumb metadata
        let header_path = chunk
            .metadata
            .get("header_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let context = if !header_path.is_empty() {
            format!("Doc: {} | Section: {} | Part {}/{}", first_line, header_path, chunk_idx + 1, total_chunks)
        } else {
            format!("Doc: {} | Part {}/{}", first_line, chunk_idx + 1, total_chunks)
        };

        if context.len() > self.max_context_chars {
            Ok(context[..self.max_context_chars].to_string())
        } else {
            Ok(context)
        }
    }
}

/// Custom context generator utilizing a closure (e.g. for LLM prompt-based context generation)
pub struct CustomContextGenerator<F>
where
    F: Fn(&str, &Document, usize, usize) -> Result<String, ChunkrError> + Send + Sync,
{
    func: F,
}

impl<F> CustomContextGenerator<F>
where
    F: Fn(&str, &Document, usize, usize) -> Result<String, ChunkrError> + Send + Sync,
{
    pub fn new(func: F) -> Self {
        Self { func }
    }
}

impl<F> ContextGenerator for CustomContextGenerator<F>
where
    F: Fn(&str, &Document, usize, usize) -> Result<String, ChunkrError> + Send + Sync,
{
    fn generate_context(
        &self,
        full_doc: &str,
        chunk: &Document,
        chunk_idx: usize,
        total_chunks: usize,
    ) -> Result<String, ChunkrError> {
        (self.func)(full_doc, chunk, chunk_idx, total_chunks)
    }
}

/// Contextual chunker (Anthropic-style Contextual Retrieval)
///
/// Wraps any underlying chunker and enriches every generated chunk with situational
/// document context, section hierarchy, or LLM-generated summaries.
pub struct ContextualChunker {
    pub base_chunker: Box<dyn Chunker>,
    pub context_generator: Arc<dyn ContextGenerator>,
    pub format: ContextFormat,
}

impl std::fmt::Debug for ContextualChunker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContextualChunker")
            .field("format", &self.format)
            .finish()
    }
}

impl Clone for ContextualChunker {
    fn clone(&self) -> Self {
        Self {
            base_chunker: Box::new(RecursiveChunker::new()),
            context_generator: Arc::clone(&self.context_generator),
            format: self.format.clone(),
        }
    }
}

impl ContextualChunker {
    /// Create a ContextualChunker wrapping the default RecursiveChunker and ExtractiveContextGenerator
    pub fn new() -> Self {
        Self {
            base_chunker: Box::new(RecursiveChunker::new()),
            context_generator: Arc::new(ExtractiveContextGenerator::new()),
            format: ContextFormat::Prefix,
        }
    }

    /// Wrap a specific base chunker
    pub fn with_base_chunker(mut self, chunker: impl Chunker + 'static) -> Self {
        self.base_chunker = Box::new(chunker);
        self
    }

    /// Set a custom context generator
    pub fn with_context_generator(mut self, generator: impl ContextGenerator + 'static) -> Self {
        self.context_generator = Arc::new(generator);
        self
    }

    /// Set the context injection format
    pub fn with_format(mut self, format: ContextFormat) -> Self {
        self.format = format;
        self
    }
}

impl Default for ContextualChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl Chunker for ContextualChunker {
    fn chunk(&self, text: &str) -> Result<Vec<Document>, ChunkrError> {
        let base_docs = self.base_chunker.chunk(text)?;
        let total_chunks = base_docs.len();

        if total_chunks == 0 {
            return Ok(base_docs);
        }

        let mut enriched_docs = Vec::with_capacity(total_chunks);

        for (idx, doc) in base_docs.into_iter().enumerate() {
            let context = self.context_generator.generate_context(text, &doc, idx, total_chunks)?;
            let mut metadata = doc.metadata;
            metadata.insert("context".to_string(), Value::from(context.clone()));

            let new_content = match &self.format {
                ContextFormat::Prefix => {
                    format!("[Context: {}]\n\n{}", context, doc.content)
                }
                ContextFormat::MetadataOnly => doc.content,
                ContextFormat::Custom(template) => {
                    template
                        .replace("{context}", &context)
                        .replace("{content}", &doc.content)
                }
            };

            metadata.insert("length".to_string(), Value::from(new_content.len()));

            enriched_docs.push(Document {
                content: new_content,
                metadata,
            });
        }

        Ok(enriched_docs)
    }
}

impl BaseChunker<Result<Vec<Document>, String>> for ContextualChunker {
    fn chunk_text(
        &self,
        text: &str,
        chunk_size: usize,
        overlap: usize,
    ) -> Result<Vec<Document>, String> {
        let chunker = Self::new()
            .with_base_chunker(RecursiveChunker::new().with_chunk_size(chunk_size).with_overlap(overlap));
        chunker.chunk(text).map_err(|e| e.to_string())
    }
}
