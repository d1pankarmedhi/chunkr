use serde_json::Value;

use crate::chunker::base::{BaseChunker, Chunker};
use crate::chunker::recursive::{KeepSeparator, RecursiveChunker};
use crate::error::ChunkrError;
use crate::structures::document::Document;

/// Programming and markup languages supported by CodeChunker
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CodeLanguage {
    #[default]
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Go,
    Cpp,
    Java,
    Html,
    Sql,
    Markdown,
    Generic,
}

impl CodeLanguage {
    pub fn as_str(&self) -> &'static str {
        match self {
            CodeLanguage::Rust => "rust",
            CodeLanguage::Python => "python",
            CodeLanguage::JavaScript => "javascript",
            CodeLanguage::TypeScript => "typescript",
            CodeLanguage::Go => "go",
            CodeLanguage::Cpp => "cpp",
            CodeLanguage::Java => "java",
            CodeLanguage::Html => "html",
            CodeLanguage::Sql => "sql",
            CodeLanguage::Markdown => "markdown",
            CodeLanguage::Generic => "generic",
        }
    }

    /// Language-specific hierarchical separators
    pub fn get_separators(&self) -> Vec<String> {
        match self {
            CodeLanguage::Rust => vec![
                "\nimpl ".to_string(),
                "\ntrait ".to_string(),
                "\npub fn ".to_string(),
                "\nfn ".to_string(),
                "\npub struct ".to_string(),
                "\nstruct ".to_string(),
                "\npub enum ".to_string(),
                "\nenum ".to_string(),
                "\npub mod ".to_string(),
                "\nmod ".to_string(),
                "\n\n".to_string(),
                "\n".to_string(),
                " ".to_string(),
                "".to_string(),
            ],
            CodeLanguage::Python => vec![
                "\nclass ".to_string(),
                "\ndef ".to_string(),
                "\nasync def ".to_string(),
                "\n\tdef ".to_string(),
                "\n    def ".to_string(),
                "\n\n".to_string(),
                "\n".to_string(),
                " ".to_string(),
                "".to_string(),
            ],
            CodeLanguage::JavaScript | CodeLanguage::TypeScript => vec![
                "\nclass ".to_string(),
                "\nexport class ".to_string(),
                "\nfunction ".to_string(),
                "\nexport function ".to_string(),
                "\nexport default ".to_string(),
                "\ninterface ".to_string(),
                "\ntype ".to_string(),
                "\nconst ".to_string(),
                "\nlet ".to_string(),
                "\n\n".to_string(),
                "\n".to_string(),
                " ".to_string(),
                "".to_string(),
            ],
            CodeLanguage::Go => vec![
                "\nfunc ".to_string(),
                "\ntype ".to_string(),
                "\npackage ".to_string(),
                "\nimport ".to_string(),
                "\n\n".to_string(),
                "\n".to_string(),
                " ".to_string(),
                "".to_string(),
            ],
            CodeLanguage::Cpp | CodeLanguage::Java => vec![
                "\nclass ".to_string(),
                "\npublic ".to_string(),
                "\nprotected ".to_string(),
                "\nprivate ".to_string(),
                "\nvoid ".to_string(),
                "\n\n".to_string(),
                "\n".to_string(),
                " ".to_string(),
                "".to_string(),
            ],
            CodeLanguage::Html => vec![
                "\n<article".to_string(),
                "\n<section".to_string(),
                "\n<div".to_string(),
                "\n<p".to_string(),
                "\n<table".to_string(),
                "\n\n".to_string(),
                "\n".to_string(),
                " ".to_string(),
                "".to_string(),
            ],
            CodeLanguage::Sql => vec![
                "\nCREATE TABLE ".to_string(),
                "\nCREATE VIEW ".to_string(),
                "\nSELECT ".to_string(),
                "\nINSERT INTO ".to_string(),
                "\nUPDATE ".to_string(),
                "\nDELETE FROM ".to_string(),
                "\n\n".to_string(),
                "\n".to_string(),
                " ".to_string(),
                "".to_string(),
            ],
            CodeLanguage::Markdown => vec![
                "\n# ".to_string(),
                "\n## ".to_string(),
                "\n### ".to_string(),
                "\n#### ".to_string(),
                "\n##### ".to_string(),
                "\n###### ".to_string(),
                "```\n".to_string(),
                "\n\n".to_string(),
                "\n".to_string(),
                " ".to_string(),
                "".to_string(),
            ],
            CodeLanguage::Generic => vec![
                "\n\n".to_string(),
                "\n".to_string(),
                " ".to_string(),
                "".to_string(),
            ],
        }
    }
}

/// Structure-aware chunker for source code across diverse programming languages
#[derive(Debug, Clone)]
pub struct CodeChunker {
    pub language: CodeLanguage,
    pub chunk_size: usize,
    pub overlap: usize,
    recursive_chunker: RecursiveChunker,
}

impl CodeChunker {
    /// Create CodeChunker for a specific language (default chunk_size: 1500, overlap: 200)
    pub fn new(language: CodeLanguage) -> Self {
        let seps = language.get_separators();
        let recursive = RecursiveChunker::new()
            .with_chunk_size(1500)
            .with_overlap(200)
            .with_separators(seps)
            .with_keep_separator(KeepSeparator::Start);

        Self {
            language,
            chunk_size: 1500,
            overlap: 200,
            recursive_chunker: recursive,
        }
    }

    pub fn with_chunk_size(mut self, chunk_size: usize) -> Self {
        self.chunk_size = chunk_size;
        self.recursive_chunker = self.recursive_chunker.with_chunk_size(chunk_size);
        self
    }

    pub fn with_overlap(mut self, overlap: usize) -> Self {
        self.overlap = overlap;
        self.recursive_chunker = self.recursive_chunker.with_overlap(overlap);
        self
    }
}

impl Chunker for CodeChunker {
    fn chunk(&self, text: &str) -> Result<Vec<Document>, ChunkrError> {
        let mut docs = self.recursive_chunker.chunk(text)?;
        for doc in &mut docs {
            doc.add_metadata("language", Value::from(self.language.as_str()));
        }
        Ok(docs)
    }
}

impl BaseChunker<Result<Vec<Document>, String>> for CodeChunker {
    fn chunk_text(
        &self,
        text: &str,
        chunk_size: usize,
        overlap: usize,
    ) -> Result<Vec<Document>, String> {
        let chunker = self
            .clone()
            .with_chunk_size(chunk_size)
            .with_overlap(overlap);
        chunker.chunk(text).map_err(|e| e.to_string())
    }
}
