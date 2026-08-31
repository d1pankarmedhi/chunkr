use serde_json::Value;

use crate::chunker::base::{BaseChunker, Chunker};
use crate::chunker::recursive::{KeepSeparator, RecursiveChunker};
use crate::error::ChunkrError;
use crate::structures::document::Document;

/// Structure-aware chunker for HTML documents
#[derive(Debug, Clone)]
pub struct HtmlChunker {
    pub chunk_size: usize,
    pub overlap: usize,
    recursive_chunker: RecursiveChunker,
}

impl HtmlChunker {
    pub fn new() -> Self {
        let seps = vec![
            "\n<article".to_string(),
            "\n<section".to_string(),
            "\n<main".to_string(),
            "\n<div".to_string(),
            "\n<h1".to_string(),
            "\n<h2".to_string(),
            "\n<h3".to_string(),
            "\n<h4".to_string(),
            "\n<table".to_string(),
            "\n<ul".to_string(),
            "\n<ol".to_string(),
            "\n<p".to_string(),
            "\n\n".to_string(),
            "\n".to_string(),
            " ".to_string(),
            "".to_string(),
        ];

        let recursive = RecursiveChunker::new()
            .with_chunk_size(1200)
            .with_overlap(150)
            .with_separators(seps)
            .with_keep_separator(KeepSeparator::Start);

        Self {
            chunk_size: 1200,
            overlap: 150,
            recursive_chunker: recursive,
        }
    }

    pub fn with_chunk_size(mut self, chunk_size: usize) -> Self {
        self.chunk_size = chunk_size;
        self.recursive_chunker = self.recursive_chunker.with_chunk_size(chunk_size);
        self
    }

    pub fn with_overlap(mut self, overlap: usize) -> Self {
        self.overlap = overlap;
        self.recursive_chunker = self.recursive_chunker.with_overlap(overlap);
        self
    }
}

impl Default for HtmlChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl Chunker for HtmlChunker {
    fn chunk(&self, text: &str) -> Result<Vec<Document>, ChunkrError> {
        let mut docs = self.recursive_chunker.chunk(text)?;
        for doc in &mut docs {
            doc.add_metadata("format", Value::from("html"));
        }
        Ok(docs)
    }
}

impl BaseChunker<Result<Vec<Document>, String>> for HtmlChunker {
    fn chunk_text(
        &self,
        text: &str,
        chunk_size: usize,
        overlap: usize,
    ) -> Result<Vec<Document>, String> {
        let chunker = self
            .clone()
            .with_chunk_size(chunk_size)
            .with_overlap(overlap);
        chunker.chunk(text).map_err(|e| e.to_string())
    }
}
