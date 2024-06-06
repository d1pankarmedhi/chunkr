use serde_json::Value;
use std::collections::HashMap;

use crate::chunker::base::BaseChunker;
use crate::structures::document::Document;

pub struct WordChunker {}
impl WordChunker {
    pub fn new() -> Self {
        Self {}
    }
}

impl BaseChunker<Result<Vec<Document>, String>> for WordChunker {
    fn chunk_text(
        &self,
        text: &str,
        chunk_size: usize,
        overlap: usize,
    ) -> Result<Vec<Document>, String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        let mut result: Vec<Document> = Vec::new();
        let mut temp_str = String::new();
        // let mut word_count = 0;
        let mut start: usize = 0;

        if text.trim().is_empty() {
            return Err("Need to pass text".to_string());
        }

        while start < words.len() {
            if (start + chunk_size) >= words.len() {
                break;
            }
            for i in 0..chunk_size {
                if !temp_str.is_empty() {
                    temp_str.push(' ');
                }
                temp_str.push_str(words[start + i]);
            }

            let mut metadata = HashMap::new();
            metadata.insert("length".to_string(), Value::from(temp_str.len()));
            metadata.insert("source".to_string(), Value::from("source"));

            let doc = Document {
                content: temp_str.trim_end().to_string(),
                metadata: metadata,
            };
            result.push(doc);
            temp_str.clear();
            start += chunk_size - overlap;
        }

        while start < words.len() {
            temp_str.push_str(words[start]);
            start += 1;
            if !temp_str.is_empty() {
                temp_str.push(' ');
            }
        }

        let mut metadata = HashMap::new();
        metadata.insert("length".to_string(), Value::from(temp_str.len()));
        metadata.insert("source".to_string(), Value::from("source"));

        let doc = Document {
            content: temp_str.trim_end().to_string(),
            metadata: metadata,
        };
        result.push(doc);
        Ok(result)
    }
}
