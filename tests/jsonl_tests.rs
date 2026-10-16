//! JSONL/NDJSON coverage: line-delimited record batching, record spans, JSONL
//! detection boundaries, dispatch/routing and packer span reconciliation.

use chunkr::prelude::*;
use serde_json::json;
use std::path::PathBuf;

fn jsonl(records: usize) -> String {
    (0..records)
        .map(|i| format!("{{\"id\":{i},\"name\":\"item-{i}\",\"active\":{}}}", i % 2 == 0))
        .collect::<Vec<_>>()
        .join("\n")
}

fn record_span(chunk: &Document) -> (u64, u64) {
    (
        chunk.metadata["start_record"].as_u64().expect("start_record"),
        chunk.metadata["end_record"].as_u64().expect("end_record"),
    )
}

fn unique_tmp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "chunkr_jsonl_{}_{}_{}",
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
fn test_jsonl_records_batch_with_exact_spans_and_whole_lines() {
    let text = jsonl(500);
    let chunks = JsonChunker::new().with_max_chunk_size(400).chunk(&text).unwrap();

    assert!(chunks.len() > 1, "500 records must batch");

    let mut expected_start = 0u64;
    let mut seen = Vec::new();
    for chunk in &chunks {
        let (start, end) = record_span(chunk);
        assert_eq!(start, expected_start, "batches must be contiguous");
        assert!(end >= start);
        assert_eq!(chunk.content.lines().count() as u64, end - start + 1);

        // Every line is intact JSON: no record is split across chunks.
        for line in chunk.content.lines() {
            let value: serde_json::Value = serde_json::from_str(line).expect("whole record");
            seen.push(value["id"].as_u64().unwrap());
        }
        assert_eq!(
            chunk.metadata["path"],
            json!(format!("$[{}..{}]", start, end + 1))
        );
        assert_eq!(chunk.metadata["is_jsonl"], json!(true));
        assert!(chunk.content.len() <= 400 || end == start, "budget respected");
        expected_start = end + 1;
    }

    assert_eq!(seen.len(), 500, "no record lost or duplicated");
    assert_eq!(seen, (0..500).collect::<Vec<u64>>(), "order preserved");
}

#[test]
fn test_jsonl_blank_lines_are_skipped_and_indices_stay_dense() {
    let text = "{\"a\":1}\n\n{\"b\":2}\n   \n{\"c\":3}\n";
    let chunks = JsonChunker::new().with_max_chunk_size(1000).chunk(text).unwrap();

    assert_eq!(chunks.len(), 1);
    assert_eq!(record_span(&chunks[0]), (0, 2));
    assert_eq!(chunks[0].content, "{\"a\":1}\n{\"b\":2}\n{\"c\":3}");
    assert_eq!(chunks[0].content.lines().count(), 3);
}

#[test]
fn test_crlf_jsonl_parses_and_normalizes() {
    let chunks = JsonChunker::new()
        .with_max_chunk_size(1000)
        .chunk("{\"a\":1}\r\n{\"b\":2}\r\n")
        .unwrap();

    assert_eq!(chunks.len(), 1);
    assert!(!chunks[0].content.contains('\r'), "line endings normalized");
    assert_eq!(chunks[0].content, "{\"a\":1}\n{\"b\":2}");
}

#[test]
fn test_oversized_jsonl_record_is_descended_and_keeps_its_record_index() {
    // Record 1 is far larger than the budget; its fields must stay in one family.
    let big = json!({"id": 1, "blob": "x".repeat(600), "meta": {"k": "v"}});
    let text = format!(
        "{}\n{}\n{}",
        json!({"id": 0}),
        serde_json::to_string(&big).unwrap(),
        json!({"id": 2})
    );

    let chunks = JsonChunker::new().with_max_chunk_size(200).chunk(&text).unwrap();

    let record_one: Vec<&Document> = chunks
        .iter()
        .filter(|chunk| record_span(chunk) == (1, 1))
        .collect();
    assert!(
        !record_one.is_empty(),
        "oversized record must be identifiable: {:?}",
        chunks.iter().map(record_span).collect::<Vec<_>>()
    );
    for chunk in &record_one {
        assert!(
            chunk.metadata["path"].as_str().unwrap().starts_with("$[1]"),
            "descended path must name the record: {}",
            chunk.metadata["path"]
        );
    }
    // Records 0 and 2 are still emitted as ordinary batches around it.
    assert!(chunks.iter().any(|chunk| record_span(chunk) == (0, 0)));
    assert!(chunks
        .iter()
        .any(|chunk| record_span(chunk) == (2, 2) && chunk.content.contains("\"id\":2")));
}

