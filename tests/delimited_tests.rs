//! `.csv` / `.tsv` load as tables: Markdown, headers attached, spans recorded.

use chunkr::loader::{load_documents, load_documents_from_bytes, parse_records};
use chunkr::prelude::*;
use serde_json::json;

fn unique_tmp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("chunkr-delimited-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn csv_loads_as_a_markdown_table_with_span_metadata() {
    let documents = load_documents_from_bytes(
        b"region,units\nNorth,120\nSouth,98\n",
        "csv",
        Some("sales.csv"),
    )
    .unwrap();
    assert_eq!(documents.len(), 1, "one delimited file is one table");
    let document = &documents[0];
    assert!(
        document.content.starts_with("| region | units |"),
        "{}",
        document.content
    );
    assert!(
        document.content.contains("| North | 120 |"),
        "{}",
        document.content
    );
    assert_eq!(document.metadata["format"], json!("table"));
    assert_eq!(document.metadata["table_format"], json!("csv"));
    assert_eq!(document.metadata["row_count"], json!(2));
    assert_eq!(document.metadata["column_count"], json!(2));
    assert_eq!(document.metadata["row_offset"], json!(0));
}

#[test]
fn a_delimited_file_on_disk_is_not_raw_text_any_more() {
    let dir = unique_tmp_dir("disk");
    let path = dir.join("sales.csv");
    std::fs::write(&path, "region,units\nNorth,120\n").unwrap();
    let documents = load_documents(&path).unwrap();
    assert_eq!(documents[0].metadata["format"], json!("table"));
    assert!(
        !documents[0].content.contains("North,120"),
        "{}",
        documents[0].content
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn quoted_fields_keep_their_delimiters_newlines_and_quotes() {
    let text = "\"a,x\",\"b\nb\"\n\"1 \"\"q\"\"\",2\n";
    let documents = load_documents_from_bytes(text.as_bytes(), "csv", None).unwrap();
    let content = &documents[0].content;
    // The embedded comma stays inside the header cell, the newline becomes a space.
    assert!(content.starts_with("| a,x | b b |"), "{content}");
    assert!(content.contains("| 1 \"q\" | 2 |"), "{content}");
    assert_eq!(documents[0].metadata["column_count"], json!(2));
}

#[test]
fn tsv_keeps_commas_and_splits_on_tabs() {
    let documents =
        load_documents_from_bytes(b"name\tnote\nAda\tmaths, logic\n", "tsv", None).unwrap();
    let content = &documents[0].content;
    assert!(content.starts_with("| name | note |"), "{content}");
    assert!(content.contains("| Ada | maths, logic |"), "{content}");
    assert_eq!(documents[0].metadata["column_count"], json!(2));
}

#[test]
fn crlf_bom_and_trailing_blank_lines_are_handled() {
    let documents =
        load_documents_from_bytes(b"\xef\xbb\xbfa,b\r\n1,2\r\n\r\n", "csv", None).unwrap();
    let document = &documents[0];
    assert_eq!(document.metadata["column_count"], json!(2));
    assert_eq!(document.metadata["row_count"], json!(1));
    // The BOM must not end up inside the first header cell.
    assert!(
        document.content.starts_with("| a | b |"),
        "{}",
        document.content
    );
}

#[test]
fn pipes_are_escaped_so_the_table_stays_parseable() {
    let documents = load_documents_from_bytes(b"a,b\nx|y,2\n", "csv", None).unwrap();
    assert!(
        documents[0].content.contains("x\\|y"),
        "{}",
        documents[0].content
    );
}

#[test]
fn a_header_only_file_still_loads() {
    let documents = load_documents_from_bytes(b"a,b\n", "csv", None).unwrap();
    assert_eq!(documents[0].metadata["row_count"], json!(0));
    assert!(documents[0].content.contains("| a | b |"));
}

#[test]
fn an_empty_file_is_an_error_not_an_empty_document() {
    let error = load_documents_from_bytes(b"", "csv", Some("empty.csv")).unwrap_err();
    assert!(matches!(error, ChunkrError::EmptyInput), "{error}");
}

#[test]
fn records_parse_leniently() {
    // An unquoted quote is kept as written; a missing closing quote keeps the rest.
    let records = parse_records("a,b\nhe said \"hi\",2\n", b',');
    assert_eq!(records, vec![vec!["a", "b"], vec!["he said \"hi\"", "2"]]);
    let records = parse_records("\"open,b\n", b',');
    assert_eq!(records, vec![vec!["open,b\n"]]); // unterminated quote keeps the rest
}

#[test]
fn a_csv_member_inside_a_zip_is_a_table_too() {
    // Same bytes, different route: archive members are dispatched by extension.
    let documents = load_documents_from_bytes(
        b"region,units\nNorth,120\n",
        "csv",
        Some("bundle.zip!a.csv"),
    )
    .unwrap();
    assert_eq!(documents[0].metadata["format"], json!("table"));
}
