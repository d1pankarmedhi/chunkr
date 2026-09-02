use chunkr::prelude::*;
use chunkr::loader::pdf::PDFLoader;

#[test]
fn test_character_chunker_legacy() {
    let char_chunker = CharacterChunker::new();
    let loader = PDFLoader::new();
    let input_text = loader
        .load_from_file("tests/test_files/sample_doc.pdf")
        .unwrap();
    let chunk_size = 1000;
    let overlap = 50;
    let chunks = char_chunker
        .chunk_text(&input_text, chunk_size, overlap)
        .unwrap();
    assert_eq!(6, chunks.len());
}

#[test]
fn test_word_chunker_legacy() {
    let word_chunker = WordChunker::new();
    let loader = PDFLoader::new();
    let input_text = loader
        .load_from_file("tests/test_files/sample_doc.pdf")
        .unwrap();

    let chunk_size = 500;
    let overlap = 50;
    let chunks = word_chunker
        .chunk_text(&input_text, chunk_size, overlap)
        .unwrap();
    assert!(!chunks.is_empty());
}

#[test]
fn test_character_chunker_modern() {
    let chunker = CharacterChunker::new()
        .with_chunk_size(100)
        .with_overlap(20);
    let text = "Hello world! This is a test for unicode characters: 🦀 Rust 🚀 is fast and safe. ".repeat(5);
    let chunks = chunker.chunk(&text).unwrap();

    assert!(!chunks.is_empty());
    for (i, chunk) in chunks.iter().enumerate() {
        assert!(!chunk.content.is_empty());
        assert!(chunk.metadata.contains_key("start_char"));
        assert!(chunk.metadata.contains_key("end_char"));
        assert_eq!(chunk.metadata.get("chunk_index").unwrap().as_u64().unwrap(), i as u64);
    }
}

#[test]
fn test_word_chunker_modern() {
    let chunker = WordChunker::new()
        .with_chunk_size(10)
        .with_overlap(2);
    let text = "The quick brown fox jumps over the lazy dog. Chunkr makes document chunking super fast and easy for all LLMs.";
    let chunks = chunker.chunk(text).unwrap();

    assert!(!chunks.is_empty());
    for chunk in &chunks {
        let count = chunk.content.split_whitespace().count();
        assert!(count <= 10);
    }
}

#[test]
fn test_token_chunker() {
    let chunker = TokenChunker::with_encoding(50, 10, TokenEncoding::Cl100kBase).unwrap();
    let text = "Large language models require accurate token counts for prompt window management. ".repeat(10);
    let chunks = chunker.chunk(&text).unwrap();

    assert!(!chunks.is_empty());
    for chunk in &chunks {
        let tokens = chunker.count_tokens(&chunk.content);
        assert!(tokens <= 55);
        assert_eq!(chunk.metadata.get("encoding").unwrap().as_str().unwrap(), "cl100k_base");
    }
}

#[test]
fn test_recursive_chunker() {
    let text = "Paragraph 1: Introduction to Rust.\nRust is a systems programming language focused on safety and speed.\n\nParagraph 2: Features of Chunkr.\nChunkr provides blazingly fast chunking strategies for RAG pipelines.\n\nParagraph 3: Parallel Processing.\nPowered by Rayon for multi-threaded batch operations across CPU cores.";

    let chunker = RecursiveChunker::new()
        .with_chunk_size(100)
        .with_overlap(20);

    let chunks = chunker.chunk(text).unwrap();
    assert!(!chunks.is_empty());
    for chunk in &chunks {
        assert!(chunk.content.chars().count() <= 120);
    }
}

