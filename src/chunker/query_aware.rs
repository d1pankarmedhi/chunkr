use std::collections::{HashMap, HashSet};
use serde_json::Value;

use crate::chunker::base::{BaseChunker, Chunker};
use crate::chunker::sentence::SentenceChunker;
use crate::error::ChunkrError;
use crate::structures::document::Document;

/// Adaptive, query-aware chunking strategy that alters chunk resolution around query keywords
#[derive(Debug, Clone)]
pub struct QueryAwareChunker {
    pub query: String,
    pub hotspot_sentences_per_chunk: usize,
    pub hotspot_overlap: usize,
    pub context_sentences_per_chunk: usize,
    pub context_overlap: usize,
    pub relevance_threshold: f64,
}

impl QueryAwareChunker {
    /// Create a QueryAwareChunker for a specific search query
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            hotspot_sentences_per_chunk: 2,
            hotspot_overlap: 1,
            context_sentences_per_chunk: 5,
            context_overlap: 1,
            relevance_threshold: 0.1,
        }
    }

    /// Builder for query string
    pub fn with_query(mut self, query: impl Into<String>) -> Self {
        self.query = query.into();
        self
    }

    /// Set sizing for query hotspot regions (dense, focused chunks)
    pub fn with_hotspot_sizing(mut self, sentences: usize, overlap: usize) -> Self {
        self.hotspot_sentences_per_chunk = sentences.max(1);
        self.hotspot_overlap = overlap.min(sentences.saturating_sub(1));
        self
    }

    /// Set sizing for background context regions (broad context chunks)
    pub fn with_context_sizing(mut self, sentences: usize, overlap: usize) -> Self {
        self.context_sentences_per_chunk = sentences.max(1);
        self.context_overlap = overlap.min(sentences.saturating_sub(1));
        self
    }

    /// Set relevance score threshold (0.0 to 1.0)
    pub fn with_relevance_threshold(mut self, threshold: f64) -> Self {
        self.relevance_threshold = threshold;
        self
    }

    /// Calculate query relevance score and matched terms for a sentence.
    ///
    /// `query_lower` is the lowercased query, hoisted by the caller so it is
    /// computed once per `chunk()` call instead of once per sentence.
    fn score_sentence(
        &self,
        sentence: &str,
        query_terms: &HashSet<String>,
        query_lower: &str,
    ) -> (f64, Vec<String>) {
        let words: Vec<String> = sentence
            .split_whitespace()
            .map(|w| {
                w.to_lowercase()
                    .trim_matches(|c: char| !c.is_alphanumeric())
                    .to_string()
            })
            .filter(|w| !w.is_empty())
            .collect();

        if words.is_empty() || query_terms.is_empty() {
            return (0.0, Vec::new());
        }

        let mut matched = Vec::new();
        let mut match_count = 0;

        for word in &words {
            if query_terms.contains(word) {
                if !matched.contains(word) {
                    matched.push(word.clone());
                }
                match_count += 1;
            }
        }

        // Exact phrase bonus if whole query appears in sentence
        let sent_lower = sentence.to_lowercase();
        let phrase_bonus = if sent_lower.contains(query_lower) { 0.5 } else { 0.0 };

        let term_density = match_count as f64 / words.len() as f64;
        let score = (term_density + phrase_bonus).min(1.0);

        (score, matched)
    }
}

impl Chunker for QueryAwareChunker {
    fn chunk(&self, text: &str) -> Result<Vec<Document>, ChunkrError> {
        if text.trim().is_empty() {
            return Err(ChunkrError::EmptyInput);
        }
        // Guard against post-construction mutation of the public fields:
        // a zero window or overlap >= window would underflow `size - overlap`
        // and spin/hang the loop below.
        for (size, overlap) in [
            (self.hotspot_sentences_per_chunk, self.hotspot_overlap),
            (self.context_sentences_per_chunk, self.context_overlap),
        ] {
            if size == 0 {
                return Err(ChunkrError::InvalidChunkSize(0));
            }
            if overlap >= size {
                return Err(ChunkrError::InvalidOverlap {
                    chunk_size: size,
                    overlap,
                });
            }
        }

        let query_terms: HashSet<String> = self
            .query
            .split_whitespace()
            .map(|w| {
                w.to_lowercase()
                    .trim_matches(|c: char| !c.is_alphanumeric())
                    .to_string()
            })
            .filter(|w| w.len() > 1)
            .collect();

        let sentences = SentenceChunker::split_sentences(text);
        if sentences.is_empty() {
            return Err(ChunkrError::EmptyInput);
        }

        // Lowercase the query once; `score_sentence` is called per sentence.
        let query_lower = self.query.to_lowercase();

        let mut scored_sentences: Vec<(&str, f64, Vec<String>)> = Vec::with_capacity(sentences.len());
        for &sent in &sentences {
            let (score, matched) = self.score_sentence(sent, &query_terms, &query_lower);
            scored_sentences.push((sent, score, matched));
        }

        let mut result = Vec::new();
        let mut i = 0;
        let mut chunk_idx = 0;
        let n = scored_sentences.len();

        while i < n {
            let is_hotspot = scored_sentences[i].1 >= self.relevance_threshold;
            let (window_size, step) = if is_hotspot {
                let size = self.hotspot_sentences_per_chunk;
                let overlap = self.hotspot_overlap;
                (size, size - overlap)
            } else {
                let size = self.context_sentences_per_chunk;
                let overlap = self.context_overlap;
                (size, size - overlap)
            };

            let end = (i + window_size).min(n);
            let chunk_slice = &scored_sentences[i..end];

            let chunk_content = chunk_slice
                .iter()
                .map(|(s, _, _)| *s)
                .collect::<Vec<&str>>()
                .join(" ");

            let mut all_matched_terms = Vec::new();
            let mut max_score: f64 = 0.0;
            for (_, score, terms) in chunk_slice {
                if *score > max_score {
                    max_score = *score;
                }
                for t in terms {
                    if !all_matched_terms.contains(t) {
                        all_matched_terms.push(t.clone());
                    }
                }
            }

            let mut metadata = HashMap::with_capacity(7);
            metadata.insert("length".to_string(), Value::from(chunk_content.len()));
            metadata.insert("chunk_index".to_string(), Value::from(chunk_idx));
            metadata.insert("query".to_string(), Value::from(self.query.clone()));
            metadata.insert("is_hotspot".to_string(), Value::from(is_hotspot));
            metadata.insert("chunk_type".to_string(), Value::from(if is_hotspot { "hotspot" } else { "context" }));
            metadata.insert("relevance_score".to_string(), Value::from((max_score * 1000.0).round() / 1000.0));
            metadata.insert("matched_terms".to_string(), serde_json::to_value(&all_matched_terms).unwrap_or(Value::Null));

            result.push(Document {
                content: chunk_content,
                metadata,
            });
            chunk_idx += 1;

            if end == n {
                break;
            }

            i += step.max(1);
        }

        Ok(result)
    }
}

impl BaseChunker<Result<Vec<Document>, String>> for QueryAwareChunker {
    fn chunk_text(
        &self,
        text: &str,
        chunk_size: usize,
        overlap: usize,
    ) -> Result<Vec<Document>, String> {
        let chunker = self
            .clone()
            .with_hotspot_sizing(chunk_size.max(1), overlap.min(chunk_size.max(1) - 1));
        chunker.chunk(text).map_err(|e| e.to_string())
    }
}
