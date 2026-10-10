//! Native high-fidelity backend, enabled by the `pdf` Cargo feature.
//!
//! Extraction is done by the `liteparse` crate (PDFium-backed spatial parsing);
//! this module only translates our config into theirs and their pages into our
//! payload, so the mapping rules stay in one place.

use std::sync::OnceLock;

use crate::error::ChunkrError;

use super::config::{FigureMode, OcrMode, OnError, ParserConfig, ParserOutput};
use super::payload::{Bbox, BlockPayload, CellPayload, ComplexityPayload, PagePayload};

fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("failed to start tokio runtime for the pdf backend")
    })
}

fn needs_blocks(cfg: &ParserConfig) -> bool {
    cfg.keep_blocks
        || !matches!(cfg.output, ParserOutput::Text)
        || !matches!(cfg.granularity, super::config::Granularity::Page)
}

/// The liteparse OCR engine trait, re-exported for custom engines.
pub type OcrEngine = std::sync::Arc<dyn liteparse::ocr::OcrEngine>;

fn liteparse_config(
    cfg: &ParserConfig,
    ocr_enabled: bool,
    target_pages: Option<String>,
) -> liteparse::LiteParseConfig {
    use liteparse::config::{ImageMode, OutputFormat};

    liteparse::LiteParseConfig {
        ocr_enabled,
        ocr_language: cfg.ocr.engine_language(),
        ocr_server_url: cfg.ocr.server_url.clone(),
        ocr_server_headers: cfg.ocr.headers.clone(),
        ocr_hedge_delays_ms: cfg.ocr.hedge_delays_ms.clone(),
        tessdata_path: cfg.ocr.tessdata_path.clone(),
        ocr_failure_fatal: cfg.ocr.failure_fatal,
        num_workers: cfg.ocr.num_workers,
        max_pages: cfg.scope.max_pages.unwrap_or(usize::MAX),
        target_pages: target_pages.or_else(|| cfg.scope.target_pages.clone()),
        password: cfg.scope.password.clone(),
        dpi: cfg.scope.dpi,
        preserve_very_small_text: cfg.scope.preserve_very_small_text,
        skip_diagonal_text: cfg.scope.skip_diagonal_text,
        keep_headers_footers: cfg.scope.keep_headers_footers,
        quiet: cfg.scope.quiet,
        // Raw markdown passthrough needs the backend's own renderer.
        output_format: match (cfg.output, cfg.sanitize.enabled) {
            (ParserOutput::Markdown | ParserOutput::Both, false) => OutputFormat::Markdown,
            _ => OutputFormat::Text,
        },
        extract_blocks: needs_blocks(cfg),
        // `mode="auto"` gates on complexity, so it always needs the signals.
        include_complexity: cfg.include_complexity || matches!(cfg.ocr.mode, OcrMode::Auto),
        extract_links: cfg.extract.links,
        image_mode: match cfg.extract.figures {
            FigureMode::Skip => ImageMode::Off,
            _ => ImageMode::Placeholder,
        },
        extract_images: cfg.extract.images,
        extract_form_fields: cfg.extract.form_fields,
        extract_annotations: cfg.extract.annotations,
        extract_structure_tree: cfg.extract.structure_tree,
        extract_vector_graphics: cfg.extract.vector_graphics,
        extract_text_metadata: cfg.extract.text_metadata,
        extract_screenshots: cfg.extract.screenshots,
        continue_on_page_error: matches!(cfg.on_error, OnError::PageError),
        ..Default::default()
    }
}

/// Parse PDF bytes into payload pages.
pub fn pages_from_bytes(
    bytes: &[u8],
    cfg: &ParserConfig,
    engine: Option<OcrEngine>,
) -> Result<Vec<PagePayload>, ChunkrError> {
    validate_backend(cfg)?;
    if matches!(cfg.ocr.mode, OcrMode::Auto) {
        return pages_auto(bytes, cfg, engine);
    }
    // Built-in engines (Tesseract, HTTP server) are liteparse's business; the
    // PP-OCR one is ours and is only built when a page will actually be OCRed.
    let engine = if cfg.ocr.is_enabled() {
        resolve_engine(cfg, engine)?
    } else {
        engine
    };
    parse_pass(bytes, cfg, engine, cfg.ocr.is_enabled(), None)
}

/// Pick up the engine the config asks for when the caller supplied none.
#[cfg(feature = "pdf-ocr-ppocr")]
fn resolve_engine(
    cfg: &ParserConfig,
    engine: Option<OcrEngine>,
) -> Result<Option<OcrEngine>, ChunkrError> {
    use super::config::OcrBackendKind;

    if engine.is_some() || !matches!(cfg.ocr.backend_kind(), OcrBackendKind::Ppocr) {
        return Ok(engine);
    }
    ppocr::engine(cfg).map(Some)
}

