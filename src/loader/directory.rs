use std::collections::HashMap;
use std::path::{Path, PathBuf};
use rayon::prelude::*;
use serde_json::Value;

use crate::chunker::base::Chunker;
use crate::chunker::code::{CodeChunker, CodeLanguage};
use crate::chunker::html::HtmlChunker;
use crate::chunker::json::JsonChunker;
use crate::chunker::markdown::MarkdownChunker;
use crate::chunker::recursive::RecursiveChunker;
use crate::chunker::table::TableChunker;
use crate::error::ChunkrError;
use crate::loader::base::BaseLoader;
use crate::loader::pdf::PDFLoader;
use crate::structures::document::Document;

/// High-performance multi-threaded directory loader with automatic chunker routing by file extension.
#[derive(Debug, Clone)]
pub struct DirectoryLoader {
    pub recursive: bool,
    pub extensions: Option<Vec<String>>,
    pub excludes: Vec<String>,
    pub chunk_size: usize,
    pub overlap: usize,
    pdf_loader: PDFLoader,
}

impl DirectoryLoader {
    /// Create a new DirectoryLoader with standard defaults:
    /// - `recursive`: true
    /// - `extensions`: None (all supported extensions)
    /// - `chunk_size`: 1000
    /// - `overlap`: 150
    pub fn new() -> Self {
        Self {
            recursive: true,
            extensions: None,
            excludes: vec![
                ".git".to_string(),
                ".venv".to_string(),
                "target".to_string(),
                "node_modules".to_string(),
                "__pycache__".to_string(),
                ".idea".to_string(),
                ".vscode".to_string(),
                ".agents".to_string(),
                ".gemini".to_string(),
                ".codex".to_string(),
            ],
            chunk_size: 1000,
            overlap: 150,
            pdf_loader: PDFLoader::new(),
        }
    }

    /// Set whether directory traversal is recursive (default: true)
    pub fn with_recursive(mut self, recursive: bool) -> Self {
        self.recursive = recursive;
        self
    }

    /// Set filter list of allowed file extensions (e.g. `vec!["md", "pdf", "csv", "py"]`)
    pub fn with_extensions(mut self, exts: Vec<String>) -> Self {
        self.extensions = Some(
            exts.into_iter()
                .map(|e| e.trim_start_matches('.').to_lowercase())
                .collect(),
        );
        self
    }

    /// Add directory/file names or patterns to exclude
    pub fn with_excludes(mut self, excludes: Vec<String>) -> Self {
        self.excludes.extend(excludes);
        self
    }

    /// Set default chunk size for routed chunkers
    pub fn with_chunk_size(mut self, chunk_size: usize) -> Self {
        self.chunk_size = chunk_size;
        self
    }

    /// Set default overlap for routed chunkers
    pub fn with_overlap(mut self, overlap: usize) -> Self {
        self.overlap = overlap;
        self
    }

    /// Check if path should be excluded based on directory components or hidden names
    fn should_exclude(&self, path: &Path) -> bool {
        for part in path.components() {
            let name = part.as_os_str().to_string_lossy();
            for ex in &self.excludes {
                if name.eq_ignore_ascii_case(ex) || (name.starts_with('.') && name != "." && name != "..") {
                    return true;
                }
            }
        }
        false
    }