#[test]
fn test_hierarchical_chunker() {
    let text = "Section 1: AI & LLMs.\nLarge language models require efficient context management.\nChunking is the foundation of retrieval augmented generation.\n\nSection 2: High Performance Chunking.\nRust enables zero-copy string slicing and high-throughput document processing.\n\nSection 3: Conclusion.\nHierarchical chunking links child chunks to parent context.";

    let chunker = HierarchicalChunker::with_sizes(200, 30, 80, 15).unwrap();
    let pairs = chunker.chunk_hierarchical(text).unwrap();

    assert!(!pairs.is_empty());
    for pair in pairs {
        assert_eq!(pair.parent.metadata.get("chunk_type").unwrap().as_str().unwrap(), "parent");
        assert!(!pair.children.is_empty());
        for child in pair.children {
            assert_eq!(child.metadata.get("chunk_type").unwrap().as_str().unwrap(), "child");
            assert_eq!(
                child.metadata.get("parent_id").unwrap().as_str().unwrap(),
                pair.parent.metadata.get("parent_id").unwrap().as_str().unwrap()
            );
        }
    }
}

#[test]
fn test_hierarchical_tree_chunker() {
    let text = "Root Topic.\nOverview of architecture.\n\nSection A: Storage details.\nPostgreSQL and disk structures.\n\nSection B: Network details.\nTCP socket connections.";

    let chunker = HierarchicalChunker::with_sizes(150, 20, 50, 10).unwrap();
    let tree = chunker.chunk_tree(text).unwrap();

    assert_eq!(tree.id, "root");
    assert_eq!(tree.depth, 0);
    assert!(!tree.children.is_empty());
    assert_eq!(tree.children[0].depth, 1);
    assert!(!tree.children[0].children.is_empty());
    assert_eq!(tree.children[0].children[0].depth, 2);

    let flattened = tree.flatten();
    assert!(flattened.len() >= 4);
}

#[test]
fn test_sentence_chunker() {
    let text = "Dr. Smith arrived at 3.14 p.m. at Google Inc. to give a keynote! He was welcomed warmly by the team. The presentation lasted for two hours. Afterwards, there was a lively Q&A session. Everyone agreed it was a great success.";

    let sentences = SentenceChunker::split_sentences(text);
    assert_eq!(sentences.len(), 5);
    assert!(sentences[0].contains("Dr. Smith arrived at 3.14 p.m. at Google Inc. to give a keynote!"));

    let chunker = SentenceChunker::new()
        .with_sentences_per_chunk(2)
        .with_sentence_overlap(1);

    let chunks = chunker.chunk(text).unwrap();
    assert_eq!(chunks.len(), 4);
    assert_eq!(chunks[0].metadata.get("sentence_count").unwrap().as_u64().unwrap(), 2);
}

#[test]
fn test_paragraph_chunker() {
    let text = "First paragraph content here.\nWith multiple lines of text.\n\nSecond paragraph begins here.\nIt discusses another topic.\n\nThird paragraph concluding the note.";

    let chunker = ParagraphChunker::new()
        .with_paragraphs_per_chunk(2)
        .with_paragraph_overlap(1);

    let chunks = chunker.chunk(text).unwrap();
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].metadata.get("paragraph_count").unwrap().as_u64().unwrap(), 2);
}

#[test]
fn test_semantic_chunker() {
    let text = "Quantum mechanics is a fundamental theory in physics that provides a description of the physical properties of nature at the scale of atoms and subatomic particles. Wave-particle duality is a central concept in quantum physics. Meanwhile, chocolate chip cookies require baking flour, sugar, butter, and chocolate chips. Mix the dry ingredients together with melted butter and bake at 350 degrees.";

    let chunker = SemanticChunker::new()
        .with_threshold(BreakpointThreshold::Percentile(50.0))
        .with_size_bounds(50, 1000);

    let chunks = chunker.chunk(text).unwrap();
    assert!(chunks.len() >= 2);
    assert!(chunks[0].content.contains("Quantum") || chunks[0].content.contains("physics"));
}

#[test]
fn test_proposition_chunker() {
    let sentence = "The Eiffel Tower, which was constructed in 1889, is located in Paris and welcomes millions of tourists every year.";
    let props = SyntacticPropositionExtractor::decompose_sentence(sentence);
    assert!(props.len() >= 2);

    let chunker = PropositionChunker::new();
    let chunks = chunker.chunk(sentence).unwrap();
    assert!(chunks.len() >= 2);
    for chunk in &chunks {
        assert!(chunk.metadata.contains_key("proposition_count"));
    }
}