#[cfg(not(feature = "pdf-ocr-ppocr"))]
fn resolve_engine(
    _cfg: &ParserConfig,
    engine: Option<OcrEngine>,
) -> Result<Option<OcrEngine>, ChunkrError> {
    Ok(engine)
}

/// In-process ONNX PP-OCR (PP-OCRv6), shared process-wide so the models load once.
#[cfg(feature = "pdf-ocr-ppocr")]
mod ppocr {
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, OnceLock};

    use crate::error::ChunkrError;

    use super::super::config::{ppocr_compiled_device, ParserConfig, PpocrConfig};
    use super::OcrEngine;

    type Engines = Mutex<BTreeMap<String, OcrEngine>>;

    fn engines() -> &'static Engines {
        static ENGINES: OnceLock<Engines> = OnceLock::new();
        ENGINES.get_or_init(|| Mutex::new(BTreeMap::new()))
    }

    /// Detector, recognizer and dictionary for a preset (upstream file names).
    fn preset_files(preset: &str) -> (&'static str, &'static str, &'static str) {
        match preset {
            "tiny" => (
                "pp-ocrv6_tiny_det.onnx",
                "pp-ocrv6_tiny_rec.onnx",
                "ppocrv6_tiny_dict.txt",
            ),
            "medium" => (
                "pp-ocrv6_medium_det.onnx",
                "pp-ocrv6_medium_rec.onnx",
                "ppocrv6_dict.txt",
            ),
            _ => (
                "pp-ocrv6_small_det.onnx",
                "pp-ocrv6_small_rec.onnx",
                "ppocrv6_dict.txt",
            ),
        }
    }

    /// A loaded PP-OCR engine for this config, building it on first use.
    pub fn engine(cfg: &ParserConfig) -> Result<OcrEngine, ChunkrError> {
        let ppocr = &cfg.ocr.ppocr;
        let key = format!(
            "{}|{}|{}",
            ppocr.preset,
            ppocr.models_dir.as_deref().unwrap_or("$OAR_HOME"),
            ppocr.device
        );
        if let Some(existing) = engines().lock().map_err(|_| poisoned())?.get(&key).cloned() {
            return Ok(existing);
        }
        let built = build(ppocr)?;
        engines()
            .lock()
            .map_err(|_| poisoned())?
            .insert(key, built.clone());
        Ok(built)
    }

    fn build(ppocr: &PpocrConfig) -> Result<OcrEngine, ChunkrError> {
        use liteparse::ocr::oar::OarOcrEngine;

        let device = ppocr_compiled_device().unwrap_or("cpu");
        let filename = preset_files(&ppocr.preset);
        let loaded = match ppocr.models_dir.as_deref() {
            // Locally cached artifacts: no download, no env-var guessing.
            Some(dir) => {
                let path = |name: &str| PathBuf::from(dir).join(name);
                OarOcrEngine::from_models(path(filename.0), path(filename.1), path(filename.2))
            }
            None => match ppocr.preset.as_str() {
                "tiny" => OarOcrEngine::ppocr_v6_tiny(),
                "medium" => OarOcrEngine::ppocr_v6_medium(),
                _ => OarOcrEngine::ppocr_v6_small(),
            },
        };
        loaded
            .map(|engine| Arc::new(engine) as OcrEngine)
            .map_err(|error| {
                ChunkrError::ParseError(format!(
                    "PP-OCR {} engine ({device}) failed to load: {error}. Models download to                      $OAR_HOME (~/.oar) on first use; point `ocr.ppocr.models_dir` at local                      {}/{}/{} artifacts to stay offline",
                    ppocr.preset, filename.0, filename.1, filename.2
                ))
            })
    }

    fn poisoned() -> ChunkrError {
        ChunkrError::ParseError("PP-OCR engine cache is poisoned".to_string())
    }
}

/// Reject OCR backends this build cannot run, before touching the document.
fn validate_backend(cfg: &ParserConfig) -> Result<(), ChunkrError> {
    use super::config::OcrBackendKind;

    if !cfg.ocr.is_enabled() {
        return Ok(());
    }
    match cfg.ocr.backend_kind() {
        OcrBackendKind::Ppocr if !super::config::ppocr_available() => Err(ChunkrError::ParseError(
            "ocr.backend=\"ppocr\" needs the `pdf-ocr-ppocr` Cargo feature; pass it to Cargo \
             (add `pdf-ocr-ppocr-coreml`/`-cuda`/... for an accelerator), use \
             backend=\"server\" with a PP-OCR service, or the Python plugin \
             (chunkr_pdf.PDFParser(ocr={\"backend\": \"paddleocr\"}))"
                .to_string(),
        )),
        OcrBackendKind::Custom(name) => Err(ChunkrError::ParseError(format!(
            "custom OCR backend {name:?} is only available through the Python plugin \
             (chunkr-rs[pdf]): register it with chunkr.register_ocr_backend({name:?}, engine), \
             or point ocr.server_url at an OCR server"
        ))),
        OcrBackendKind::Server if cfg.ocr.server_url.is_none() => Err(ChunkrError::ParseError(
            "ocr.backend=\"server\" needs ocr.server_url".to_string(),
        )),
        _ => Ok(()),
    }
}