    /// Check if path extension matches filter
    fn extension_matches(&self, path: &Path) -> bool {
        if let Some(ref allowed) = self.extensions {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                allowed.iter().any(|a| a.eq_ignore_ascii_case(ext))
            } else {
                false
            }
        } else {
            true
        }
    }

    /// Recursive directory scan collecting matching file paths
    fn scan_dir(&self, dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), ChunkrError> {
        if !dir.is_dir() {
            return Err(ChunkrError::IoError(format!("Path is not a directory: {:?}", dir)));
        }

        let entries = std::fs::read_dir(dir).map_err(|e| ChunkrError::IoError(e.to_string()))?;

        for entry in entries {
            let entry = entry.map_err(|e| ChunkrError::IoError(e.to_string()))?;
            let path = entry.path();

            // Skip symlinks entirely: path.is_dir()/is_file() follow links,
            // so a symlink cycle would otherwise cause unbounded recursion.
            let file_type = entry
                .file_type()
                .map_err(|e| ChunkrError::IoError(format!("{}: {}", path.display(), e)))?;
            if file_type.is_symlink() {
                continue;
            }

            if self.should_exclude(&path) {
                continue;
            }

            if path.is_dir() {
                if self.recursive {
                    self.scan_dir(&path, files)?;
                }
            } else if path.is_file() && self.extension_matches(&path) {
                files.push(path);
            }
        }

        Ok(())
    }

    /// Collect all eligible file paths in the directory
    pub fn collect_files<P: AsRef<Path>>(&self, dir: P) -> Result<Vec<PathBuf>, ChunkrError> {
        let mut files = Vec::new();
        self.scan_dir(dir.as_ref(), &mut files)?;
        Ok(files)
    }

    /// Load a single file into a raw Document
    fn load_single_file(&self, path: &Path) -> Result<Document, ChunkrError> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        if ext == "pdf" {
            return self.pdf_loader.load_document(path);
        }

        let content = std::fs::read_to_string(path)
            .map_err(|e| ChunkrError::IoError(format!("{}: {}", path.display(), e)))?;
        let metadata_fs = std::fs::metadata(path).ok();
        let file_size = metadata_fs.map(|m| m.len()).unwrap_or(0);

        let mut metadata = HashMap::new();
        metadata.insert("file_path".to_string(), Value::from(path.to_string_lossy().to_string()));
        if let Some(file_name) = path.file_name().and_then(|f| f.to_str()) {
            metadata.insert("file_name".to_string(), Value::from(file_name));
        }
        metadata.insert("file_extension".to_string(), Value::from(ext));
        metadata.insert("file_size_bytes".to_string(), Value::from(file_size));

        Ok(Document::new(content, metadata))
    }

    /// Load all files in the directory as unchunked Document instances in parallel
    pub fn load_files<P: AsRef<Path>>(&self, dir: P) -> Result<Vec<Document>, ChunkrError> {
        let file_paths = self.collect_files(dir)?;

        let docs: Result<Vec<Document>, ChunkrError> = file_paths
            .par_iter()
            .map(|path| self.load_single_file(path))
            .collect();

        docs
    }

    /// Lenient variant of [`Self::load_files`]: collects per-file successes
    /// and per-file `(path, error)` failures instead of short-circuiting
    /// the whole batch on the first bad file.
    pub fn load_files_lenient<P: AsRef<Path>>(
        &self,
        dir: P,
    ) -> (Vec<Document>, Vec<(PathBuf, ChunkrError)>) {
        let file_paths = match self.collect_files(dir.as_ref()) {
            Ok(paths) => paths,
            Err(e) => {
                let dir_path = dir.as_ref().to_path_buf();
                return (Vec::new(), vec![(dir_path, e)]);
            }
        };

        let results: Vec<(PathBuf, Result<Document, ChunkrError>)> = file_paths
            .par_iter()
            .map(|path| {
                let result = self.load_single_file(path);
                (path.clone(), result)
            })
            .collect();

        let mut docs = Vec::new();
        let mut errors = Vec::new();
        for (path, result) in results {
            match result {
                Ok(doc) => docs.push(doc),
                Err(e) => errors.push((path, e)),
            }
        }
        (docs, errors)
    }

    /// Auto-route and chunk a single file based on its file extension
    fn chunk_single_file(&self, path: &Path) -> Result<Vec<Document>, ChunkrError> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let path_str = path.to_string_lossy().to_string();
        let file_name = path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("")
            .to_string();
        let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

        // Helper to attach file metadata to generated chunks
        let enrich_chunks = |mut chunks: Vec<Document>| -> Vec<Document> {
            for chunk in &mut chunks {
                chunk.metadata.insert("file_path".to_string(), Value::from(path_str.clone()));
                chunk.metadata.insert("file_name".to_string(), Value::from(file_name.clone()));
                chunk.metadata.insert("file_extension".to_string(), Value::from(ext.clone()));
                chunk.metadata.insert("file_size_bytes".to_string(), Value::from(file_size));
            }
            chunks
        };

        if ext == "pdf" {
            let pages = self.pdf_loader.load_pages_from_file(path)?;
            let chunker = RecursiveChunker::new()
                .with_chunk_size(self.chunk_size)
                .with_overlap(self.overlap);
            let chunks = chunker.chunk_documents(&pages)?;
            return Ok(enrich_chunks(chunks));
        }

        let content = std::fs::read_to_string(path)
            .map_err(|e| ChunkrError::IoError(format!("{}: {}", path.display(), e)))?;
        if content.trim().is_empty() {
            return Ok(Vec::new());
        }

        let chunks = match ext.as_str() {
            "md" | "markdown" => {
                let chunker = MarkdownChunker::new()
                    .with_chunk_size(self.chunk_size)
                    .with_overlap(self.overlap);
                chunker.chunk(&content)?
            }
            "csv" | "tsv" => {
                let chunker = TableChunker::new()
                    .with_chunk_size(self.chunk_size)
                    .with_overlap_rows(1);
                chunker.chunk(&content)?
            }
            "json" => {
                let chunker = JsonChunker::new()
                    .with_max_chunk_size(self.chunk_size);
                chunker.chunk(&content)?
            }
            "html" | "htm" => {
                let chunker = HtmlChunker::new()
                    .with_chunk_size(self.chunk_size)
                    .with_overlap(self.overlap);
                chunker.chunk(&content)?
            }
            "rs" => CodeChunker::new(CodeLanguage::Rust)
                .with_chunk_size(self.chunk_size)
                .with_overlap(self.overlap)
                .chunk(&content)?,
            "py" => CodeChunker::new(CodeLanguage::Python)
                .with_chunk_size(self.chunk_size)
                .with_overlap(self.overlap)
                .chunk(&content)?,
            "js" => CodeChunker::new(CodeLanguage::JavaScript)
                .with_chunk_size(self.chunk_size)
                .with_overlap(self.overlap)
                .chunk(&content)?,
            "ts" => CodeChunker::new(CodeLanguage::TypeScript)
                .with_chunk_size(self.chunk_size)
                .with_overlap(self.overlap)
                .chunk(&content)?,
            "go" => CodeChunker::new(CodeLanguage::Go)
                .with_chunk_size(self.chunk_size)
                .with_overlap(self.overlap)
                .chunk(&content)?,
            "cpp" | "cc" | "cxx" | "c" | "h" | "hpp" => CodeChunker::new(CodeLanguage::Cpp)
                .with_chunk_size(self.chunk_size)
                .with_overlap(self.overlap)
                .chunk(&content)?,
            "java" => CodeChunker::new(CodeLanguage::Java)
                .with_chunk_size(self.chunk_size)
                .with_overlap(self.overlap)
                .chunk(&content)?,
            "sql" => CodeChunker::new(CodeLanguage::Sql)
                .with_chunk_size(self.chunk_size)
                .with_overlap(self.overlap)
                .chunk(&content)?,
            _ => {
                let chunker = RecursiveChunker::new()
                    .with_chunk_size(self.chunk_size)
                    .with_overlap(self.overlap);
                chunker.chunk(&content)?
            }
        };

        Ok(enrich_chunks(chunks))
    }

    /// Recursively crawl directory, auto-route files to optimal chunkers, and return all chunks in parallel
    pub fn load_and_chunk<P: AsRef<Path>>(&self, dir: P) -> Result<Vec<Document>, ChunkrError> {
        let file_paths = self.collect_files(dir)?;

        let chunk_results: Result<Vec<Vec<Document>>, ChunkrError> = file_paths
            .par_iter()
            .map(|path| self.chunk_single_file(path))
            .collect();

        Ok(chunk_results?.into_iter().flatten().collect())
    }

    /// Lenient variant of [`Self::load_and_chunk`]: collects per-file chunk
    /// successes and per-file `(path, error)` failures instead of
    /// short-circuiting the whole batch on the first bad file.
    pub fn load_and_chunk_lenient<P: AsRef<Path>>(
        &self,
        dir: P,
    ) -> (Vec<Document>, Vec<(PathBuf, ChunkrError)>) {
        let file_paths = match self.collect_files(dir.as_ref()) {
            Ok(paths) => paths,
            Err(e) => {
                let dir_path = dir.as_ref().to_path_buf();
                return (Vec::new(), vec![(dir_path, e)]);
            }
        };

        let results: Vec<(PathBuf, Result<Vec<Document>, ChunkrError>)> = file_paths
            .par_iter()
            .map(|path| {
                let result = self.chunk_single_file(path);
                (path.clone(), result)
            })
            .collect();

        let mut docs = Vec::new();
        let mut errors = Vec::new();
        for (path, result) in results {
            match result {
                Ok(chunks) => docs.extend(chunks),
                Err(e) => errors.push((path, e)),
            }
        }
        (docs, errors)
    }
}

impl Default for DirectoryLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl BaseLoader<Result<Vec<Document>, ChunkrError>> for DirectoryLoader {
    fn load_from_file(&self, path: &str) -> Result<Vec<Document>, ChunkrError> {
        self.load_and_chunk(path)
    }
}
