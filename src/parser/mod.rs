//! High-fidelity PDF parsing: backend selection, layout-aware structure, and
//! the mapping from parsed pages to [`Document`s](crate::structures::document::Document).
//!
//! The base crate ships two backends:
//!
//! * [`Backend::Fast`]: the built-in `lopdf` text extractor (no extra deps).
//! * [`Backend::Liteparse`]: PDFium-backed spatial parsing, available with the
//!   `pdf` Cargo feature or from the `chunkr-rs[pdf]` Python extra.
//!
//! Everything the backend returns passes through the sanitizer in
//! [`sanitize`], and every rule is user-selectable through [`ParserConfig`].

pub mod config;
mod fast;
#[cfg(all(feature = "pdf", not(target_arch = "wasm32")))]
mod liteparse_backend;
pub mod map;
pub mod ocr;
pub mod payload;
pub mod sanitize;

use std::path::Path;

pub use config::{
    ocr_language_iso, ocr_language_tesseract, Backend, DemoteTo, ExtractConfig, FigureMode,
    GlyphMode, Granularity, HeaderMissing, JunkGuard, LevelMode, OcrBackendKind, OcrConfig,
    OcrMode, OnError, ParserConfig, ParserOutput, PluginOcrConfig, PpocrConfig, SanitizeConfig,
    ScopeConfig, TableMode, TableSanitize, DEFAULT_OCR_REASONS,
};
pub use map::{pages_to_documents, render_markdown, ParseOutcome, SourceMeta};
pub use ocr::{format_page_range, select_ocr_pages};
pub use payload::{BlockPayload, CellPayload, ComplexityPayload, PagePayload};
pub use sanitize::{sanitize_pages, SanitizeReport};

// Custom OCR engines: implement `OcrEngine` and hand it to
// [`PdfParser::with_ocr_engine`].
#[cfg(all(feature = "pdf", not(target_arch = "wasm32")))]
pub use liteparse::ocr::{OcrEngine, OcrOptions, OcrResult};

use crate::error::ChunkrError;
use crate::structures::document::Document;

/// True when a native high-fidelity backend was compiled in.
pub const fn native_backend_available() -> bool {
    cfg!(all(feature = "pdf", not(target_arch = "wasm32")))
}

/// Names of the backends this build can run.
pub fn available_backends() -> &'static [&'static str] {
    if native_backend_available() {
        &["fast", "liteparse", "auto"]
    } else {
        &["fast", "auto"]
    }
}

/// PDF parser with a fixed configuration.
#[derive(Clone, Default)]
pub struct PdfParser {
    config: ParserConfig,
    /// Optional custom OCR engine, applied to every OCR pass.
    #[cfg(all(feature = "pdf", not(target_arch = "wasm32")))]
    ocr_engine: Option<std::sync::Arc<dyn OcrEngine>>,
}

impl std::fmt::Debug for PdfParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = f.debug_struct("PdfParser");
        debug.field("config", &self.config);
        #[cfg(all(feature = "pdf", not(target_arch = "wasm32")))]
        debug.field(
            "ocr_engine",
            &self.ocr_engine.as_ref().map(|engine| engine.name()),
        );
        debug.finish()
    }
}

impl PdfParser {
    pub fn new(config: ParserConfig) -> Self {
        Self {
            config,
            #[cfg(all(feature = "pdf", not(target_arch = "wasm32")))]
            ocr_engine: None,
        }
    }

    /// Use a custom OCR engine for every OCR pass (native `pdf` feature only).
    #[cfg(all(feature = "pdf", not(target_arch = "wasm32")))]
    pub fn with_ocr_engine(mut self, engine: std::sync::Arc<dyn OcrEngine>) -> Self {
        self.ocr_engine = Some(engine);
        self
    }

    /// Build from a preset name (`retrieval`, `faithful`, `structure`) or a
    /// JSON object of overrides. `None` yields the defaults.
    pub fn from_spec(spec: Option<&str>) -> Result<Self, ChunkrError> {
        ParserConfig::from_json_or_preset(spec)
            .map(Self::new)
            .map_err(ChunkrError::ParseError)
    }

