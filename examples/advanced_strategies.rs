use chunkr::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("===========================================================");
    println!("           CHUNKR ALL ADVANCED STRATEGIES DEMO             ");
    println!("===========================================================\n");

    let text = "Paragraph 1: Introduction.\nRust is blazing fast.\n\nParagraph 2: Features.\nChunkr provides multiple chunking strategies for LLM & RAG pipelines.\n\nParagraph 3: Speed.\nZero-copy string slicing with zero heap allocations.";

    // 1. Recursive Chunking
    println!("--- 1. Recursive Chunker ---");
    let recursive_chunker = RecursiveChunker::new().with_chunk_size(80).with_overlap(15);
    let chunks = recursive_chunker.chunk(text)?;
    println!("Generated {} recursive chunks:", chunks.len());
    for (i, c) in chunks.iter().enumerate() {
        println!("  [{}] ({} chars) {:?}", i + 1, c.content.len(), c.content);
    }

    // 2. Token-Based Chunking with OpenAI BPE
    println!("\n--- 2. Token Chunker (cl100k_base) ---");
    let token_chunker = TokenChunker::with_encoding(20, 5, TokenEncoding::Cl100kBase)?;
    let prompt = "Large Language Models process prompts as sequences of BPE tokens. Accurate token chunking avoids truncation.";
    let token_chunks = token_chunker.chunk(prompt)?;
    println!("Generated {} token chunks:", token_chunks.len());
    for (i, c) in token_chunks.iter().enumerate() {
        let count = c.metadata.get("token_count").unwrap();
        println!("  [{}] ({} tokens) {:?}", i + 1, count, c.content);
    }

    // 3. Sentence Boundary Chunker
    println!("\n--- 3. Sentence Boundary Chunker ---");
    let sentence_doc = "Dr. Smith arrived at 3.14 p.m. at Google Inc. to give a keynote! He was welcomed warmly by the AI team. The presentation lasted for two hours. Afterwards, there was a lively Q&A session. Everyone agreed it was a great success.";
    let sentence_chunker = SentenceChunker::new()
        .with_sentences_per_chunk(2)
        .with_sentence_overlap(1);
    let sentence_chunks = sentence_chunker.chunk(sentence_doc)?;
    println!(
        "Generated {} sentence chunks (abbreviations protected):",
        sentence_chunks.len()
    );
    for (i, c) in sentence_chunks.iter().enumerate() {
        println!("  [{}] {:?}", i + 1, c.content);
    }

    // 4. Paragraph Chunker
    println!("\n--- 4. Paragraph Chunker ---");
    let para_chunker = ParagraphChunker::new()
        .with_paragraphs_per_chunk(2)
        .with_paragraph_overlap(1);
    let para_chunks = para_chunker.chunk(text)?;
    println!("Generated {} paragraph chunks:", para_chunks.len());
    for (i, c) in para_chunks.iter().enumerate() {
        println!("  [{}] {:?}", i + 1, c.content);
    }

    // 5. Semantic Chunker (Distance Breakpoint Detection)
    println!("\n--- 5. Semantic Chunker ---");
    let semantic_doc = "Quantum mechanics governs the behavior of atoms and subatomic particles. Wave-particle duality is a fundamental principle of modern physics. In contrast, making delicious chocolate chip cookies requires flour, butter, sugar, and chocolate chips. Bake the cookie dough in an oven at 350 degrees.";
    let semantic_chunker = SemanticChunker::new()
        .with_threshold(BreakpointThreshold::Percentile(50.0))
        .with_size_bounds(50, 500);
    let semantic_chunks = semantic_chunker.chunk(semantic_doc)?;
    println!("Generated {} semantic clusters:", semantic_chunks.len());
    for (i, c) in semantic_chunks.iter().enumerate() {
        println!("  [Cluster {}] {:?}", i + 1, c.content);
    }

    // 6. Proposition Chunker (Atomic Factual Claims)
    println!("\n--- 6. Proposition Chunker (Atomic Claims) ---");
    let complex_sentence = "The Eiffel Tower, which was constructed in 1889, is located in Paris and welcomes millions of tourists every year.";
    let prop_chunker = PropositionChunker::new();
    let prop_chunks = prop_chunker.chunk(complex_sentence)?;
    println!("Generated {} atomic propositions:", prop_chunks.len());
    for (i, c) in prop_chunks.iter().enumerate() {
        println!("  [Proposition {}] {:?}", i + 1, c.content);
    }

    // 7. Contextual Chunker (Anthropic-Style Contextual Retrieval)
    println!("\n--- 7. Contextual Chunker ---");
    let doc_with_context = "# Cloud Database Architecture\n\nPostgreSQL handles transactional ACID data.\n\nRedis cache provides sub-millisecond query responses.";
    let contextual_chunker = ContextualChunker::new()
        .with_base_chunker(
            ParagraphChunker::new()
                .with_paragraphs_per_chunk(1)
                .with_paragraph_overlap(0),
        )
        .with_format(ContextFormat::Prefix);
    let context_chunks = contextual_chunker.chunk(doc_with_context)?;
    println!(
        "Generated {} context-enriched chunks:",
        context_chunks.len()
    );
    for (i, c) in context_chunks.iter().enumerate() {
        println!("  [{}]\n{}", i + 1, c.content);
    }

    // 8. Query-Aware / Adaptive Chunker
    println!("\n--- 8. Query-Aware / Adaptive Chunker ---");
    let doc_for_query = "Introduction to neural network architectures. Convolutional neural networks specialize in visual imagery processing. Recurrent networks handle sequences. The weather today in Madrid is sunny. In summary, deep neural networks power vision.";
    let query_chunker = QueryAwareChunker::new("convolutional neural networks")
        .with_hotspot_sizing(1, 0)
        .with_context_sizing(2, 0)
        .with_relevance_threshold(0.2);
    let query_chunks = query_chunker.chunk(doc_for_query)?;
    println!("Generated {} adaptive query chunks:", query_chunks.len());
    for (i, c) in query_chunks.iter().enumerate() {
        let is_hotspot = c.metadata.get("is_hotspot").unwrap().as_bool().unwrap();
        let tag = if is_hotspot { "HOTSPOT" } else { "CONTEXT" };
        println!(
            "  [{}] [{}] (Score: {}) {:?}",
            i + 1,
            tag,
            c.metadata.get("relevance_score").unwrap(),
            c.content
        );
    }

    // 9. Agentic / Model-Based Chunker
    println!("\n--- 9. Agentic Chunker ---");
    let agentic_doc = "First section discusses storage clustering. Distributed nodes replicate shards across failure domains. Furthermore, leader election ensures consensus. In conclusion, distributed durability guarantees zero data loss.";
    let agentic_chunker = AgenticChunker::new()
        .with_decision_maker(HeuristicAgenticDecisionMaker::new().with_size_limits(50, 400));
    let agentic_chunks = agentic_chunker.chunk(agentic_doc)?;
    println!("Generated {} agentic topic chunks:", agentic_chunks.len());
    for (i, c) in agentic_chunks.iter().enumerate() {
        println!(
            "  [{}] Topic: {} | Reason: {} | Content: {:?}",
            i + 1,
            c.metadata.get("topic_label").unwrap(),
            c.metadata.get("split_reason").unwrap(),
            c.content
        );
    }

    // 10. Hierarchical Parent-Child Tree Chunker
    println!("\n--- 10. Hierarchical Parent-Child Tree Chunker ---");
    let hier_chunker = HierarchicalChunker::with_sizes(150, 20, 50, 10)?;
    let tree = hier_chunker.chunk_tree(text)?;
    println!("Generated Hierarchical Tree:");
    println!("  Root: {} (children: {})", tree.id, tree.children.len());
    for parent in &tree.children {
        println!(
            "    ├─ Parent [{}]: {:?} (children: {})",
            parent.id,
            parent.document.content,
            parent.children.len()
        );
        for child in &parent.children {
            println!(
                "    │   └─ Child [{}]: {:?}",
                child.id, child.document.content
            );
        }
    }

    // 11. Markdown Structure Chunker
    println!("\n--- 11. Markdown Structure Chunker ---");
    let markdown_doc = r#"# User Guide
