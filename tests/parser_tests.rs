//! Tests for the parser payload mapping, sanitizer and backend dispatch.

use chunkr::parser::config::{HeaderMissing, TableSanitize};
use chunkr::parser::payload::{BlockPayload, CellPayload, PagePayload};
use chunkr::parser::{
    pages_to_documents, sanitize_pages, ExtractConfig, GlyphMode, Granularity, JunkGuard,
    LevelMode, ParserConfig, ParserOutput, PdfParser, SourceMeta, TableMode,
};
use std::path::PathBuf;

fn cell(text: &str) -> CellPayload {
    CellPayload {
        text: text.to_string(),
        ..Default::default()
    }
}

fn row(cells: &[&str]) -> Vec<CellPayload> {
    cells.iter().map(|c| cell(c)).collect()
}

/// The prose-as-grid case observed on the two-column textbook fixture.
fn prose_grid() -> BlockPayload {
    BlockPayload {
        kind: "table".to_string(),
        header: Some(vec![
            cell(""),
            cell(""),
            cell("It can, thus, be stated that the"),
        ]),
        rows: Some(vec![
            row(&["financial", "statements", "of a business"]),
            row(&["are", "largely determined by", "financial"]),
        ]),
        ..Default::default()
    }
}

/// The EBIT/EPS table observed on the same fixture: real, headerless.
fn real_table() -> BlockPayload {
    BlockPayload {
        kind: "table".to_string(),
        header: None,
        rows: Some(vec![
            row(&["EBIT", "4,00,000", "4,00,000", "4,00,000"]),
            row(&["Interest", "NIL", "1,00,000", "2,00,000"]),
            row(&["EBT", "4,00,000", "3,00,000", "2,00,000"]),
            row(&["EAT", "2,80,000", "2,10,000", "1,40,000"]),
            row(&["EPS", "0.93", "1.05", "0.87"]),
        ]),
        ..Default::default()
    }
}

fn table_page(block: BlockPayload) -> PagePayload {
    PagePayload {
        page_number: 1,
        text: "page text".to_string(),
        blocks: vec![block],
        ..Default::default()
    }
}

fn meta() -> SourceMeta {
    SourceMeta {
        source: Some("fixture.pdf".to_string()),
        file_name: Some("fixture.pdf".to_string()),
        backend: "test".to_string(),
        parser_version: None,
    }
}

#[cfg(feature = "pdf")]
fn table_blocks(documents: &[chunkr::Document]) -> Vec<serde_json::Value> {
    documents
        .iter()
        .filter_map(|d| d.metadata.get("blocks"))
        .filter_map(|b| b.as_array())
        .flatten()
        .filter(|b| b["kind"] == "table")
        .cloned()
        .collect()
}

#[test]
fn prose_grid_is_demoted_and_reflowed() {
    let mut pages = vec![table_page(prose_grid())];
    let report = sanitize_pages(&mut pages, &ParserConfig::default().sanitize);

    assert_eq!(report.tables_demoted, 1);
    assert_eq!(report.tables_kept, 0);
    let block = &pages[0].blocks[0];
    assert_eq!(block.kind, "paragraph");
    // Row-major reflow keeps the sentence order.
    assert!(block
        .text
        .as_deref()
        .unwrap()
        .starts_with("It can, thus, be stated that the financial statements"));
}

#[test]
fn headerless_numeric_table_policy_is_configurable() {
    // Default: headerless tables are demoted.
    let mut pages = vec![table_page(real_table())];
    let report = sanitize_pages(&mut pages, &ParserConfig::default().sanitize);
    assert_eq!(report.tables_demoted, 1);

    // Opt in to keeping them with a synthesized header.
    let mut cfg = ParserConfig::default();
    cfg.sanitize.tables = TableSanitize {
        header_missing: HeaderMissing::Synthesize,
        ..cfg.sanitize.tables
    };
    let mut pages = vec![table_page(real_table())];
    let report = sanitize_pages(&mut pages, &cfg.sanitize);
    assert_eq!(report.tables_kept, 1);
    let block = &pages[0].blocks[0];
    assert_eq!(block.kind, "table");
    assert_eq!(block.header_cells()[0], "EBIT");
    assert_eq!(block.table_rows().len(), 5);
}

#[test]
fn raw_mode_keeps_tables_and_markdown_verbatim() {
    let mut cfg = ParserConfig {
        output: ParserOutput::Markdown,
        ..ParserConfig::preset("faithful").unwrap()
    };
    cfg.sanitize.enabled = false;
    let page = PagePayload {
        markdown: Some("# Raw heading\n\n| a | b |\n| --- | --- |\n| 1 | 2 |".to_string()),
        blocks: vec![prose_grid()],
        ..table_page(prose_grid())
    };
    let outcome = pages_to_documents(&[page], &cfg, &meta());
    assert!(outcome.report.is_clean());
    assert_eq!(
        outcome.documents[0].content,
        "# Raw heading\n\n| a | b |\n| --- | --- |\n| 1 | 2 |"
    );
}