#[test]
fn test_contextual_chunker() {
    let document = "# ACME Corp Technical Architecture\n\nSection 1: Database Layer.\nACME uses distributed PostgreSQL with replication.\n\nSection 2: Caching Layer.\nACME uses Redis cluster for sub-millisecond retrieval.";

    let chunker = ContextualChunker::new()
        .with_base_chunker(ParagraphChunker::new().with_paragraphs_per_chunk(1).with_paragraph_overlap(0))
        .with_format(ContextFormat::Prefix);

    let chunks = chunker.chunk(document).unwrap();
    assert!(!chunks.is_empty());
    for chunk in &chunks {
        assert!(chunk.content.starts_with("[Context:"));
        assert!(chunk.content.contains("ACME Corp Technical Architecture"));
        assert!(chunk.metadata.contains_key("context"));
    }
}

#[test]
fn test_query_aware_chunker() {
    let document = "Introduction to neural network architectures. Convolutional neural networks are specialized for processing visual imagery. Recurrent neural networks process sequential text. Meanwhile, the weather in Madrid is sunny with a gentle breeze. In conclusion, deep learning powers modern vision systems.";

    let chunker = QueryAwareChunker::new("convolutional neural networks")
        .with_hotspot_sizing(1, 0)
        .with_context_sizing(3, 1)
        .with_relevance_threshold(0.2);

    let chunks = chunker.chunk(document).unwrap();
    assert!(!chunks.is_empty());

    let hotspot_chunks: Vec<_> = chunks.iter().filter(|c| c.metadata.get("is_hotspot").unwrap().as_bool().unwrap()).collect();
    assert!(!hotspot_chunks.is_empty());
    assert!(hotspot_chunks.iter().any(|c| c.content.contains("Convolutional neural networks")));
    assert_eq!(hotspot_chunks[0].metadata.get("chunk_type").unwrap().as_str().unwrap(), "hotspot");
}

#[test]
fn test_agentic_chunker() {
    let document = "First section on distributed database scaling. Horizontal sharding allows databases to scale across many nodes. Furthermore, replication provides high availability. In conclusion, data durability is guaranteed through consensus protocols.";

    let chunker = AgenticChunker::new()
        .with_decision_maker(HeuristicAgenticDecisionMaker::new().with_size_limits(50, 500));

    let chunks = chunker.chunk(document).unwrap();
    assert!(chunks.len() >= 2);
    for chunk in &chunks {
        assert!(chunk.metadata.contains_key("topic_label"));
        assert!(chunk.metadata.contains_key("split_reason"));
    }
}

#[test]
fn test_markdown_chunker() {
    let markdown_text = r#"# Main Guide

This is the main introduction.

## Getting Started

Follow these steps to install the tool.

```rust
fn main() {
    println!("Hello Chunkr!");
}
```

## Advanced Features

### Recursive Chunking

Details on recursive text splitting.

### Token Chunking

Details on BPE token level splitting.
"#;

    let chunker = MarkdownChunker::new()
        .with_chunk_size(250)
        .with_overlap(30);

    let chunks = chunker.chunk(markdown_text).unwrap();
    assert!(!chunks.is_empty());

    let code_chunk = chunks.iter().find(|c| c.content.contains("fn main()")).unwrap();
    assert!(code_chunk.metadata.get("has_code_block").unwrap().as_bool().unwrap());
    assert!(code_chunk.metadata.get("header_path").unwrap().as_str().unwrap().contains("Getting Started"));
}

#[test]
fn test_code_chunker_rust() {
    let code = r#"
pub struct User {
    pub name: String,
    pub age: u32,
}

impl User {
    pub fn new(name: String, age: u32) -> Self {
        Self { name, age }
    }

    pub fn is_adult(&self) -> bool {
        self.age >= 18
    }
}

pub fn greet_user(user: &User) -> String {
    format!("Hello, {}!", user.name)
}
"#;

    let chunker = CodeChunker::new(CodeLanguage::Rust)
        .with_chunk_size(150)
        .with_overlap(20);

    let chunks = chunker.chunk(code).unwrap();
    assert!(!chunks.is_empty());
    for chunk in &chunks {
        assert_eq!(chunk.metadata.get("language").unwrap().as_str().unwrap(), "rust");
    }
}