Welcome to the guide.

## Installation
Run `cargo add chunkr` to install.

```rust
fn main() {
    println!("Chunkr is fast!");
}
```

## Advanced API
### Hierarchical Chunking
Details on parent-child chunking.
"#;
    let md_chunker = MarkdownChunker::new().with_chunk_size(120).with_overlap(20);
    let md_chunks = md_chunker.chunk(markdown_doc)?;
    println!("Generated {} markdown chunks:", md_chunks.len());
    for (i, c) in md_chunks.iter().enumerate() {
        let path = c.metadata.get("header_path").unwrap();
        println!("  [{}] Path: {} | Content: {:?}", i + 1, path, c.content);
    }

    // 12. Code Structure Chunker
    println!("\n--- 12. Code Structure Chunker ---");
    let code_sample = r#"
pub struct ChunkerEngine {
    pub name: String,
}

impl ChunkerEngine {
    pub fn new() -> Self {
        Self { name: "Chunkr".to_string() }
    }

    pub fn run(&self) {
        println!("Running {}", self.name);
    }
}
"#;
    let code_chunker = CodeChunker::new(CodeLanguage::Rust)
        .with_chunk_size(80)
        .with_overlap(10);
    let code_chunks = code_chunker.chunk(code_sample)?;
    println!("Generated {} code chunks:", code_chunks.len());
    for (i, c) in code_chunks.iter().enumerate() {
        println!(
            "  [{}] Language: {} | {:?}",
            i + 1,
            c.metadata.get("language").unwrap(),
            c.content
        );
    }

    println!("\n=== All 12 Chunking Strategies Executed Successfully! ===");
    Ok(())
}
