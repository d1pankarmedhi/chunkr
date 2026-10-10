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
        text: "Arti\u{FA00}\u{6900}cially V\u{6900}\u{6800}ay\u{FFFD}".to_string(),
        ..Default::default()
    };
    let mut pages = vec![page.clone()];
    let report = sanitize_pages(&mut pages, &cfg.sanitize);
    assert!(report.glyph_repairs >= 4);
    assert_eq!(pages[0].text, "Artificially Vijay");

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

    #[test]
    fn liteparse_backend_recovers_glyphs_lopdf_drops() {
        let Some(path) = fixture("lebs201.pdf") else {
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
        let Some(path) = fixture("lebs201.pdf") else {
            return;
        };
        let parser = PdfParser::new(ParserConfig {
            backend: chunkr::parser::Backend::Liteparse,
            output: ParserOutput::Markdown,
            ..Default::default()
        });
        let outcome = parser
            .parse(&std::fs::read(&path).unwrap(), Some("lebs201.pdf"))
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
        let Some(path) = fixture("lebs201.pdf") else {
            return;
        };
        let parser = PdfParser::from_spec(Some("faithful")).unwrap();
        let outcome = parser
            .parse(&std::fs::read(&path).unwrap(), Some("lebs201.pdf"))
            .unwrap();
        assert!(outcome.report.is_clean());
        assert!(table_blocks(&outcome.documents).len() >= 5);
    }

    #[test]
    fn output_modes_and_granularity_compose() {
        let Some(path) = fixture("finance.pdf") else {
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