#[test]
fn truncated_heading_is_demoted_but_real_heading_is_kept() {
    let page = PagePayload {
        page_number: 1,
        text: "text".to_string(),
        blocks: vec![
            BlockPayload {
                kind: "heading".to_string(),
                text: Some("1. Cash Flow Position: Size of projected".to_string()),
                level: Some(2),
                ..Default::default()
            },
            BlockPayload {
                kind: "paragraph".to_string(),
                text: Some("cash flows from operations.".to_string()),
                ..Default::default()
            },
            BlockPayload {
                kind: "heading".to_string(),
                text: Some("Key Terms".to_string()),
                level: Some(2),
                ..Default::default()
            },
            BlockPayload {
                kind: "paragraph".to_string(),
                text: Some("Assets are resources owned.".to_string()),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let mut pages = vec![page];
    let report = sanitize_pages(&mut pages, &ParserConfig::default().sanitize);
    assert_eq!(report.headings_demoted, 1);
    assert_eq!(report.headings_kept, 1);
    let kinds: Vec<&str> = pages[0].blocks.iter().map(|b| b.kind.as_str()).collect();
    assert_eq!(
        kinds,
        vec!["paragraph", "paragraph", "heading", "paragraph"]
    );
}

#[test]
fn long_heading_is_demoted() {
    let page = PagePayload {
        blocks: vec![BlockPayload {
            kind: "heading".to_string(),
            text: Some(
                "Objectives and Financial Decisions The primary aim of financial management \
                 is to maximise shareholders' wealth, which is reflected in the market price"
                    .to_string(),
            ),
            level: Some(2),
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut pages = vec![page];
    let report = sanitize_pages(&mut pages, &ParserConfig::default().sanitize);
    assert_eq!(report.headings_demoted, 1);
    assert_eq!(pages[0].blocks[0].kind, "paragraph");
}

#[test]
fn flat_single_level_headings_get_one_level() {
    let page = PagePayload {
        blocks: vec![
            BlockPayload {
                kind: "heading".to_string(),
                text: Some("Introduction".to_string()),
                level: Some(2),
                ..Default::default()
            },
            BlockPayload {
                kind: "heading".to_string(),
                text: Some("Summary".to_string()),
                level: Some(2),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let mut pages = vec![page];
    sanitize_pages(&mut pages, &ParserConfig::default().sanitize);
    assert!(pages[0].blocks.iter().all(|b| b.level == Some(2)));
}

#[test]
fn derive_levels_ranks_heading_sizes() {
    let heading = |text: &str, height: f32| BlockPayload {
        kind: "heading".to_string(),
        text: Some(text.to_string()),
        level: Some(2),
        bbox: Some(chunkr::parser::payload::Bbox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height,
        }),
        ..Default::default()
    };
    let page = PagePayload {
        blocks: vec![
            heading("Chapter One", 20.0),
            heading("Section", 14.0),
            heading("Subsection", 11.0),
        ],
        ..Default::default()
    };
    let mut cfg = ParserConfig::default();
    cfg.sanitize.headings.levels = LevelMode::Derive;
    let mut pages = vec![page];
    sanitize_pages(&mut pages, &cfg.sanitize);
    let levels: Vec<Option<u8>> = pages[0].blocks.iter().map(|b| b.level).collect();
    assert_eq!(levels, vec![Some(1), Some(2), Some(3)]);
}

#[test]
fn glyph_repair_and_report_modes() {
    let mut cfg = ParserConfig::default();
    cfg.sanitize.glyphs = GlyphMode::Repair;
    let page = PagePayload {
        text: "Arti\u{FA00}\u{6900}cially F\u{6900}\u{6800}\u{6900}\u{FFFD}".to_string(),
        ..Default::default()
    };
    let mut pages = vec![page.clone()];
    let report = sanitize_pages(&mut pages, &cfg.sanitize);
    assert!(report.glyph_repairs >= 4);
    assert_eq!(pages[0].text, "Artificially Fiji");

    cfg.sanitize.glyphs = GlyphMode::Report;
    let mut pages = vec![page.clone()];
    let report = sanitize_pages(&mut pages, &cfg.sanitize);
    assert!(report.glyph_repairs >= 4);
    assert_eq!(pages[0].text, page.text);
}

#[test]
fn junk_guard_flags_and_can_request_fallback() {
    let page = PagePayload {
        text: "?Identity-H Unimplemented?. Real text.".to_string(),
        ..Default::default()
    };
    let mut pages = vec![page.clone()];
    let report = sanitize_pages(&mut pages, &ParserConfig::default().sanitize);
    assert_eq!(report.junk_hits, 2);
    assert!(!report.fallback_fast);

    let mut cfg = ParserConfig::default();
    cfg.sanitize.junk_guard = JunkGuard::FallbackFast;
    let mut pages = vec![page];
    let report = sanitize_pages(&mut pages, &cfg.sanitize);
    assert!(report.fallback_fast);
}

#[test]
fn granularity_controls_document_shape() {
    let pages = vec![
        PagePayload {
            page_number: 1,
            page_label: Some("iv".to_string()),
            text: "first".to_string(),
            blocks: vec![BlockPayload {
                kind: "paragraph".to_string(),
                text: Some("first".to_string()),
                ..Default::default()
            }],
            ..Default::default()
        },
        PagePayload {
            page_number: 2,
            text: "second".to_string(),
            blocks: vec![BlockPayload {
                kind: "heading".to_string(),
                text: Some("Second".to_string()),
                level: Some(2),
                ..Default::default()
            }],
            ..Default::default()
        },
    ];

    let page_cfg = ParserConfig {
        granularity: Granularity::Page,
        ..Default::default()
    };
    let outcome = pages_to_documents(&pages, &page_cfg, &meta());
    assert_eq!(outcome.documents.len(), 2);
    assert_eq!(outcome.documents[0].metadata["page_number"], 1);
    assert_eq!(outcome.documents[0].metadata["page_label"], "iv");
    assert_eq!(outcome.documents[0].metadata["total_pages"], 2);

    let block_cfg = ParserConfig {
        granularity: Granularity::Block,
        ..Default::default()
    };
    let outcome = pages_to_documents(&pages, &block_cfg, &meta());
    assert_eq!(outcome.documents.len(), 2);
    assert_eq!(outcome.documents[1].metadata["block_type"], "heading");
    assert_eq!(outcome.documents[1].metadata["heading_level"], 2);

    let doc_cfg = ParserConfig {
        granularity: Granularity::Document,
        output: ParserOutput::Both,
        ..Default::default()
    };
    let outcome = pages_to_documents(&pages, &doc_cfg, &meta());
    assert_eq!(outcome.documents.len(), 1);
    assert!(outcome.documents[0].content.contains("first"));
    assert!(outcome.documents[0].content.contains("Second"));

    let flat = ParserConfig {
        granularity: Granularity::Block,
        output: ParserOutput::Markdown,
        ..Default::default()
    };
    let outcome = pages_to_documents(&pages, &flat, &meta());
    assert_eq!(outcome.documents[1].content, "## Second");
}

#[test]
fn tables_mode_off_reflows_tables_even_when_kept() {
    let cfg = ParserConfig {
        output: ParserOutput::Markdown,
        extract: ExtractConfig {
            tables: TableMode::Off,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut pages = vec![table_page(real_table())];
    // Keep the table past the sanitizer by synthesizing a header.
    let mut cfg = cfg;
    cfg.sanitize.tables.header_missing = HeaderMissing::Synthesize;
    let outcome = pages_to_documents(&pages, &cfg, &meta());
    let content = &outcome.documents[0].content;
    assert!(!content.contains("| --- |"), "table markup should be off");
    assert!(content.contains("EBIT 4,00,000"));

    pages[0].blocks[0].kind = "table".to_string();
    let _ = pages;
}

#[test]
fn presets_resolve_and_reject_unknown_names() {
    assert!(!ParserConfig::preset("faithful").unwrap().sanitize.enabled);
    assert_eq!(
        ParserConfig::preset("structure").unwrap().granularity,
        Granularity::Block
    );
    let custom =
        ParserConfig::from_json_or_preset(Some(r#"{"output":"markdown","keep_blocks":false}"#))
            .unwrap();
    assert_eq!(custom.output, ParserOutput::Markdown);
    assert!(!custom.keep_blocks);
    assert!(ParserConfig::from_json_or_preset(Some("nope")).is_err());
}

#[test]
fn documents_are_produced_for_the_fast_backend() {
    let fixture = PathBuf::from("tests/test_files/sample_doc.pdf");
    if !fixture.exists() {
        return;
    }
    let parser = PdfParser::new(ParserConfig {
        backend: chunkr::parser::Backend::Fast,
        ..Default::default()
    });
    let documents = parser.load_pages(&fixture).unwrap();
    assert_eq!(documents.len(), 10);
    assert!(documents[0].content.contains("Sample PDF Document"));
    assert_eq!(documents[0].metadata["parser_backend"], "fast");
}

#[test]
fn table_helpers_report_liteparse_specifics() {
    let block = real_table();
    assert_eq!(block.table_rows().len(), 5);
    assert!(!block.has_header());
    assert!(block.table_text().starts_with("EBIT 4,00,000"));
}

#[cfg(feature = "pdf")]
mod native_backend {
    use super::*;

    fn fixture(name: &str) -> Option<PathBuf> {
        let path = PathBuf::from("tests/test_files").join(name);
        path.exists().then_some(path)
    }

    fn collect_text(documents: &[chunkr::Document]) -> String {
        documents
            .iter()
            .map(|d| d.content.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Opt-in: point CHUNKR_BIG_PDF at a PDF with more than 1000 pages.
    ///
    /// Liteparse's own default caps parsing at 1000 pages; the backend must pass
    /// `usize::MAX` for `max_pages: None` so a long document is never silently cut off.
    #[test]
    fn long_documents_are_not_truncated() {
        let Ok(path) = std::env::var("CHUNKR_BIG_PDF") else {
            eprintln!("set CHUNKR_BIG_PDF to a >1000-page PDF to run this");
            return;
        };
        let parser = PdfParser::new(ParserConfig {
            backend: chunkr::parser::Backend::Liteparse,
            ..Default::default()
        });
        let documents = parser.load_pages(&path).unwrap();
        assert!(
            documents.len() > 1000,
            "parsed {} pages, expected the whole document",
            documents.len()
        );
        let last = documents.last().unwrap();
        let page = last.metadata["page_number"].as_u64().unwrap();
        assert!(page > 1000, "last page is {page}, so pages were dropped");
    }

    #[test]
    fn liteparse_backend_recovers_glyphs_lopdf_drops() {
        let Some(path) = fixture("textbook_27p.pdf") else {
            return;
        };
        let parser = PdfParser::new(ParserConfig {
            backend: chunkr::parser::Backend::Liteparse,
            ..Default::default()
        });
        let documents = parser.load_pages(&path).unwrap();
        assert_eq!(documents.len(), 27);
        let text = collect_text(&documents);
        assert_eq!(text.matches("Unimplemented").count(), 0);
        assert!(
            text.chars().count() > 80_000,
            "expected full text, got {} chars",
            text.chars().count()
        );
        // Reading order: the left column sentence is intact.
        assert!(text.contains("Contractual Constraints"));
    }

    #[test]
    fn sanitized_markdown_has_no_prose_grids() {
        let Some(path) = fixture("textbook_27p.pdf") else {
            return;
        };
        let parser = PdfParser::new(ParserConfig {
            backend: chunkr::parser::Backend::Liteparse,
            output: ParserOutput::Markdown,
            ..Default::default()
        });
        let outcome = parser
            .parse(&std::fs::read(&path).unwrap(), Some("textbook_27p.pdf"))
            .unwrap();
        let tables = table_blocks(&outcome.documents);
        assert!(
            tables.len() <= 4,
            "expected the prose grids to be demoted, kept {} tables",
            tables.len()
        );
        assert!(outcome.report.tables_demoted >= 5);
        assert!(outcome.report.headings_demoted > 0);
    }

    #[test]
    fn faithful_preset_keeps_backend_output() {
        let Some(path) = fixture("textbook_27p.pdf") else {
            return;
        };
        let parser = PdfParser::from_spec(Some("faithful")).unwrap();
        let outcome = parser
            .parse(&std::fs::read(&path).unwrap(), Some("textbook_27p.pdf"))
            .unwrap();
        assert!(outcome.report.is_clean());
        assert!(table_blocks(&outcome.documents).len() >= 5);
    }

    #[test]
    fn output_modes_and_granularity_compose() {
        let Some(path) = fixture("deck_16p.pdf") else {
            return;
        };
        let parser = PdfParser::new(ParserConfig {
            backend: chunkr::parser::Backend::Liteparse,
            output: ParserOutput::Markdown,
            granularity: Granularity::Block,
            ..Default::default()
        });
        let documents = parser.load(&path).unwrap();
        let kinds: std::collections::BTreeSet<String> = documents
            .iter()
            .filter_map(|d| d.metadata.get("block_type"))
            .filter_map(|v| v.as_str())
            .map(|s| s.to_string())
            .collect();
        assert!(kinds.contains("heading"));
        assert!(kinds.contains("list_item"));
        assert!(documents
            .iter()
            .any(|d| d.metadata["parser_backend"] == "liteparse"));
    }
}

// --- OCR backend selection, gating and language handling ---

fn page(number: usize, text: &str, reasons: &[&str], needs_ocr: bool) -> PagePayload {
    use chunkr::parser::payload::ComplexityPayload;
    PagePayload {
        page_number: number,
        text: text.to_string(),
        complexity: Some(ComplexityPayload {
            needs_ocr,
            reasons: reasons.iter().map(|r| r.to_string()).collect(),
            text_length: text.chars().count(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn ocr_config_defaults_are_conservative() {
    let ocr = ParserConfig::default().ocr;
    assert_eq!(ocr.backend, "tesseract");
    assert_eq!(ocr.mode, chunkr::parser::OcrMode::Off);
    assert!(!ocr.is_enabled());
    assert!(ocr.hedge_delays_ms.is_empty());
    assert_eq!(ocr.reasons(), chunkr::parser::DEFAULT_OCR_REASONS.to_vec());
    assert_eq!(ocr.ppocr.preset, "small");
    assert_eq!(ocr.ppocr.device, "cpu");
    assert_eq!(ocr.plugin.timeout_ms, 60_000);
    assert_eq!(ocr.plugin.concurrency, 1);
    assert!(ocr.plugin.warmup);
    assert_eq!(ocr.plugin.port, 0);
}

#[test]
fn ocr_config_round_trips_and_rejects_typos() {
    let config = ParserConfig::from_json_or_preset(Some(
        r#"{"ocr": {"mode": "always", "backend": "rapidocr", "hedge_delays_ms": [0, 250],
             "language": "en", "auto_reasons": ["scanned"], "auto_min_chars": 12,
             "ppocr": {"preset": "tiny", "device": "coreml"},
             "plugin": {"options": {"model": "ppocr_v5"}, "concurrency": 2, "warmup": false}}}"#,
    ))
    .unwrap();
    assert_eq!(
        config.ocr.backend_kind(),
        chunkr::parser::OcrBackendKind::Custom("rapidocr".into())
    );
    assert_eq!(config.ocr.hedge_delays_ms, vec![0, 250]);
    assert_eq!(config.ocr.reasons(), vec!["scanned".to_string()]);
    assert_eq!(config.ocr.auto_min_chars, 12);
    assert_eq!(config.ocr.ppocr.preset, "tiny");
    assert_eq!(config.ocr.plugin.concurrency, 2);
    assert!(!config.ocr.plugin.warmup);
    let json = serde_json::to_string(&config).unwrap();
    let reparsed: ParserConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(reparsed, config);

    for spec in [
        r#"{"ocr": {"hedge_delay_ms": [1]}}"#,
        r#"{"ocr": {"ppocr": {"preset_typo": "tiny"}}}"#,
        r#"{"ocr": {"plugin": {"timeouts": 5}}}"#,
    ] {
        assert!(
            ParserConfig::from_json_or_preset(Some(spec)).is_err(),
            "{spec}"
        );
    }
}

#[test]
fn ocr_backend_kind_classifies_builtins_and_names() {
    use chunkr::parser::{OcrBackendKind, OcrMode};
    let config = ParserConfig::default();
    let kind = |config: &ParserConfig| config.ocr.backend_kind();

    assert_eq!(kind(&config), OcrBackendKind::Tesseract);
    for alias in ["ppocr", "PP-OCR", "oar-ocr"] {
        let mut ppocr = config.clone();
        ppocr.ocr.backend = alias.to_string();
        assert_eq!(kind(&ppocr), OcrBackendKind::Ppocr, "{alias}");
    }
    let mut server = config.clone();
    server.ocr.backend = "server".to_string();
    assert_eq!(kind(&server), OcrBackendKind::Server);
    let mut named = config.clone();
    named.ocr.backend = "surya".to_string();
    assert_eq!(kind(&named), OcrBackendKind::Custom("surya".to_string()));
    // A server_url with the default backend still means the HTTP engine.
    let mut implied = config.clone();
    implied.ocr.backend = "tesseract".to_string();
    implied.ocr.server_url = Some("http://localhost:8829/ocr".to_string());
    assert_eq!(kind(&implied), OcrBackendKind::Server);
    // `mode="server"` wins over an explicitly named engine.
    let mut mode_server = config.clone();
    mode_server.ocr.mode = OcrMode::Server;
    mode_server.ocr.backend = "ppocr".to_string();
    assert_eq!(kind(&mode_server), OcrBackendKind::Server);
}

#[test]
fn ocr_language_is_converted_per_engine() {
    use chunkr::parser::{ocr_language_iso, ocr_language_tesseract, OcrBackendKind};

    assert_eq!(ocr_language_iso("eng"), "en");
    assert_eq!(ocr_language_iso("chi_sim"), "zh");
    assert_eq!(ocr_language_iso("zh-cn"), "zh-cn");
    assert_eq!(ocr_language_tesseract("en"), "eng");
    assert_eq!(ocr_language_tesseract("zh"), "chi_sim");
    assert_eq!(ocr_language_tesseract("de"), "deu");

    let mut config = ParserConfig::default();
    assert_eq!(config.ocr.engine_language(), "eng");
    config.ocr.language = "en".to_string();
    assert_eq!(config.ocr.engine_language(), "eng");
    config.ocr.backend = "server".to_string();
    assert_eq!(config.ocr.engine_language(), "en");
    assert_eq!(config.ocr.backend_kind(), OcrBackendKind::Server);
}

#[test]
fn page_ranges_are_compressed() {
    use chunkr::parser::format_page_range;
    assert_eq!(format_page_range(&[]), "");
    assert_eq!(format_page_range(&[4]), "4");
    assert_eq!(format_page_range(&[1, 2, 3, 7, 9, 10]), "1-3,7,9-10");
}

#[test]
fn auto_gating_uses_reasons_not_the_backend_boolean() {
    let mut config = ParserConfig::default();
    config.ocr.mode = chunkr::parser::OcrMode::Auto;
    config.ocr.backend = "server".to_string();
    config.ocr.server_url = Some("http://localhost:8829/ocr".to_string());

    // Slide/textbook pages flag `embedded-images`/`vector-text` on every page
    // while having a working text layer: those must not trigger OCR. The gate
    // normalizes `sparse-text` and `sparse_text` to the same signal.
    let pages = vec![
        page(
            1,
            "a full page of digital text",
            &["embedded-images", "vector-text"],
            true,
        ),
        page(2, "", &["scanned", "no-text"], true),
        page(3, "short", &["sparse-text"], false),
        page(4, "good page", &[], false),
    ];
    assert_eq!(chunkr::parser::select_ocr_pages(&pages, &config), vec![2]);

    // Garbled text and a `min_chars` floor also select a page.
    let hopeless = vec![
        page(1, "V椀jau E昀케ciency", &["garbled"], false),
        page(2, "tiny", &[], false),
    ];
    assert_eq!(
        chunkr::parser::select_ocr_pages(&hopeless, &config),
        vec![1]
    );
    config.ocr.auto_min_chars = 6;
    assert_eq!(
        chunkr::parser::select_ocr_pages(&hopeless, &config),
        vec![1, 2]
    );

    // `auto_reasons` replaces the default gate (`sparse-text` becomes selectable).
    config.ocr.auto_min_chars = 0;
    config.ocr.auto_reasons = vec!["sparse_text".to_string()];
    assert_eq!(chunkr::parser::select_ocr_pages(&pages, &config), vec![3]);
    config.ocr.auto_reasons = vec!["embedded-images".to_string()];
    assert_eq!(chunkr::parser::select_ocr_pages(&pages, &config), vec![1]);

    // A backend that reports no reasons still gets the boolean honoured.
    let reasonless = vec![page(5, "no signals", &[], true)];
    config.ocr.auto_reasons.clear();
    assert_eq!(
        chunkr::parser::select_ocr_pages(&reasonless, &config),
        vec![5]
    );

    // An explicit scope never widens.
    config.scope.target_pages = Some("1,3".to_string());
    assert_eq!(
        chunkr::parser::select_ocr_pages(&pages, &config),
        Vec::<usize>::new()
    );
    config.scope.target_pages = Some("2-4".to_string());
    assert_eq!(chunkr::parser::select_ocr_pages(&pages, &config), vec![2]);

    // Without complexity signals, `auto` OCRs rather than returning empty pages.
    config.ocr.auto_reasons.clear();
    config.scope.target_pages = None;
    let opaque = vec![PagePayload {
        page_number: 9,
        text: String::new(),
        ..Default::default()
    }];
    assert_eq!(chunkr::parser::select_ocr_pages(&opaque, &config), vec![9]);
}

#[test]
fn config_validation_catches_unusable_ocr_setups() {
    let base = ParserConfig::default();
    assert!(base.validate().is_ok());

    let mut server_without_url = base.clone();
    server_without_url.ocr.mode = chunkr::parser::OcrMode::Always;
    server_without_url.ocr.backend = "server".to_string();
    let error = server_without_url.validate().unwrap_err();
    assert!(error.contains("server_url"), "{error}");

    let mut bad_preset = base.clone();
    bad_preset.ocr.backend = "ppocr".to_string();
    bad_preset.ocr.ppocr.preset = "huge".to_string();
    assert!(bad_preset.validate().unwrap_err().contains("preset"));

    let mut bad_device = base.clone();
    bad_device.ocr.backend = "ppocr".to_string();
    bad_device.ocr.ppocr.device = "tpu".to_string();
    assert!(bad_device.validate().unwrap_err().contains("device"));

    let mut empty_backend = base.clone();
    empty_backend.ocr.mode = chunkr::parser::OcrMode::Always;
    empty_backend.ocr.backend = " ".to_string();
    assert!(empty_backend
        .validate()
        .unwrap_err()
        .contains("must not be empty"));

    let mut bad_dpi = base.clone();
    bad_dpi.scope.dpi = 0.0;
    assert!(bad_dpi.validate().unwrap_err().contains("dpi"));

    let mut bad_pages = base.clone();
    bad_pages.scope.target_pages = Some("1-".to_string());
    assert!(bad_pages.validate().unwrap_err().contains("target_pages"));

    let mut bad_plugin = base.clone();
    bad_plugin.ocr.plugin.concurrency = 0;
    assert!(bad_plugin.validate().unwrap_err().contains("concurrency"));

    // Names resolved by a plugin pass validation; the parse fails instead.
    let mut custom = base.clone();
    custom.ocr.mode = chunkr::parser::OcrMode::Always;
    custom.ocr.backend = "our-inhouse".to_string();
    assert!(custom.validate().is_ok());
}

#[cfg(feature = "pdf")]
mod ocr_native {
    use super::*;

    fn fixture(name: &str) -> Option<PathBuf> {
        let path = PathBuf::from("tests/test_files").join(name);
        path.exists().then_some(path)
    }

    /// Counts OCR calls and returns one plausible word per page.
    struct RecordingEngine {
        hits: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl chunkr::parser::OcrEngine for RecordingEngine {
        fn name(&self) -> &str {
            "recording"
        }

        fn recognize<'a, 'b: 'a, 'c: 'a>(
            &'a self,
            _image: &'c [u8],
            _width: u32,
            _height: u32,
            _options: &'b chunkr::parser::OcrOptions,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<
                            Vec<chunkr::parser::OcrResult>,
                            Box<dyn std::error::Error + Send + Sync>,
                        >,
                    > + Send
                    + '_,
            >,
        > {
            self.hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Box::pin(async {
                Ok(vec![chunkr::parser::OcrResult {
                    text: "ocrword".to_string(),
                    bbox: [10.0, 10.0, 60.0, 30.0],
                    confidence: 0.9,
                    polygon: None,
                }])
            })
        }
    }

    fn parser(spec: &str) -> (PdfParser, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let parser = PdfParser::from_spec(Some(spec))
            .unwrap()
            .with_ocr_engine(std::sync::Arc::new(RecordingEngine { hits: hits.clone() }));
        (parser, hits)
    }

    fn text_of(parser: &PdfParser, path: &PathBuf) -> String {
        parser
            .load_pages(path)
            .unwrap()
            .iter()
            .map(|document| document.content.clone())
            .collect()
    }

    /// deck_16p.pdf flags `vector-text`/`embedded-images`/`sparse-text` on nearly
    /// every page: none of those gate the default auto rule, so auto must skip
    /// the OCR pass entirely and match `mode="off"`.
    #[test]
    fn auto_mode_skips_ocr_on_digital_documents() {
        let Some(path) = fixture("deck_16p.pdf") else {
            return;
        };
        let (off, off_hits) =
            parser(r#"{"backend": "liteparse", "output": "markdown", "ocr": {"mode": "off"}}"#);
        let (auto, auto_hits) =
            parser(r#"{"backend": "liteparse", "output": "markdown", "ocr": {"mode": "auto"}}"#);

        let off_text = text_of(&off, &path);
        let auto_text = text_of(&auto, &path);
        assert_eq!(auto_text, off_text);
        assert!(auto_text.chars().count() > 4_000);
        assert_eq!(auto_hits.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(off_hits.load(std::sync::atomic::Ordering::SeqCst), 0);

        // Complexity signals stay internal to the gate.
        let (pages, _) = auto.pages(&std::fs::read(&path).unwrap()).unwrap();
        assert!(pages.iter().all(|page| page.complexity.is_none()));
    }

    /// With `sparse-text` opted into the gate, only the pages carrying it are
    /// re-parsed with OCR: four engine calls instead of one per page.
    #[test]
    fn auto_mode_ocrs_only_the_gated_pages() {
        let Some(path) = fixture("deck_16p.pdf") else {
            return;
        };
        let bytes = std::fs::read(&path).unwrap();

        let (auto, auto_hits) = parser(
            r#"{"backend": "liteparse", "output": "markdown",
                 "ocr": {"mode": "auto", "auto_reasons": ["sparse-text"]}}"#,
        );
        let (always, always_hits) =
            parser(r#"{"backend": "liteparse", "output": "markdown", "ocr": {"mode": "always"}}"#);
        // Gate expectation straight from the fixture's own complexity signals.
        let (off, _) =
            parser(r#"{"backend": "liteparse", "output": "markdown", "ocr": {"mode": "off"}}"#);
        let (probe, _) = parser(
            r#"{"backend": "liteparse", "output": "text", "include_complexity": true,
                 "ocr": {"mode": "off"}}"#,
        );
        let (probe_pages, _) = probe.pages(&bytes).unwrap();
        let expected = chunkr::parser::select_ocr_pages(&probe_pages, auto.config());
        assert_eq!(expected, vec![1, 11, 12, 14]);

        let off_documents = off.load_pages(&path).unwrap();
        let auto_documents = auto.load_pages(&path).unwrap();
        let always_documents = always.load_pages(&path).unwrap();
        assert_eq!(auto_documents.len(), 16);
        assert_eq!(auto_documents.len(), always_documents.len());
        assert_eq!(
            auto_hits.load(std::sync::atomic::Ordering::SeqCst),
            expected.len()
        );
        assert_eq!(always_hits.load(std::sync::atomic::Ordering::SeqCst), 16);
        assert_eq!(auto_hits.load(std::sync::atomic::Ordering::SeqCst), 4);
        // Pages outside the gate are untouched; the merged pages keep their text.
        for (index, pages) in [(1usize, &auto_documents), (2usize, &auto_documents)] {
            assert_eq!(pages[index].content, off_documents[index].content);
        }
        assert!(auto_documents.iter().all(|d| !d.content.trim().is_empty()));
    }

    #[test]
    fn unavailable_ocr_backends_fail_before_parsing() {
        #[cfg(not(feature = "pdf-ocr-ppocr"))]
        {
            let ppocr = PdfParser::from_spec(Some(
                r#"{"backend": "liteparse", "ocr": {"mode": "always", "backend": "ppocr"}}"#,
            ))
            .unwrap();
            let error = ppocr.parse(&[], None).unwrap_err().to_string();
            assert!(error.contains("pdf-ocr-ppocr"), "{error}");
        }

        let custom = PdfParser::from_spec(Some(
            r#"{"backend": "liteparse", "ocr": {"mode": "always", "backend": "our-inhouse"}}"#,
        ))
        .unwrap();
        let error = custom.parse(&[], None).unwrap_err().to_string();
        assert!(
            error.contains("our-inhouse") && error.contains("server"),
            "{error}"
        );

        // The `server` backend is rejected at construction, before any parse.
        let error = PdfParser::from_spec(Some(
            r#"{"backend": "liteparse", "ocr": {"mode": "always", "backend": "server"}}"#,
        ))
        .unwrap_err()
        .to_string();
        assert!(error.contains("server_url"), "{error}");
    }
}

/// The accelerator is a compile-time choice upstream, so a config that asks for
/// a different one is an error rather than a silent CPU fallback.
#[cfg(feature = "pdf-ocr-ppocr")]
#[test]
fn ppocr_device_must_match_the_compiled_feature() {
    let mut config = ParserConfig::default();
    config.ocr.mode = chunkr::parser::OcrMode::Always;
    config.ocr.backend = "ppocr".to_string();
    config.ocr.ppocr.device = "tensorrt".to_string();
    let error = config.validate().unwrap_err();
    assert!(error.contains("compiled PP-OCR for"), "{error}");

    config.ocr.ppocr.device = chunkr::parser::config::ppocr_compiled_device()
        .unwrap()
        .to_string();
    assert!(config.validate().is_ok());
}

/// Opt-in: `CHUNKR_PPOCR_E2E=<page.png>` runs the real ONNX engine over a
/// rasterised page (models download into $OAR_HOME on first use). Optional
/// `CHUNKR_PPOCR_REF=<pdf>` + `CHUNKR_PPOCR_REF_PAGE=<n>` score the result
/// against that page's own text layer. Rasterise with `liteparse` and
/// `extract_screenshots=True`.
#[cfg(feature = "pdf-ocr-ppocr")]
#[test]
fn ppocr_reads_a_rasterized_page() {
    let Ok(page) = std::env::var("CHUNKR_PPOCR_E2E") else {
        return;
    };
    let parser = PdfParser::from_spec(Some(
        r#"{"backend": "liteparse", "output": "markdown",
             "ocr": {"mode": "always", "backend": "ppocr", "language": "en",
                     "ppocr": {"preset": "tiny"}}}"#,
    ))
    .unwrap();
    let documents = parser.load_pages(&page).unwrap();
    let text = documents
        .iter()
        .map(|document| document.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let seen: std::collections::HashSet<String> = page_tokens(&text);
    assert!(seen.len() > 20, "PP-OCR returned too little text: {text:?}");

    // Optional scoring against the page's own text layer, so the assertion
    // carries no hardcoded phrase from any document.
    let Ok(reference_pdf) = std::env::var("CHUNKR_PPOCR_REF") else {
        return;
    };
    let page_number: usize = std::env::var("CHUNKR_PPOCR_REF_PAGE")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);
    let reference = PdfParser::from_spec(Some(
        r#"{"backend": "liteparse", "output": "markdown", "ocr": {"mode": "off"}}"#,
    ))
    .unwrap()
    .load_pages(&reference_pdf)
    .map(|documents| {
        documents
            .get(page_number.saturating_sub(1))
            .map(|document| document.content.clone())
            .unwrap_or_default()
    })
    .unwrap_or_default();
    let expected: std::collections::HashSet<String> = page_tokens(&reference);
    let recall = expected
        .iter()
        .filter(|token| seen.contains(*token))
        .count() as f64
        / expected.len().max(1) as f64;
    assert!(
        recall > 0.5,
        "PP-OCR recovered {recall:.2} of the page's words ({text:?})"
    );
}

/// Alphanumeric lowercase tokens, for OCR comparisons.
#[cfg(feature = "pdf-ocr-ppocr")]
fn page_tokens(text: &str) -> std::collections::HashSet<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_lowercase())
        .collect()
}
