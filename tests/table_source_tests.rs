//! Registered table sources: dispatch, Markdown mapping, metadata, routing and
//! the guarantees an extension package relies on.

use std::path::{Path, PathBuf};

use chunkr::prelude::*;
use serde_json::json;

/// A source that renders a fixed table for one extension, standing in for a
/// real columnar extractor.
struct FakeTableSource {
    extensions: Vec<String>,
    tables: Vec<TableData>,
    fail: bool,
}

impl TableSource for FakeTableSource {
    fn extensions(&self) -> Vec<String> {
        self.extensions.clone()
    }

    fn load(
        &self,
        path: Option<&Path>,
        bytes: Option<&[u8]>,
        extension: &str,
        _source: Option<&str>,
    ) -> Result<Vec<TableData>, ChunkrError> {
        assert_eq!(extension, self.extensions[0]);
        if self.fail {
            return Err(ChunkrError::ParseError("fake extractor failed".to_string()));
        }
        // Exactly one input flavour is offered, never both and never neither.
        assert!(
            path.is_some() ^ bytes.is_some(),
            "paths and bytes are mutually exclusive"
        );
        Ok(self.tables.clone())
    }
}

fn table(name: &str, columns: &[&str], rows: &[&[&str]], row_offset: Option<u64>) -> TableData {
    TableData {
        name: Some(name.to_string()),
        columns: columns.iter().map(|c| c.to_string()).collect(),
        rows: rows
            .iter()
            .map(|row| row.iter().map(|cell| cell.to_string()).collect())
            .collect(),
        row_offset,
    }
}

fn fake_tables() -> Vec<TableData> {
    vec![
        table(
            "rows_0_1",
            &["city", "pop"],
            &[&["Paris", "2.1M"], &["Berlin", "3.7M"]],
            Some(0),
        ),
        table("rows_2_2", &["city", "pop"], &[&["Rome", "2.8M"]], Some(2)),
    ]
}

fn register(name: &str, tables: Vec<TableData>, fail: bool) {
    // Extensions are unique per test run so tests never fight over the registry.
    let extension = name.to_string();
    register_table_source(
        name,
        Box::new(FakeTableSource {
            extensions: vec![extension],
            tables,
            fail,
        }),
    );
}

fn unique_tmp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "chunkr_tablesource_{}_{}_{}",
        tag,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp test dir");
    dir
}

#[test]
fn test_registered_source_renders_markdown_with_spans() {
    register("fake_markdown", fake_tables(), false);
    let documents = load_documents_from_bytes(b"ignored", "fake_markdown", Some("data.col")).unwrap();

    assert_eq!(documents.len(), 2);
    let first = &documents[0];
    assert_eq!(
        first.content,
        "| city | pop |\n| --- | --- |\n| Paris | 2.1M |\n| Berlin | 3.7M |"
    );
    assert_eq!(first.metadata["format"], json!("table"));
    assert_eq!(first.metadata["table_format"], json!("fake_markdown"));
    assert_eq!(first.metadata["table_name"], json!("rows_0_1"));
    assert_eq!(first.metadata["table_index"], json!(0));
    assert_eq!(first.metadata["total_tables"], json!(2));
    assert_eq!(first.metadata["row_count"], json!(2));
    assert_eq!(first.metadata["column_count"], json!(2));
    assert_eq!(first.metadata["row_offset"], json!(0));
    assert_eq!(first.metadata["file_name"], json!("data.col"));

    assert_eq!(documents[1].metadata["table_index"], json!(1));
    assert_eq!(documents[1].metadata["row_offset"], json!(2));
    assert_eq!(documents[1].metadata["row_count"], json!(1));
}

#[test]
fn test_cells_are_escaped_and_blank_rows_dropped() {
    register(
        "fake_escaping",
        vec![table(
            "t",
            // An empty header gets a positional name, like spreadsheets do.
            &["a", "", "c\nd"],
            &[&["x | y", "  ", "z"], &["", "", ""]],
            None,
        )],
        false,
    );

    let documents = load_documents_from_bytes(b"", "fake_escaping", None).unwrap();
    assert_eq!(documents.len(), 1);
    assert_eq!(
        documents[0].content,
        "| a | column_2 | c d |\n| --- | --- | --- |\n| x \\| y |    | z |"
    );
    assert!(
        !documents[0].metadata.contains_key("row_offset"),
        "no offset when the source does not report one"
    );
}

