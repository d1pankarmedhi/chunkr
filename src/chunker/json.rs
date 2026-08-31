use std::collections::HashMap;
use serde_json::Value;

use crate::chunker::base::{BaseChunker, Chunker};
use crate::chunker::recursive::RecursiveChunker;
use crate::error::ChunkrError;
use crate::structures::document::Document;

/// Structure-aware chunker for JSON payloads
#[derive(Debug, Clone)]
pub struct JsonChunker {
    pub max_chunk_size: usize,
    pub pretty: bool,
    fallback_chunker: RecursiveChunker,
}

impl JsonChunker {
    pub fn new() -> Self {
        Self {
            max_chunk_size: 1500,
            pretty: true,
            fallback_chunker: RecursiveChunker::new()
                .with_chunk_size(1500)
                .with_overlap(150),
        }
    }

    pub fn with_max_chunk_size(mut self, max_size: usize) -> Self {
        self.max_chunk_size = max_size;
        self.fallback_chunker = self.fallback_chunker.with_chunk_size(max_size);
        self
    }

    pub fn with_pretty(mut self, pretty: bool) -> Self {
        self.pretty = pretty;
        self
    }

    fn serialize_value(&self, val: &Value) -> String {
        if self.pretty {
            serde_json::to_string_pretty(val).unwrap_or_default()
        } else {
            serde_json::to_string(val).unwrap_or_default()
        }
    }

    fn chunk_json_value(
        &self,
        value: &Value,
        path: &str,
        docs: &mut Vec<Document>,
    ) -> Result<(), ChunkrError> {
        let serialized = self.serialize_value(value);

        if serialized.chars().count() <= self.max_chunk_size {
            let mut metadata = HashMap::new();
            metadata.insert("path".to_string(), Value::from(if path.is_empty() { "$" } else { path }));
            metadata.insert("length".to_string(), Value::from(serialized.len()));
            metadata.insert("chunk_index".to_string(), Value::from(docs.len()));
            metadata.insert("is_json".to_string(), Value::from(true));

            docs.push(Document {
                content: serialized,
                metadata,
            });
            return Ok(());
        }

        match value {
            Value::Object(map) => {
                for (key, child_val) in map {
                    let child_path = if path.is_empty() {
                        format!("$.{}", key)
                    } else {
                        format!("{}.{}", path, key)
                    };
                    self.chunk_json_value(child_val, &child_path, docs)?;
                }
            }
            Value::Array(arr) => {
                let mut current_batch = Vec::new();
                let mut start_idx = 0;

                for (idx, item) in arr.iter().enumerate() {
                    let item_str = self.serialize_value(item);
                    if item_str.chars().count() > self.max_chunk_size {
                        if !current_batch.is_empty() {
                            let batch_val = Value::Array(current_batch.clone());
                            let batch_str = self.serialize_value(&batch_val);
                            let batch_path = format!("{}[{}..{}]", path, start_idx, idx);

                            let mut metadata = HashMap::new();
                            metadata.insert("path".to_string(), Value::from(batch_path));
                            metadata.insert("length".to_string(), Value::from(batch_str.len()));
                            metadata.insert("chunk_index".to_string(), Value::from(docs.len()));
                            metadata.insert("is_json".to_string(), Value::from(true));

                            docs.push(Document {
                                content: batch_str,
                                metadata,
                            });
                            current_batch.clear();
                        }
                        let item_path = format!("{}[{}]", path, idx);
                        self.chunk_json_value(item, &item_path, docs)?;
                        start_idx = idx + 1;
                    } else {
                        current_batch.push(item.clone());
                        let batch_val = Value::Array(current_batch.clone());
                        let batch_str = self.serialize_value(&batch_val);

                        if batch_str.chars().count() > self.max_chunk_size {
                            current_batch.pop();
                            let prev_val = Value::Array(current_batch.clone());
                            let prev_str = self.serialize_value(&prev_val);
                            let batch_path = format!("{}[{}..{}]", path, start_idx, idx);

                            let mut metadata = HashMap::new();
                            metadata.insert("path".to_string(), Value::from(batch_path));
                            metadata.insert("length".to_string(), Value::from(prev_str.len()));
                            metadata.insert("chunk_index".to_string(), Value::from(docs.len()));
                            metadata.insert("is_json".to_string(), Value::from(true));

                            docs.push(Document {
                                content: prev_str,
                                metadata,
                            });
                            current_batch = vec![item.clone()];
                            start_idx = idx;
                        }
                    }
                }

                if !current_batch.is_empty() {
                    let batch_val = Value::Array(current_batch);
                    let batch_str = self.serialize_value(&batch_val);
                    let batch_path = format!("{}[{}..{}]", path, start_idx, arr.len());

                    let mut metadata = HashMap::new();
                    metadata.insert("path".to_string(), Value::from(batch_path));
                    metadata.insert("length".to_string(), Value::from(batch_str.len()));
                    metadata.insert("chunk_index".to_string(), Value::from(docs.len()));
                    metadata.insert("is_json".to_string(), Value::from(true));

                    docs.push(Document {
                        content: batch_str,
                        metadata,
                    });
                }
            }
            _ => {
                // Scalar primitive larger than max_chunk_size
                let sub_docs = self.fallback_chunker.chunk(&serialized)?;
                docs.extend(sub_docs);
            }
        }

        Ok(())
    }
}

impl Default for JsonChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl Chunker for JsonChunker {
    fn chunk(&self, text: &str) -> Result<Vec<Document>, ChunkrError> {
        if text.trim().is_empty() {
            return Err(ChunkrError::EmptyInput);
        }

        match serde_json::from_str::<Value>(text) {
            Ok(json_value) => {
                let mut docs = Vec::new();
                self.chunk_json_value(&json_value, "", &mut docs)?;
                Ok(docs)
            }
            Err(_) => {
                // Fallback to recursive chunker if not valid JSON
                self.fallback_chunker.chunk(text)
            }
        }
    }
}

impl BaseChunker<Result<Vec<Document>, String>> for JsonChunker {
    fn chunk_text(
        &self,
        text: &str,
        chunk_size: usize,
        _overlap: usize,
    ) -> Result<Vec<Document>, String> {
        let chunker = self
            .clone()
            .with_max_chunk_size(chunk_size);
        chunker.chunk(text).map_err(|e| e.to_string())
    }
}
