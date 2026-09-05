#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::structures::document::Document;

/// Metadata enricher that injects cryptographic hashes, text metrics, and ID keys.
#[derive(Debug, Clone)]
pub struct MetadataEnricher {
    pub compute_hash: bool,
    pub compute_metrics: bool,
    pub add_chunk_id: bool,
    pub id_prefix: Option<String>,
}

impl MetadataEnricher {
    /// Create a new MetadataEnricher with all standard enrichments enabled
    pub fn new() -> Self {
        Self {
            compute_hash: true,
            compute_metrics: true,
            add_chunk_id: true,
            id_prefix: None,
        }
    }

    /// Set whether SHA-256 chunk hash is computed (default: true)
    pub fn with_compute_hash(mut self, compute: bool) -> Self {
        self.compute_hash = compute;
        self
    }

    /// Set whether word, char, and reading time metrics are computed (default: true)
    pub fn with_compute_metrics(mut self, compute: bool) -> Self {
        self.compute_metrics = compute;
        self
    }

    /// Set whether sequential chunk_id is generated (default: true)
    pub fn with_chunk_id(mut self, add_id: bool) -> Self {
        self.add_chunk_id = add_id;
        self
    }

    /// Set custom prefix for generated chunk IDs (e.g. "report_2024_")
    pub fn with_id_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.id_prefix = Some(prefix.into());
        self
    }

    fn compute_sha256(text: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(text.as_bytes());
        let result = hasher.finalize();
        // sha2 0.11's output no longer implements LowerHex; format the
        // digest bytes manually. Output is identical lowercase hex (64 chars).
        result.iter().map(|b| format!("{:02x}", b)).collect()
    }

    /// Enrich a single document
    pub fn enrich_document(&self, doc: &Document, index: usize) -> Document {
        let mut enriched = doc.clone();
        let text = doc.content.trim();

        if self.compute_hash {
            let hash = Self::compute_sha256(text);
            enriched
                .metadata
                .insert("chunk_hash".to_string(), Value::from(hash));
        }

        if self.compute_metrics {
            let char_count = text.chars().count();
            let word_count = text.split_whitespace().count();
            let line_count = text.lines().count();
            // Average adult reading speed: ~200 words per minute (approx 3.3 words/sec)
            let reading_time_secs = ((word_count as f32 / 200.0) * 60.0).round() as u64;

            enriched
                .metadata
                .insert("char_count".to_string(), Value::from(char_count));
            enriched
                .metadata
                .insert("word_count".to_string(), Value::from(word_count));
            enriched
                .metadata
                .insert("line_count".to_string(), Value::from(line_count));
            enriched.metadata.insert(
                "reading_time_secs".to_string(),
                Value::from(reading_time_secs),
            );
        }

        if self.add_chunk_id {
            let prefix = self.id_prefix.as_deref().unwrap_or("chunk_");
            let chunk_id = format!("{}{}", prefix, index);
            enriched
                .metadata
                .insert("chunk_id".to_string(), Value::from(chunk_id));
        }

        enriched
    }

    /// Enrich a slice of documents sequentially
    pub fn enrich(&self, docs: &[Document]) -> Vec<Document> {
        docs.iter()
            .enumerate()
            .map(|(idx, doc)| self.enrich_document(doc, idx))
            .collect()
    }

    /// Enrich a slice of documents concurrently across CPU cores (sequential on wasm32)
    pub fn par_enrich(&self, docs: &[Document]) -> Vec<Document> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            docs.par_iter()
                .enumerate()
                .map(|(idx, doc)| self.enrich_document(doc, idx))
                .collect()
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.enrich(docs)
        }
    }
}

impl Default for MetadataEnricher {
    fn default() -> Self {
        Self::new()
    }
}
