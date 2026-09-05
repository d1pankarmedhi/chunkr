use serde_json::Value;
use std::collections::HashMap;

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

    /// Length (in chars) that `item_str` — the standalone serialization of one
    /// array element — occupies once embedded in a standalone serialized batch.
    ///
    /// Compact mode embeds items verbatim. Pretty mode adds 2 spaces to every
    /// newline already inside the item from the extra nesting level (the
    /// per-item `"  "` base indent is accounted in `batch_total_chars`).
    fn embedded_item_chars(item_str: &str, pretty: bool) -> usize {
        let chars = item_str.chars().count();
        if !pretty {
            return chars;
        }
        let newlines = item_str.bytes().filter(|&b| b == b'\n').count();
        chars + 2 * newlines
    }

    /// Standalone serialized char length of a batch holding `count` items whose
    /// embedded lengths sum to `body`.
    ///
    /// Compact: `"[" + items.join(",") + "]"`. Pretty: `"[\n" + ("  " + item)
    /// joined by ",\n" + "\n]"`.
    fn batch_total_chars(count: usize, body: usize, pretty: bool) -> usize {
        if count == 0 {
            return 0;
        }
        if pretty {
            body + 4 * count + 2
        } else {
            body + count + 1
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
            metadata.insert(
                "path".to_string(),
                Value::from(if path.is_empty() { "$" } else { path }),
            );
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
                // Incremental batch-size tracking: each item is serialized
                // exactly once, and the batch is serialized only when flushed.
                // (Previously the whole growing batch was cloned + re-serialized
                // per item — quadratic on large arrays.) `batch_body` tracks the
                // sum of per-item embedded lengths so the standalone batch char
                // length is exact; debug builds verify this at every flush.
                let mut current_batch: Vec<Value> = Vec::new();
                let mut batch_body: usize = 0;
                let mut start_idx = 0;

                // Flush helper shared by the loop and the trailing batch.
                // `body` is the tracked sum of embedded item lengths; the
                // debug assertion proves tracking matches real serialization.
                let flush_batch = |batch: &mut Vec<Value>,
                                   body: &mut usize,
                                   start: usize,
                                   end: usize,
                                   docs: &mut Vec<Document>| {
                    if batch.is_empty() {
                        return;
                    }
                    let count = batch.len();
                    let batch_val = Value::Array(std::mem::take(batch));
                    let batch_str = self.serialize_value(&batch_val);
                    debug_assert_eq!(
                        batch_str.chars().count(),
                        Self::batch_total_chars(count, *body, self.pretty),
                        "tracked batch length diverged from serialized length"
                    );
                    let batch_path = format!("{}[{}..{}]", path, start, end);

                    let mut metadata = HashMap::new();
                    metadata.insert("path".to_string(), Value::from(batch_path));
                    metadata.insert("length".to_string(), Value::from(batch_str.len()));
                    metadata.insert("chunk_index".to_string(), Value::from(docs.len()));
                    metadata.insert("is_json".to_string(), Value::from(true));

                    docs.push(Document {
                        content: batch_str,
                        metadata,
                    });
                    *body = 0;
                };

                for (idx, item) in arr.iter().enumerate() {
                    let item_str = self.serialize_value(item);
                    if item_str.chars().count() > self.max_chunk_size {
                        flush_batch(&mut current_batch, &mut batch_body, start_idx, idx, docs);
                        let item_path = format!("{}[{}]", path, idx);
                        self.chunk_json_value(item, &item_path, docs)?;
                        start_idx = idx + 1;
                    } else {
                        // Embedded length of this item inside a standalone batch.
                        let emb = Self::embedded_item_chars(&item_str, self.pretty);
                        let projected = Self::batch_total_chars(
                            current_batch.len() + 1,
                            batch_body + emb,
                            self.pretty,
                        );
                        if !current_batch.is_empty() && projected > self.max_chunk_size {
                            flush_batch(&mut current_batch, &mut batch_body, start_idx, idx, docs);
                            start_idx = idx;
                        }
                        // Single clone per item; batch serialized only on flush.
                        batch_body += emb;
                        current_batch.push(item.clone());
                    }
                }

                flush_batch(
                    &mut current_batch,
                    &mut batch_body,
                    start_idx,
                    arr.len(),
                    docs,
                );
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
        let chunker = self.clone().with_max_chunk_size(chunk_size);
        chunker.chunk(text).map_err(|e| e.to_string())
    }
}