#[test]
fn test_empty_tables_are_skipped() {
    register(
        "fake_empty",
        vec![
            table("empty", &["a"], &[], None),
            table("headers_only", &["a"], &[], None),
        ],
        false,
    );

    // A header-only table still renders its schema, so nothing is dropped here.
    let documents = load_documents_from_bytes(b"", "fake_empty", None).unwrap();
    assert_eq!(documents.len(), 2);

    register("fake_no_columns", vec![TableData::default()], false);
    let documents = load_documents_from_bytes(b"", "fake_no_columns", None).unwrap();
    assert!(documents.is_empty(), "a table without columns renders nothing");
}

#[test]
fn test_registered_extension_is_known_and_dispatched_by_path() {
    register("fake_path", fake_tables(), false);

    assert!(supported_extensions().iter().any(|e| e == "fake_path"));
    assert!(is_structured("fake_path"));
    assert!(is_structured("FAKE_PATH"), "extension lookup is case-insensitive");
    assert!(table_sources().iter().any(|name| name == "fake_path"));

    let dir = unique_tmp_dir("path");
    let file = dir.join("data.fake_path");
    std::fs::write(&file, b"not read by the source").unwrap();

    let documents = load_documents(&file).unwrap();
    assert_eq!(documents.len(), 2);
    assert_eq!(documents[0].metadata["file_extension"], json!("fake_path"));

    // DirectoryLoader routes registered extensions to row-based table chunking.
    let chunks = DirectoryLoader::new()
        .with_chunk_size(200)
        .with_overlap(0)
        .with_extensions(vec!["fake_path".to_string()])
        .load_and_chunk(&dir)
        .unwrap();
    assert!(!chunks.is_empty());
    assert!(
        chunks.iter().all(|chunk| chunk.metadata["row_count"].is_number()),
        "routed chunks keep table metadata"
    );
    assert!(
        chunks.iter().all(|chunk| chunk.metadata["table_format"] == json!("fake_path")),
        "routed chunks keep the source format"
    );

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_errors_are_qualified_with_the_source_and_unregister_restores_text_fallback() {
    register("fake_failing", fake_tables(), true);
    let error = load_documents_from_bytes(b"", "fake_failing", Some("broken.col")).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("broken.col"), "source missing from: {message}");
    assert!(
        message.contains("fake extractor failed"),
        "cause missing from: {message}"
    );

    assert!(unregister_table_source("fake_failing"));
    assert!(!unregister_table_source("fake_failing"), "second removal is a no-op");

    // Without a source, an unknown extension keeps its old text behaviour.
    let documents = load_documents_from_bytes(b"plain text", "fake_failing", None).unwrap();
    assert_eq!(documents[0].content, "plain text");
    assert_eq!(documents[0].metadata["format"], json!("text"));
}

#[test]
fn test_registering_twice_replaces_the_source() {
    register("fake_replace", fake_tables(), false);
    register(
        "fake_replace",
        vec![table("only", &["a"], &[&["1"]], None)],
        false,
    );

    let documents = load_documents_from_bytes(b"", "fake_replace", None).unwrap();
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].metadata["table_name"], json!("only"));
    assert_eq!(
        table_sources()
            .iter()
            .filter(|name| name.as_str() == "fake_replace")
            .count(),
        1,
        "a name is registered once"
    );
}

#[test]
fn test_packer_reconciles_table_spans_and_sums_row_counts() {
    register("fake_pack", fake_tables(), false);
    let documents = load_documents_from_bytes(b"", "fake_pack", Some("data.col")).unwrap();
    assert_eq!(documents.len(), 2);

    let packed = ChunkPacker::new(10_000).pack(&documents);
    assert_eq!(packed.len(), 1);
    let metadata = &packed[0].metadata;

    for key in ["table_index", "table_name", "row_offset"] {
        assert!(
            !metadata.contains_key(key),
            "merged chunk must not claim one source's {key}: {metadata:?}"
        );
        assert!(metadata.contains_key(&format!("merged_{key}")));
    }
    assert_eq!(
        metadata["row_count"],
        json!(3),
        "row counts are summed over merged tables"
    );
}
