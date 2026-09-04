use crate::error::ChunkrError;
use crate::structures::document::Document;
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;

/// Legacy chunker trait maintained for backward compatibility
pub trait BaseChunker<O> {
    fn chunk_text(&self, text: &str, chunk_size: usize, overlap: usize) -> O;
}

/// Standard interface for all modern chunkers in Chunkr
pub trait Chunker: Send + Sync {
    /// Split a single text string into chunks as Documents
    fn chunk(&self, text: &str) -> Result<Vec<Document>, ChunkrError>;

    /// Split multiple documents into chunks, inheriting and enriching original metadata
    fn chunk_documents(&self, docs: &[Document]) -> Result<Vec<Document>, ChunkrError> {
        let mut all_chunks = Vec::new();
        for (doc_idx, doc) in docs.iter().enumerate() {
            if doc.content.trim().is_empty() {
                continue;
            }
            let chunks = self.chunk(&doc.content)?;
            for (chunk_idx, mut chunk) in chunks.into_iter().enumerate() {
                for (k, v) in &doc.metadata {
                    chunk.metadata.entry(k.clone()).or_insert_with(|| v.clone());
                }
                chunk.add_metadata("doc_index", serde_json::Value::from(doc_idx));
                chunk.add_metadata("chunk_index", serde_json::Value::from(chunk_idx));
                all_chunks.push(chunk);
            }
        }
        Ok(all_chunks)
    }

    /// Parallel chunking of multiple documents across CPU cores using Rayon (sequential on wasm32)
    fn par_chunk_documents(&self, docs: &[Document]) -> Result<Vec<Document>, ChunkrError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let results: Result<Vec<Vec<Document>>, ChunkrError> = docs
                .par_iter()
                .enumerate()
                .map(|(doc_idx, doc)| {
                    if doc.content.trim().is_empty() {
                        return Ok(Vec::new());
                    }
                    let chunks = self.chunk(&doc.content)?;
                    let enriched = chunks
                        .into_iter()
                        .enumerate()
                        .map(|(chunk_idx, mut chunk)| {
                            for (k, v) in &doc.metadata {
                                chunk.metadata.entry(k.clone()).or_insert_with(|| v.clone());
                            }
                            chunk.add_metadata("doc_index", serde_json::Value::from(doc_idx));
                            chunk.add_metadata("chunk_index", serde_json::Value::from(chunk_idx));
                            chunk
                        })
                        .collect();
                    Ok(enriched)
                })
                .collect();

            results.map(|chunk_lists| chunk_lists.into_iter().flatten().collect())
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.chunk_documents(docs)
        }
    }

    /// Parallel chunking of multiple raw text strings (sequential on wasm32)
    fn par_chunk_texts(&self, texts: &[&str]) -> Result<Vec<Vec<Document>>, ChunkrError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            texts
                .par_iter()
                .map(|text| {
                    if text.trim().is_empty() {
                        Ok(Vec::new())
                    } else {
                        self.chunk(text)
                    }
                })
                .collect()
        }
        #[cfg(target_arch = "wasm32")]
        {
            texts
                .iter()
                .map(|text| {
                    if text.trim().is_empty() {
                        Ok(Vec::new())
                    } else {
                        self.chunk(text)
                    }
                })
                .collect()
        }
    }
}
