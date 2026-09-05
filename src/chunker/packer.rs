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

/// Metadata keys that denote a single source chunk's position. When a
/// merged chunk spans multiple sources whose values for one of these keys
/// differ, the singular key is replaced by `merged_<key>` holding the
/// per-source values in merge order.
const SPAN_SENSITIVE_KEYS: &[&str] = &[
    "page_number",
    "header_path",
    "headers",
    "start_row",
    "end_row",
    "start_char",
    "end_char",
    "start_word",
    "end_word",
    "token_start",
    "token_end",
    "char_start",
    "char_end",
    "start_sentence",
    "end_sentence",
    "start_paragraph",
    "end_paragraph",
];

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
        let mut current_metas: Vec<HashMap<String, Value>> = Vec::new();
        let mut base_metadata: HashMap<String, Value> = HashMap::new();

        let flush_current = |content: &mut String,
                             sources: &mut Vec<usize>,
                             source_metas: &mut Vec<HashMap<String, Value>>,
                             meta: &mut HashMap<String, Value>,
                             packed_list: &mut Vec<Document>| {
            if !content.is_empty() {
                let mut chunk_meta = meta.clone();
                if sources.len() > 1 {
                    Self::reconcile_span_metadata(&mut chunk_meta, source_metas);
                }
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
                source_metas.clear();
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
                current_metas.push(chunk.metadata.clone());
            } else {
                flush_current(
                    &mut current_content,
                    &mut current_sources,
                    &mut current_metas,
                    &mut base_metadata,
                    &mut packed,
                );
                current_content.push_str(chunk_text);
                current_sources.push(idx);
                current_metas.push(chunk.metadata.clone());
                base_metadata = chunk.metadata.clone();
            }
        }

        flush_current(
            &mut current_content,
            &mut current_sources,
            &mut current_metas,
            &mut base_metadata,
            &mut packed,
        );

        packed
    }

    /// Reconcile span-sensitive metadata for a multi-source merged chunk.
    ///
    /// For each listed key present in any source: if every present value is
    /// identical, the merged chunk keeps that single value; otherwise the
    /// singular key is removed and `merged_<key>` records the per-source
    /// values in merge order (`Null` where a source lacks the key).
    /// Non-listed keys keep first-wins inheritance via `base_metadata`.
    fn reconcile_span_metadata(
        chunk_meta: &mut HashMap<String, Value>,
        source_metas: &[HashMap<String, Value>],
    ) {
        for key in SPAN_SENSITIVE_KEYS {
            let present: Vec<&Value> = source_metas
                .iter()
                .filter_map(|m| m.get(*key))
                .collect();
            if present.is_empty() {
                continue;
            }
            if present.iter().all(|v| *v == present[0]) {
                chunk_meta.insert((*key).to_string(), present[0].clone());
            } else {
                chunk_meta.remove(*key);
                let per_source: Vec<Value> = source_metas
                    .iter()
                    .map(|m| m.get(*key).cloned().unwrap_or(Value::Null))
                    .collect();
                chunk_meta.insert(format!("merged_{key}"), Value::Array(per_source));
            }
        }
    }
}

impl Default for ChunkPacker {
    fn default() -> Self {
        Self::new(1000)
    }
}
