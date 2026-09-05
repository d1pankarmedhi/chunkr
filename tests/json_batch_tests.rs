use chunkr::prelude::*;

#[test]
fn test_json_large_array_round_trips_in_order() {
    // 2000 small objects: exercises the incremental batch tracker.
    let items: Vec<serde_json::Value> = (0..2000)
        .map(|i| serde_json::json!({"id": i, "name": format!("item-{}", i), "active": i % 2 == 0}))
        .collect();
    let text = serde_json::to_string(&items).unwrap();

    let chunker = JsonChunker::new().with_max_chunk_size(1500);
    let chunks = chunker.chunk(&text).unwrap();

    assert!(!chunks.is_empty());

    // Every chunk is valid JSON with the expected metadata.
    let mut round_tripped = Vec::new();
    for (idx, chunk) in chunks.iter().enumerate() {
        let val: serde_json::Value =
            serde_json::from_str(&chunk.content).expect("chunk must be valid JSON");
        let arr = val.as_array().expect("chunk must be a JSON array");
        round_tripped.extend(arr.clone());
        assert!(chunk.metadata.contains_key("path"), "missing path");
        assert_eq!(
            chunk.metadata.get("is_json").and_then(|v| v.as_bool()),
            Some(true),
            "missing is_json flag"
        );
        assert_eq!(
            chunk.metadata.get("chunk_index").and_then(|v| v.as_u64()),
            Some(idx as u64),
            "chunk_index must sequence"
        );
    }

    // Concatenated items equal the original 2000, in order.
    assert_eq!(round_tripped.len(), 2000);
    for (i, item) in round_tripped.iter().enumerate() {
        assert_eq!(item.get("id").and_then(|v| v.as_u64()), Some(i as u64));
    }
}

#[test]
fn test_json_large_array_compact_mode_round_trips() {
    let items: Vec<serde_json::Value> = (0..500)
        .map(|i| serde_json::json!({"id": i, "v": "x".repeat(20)}))
        .collect();
    let text = serde_json::to_string(&items).unwrap();

    let chunker = JsonChunker::new()
        .with_max_chunk_size(1500)
        .with_pretty(false);
    let chunks = chunker.chunk(&text).unwrap();

    assert!(!chunks.is_empty());
    let mut round_tripped = Vec::new();
    for chunk in &chunks {
        let val: serde_json::Value = serde_json::from_str(&chunk.content).unwrap();
        round_tripped.extend(val.as_array().unwrap().clone());
    }
    assert_eq!(round_tripped.len(), 500);
    for (i, item) in round_tripped.iter().enumerate() {
        assert_eq!(item.get("id").and_then(|v| v.as_u64()), Some(i as u64));
    }
}