    pub fn config(&self) -> &ParserConfig {
        &self.config
    }

    pub fn with_config(mut self, config: ParserConfig) -> Self {
        self.config = config;
        self
    }

    /// Which backend the current config resolves to.
    pub fn resolved_backend(&self) -> Backend {
        match self.config.backend {
            Backend::Auto if native_backend_available() => Backend::Liteparse,
            Backend::Auto => Backend::Fast,
            other => other,
        }
    }

    /// Parse a file and map it to documents using the configured granularity.
    pub fn load<P: AsRef<Path>>(&self, path: P) -> Result<Vec<Document>, ChunkrError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)?;
        self.load_from_bytes(&bytes, Some(&path.to_string_lossy()))
    }

    /// Parse a file with page granularity regardless of `config.granularity`.
    pub fn load_pages<P: AsRef<Path>>(&self, path: P) -> Result<Vec<Document>, ChunkrError> {
        self.page_parser().load(path)
    }

    /// Parse in-memory bytes; `source` is stamped into metadata when present.
    pub fn load_from_bytes(
        &self,
        bytes: &[u8],
        source: Option<&str>,
    ) -> Result<Vec<Document>, ChunkrError> {
        Ok(self.parse(bytes, source)?.documents)
    }

    /// Parse and return documents plus the sanitizer report.
    pub fn parse(&self, bytes: &[u8], source: Option<&str>) -> Result<ParseOutcome, ChunkrError> {
        let (pages, backend) = self.pages(bytes)?;
        let meta = SourceMeta {
            source: source.map(|s| s.to_string()),
            file_name: source
                .and_then(|s| Path::new(s).file_name())
                .map(|f| f.to_string_lossy().to_string()),
            backend: backend_name(backend).to_string(),
            parser_version: None,
        };
        Ok(pages_to_documents(&pages, &self.config, &meta))
    }

    /// Extract payload pages without mapping them to documents.
    pub fn pages(&self, bytes: &[u8]) -> Result<(Vec<PagePayload>, Backend), ChunkrError> {
        self.config.validate().map_err(ChunkrError::ParseError)?;
        match self.resolved_backend() {
            Backend::Liteparse => self.pages_liteparse(bytes),
            _ => Ok((fast::pages_from_bytes(bytes)?, Backend::Fast)),
        }
    }

    #[cfg(all(feature = "pdf", not(target_arch = "wasm32")))]
    fn pages_liteparse(&self, bytes: &[u8]) -> Result<(Vec<PagePayload>, Backend), ChunkrError> {
        Ok((
            liteparse_backend::pages_from_bytes(bytes, &self.config, self.ocr_engine.clone())?,
            Backend::Liteparse,
        ))
    }

    #[cfg(not(all(feature = "pdf", not(target_arch = "wasm32"))))]
    fn pages_liteparse(&self, _bytes: &[u8]) -> Result<(Vec<PagePayload>, Backend), ChunkrError> {
        Err(ChunkrError::ParseError(
            "the liteparse backend is not compiled in: enable the `pdf` Cargo feature \
             (`chunkr = { features = [\"pdf\"] }`) or install the `chunkr-rs[pdf]` extra"
                .to_string(),
        ))
    }

    fn page_parser(&self) -> Self {
        let mut config = self.config.clone();
        config.granularity = Granularity::Page;
        Self {
            config,
            #[cfg(all(feature = "pdf", not(target_arch = "wasm32")))]
            ocr_engine: self.ocr_engine.clone(),
        }
    }
}

fn backend_name(backend: Backend) -> &'static str {
    match backend {
        Backend::Fast => "fast",
        Backend::Liteparse => "liteparse",
        Backend::Auto => "auto",
    }
}

impl From<Backend> for String {
    fn from(backend: Backend) -> Self {
        backend_name(backend).to_string()
    }
}
