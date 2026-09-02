use std::collections::HashMap;
use serde_json::Value;

use crate::structures::document::Document;

/// Post-processing optimizer that greedily bin-packs small fragmented chunks
/// into cohesive chunks up to a character budget.
#[derive(Debug, Clone)]
pub struct ChunkPacker {
    pub max_characters: usize,
    pub separator: String,
}

impl ChunkPacker {
    /// Create a new ChunkPacker with the specified maximum character budget
    pub fn new(max_characters: usize) -> Self {
        Self {
            max_characters,
            separator: "\n\n".to_string(),
        }
    }

    /// Set the string separator placed between merged chunks (default: "\n\n")
    pub fn with_separator(mut self, separator: impl Into<String>) -> Self {
        self.separator = separator.into();
        self
    }

    /// Pack a list of documents into larger combined chunks up to `max_characters`
    pub fn pack(&self, chunks: &[Document]) -> Vec<Document> {
        if chunks.is_empty() {
            return Vec::new();
        }

        let mut packed = Vec::new();
        let mut current_content = String::new();
        let mut current_sources = Vec::new();
        let mut base_metadata: HashMap<String, Value> = HashMap::new();

        let flush_current = |content: &mut String,
                             sources: &mut Vec<usize>,
                             meta: &mut HashMap<String, Value>,
                             packed_list: &mut Vec<Document>| {
            if !content.is_empty() {
                let mut chunk_meta = meta.clone();
                chunk_meta.insert("merged_chunk_count".to_string(), Value::from(sources.len()));
                chunk_meta.insert(
                    "source_indices".to_string(),
                    serde_json::to_value(&sources).unwrap_or(Value::Null),
                );
                chunk_meta.insert("length".to_string(), Value::from(content.len()));
                chunk_meta.insert("chunk_index".to_string(), Value::from(packed_list.len()));

                packed_list.push(Document {
                    content: content.clone(),
                    metadata: chunk_meta,
                });
                content.clear();
                sources.clear();
                meta.clear();
            }
        };

        for (idx, chunk) in chunks.iter().enumerate() {
            let chunk_text = chunk.content.trim();
            if chunk_text.is_empty() {
                continue;
            }

            let sep_len = if current_content.is_empty() {
                0
            } else {
                self.separator.len()
            };
            let projected_len = current_content.len() + sep_len + chunk_text.len();

            if projected_len <= self.max_characters || current_content.is_empty() {
                if current_content.is_empty() {
                    base_metadata = chunk.metadata.clone();
                } else {
                    current_content.push_str(&self.separator);
                }
                current_content.push_str(chunk_text);
                current_sources.push(idx);
            } else {
                flush_current(
                    &mut current_content,
                    &mut current_sources,
                    &mut base_metadata,
                    &mut packed,
                );
                current_content.push_str(chunk_text);
                current_sources.push(idx);
                base_metadata = chunk.metadata.clone();
            }
        }

        flush_current(
            &mut current_content,
            &mut current_sources,
            &mut base_metadata,
            &mut packed,
        );

        packed
    }
}

impl Default for ChunkPacker {
    fn default() -> Self {
        Self::new(1000)
    }
}
