use serde_json::Value;
use std::collections::{HashMap, HashSet};

use crate::structures::document::Document;

/// Deduplicator that removes redundant chunks across the dataset.
#[derive(Debug, Clone)]
pub struct ChunkDeduplicator {
    pub exact: bool,
    pub case_sensitive: bool,
    pub track_duplicates: bool,
}

impl ChunkDeduplicator {
    /// Create a new ChunkDeduplicator
    pub fn new() -> Self {
        Self {
            exact: true,
            case_sensitive: true,
            track_duplicates: true,
        }
    }

    /// Set whether exact string matching is used (default: true)
    pub fn with_exact(mut self, exact: bool) -> Self {
        self.exact = exact;
        self
    }

    /// Set whether matching is case sensitive (default: true)
    pub fn with_case_sensitive(mut self, case_sensitive: bool) -> Self {
        self.case_sensitive = case_sensitive;
        self
    }

    /// Set whether duplicate occurrences are counted in the chunk metadata (default: true)
    pub fn with_track_duplicates(mut self, track: bool) -> Self {
        self.track_duplicates = track;
        self
    }

    fn normalize_key(&self, text: &str) -> String {
        let trimmed = text.trim();
        if self.exact {
            if self.case_sensitive {
                trimmed.to_string()
            } else {
                trimmed.to_lowercase()
            }
        } else {
            // Normalized: collapse whitespace & optionally lowercase
            let mut words = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
            if !self.case_sensitive {
                words = words.to_lowercase();
            }
            words
        }
    }

    /// Deduplicate a slice of documents in-order
    pub fn deduplicate(&self, docs: &[Document]) -> Vec<Document> {
        let mut seen = HashSet::new();
        let mut key_to_index: HashMap<String, usize> = HashMap::new();
        let mut result: Vec<Document> = Vec::new();

        for doc in docs {
            let key = self.normalize_key(&doc.content);
            if key.is_empty() {
                continue;
            }

            if seen.insert(key.clone()) {
                let idx = result.len();
                let mut new_doc = doc.clone();
                if self.track_duplicates {
                    new_doc
                        .metadata
                        .insert("duplicate_count".to_string(), Value::from(1));
                }
                result.push(new_doc);
                key_to_index.insert(key, idx);
            } else if self.track_duplicates {
                if let Some(&orig_idx) = key_to_index.get(&key) {
                    if let Some(val) = result[orig_idx].metadata.get_mut("duplicate_count") {
                        if let Some(count) = val.as_u64() {
                            *val = Value::from(count + 1);
                        }
                    }
                }
            }
        }

        result
    }
}

impl Default for ChunkDeduplicator {
    fn default() -> Self {
        Self::new()
    }
}
