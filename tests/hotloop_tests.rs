use chunkr::prelude::*;

#[test]
fn test_hotloop_agentic_multi_topic_splits() {
    let document = "First section on distributed database scaling. Horizontal sharding allows databases to scale across many nodes. Furthermore, replication provides high availability. In conclusion, data durability is guaranteed through consensus protocols.";

    let chunker = AgenticChunker::new()
        .with_decision_maker(HeuristicAgenticDecisionMaker::new().with_size_limits(50, 500));

    let chunks = chunker.chunk(document).unwrap();
    assert!(
        chunks.len() >= 2,
        "expected topic splits, got {}",
        chunks.len()
    );
    for chunk in &chunks {
        assert!(chunk.metadata.contains_key("topic_label"));
        assert!(chunk.metadata.contains_key("split_reason"));
        assert!(!chunk.content.is_empty());
    }
}

#[test]
fn test_hotloop_query_aware_hotspots() {
    let document = "Introduction to neural network architectures. Convolutional neural networks are specialized for processing visual imagery. Recurrent neural networks process sequential text. Meanwhile, the weather in Madrid is sunny with a gentle breeze. In conclusion, deep learning powers modern vision systems.";

    let chunker = QueryAwareChunker::new("convolutional neural networks")
        .with_hotspot_sizing(1, 0)
        .with_context_sizing(3, 1)
        .with_relevance_threshold(0.2);

    let chunks = chunker.chunk(document).unwrap();
    assert!(!chunks.is_empty());

    let hotspot_chunks: Vec<_> = chunks
        .iter()
        .filter(|c| c.metadata.get("is_hotspot").unwrap().as_bool().unwrap())
        .collect();
    assert!(!hotspot_chunks.is_empty(), "expected hotspot chunks");
    assert!(hotspot_chunks
        .iter()
        .any(|c| c.content.contains("Convolutional neural networks")));
    assert_eq!(
        hotspot_chunks[0]
            .metadata
            .get("chunk_type")
            .unwrap()
            .as_str()
            .unwrap(),
        "hotspot"
    );
}

#[test]
fn test_hotloop_html_uppercase_tags() {
    let html_content = r#"<!DOCTYPE html>
<html>
<body>
<DIV class="main">
<H1>Uppercase Title Heading</H1>
<P>This is an uppercase paragraph with enough text content to force the chunker to split across multiple chunks.</P>
<P>A second uppercase paragraph adding more content so several tag boundaries are exercised.</P>
</DIV>
</body>
</html>"#;

    let chunker = HtmlChunker::new().with_chunk_size(120).with_overlap(20);

    let chunks = chunker.chunk(html_content).unwrap();
    assert!(chunks.len() >= 2, "expected splits, got {}", chunks.len());

    for chunk in &chunks {
        assert_eq!(
            chunk.metadata.get("format").unwrap().as_str().unwrap(),
            "html"
        );
        assert!(!chunk.content.is_empty());
        // Offset mapping must yield verbatim slices of the original text.
        assert!(
            html_content.contains(chunk.content.as_str()),
            "chunk is not a verbatim slice: {:?}",
            chunk.content
        );
    }

    // Original case is preserved (not lowercased).
    let combined: String = chunks
        .iter()
        .map(|c| c.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        combined.contains("<DIV"),
        "uppercase DIV lost: {}",
        combined
    );
    assert!(combined.contains("<H1"), "uppercase H1 lost: {}", combined);

    // At least one chunk boundary aligns with a tag boundary.
    assert!(
        chunks.iter().any(|c| {
            let t = c.content.trim_start();
            t.starts_with("<DIV") || t.starts_with("<H1") || t.starts_with("<P")
        }),
        "no chunk starts at a tag boundary: {:?}",
        chunks.iter().map(|c| c.content.clone()).collect::<Vec<_>>()
    );
}