#[test]
fn test_code_chunker_python() {
    let py_code = r#"
class DocumentProcessor:
    def __init__(self, model_name: str):
        self.model_name = model_name

    def process_text(self, text: str) -> list[str]:
        return text.split("\n")

def helper_func(x: int) -> int:
    return x * 2
"#;

    let chunker = CodeChunker::new(CodeLanguage::Python)
        .with_chunk_size(150)
        .with_overlap(20);

    let chunks = chunker.chunk(py_code).unwrap();
    assert!(!chunks.is_empty());
    for chunk in &chunks {
        assert_eq!(chunk.metadata.get("language").unwrap().as_str().unwrap(), "python");
    }
}

#[test]
fn test_json_chunker() {
    let json_str = r#"{
        "store": {
            "book": [
                {"category": "reference", "author": "Nigel Rees", "title": "Sayings of the Century", "price": 8.95},
                {"category": "fiction", "author": "Evelyn Waugh", "title": "Sword of Honour", "price": 12.99},
                {"category": "fiction", "author": "Herman Melville", "title": "Moby Dick", "price": 8.99}
            ],
            "bicycle": {
                "color": "red",
                "price": 19.95
            }
        }
    }"#;

    let chunker = JsonChunker::new().with_max_chunk_size(180);
    let chunks = chunker.chunk(json_str).unwrap();

    assert!(!chunks.is_empty());
    for chunk in &chunks {
        assert!(chunk.metadata.contains_key("path"));
        assert!(chunk.metadata.get("is_json").unwrap().as_bool().unwrap());
    }
}

#[test]
fn test_html_chunker() {
    let html_content = r#"
<!DOCTYPE html>
<html>
<body>
<article>
    <h1>Article Title</h1>
    <p>This is the first paragraph with important text.</p>
    <section>
        <h2>Section 1</h2>
        <p>Details about section 1.</p>
    </section>
</article>
</body>
</html>
"#;

    let chunker = HtmlChunker::new()
        .with_chunk_size(120)
        .with_overlap(20);

    let chunks = chunker.chunk(html_content).unwrap();
    assert!(!chunks.is_empty());
    for chunk in &chunks {
        assert_eq!(chunk.metadata.get("format").unwrap().as_str().unwrap(), "html");
    }
}

#[test]
fn test_parallel_batch_chunking() {
    let chunker = RecursiveChunker::new()
        .with_chunk_size(100)
        .with_overlap(20);

    let docs: Vec<Document> = (0..50)
        .map(|i| {
            Document::from_text(format!(
                "Document #{} paragraph one.\nThis is sample content for document number {}.\n\nParagraph two with more text.",
                i, i
            ))
            .with_metadata("source_id", serde_json::Value::from(i))
        })
        .collect();

    let chunks = chunker.par_chunk_documents(&docs).unwrap();
    assert!(chunks.len() >= 50);

    // Verify metadata propagation
    assert!(chunks[0].metadata.contains_key("doc_index"));
    assert!(chunks[0].metadata.contains_key("source_id"));
}

#[test]
fn test_pdf_loader_from_file() {
    let loader = PDFLoader::new();
    let text = loader.load_from_file("tests/test_files/sample_doc.pdf").unwrap();
    assert!(!text.is_empty());
    assert!(text.contains("Sample") || text.len() > 100);
}

#[test]
fn test_pdf_loader_from_bytes() {
    let bytes = std::fs::read("tests/test_files/sample_doc.pdf").unwrap();
    let loader = PDFLoader::new();
    let text = loader.load_from_bytes(&bytes).unwrap();
    assert!(!text.is_empty());
}

#[test]
fn test_pdf_loader_document() {
    let loader = PDFLoader::new();
    let doc = loader.load_document("tests/test_files/sample_doc.pdf").unwrap();
    assert!(!doc.content.is_empty());
    assert!(doc.metadata.contains_key("total_pages"));
    assert!(doc.metadata.contains_key("source"));
    assert_eq!(doc.metadata.get("file_name").unwrap().as_str().unwrap(), "sample_doc.pdf");
}

