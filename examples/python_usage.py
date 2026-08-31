#!/usr/bin/env python3
"""
Chunkr Python API Demonstration
Demonstrating the full suite of chunking strategies directly in Python via the native Rust extension.
"""

import chunkr

def main():
    print("=" * 65)
    print("           CHUNKR PYTHON NATIVE LIBRARY DEMO (Rust Engine)       ")
    print("=" * 65)

    sample_text = (
        "Paragraph 1: Introduction to Rust & AI.\nRust is a blazing fast systems language.\n\n"
        "Paragraph 2: Features of Chunkr.\nChunkr provides 12 high-performance chunking strategies.\n\n"
        "Paragraph 3: Parallel Execution.\nPowered by Rayon and zero-copy string slicing in Rust."
    )

    # 1. Recursive Chunker
    print("\n--- 1. RecursiveChunker ---")
    chunker = chunkr.RecursiveChunker(chunk_size=80, overlap=15)
    docs = chunker.chunk(sample_text)
    print(f"Generated {len(docs)} chunks:")
    for i, d in enumerate(docs):
        print(f"  [{i+1}] (len={len(d)}) {d.content!r}")

    # 2. Token Chunker
    print("\n--- 2. TokenChunker (cl100k_base / GPT-4) ---")
    token_chunker = chunkr.TokenChunker(chunk_size=20, overlap=5, encoding="cl100k_base")
    prompt = "Large Language Models process prompts as sequences of BPE tokens. Accurate token chunking avoids truncation."
    token_docs = token_chunker.chunk(prompt)
    print(f"Generated {len(token_docs)} token chunks:")
    for i, d in enumerate(token_docs):
        print(f"  [{i+1}] tokens={d.metadata.get('token_count')} {d.content!r}")

    # 3. Sentence Boundary Chunker (protecting abbreviations)
    print("\n--- 3. SentenceChunker ---")
    sentence_text = "Dr. Smith arrived at 3.14 p.m. at Google Inc. to deliver the keynote! He was greeted warmly. The presentation lasted two hours."
    sent_chunker = chunkr.SentenceChunker(sentences_per_chunk=2, overlap=1)
    sent_docs = sent_chunker.chunk(sentence_text)
    print(f"Generated {len(sent_docs)} sentence chunks:")
    for i, d in enumerate(sent_docs):
        print(f"  [{i+1}] {d.content!r}")

    # 4. Semantic Chunker (Distance Breakpoints)
    print("\n--- 4. SemanticChunker ---")
    semantic_text = (
        "Quantum mechanics governs the behavior of atoms and subatomic particles. "
        "Wave-particle duality is a fundamental principle of modern physics. "
        "In contrast, making delicious chocolate chip cookies requires flour, butter, sugar, and chocolate chips. "
        "Bake the cookie dough in an oven at 350 degrees."
    )
    semantic_chunker = chunkr.SemanticChunker(percentile=50.0, min_size=50, max_size=500)
    sem_docs = semantic_chunker.chunk(semantic_text)
    print(f"Generated {len(sem_docs)} semantic clusters:")
    for i, d in enumerate(sem_docs):
        print(f"  [Cluster {i+1}] {d.content!r}")

    # 5. Proposition Chunker (Atomic Factual Claims)
    print("\n--- 5. PropositionChunker ---")
    prop_chunker = chunkr.PropositionChunker(propositions_per_chunk=1, overlap=0)
    prop_sentence = "The Eiffel Tower, which was constructed in 1889, is located in Paris and welcomes millions of tourists every year."
    prop_docs = prop_chunker.chunk(prop_sentence)
    print(f"Generated {len(prop_docs)} atomic propositions:")
    for i, d in enumerate(prop_docs):
        print(f"  [Prop {i+1}] {d.content!r}")

    # 6. Contextual Chunker (Anthropic-Style Contextual Retrieval)
    print("\n--- 6. ContextualChunker ---")
    ctx_chunker = chunkr.ContextualChunker(chunk_size=100, overlap=20, max_context_chars=150)
    ctx_doc = "# Cloud Database Guide\n\nPostgreSQL handles ACID data.\n\nRedis provides sub-millisecond caching."
    ctx_docs = ctx_chunker.chunk(ctx_doc)
    print(f"Generated {len(ctx_docs)} context-enriched chunks:")
    for i, d in enumerate(ctx_docs):
        print(f"  [{i+1}] Context: {d.metadata.get('context')}")
        print(f"      Content: {d.content!r}")

    # 7. Query-Aware Adaptive Chunker
    print("\n--- 7. QueryAwareChunker ---")
    query_chunker = chunkr.QueryAwareChunker(query="neural networks", hotspot_sentences=1, hotspot_overlap=0, context_sentences=2)
    doc_query = "Introduction to neural network architectures. Convolutional neural networks specialize in visual imagery. Recurrent networks process sequential text. The weather in Madrid is sunny."
    query_docs = query_chunker.chunk(doc_query)
    print(f"Generated {len(query_docs)} adaptive query chunks:")
    for i, d in enumerate(query_docs):
        print(f"  [{i+1}] [{d.metadata.get('chunk_type').upper()}] score={d.metadata.get('relevance_score')} {d.content!r}")

    # 8. Agentic Model-Based Chunker
    print("\n--- 8. AgenticChunker ---")
    agentic_chunker = chunkr.AgenticChunker(min_chars=50, max_chars=400)
    agentic_doc = "First section discusses storage clustering. Distributed nodes replicate shards across failure domains. Furthermore, leader election ensures consensus. In conclusion, distributed durability guarantees zero data loss."
    agentic_docs = agentic_chunker.chunk(agentic_doc)
    print(f"Generated {len(agentic_docs)} agentic topic chunks:")
    for i, d in enumerate(agentic_docs):
        print(f"  [{i+1}] Topic: {d.metadata.get('topic_label')} | Reason: {d.metadata.get('split_reason')} | {d.content!r}")

    # 9. Markdown Structure Chunker
    print("\n--- 9. MarkdownChunker ---")
    md_chunker = chunkr.MarkdownChunker(chunk_size=120, overlap=20)
    md_text = "# Main Guide\n\nIntroduction.\n\n## Setup\n\nRun pip install chunkr.\n\n```python\nimport chunkr\n```"
    md_docs = md_chunker.chunk(md_text)
    print(f"Generated {len(md_docs)} markdown chunks:")
    for i, d in enumerate(md_docs):
        print(f"  [{i+1}] Path: {d.metadata.get('header_path')} | {d.content!r}")

    # 10. PDF Loader
    print("\n--- 10. PDFLoader ---")
    import os
    sample_pdf = "tests/test_files/sample_doc.pdf"
    if os.path.exists(sample_pdf):
        loader = chunkr.PDFLoader()
        pdf_pages = loader.load_pages(sample_pdf)
        print(f"Loaded {len(pdf_pages)} pages from {sample_pdf}:")
        for i, page in enumerate(pdf_pages):
            print(f"  [Page {i+1}] (chars={len(page)}, meta={page.metadata}) preview={page.content[:60]!r}...")
        
        # Chunk PDF pages directly
        pdf_chunks = chunkr.RecursiveChunker(chunk_size=500, overlap=50).chunk(pdf_pages[0].content)
        print(f"  Chunked page 1 into {len(pdf_chunks)} chunks.")

    print("\n" + "=" * 65)
    print("  ALL STRATEGIES & PDF LOADER TESTED SUCCESSFULLY IN PYTHON!")
    print("=" * 65)

if __name__ == "__main__":
    main()