#[test]
fn test_top_level_array_elements_also_carry_record_spans() {
    let text = serde_json::to_string(
        &(0..40)
            .map(|i| json!({"id": i}))
            .collect::<Vec<serde_json::Value>>(),
    )
    .unwrap();

    let chunks = JsonChunker::new().with_max_chunk_size(200).chunk(&text).unwrap();

    assert!(chunks.len() > 1);
    let mut expected_start = 0u64;
    for chunk in &chunks {
        let (start, end) = record_span(chunk);
        assert_eq!(start, expected_start);
        assert_eq!(
            chunk.metadata["path"],
            json!(format!("$[{}..{}]", start, end + 1))
        );
        let items: Vec<serde_json::Value> = serde_json::from_str(&chunk.content).unwrap();
        assert_eq!(items.len() as u64, end - start + 1);
        expected_start = end + 1;
    }
    assert_eq!(expected_start, 40);
}

#[test]
fn test_prose_and_pretty_json_never_look_like_jsonl() {
    // Prose: no line is JSON at all -> recursive fallback, not an error.
    let prose = JsonChunker::new()
        .chunk("This is prose.\nIt has several lines.\nNone of them is a record.")
        .unwrap();
    assert!(prose.iter().all(|c| !c.metadata.contains_key("is_jsonl")));

    // Source code containing lone braces or brackets must not be mistaken for JSONL.
    let code = "fn main() {\n    {}\n    let v = [1, 2];\n    println!(\"hi\");\n}";
    let code_chunks = JsonChunker::new().chunk(code).unwrap();
    assert!(
        code_chunks.iter().all(|c| !c.metadata.contains_key("is_jsonl")),
        "a lone `{{}}` line is not a JSONL corpus: {:?}",
        code_chunks.iter().map(|c| &c.metadata).collect::<Vec<_>>()
    );

    // Pretty-printed JSON stays on the structural path.
    let pretty = "{\n  \"a\": 1,\n  \"b\": [1, 2]\n}";
    let pretty_chunks = JsonChunker::new().with_max_chunk_size(500).chunk(pretty).unwrap();
    assert_eq!(pretty_chunks.len(), 1);
    assert!(!pretty_chunks[0].metadata.contains_key("is_jsonl"));
    assert_eq!(pretty_chunks[0].metadata["path"], json!("$"));
}

#[test]
fn test_single_line_json_stays_a_document() {
    let chunks = JsonChunker::new()
        .with_max_chunk_size(500)
        .chunk("{\"a\":1}")
        .unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].metadata["path"], json!("$"));
    assert!(!chunks[0].metadata.contains_key("start_record"));
}

#[test]
fn test_malformed_jsonl_falls_back_to_text_chunking() {
    // A JSONL-looking corpus with one broken record must not silently produce
    // record spans claiming otherwise — it degrades to plain text chunking.
    let text = "{\"a\":1}\n{\"b\":2}\n{oops\n";
    let chunks = JsonChunker::new()
        .with_max_chunk_size(500)
        .chunk(text)
        .unwrap();
    assert!(!chunks.is_empty(), "fallback must still return content");
    assert!(chunks.iter().all(|c| !c.metadata.contains_key("is_jsonl")));
}

#[test]
fn test_dispatch_and_directory_routing_cover_jsonl() {
    let extensions = supported_extensions();
    assert!(extensions.iter().any(|e| e == "jsonl") && extensions.iter().any(|e| e == "ndjson"));

    // The dispatcher loads JSONL as text (UTF-8 strict), like JSON today.
    let documents = load_documents_from_bytes(jsonl(3).as_bytes(), "jsonl", Some("db.jsonl")).unwrap();
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].metadata["file_extension"], json!("jsonl"));
    assert_eq!(documents[0].metadata["format"], json!("json"));

    let dir = unique_tmp_dir("route");
    std::fs::write(dir.join("records.ndjson"), jsonl(40)).unwrap();
    std::fs::write(dir.join("notes.json"), "{\"a\":1}").unwrap();

    let chunks = DirectoryLoader::new()
        .with_chunk_size(200)
        .with_overlap(0)
        .load_and_chunk(&dir)
        .unwrap();

    let ndjson_chunks: Vec<&Document> = chunks
        .iter()
        .filter(|chunk| chunk.metadata["file_extension"] == json!("ndjson"))
        .collect();
    assert!(ndjson_chunks.len() > 1, "ndjson must batch by record");
    assert!(
        ndjson_chunks.iter().all(|c| c.metadata.contains_key("start_record")),
        "routed ndjson chunks keep record spans"
    );
    assert!(
        ndjson_chunks.iter().all(|c| c.metadata["file_name"] == json!("records.ndjson")),
        "routed chunks keep provenance"
    );

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_packing_reconciles_json_spans() {
    let chunks = JsonChunker::new()
        .with_max_chunk_size(200)
        .chunk(&jsonl(40))
        .unwrap();
    assert!(chunks.len() > 1);

    let packed = ChunkPacker::new(10_000).pack(&chunks);
    assert_eq!(packed.len(), 1);
    let metadata = &packed[0].metadata;

    for key in ["path", "start_record", "end_record"] {
        assert!(
            !metadata.contains_key(key),
            "merged chunk must not claim one source's {key}: {metadata:?}"
        );
        assert!(metadata.contains_key(&format!("merged_{key}")));
    }
}