#[test]
fn test_pdf_loader_pages() {
    let loader = PDFLoader::new();
    let pages = loader.load_pages_from_file("tests/test_files/sample_doc.pdf").unwrap();
    assert!(!pages.is_empty());
    for (i, page) in pages.iter().enumerate() {
        let expected_page = (i + 1) as u64;
        assert_eq!(
            page.metadata.get("page_number").unwrap().as_u64().unwrap(),
            expected_page
        );
        assert_eq!(
            page.metadata.get("total_pages").unwrap().as_u64().unwrap(),
            pages.len() as u64
        );
        assert!(page.metadata.contains_key("source"));
    }
}

#[test]
fn test_pdf_loader_pages_from_bytes() {
    let bytes = std::fs::read("tests/test_files/sample_doc.pdf").unwrap();
    let loader = PDFLoader::new();
    let pages = loader.load_pages_from_bytes(&bytes).unwrap();
    assert!(!pages.is_empty());
    assert_eq!(
        pages[0].metadata.get("page_number").unwrap().as_u64().unwrap(),
        1
    );
}

#[test]
fn test_pdf_loader_error_handling() {
    let loader = PDFLoader::new();
    // Non-existent file
    let file_err = loader.load_from_file("tests/test_files/non_existent_file.pdf");
    assert!(file_err.is_err());

    // Invalid bytes
    let invalid_bytes = b"not a valid pdf header or content";
    let bytes_err = loader.load_from_bytes(invalid_bytes);
    assert!(bytes_err.is_err());

    let pages_err = loader.load_pages_from_bytes(invalid_bytes);
    assert!(pages_err.is_err());
}

#[test]
fn test_pdf_chunking_pipeline() {
    let loader = PDFLoader::new();
    let pages = loader.load_pages_from_file("tests/test_files/sample_doc.pdf").unwrap();

    let chunker = RecursiveChunker::new()
        .with_chunk_size(500)
        .with_overlap(50);

    let chunks = chunker.chunk_documents(&pages).unwrap();
    assert!(!chunks.is_empty());

    // Verify inherited metadata from PDF page documents
    assert!(chunks[0].metadata.contains_key("page_number"));
    assert!(chunks[0].metadata.contains_key("doc_index"));
    assert!(chunks[0].metadata.contains_key("chunk_index"));
}

#[test]
fn test_sentence_chunker_utf8_multibyte_safety() {
    let text = "C'est un été très chaud avec des journées ensoleillées. 🦀 快適なプログラミング言語Rustが大好きです！ Another short sentence.";
    let chunker = SentenceChunker::new()
        .with_sentences_per_chunk(1)
        .with_sentence_overlap(0)
        .with_max_characters(25);
    let chunks = chunker.chunk(text).unwrap();
    assert!(!chunks.is_empty());
    for chunk in chunks {
        assert!(chunk.content.chars().count() <= 25);
    }
}

#[test]
fn test_contextual_chunker_clone_preserves_base_chunker() {
    let custom_chunker = SentenceChunker::new()
        .with_sentences_per_chunk(1)
        .with_sentence_overlap(0);
    let contextual = ContextualChunker::new()
        .with_base_chunker(custom_chunker)
        .with_format(ContextFormat::MetadataOnly);

    let cloned = contextual.clone();
    let text = "Sentence one. Sentence two. Sentence three.";
    let chunks = cloned.chunk(text).unwrap();
    assert_eq!(chunks.len(), 3);
}

#[test]
fn test_proposition_extractor_relative_clause() {
    let sentence = "The Eiffel Tower, which was constructed in 1889, is located in Paris and welcomes millions of tourists every year.";
    let props = SyntacticPropositionExtractor::decompose_sentence(sentence);
    assert_eq!(props.len(), 2);
    assert_eq!(props[0], "The Eiffel Tower is located in Paris and welcomes millions of tourists every year.");
    assert_eq!(props[1], "The Eiffel Tower was constructed in 1889.");
}