/// Two passes for `mode="auto"`: a cheap text-layer pass to find the pages whose
/// text is broken, then an OCR pass over just those pages.
fn pages_auto(
    bytes: &[u8],
    cfg: &ParserConfig,
    engine: Option<OcrEngine>,
) -> Result<Vec<PagePayload>, ChunkrError> {
    let mut pages = parse_pass(bytes, cfg, engine.clone(), false, None)?;
    let selected = super::ocr::select_ocr_pages(&pages, cfg);
    if selected.is_empty() {
        if !cfg.include_complexity {
            for page in pages.iter_mut() {
                page.complexity = None;
            }
        }
        return Ok(pages);
    }

    let target = super::ocr::format_page_range(&selected);
    // Only now is an engine worth building: `mode="auto"` must not download or
    // load models for a document that never gets OCRed.
    let engine = resolve_engine(cfg, engine)?;
    let ocr_pages = parse_pass(bytes, cfg, engine, true, Some(target))?;
    let mut replacements: std::collections::BTreeMap<usize, PagePayload> = ocr_pages
        .into_iter()
        .map(|page| (page.page_number, page))
        .collect();
    for page in pages.iter_mut() {
        if let Some(replacement) = replacements.remove(&page.page_number) {
            *page = replacement;
        }
    }
    if !cfg.include_complexity {
        for page in pages.iter_mut() {
            page.complexity = None;
        }
    }
    Ok(pages)
}

fn parse_pass(
    bytes: &[u8],
    cfg: &ParserConfig,
    engine: Option<OcrEngine>,
    ocr_enabled: bool,
    target_pages: Option<String>,
) -> Result<Vec<PagePayload>, ChunkrError> {
    use liteparse::types::PdfInput;
    use liteparse::LiteParse;

    let parser = match engine {
        Some(engine) => {
            LiteParse::new(liteparse_config(cfg, ocr_enabled, target_pages)).with_ocr_engine(engine)
        }
        None => LiteParse::new(liteparse_config(cfg, ocr_enabled, target_pages)),
    };
    let result = runtime()
        .block_on(parser.parse_input(PdfInput::Bytes(bytes.to_vec())))
        .map_err(ocr_error_hint)?;

    Ok(result.pages.iter().map(convert_page).collect())
}

/// Turn the backend's "no engine compiled" failure into something actionable.
fn ocr_error_hint(error: liteparse::LiteParseError) -> ChunkrError {
    let message = error.to_string();
    if message.contains("tesseract feature is disabled") {
        return ChunkrError::ParseError(format!(
            "{message}: enable the `pdf-ocr` Cargo feature (Tesseract) or point \
             `ocr.server_url` at an OCR server"
        ));
    }
    ChunkrError::ParseError(format!("pdf backend failed: {message}"))
}

fn convert_page(page: &liteparse::types::ParsedPage) -> PagePayload {
    PagePayload {
        page_number: page.page_number,
        page_label: page.page_label.clone(),
        page_width: page.page_width,
        page_height: page.page_height,
        text: page.text.clone(),
        markdown: (!page.markdown.trim().is_empty()).then(|| page.markdown.clone()),
        blocks: page
            .blocks
            .as_ref()
            .map(|blocks| blocks.iter().map(convert_block).collect())
            .unwrap_or_default(),
        complexity: page
            .complexity
            .as_ref()
            .and_then(|c| serde_json::to_value(c).ok())
            .and_then(|v| serde_json::from_value::<ComplexityPayload>(v).ok()),
    }
}

fn convert_block(block: &liteparse::layout::LayoutBlock) -> BlockPayload {
    BlockPayload {
        kind: block.kind.clone(),
        text: block.text.clone(),
        level: block.level,
        bold: block.bold,
        italic: block.italic,
        ordered: block.ordered,
        marker: block.marker.clone(),
        lines: block.lines.clone(),
        lang: block.lang.clone(),
        header: block
            .header
            .as_ref()
            .map(|cells| cells.iter().map(convert_cell).collect()),
        rows: block.rows.as_ref().map(|rows| {
            rows.iter()
                .map(|row| row.iter().map(convert_cell).collect())
                .collect()
        }),
        id: block.id.clone(),
        format: block.format.clone(),
        bbox: block.bbox.as_ref().map(convert_bbox),
    }
}

fn convert_cell(cell: &liteparse::layout::LayoutCell) -> CellPayload {
    CellPayload {
        text: cell.text.clone(),
        bbox: cell.bbox.as_ref().map(convert_bbox),
        colspan: cell.colspan,
        rowspan: cell.rowspan,
    }
}

fn convert_bbox(rect: &liteparse::types::Rect) -> Bbox {
    Bbox {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    }
}
