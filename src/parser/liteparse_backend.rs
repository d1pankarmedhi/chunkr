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

fn liteparse_config(cfg: &ParserConfig) -> liteparse::LiteParseConfig {
    use liteparse::config::{ImageMode, OutputFormat};

    let ocr_enabled = matches!(
        cfg.ocr.mode,
        OcrMode::Auto | OcrMode::Always | OcrMode::Server
    );
    liteparse::LiteParseConfig {
        ocr_enabled,
        ocr_language: cfg.ocr.language.clone(),
        ocr_server_url: cfg.ocr.server_url.clone(),
        ocr_server_headers: cfg.ocr.headers.clone(),
        tessdata_path: cfg.ocr.tessdata_path.clone(),
        ocr_failure_fatal: cfg.ocr.failure_fatal,
        num_workers: cfg.ocr.num_workers,
        max_pages: cfg.scope.max_pages.unwrap_or(usize::MAX),
        target_pages: cfg.scope.target_pages.clone(),
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
        include_complexity: cfg.include_complexity,
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
pub fn pages_from_bytes(bytes: &[u8], cfg: &ParserConfig) -> Result<Vec<PagePayload>, ChunkrError> {
    use liteparse::types::PdfInput;
    use liteparse::LiteParse;

    let parser = LiteParse::new(liteparse_config(cfg));
    let result = runtime()
        .block_on(parser.parse_input(PdfInput::Bytes(bytes.to_vec())))
        .map_err(|e| ChunkrError::ParseError(format!("pdf backend failed: {e}")))?;

    Ok(result.pages.iter().map(convert_page).collect())
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
