use std::collections::HashMap;
use std::path::Path;
use lopdf::Document as LopdfDoc;
use serde_json::Value;

use super::base::BaseLoader;
use crate::error::ChunkrError;
use crate::structures::document::Document;

/// High-performance PDF loader for extracting text and structured page documents.
#[derive(Debug, Clone, Default)]
pub struct PDFLoader {}

impl PDFLoader {
    /// Create a new PDFLoader instance.
    pub fn new() -> Self {
        Self {}
    }

    /// Load and extract all text from a PDF file located at the specified path.
    pub fn load_from_file<P: AsRef<Path>>(&self, path: P) -> Result<String, ChunkrError> {
        let path_ref = path.as_ref();
        let doc = LopdfDoc::load(path_ref)?;
        Self::extract_all_text(&doc)
    }

    /// Load and extract all text from PDF bytes in memory.
    pub fn load_from_bytes(&self, bytes: &[u8]) -> Result<String, ChunkrError> {
        let doc = LopdfDoc::load_mem(bytes)?;
        Self::extract_all_text(&doc)
    }

    /// Load a PDF file into a single `Document` structure with file and page metadata.
    pub fn load_document<P: AsRef<Path>>(&self, path: P) -> Result<Document, ChunkrError> {
        let path_ref = path.as_ref();
        let doc = LopdfDoc::load(path_ref)?;
        let content = Self::extract_all_text(&doc)?;
        let total_pages = doc.get_pages().len();

        let mut metadata = HashMap::new();
        metadata.insert("source".to_string(), Value::from(path_ref.to_string_lossy().to_string()));
        if let Some(file_name) = path_ref.file_name() {
            metadata.insert("file_name".to_string(), Value::from(file_name.to_string_lossy().to_string()));
        }
        metadata.insert("total_pages".to_string(), Value::from(total_pages));

        Ok(Document::new(content, metadata))
    }

    /// Load PDF bytes into a single `Document` structure with metadata.
    pub fn load_document_from_bytes(&self, bytes: &[u8]) -> Result<Document, ChunkrError> {
        let doc = LopdfDoc::load_mem(bytes)?;
        let content = Self::extract_all_text(&doc)?;
        let total_pages = doc.get_pages().len();

        let mut metadata = HashMap::new();
        metadata.insert("total_pages".to_string(), Value::from(total_pages));

        Ok(Document::new(content, metadata))
    }

    /// Load each page of a PDF file as an individual `Document` with page number and source metadata.
    pub fn load_pages_from_file<P: AsRef<Path>>(&self, path: P) -> Result<Vec<Document>, ChunkrError> {
        let path_ref = path.as_ref();
        let doc = LopdfDoc::load(path_ref)?;
        let source_str = path_ref.to_string_lossy().to_string();
        let file_name_str = path_ref.file_name().map(|f| f.to_string_lossy().to_string());
        Self::extract_pages(&doc, Some(&source_str), file_name_str.as_deref())
    }

    /// Load each page from PDF bytes as an individual `Document` with page number metadata.
    pub fn load_pages_from_bytes(&self, bytes: &[u8]) -> Result<Vec<Document>, ChunkrError> {
        let doc = LopdfDoc::load_mem(bytes)?;
        Self::extract_pages(&doc, None, None)
    }

    /// Helper to extract all text across all pages of a loaded lopdf document.
    fn extract_all_text(doc: &LopdfDoc) -> Result<String, ChunkrError> {
        let pages = doc.get_pages();
        let mut content = String::new();
        for (&page_num, _) in pages.iter() {
            if let Ok(text) = doc.extract_text(&[page_num]) {
                content.push_str(&text);
            }
        }
        Ok(content)
    }

    /// Helper to extract individual pages as `Document` instances.
    fn extract_pages(
        doc: &LopdfDoc,
        source: Option<&str>,
        file_name: Option<&str>,
    ) -> Result<Vec<Document>, ChunkrError> {
        let pages = doc.get_pages();
        let total_pages = pages.len();
        let mut result = Vec::with_capacity(total_pages);

        for (&page_num, _) in pages.iter() {
            let page_text = doc.extract_text(&[page_num]).unwrap_or_default();
            let mut metadata = HashMap::new();
            metadata.insert("page_number".to_string(), Value::from(page_num));
            metadata.insert("total_pages".to_string(), Value::from(total_pages));

            if let Some(src) = source {
                metadata.insert("source".to_string(), Value::from(src));
            }
            if let Some(fname) = file_name {
                metadata.insert("file_name".to_string(), Value::from(fname));
            }

            result.push(Document::new(page_text, metadata));
        }

        Ok(result)
    }
}

impl BaseLoader<Result<String, ChunkrError>> for PDFLoader {
    fn load_from_file(&self, path: &str) -> Result<String, ChunkrError> {
        self.load_from_file(path)
    }
}
