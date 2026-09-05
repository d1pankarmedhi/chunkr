use chunkr::prelude::*;
use std::collections::HashMap;

fn doc_with(content: &str, pairs: &[(&str, serde_json::Value)]) -> Document {
    let mut meta = HashMap::new();
    for (k, v) in pairs {
        meta.insert(k.to_string(), v.clone());
    }
    Document::new(content.to_string(), meta)
}

#[test]
fn test_csv_quoted_comma_header_columns() {
    let csv = "\"last, first\",age,city\n\"doe, john\",30,NYC\n\"smith, jane\",25,LA";

    let chunker = TableChunker::new()
        .with_format(TableFormat::Csv)
        .with_rows_per_chunk(Some(10))
        .with_overlap_rows(0);

    let chunks = chunker.chunk(csv).unwrap();
    assert_eq!(chunks.len(), 1);

    let cols = chunks[0]
        .metadata
        .get("columns")
        .unwrap()
        .as_array()
        .unwrap();
    let cols: Vec<&str> = cols.iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(cols, vec!["last, first", "age", "city"]);

    // Header row repeated intact (quotes preserved).
    assert!(chunks[0].content.starts_with("\"last, first\",age,city"));
    assert!(chunks[0].content.contains("\"doe, john\",30,NYC"));
}

#[test]
fn test_csv_quoted_header_repeated_across_chunks() {
    let csv = "\"last, first\",age,city\na,1,x\nb,2,y\nc,3,z";

    let chunker = TableChunker::new()
        .with_format(TableFormat::Csv)
        .with_rows_per_chunk(Some(1))
        .with_overlap_rows(0);

    let chunks = chunker.chunk(csv).unwrap();
    assert_eq!(chunks.len(), 3);
    for chunk in &chunks {
        assert!(chunk.content.starts_with("\"last, first\",age,city"));
    }
}

#[test]
fn test_tsv_quoted_tab_header_columns() {
    let tsv = "\"a\tb\"\tc\n1\t2";

    let chunker = TableChunker::new()
        .with_format(TableFormat::Tsv)
        .with_rows_per_chunk(Some(10))
        .with_overlap_rows(0);

    let chunks = chunker.chunk(tsv).unwrap();
    assert_eq!(chunks.len(), 1);
    let cols = chunks[0]
        .metadata
        .get("columns")
        .unwrap()
        .as_array()
        .unwrap();
    let cols: Vec<&str> = cols.iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(cols, vec!["a\tb", "c"]);
}

#[test]
fn test_csv_quoted_field_auto_detect() {
    // A quoted comma must not make a plain line look like CSV... and a real
    // quoted CSV must still be detected as CSV.
    let csv = "name,note\n\"doe, john\",\"likes, commas\"";
    let chunker = TableChunker::new().with_rows_per_chunk(Some(10));
    let chunks = chunker.chunk(csv).unwrap();
    assert!(!chunks.is_empty());
    assert_eq!(
        chunks[0].metadata.get("format").unwrap().as_str().unwrap(),
        "csv"
    );
}

#[test]
fn test_packer_merged_page_numbers() {
    let docs = vec![
        doc_with(
            "Chunk one.",
            &[
                ("page_number", serde_json::Value::from(1)),
                ("file_path", serde_json::Value::from("doc.pdf")),
            ],
        ),
        doc_with(
            "Chunk two.",
            &[
                ("page_number", serde_json::Value::from(2)),
                ("file_path", serde_json::Value::from("doc.pdf")),
            ],
        ),
        doc_with(
            "Chunk three.",
            &[
                ("page_number", serde_json::Value::from(3)),
                ("file_path", serde_json::Value::from("doc.pdf")),
            ],
        ),
    ];

    let packed = ChunkPacker::new(1000).pack(&docs);
    assert_eq!(packed.len(), 1);

    // Divergent span key: singular removed, merged array recorded.
    assert!(!packed[0].metadata.contains_key("page_number"));
    assert_eq!(
        packed[0].metadata.get("merged_page_number").unwrap(),
        &serde_json::json!([1, 2, 3])
    );
    // Shared key keeps first-wins inheritance.
    assert_eq!(
        packed[0].metadata.get("file_path").unwrap().as_str().unwrap(),
        "doc.pdf"
    );
    assert_eq!(
        packed[0]
            .metadata
            .get("merged_chunk_count")
            .unwrap()
            .as_u64()
            .unwrap(),
        3
    );
}

#[test]
fn test_packer_identical_page_number_kept() {
    let docs = vec![
        doc_with("Chunk one.", &[("page_number", serde_json::Value::from(5))]),
        doc_with("Chunk two.", &[("page_number", serde_json::Value::from(5))]),
        doc_with("Chunk three.", &[("page_number", serde_json::Value::from(5))]),
    ];

    let packed = ChunkPacker::new(1000).pack(&docs);
    assert_eq!(packed.len(), 1);
    assert_eq!(
        packed[0].metadata.get("page_number").unwrap().as_u64().unwrap(),
        5
    );
    assert!(!packed[0]
        .metadata
        .keys()
        .any(|k| k.starts_with("merged_page")));
}

#[test]
fn test_packer_single_source_unchanged() {
    let docs = vec![doc_with(
        "Lonely chunk.",
        &[("page_number", serde_json::Value::from(7))],
    )];

    let packed = ChunkPacker::new(1000).pack(&docs);
    assert_eq!(packed.len(), 1);
    assert_eq!(
        packed[0].metadata.get("page_number").unwrap().as_u64().unwrap(),
        7
    );
    // Only the standard merge counters may use the merged_ prefix.
    assert!(packed[0]
        .metadata
        .keys()
        .filter(|k| k.starts_with("merged_"))
        .all(|k| k == "merged_chunk_count"));
}

#[test]
fn test_packer_existing_expectations_hold() {
    // Mirrors tests/chunker_tests.rs::test_chunk_packer.
    let docs = vec![
        Document::new("Short heading 1", HashMap::new()),
        Document::new("Sentence A", HashMap::new()),
        Document::new("Sentence B", HashMap::new()),
        Document::new(
            "Another long paragraph that will push past the max characters budget.",
            HashMap::new(),
        ),
    ];

    let packer = ChunkPacker::new(50);
    let packed = packer.pack(&docs);

    assert_eq!(packed.len(), 2);
    assert_eq!(
        packed[0]
            .metadata
            .get("merged_chunk_count")
            .unwrap()
            .as_u64()
            .unwrap(),
        3
    );
    assert!(packed[0].content.contains("Short heading 1"));
    assert!(packed[0].content.contains("Sentence B"));
}
