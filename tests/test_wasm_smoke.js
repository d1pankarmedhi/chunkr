// Smoke test for Chunkr WebAssembly bindings
const assert = require("assert");
const {
  Document,
  RecursiveChunker,
  CharacterChunker,
  WordChunker,
  SentenceChunker,
  ParagraphChunker,
  MarkdownChunker,
  HtmlChunker,
  JsonChunker,
  TableChunker,
  TokenChunker,
  CodeChunker,
  SemanticChunker,
  LateChunker,
  PropositionChunker,
  HierarchicalChunker,
  QueryAwareChunker,
  StreamChunker,
  PDFLoader,
  ChunkPipeline,
  chunk,
  countTokens,
} = require("../wasm/nodejs/chunkr.js");

console.log("⚡ Starting Chunkr WebAssembly smoke test...\n");

// 1. Document
console.log("1. Testing Document...");
const doc = new Document("Hello world from Wasm!", { author: "Dipankar", score: 42 });
assert.strictEqual(doc.content, "Hello world from Wasm!");
assert.strictEqual(doc.charCount, 22);
assert.strictEqual(doc.wordCount, 4);
const meta = doc.metadata;
assert.strictEqual(meta.author, "Dipankar");
assert.strictEqual(meta.score, 42);
doc.addMetadata("status", "verified");
assert.strictEqual(doc.metadata.status, "verified");
console.log("   ✓ Document passed\n");

// 2. RecursiveChunker
console.log("2. Testing RecursiveChunker...");
const recChunker = new RecursiveChunker(50, 10);
const recChunks = recChunker.chunk("Paragraph one is here.\n\nParagraph two has different content that is a bit longer.\n\nParagraph three completes the text.");
assert(recChunks.length >= 2, "Expected multiple recursive chunks");
assert(recChunks[0].content.length > 0);
assert(typeof recChunks[0].metadata === "object");
const textChunks = recChunker.chunkText("A simple short text to chunk.");
assert(Array.isArray(textChunks));
assert(typeof textChunks[0] === "string");
console.log(`   ✓ RecursiveChunker produced ${recChunks.length} chunks\n`);

// 3. CharacterChunker & WordChunker
console.log("3. Testing CharacterChunker & WordChunker...");
const charChunker = new CharacterChunker(30, 5);
const charChunks = charChunker.chunk("The quick brown fox jumps over the lazy dog and runs away.");
assert(charChunks.length >= 2);

const wordChunker = new WordChunker(5, 1);
const wordChunks = wordChunker.chunk("one two three four five six seven eight nine ten eleven twelve");
assert(wordChunks.length >= 2);
console.log(`   ✓ Character & Word chunkers passed\n`);

// 4. SentenceChunker & ParagraphChunker
console.log("4. Testing SentenceChunker & ParagraphChunker...");
const sentChunker = new SentenceChunker(2, 1);
const sentChunks = sentChunker.chunk("First sentence here. Second sentence follows. Third sentence wraps up. Fourth sentence concludes.");
assert(sentChunks.length >= 2);

const paraChunker = new ParagraphChunker(1, 0);
const paraChunks = paraChunker.chunk("First paragraph.\n\nSecond paragraph.\n\nThird paragraph.");
assert.strictEqual(paraChunks.length, 3);
console.log(`   ✓ Sentence & Paragraph chunkers passed\n`);

// 5. MarkdownChunker
console.log("5. Testing MarkdownChunker...");
const mdChunker = new MarkdownChunker(200, 20, true);
const mdText = `# Title
Introduction text.

## Section 1
Content of section 1.

## Section 2
Content of section 2 with more details.`;
const mdChunks = mdChunker.chunk(mdText);
assert(mdChunks.length >= 2);
assert(mdChunks[0].metadata.header_path !== undefined);
console.log(`   ✓ MarkdownChunker produced ${mdChunks.length} chunks with header breadcrumbs\n`);

// 6. HtmlChunker & JsonChunker & TableChunker
console.log("6. Testing HtmlChunker, JsonChunker & TableChunker...");
const htmlChunker = new HtmlChunker(100, 10);
const htmlChunks = htmlChunker.chunk("<article><h1>Header</h1><p>Body paragraph one.</p><p>Body paragraph two.</p></article>");
assert(htmlChunks.length >= 1);

const jsonChunker = new JsonChunker(100, true);
const jsonChunks = jsonChunker.chunk(JSON.stringify({ a: 1, b: [1, 2, 3], c: { nested: "val" } }));
assert(jsonChunks.length >= 1);

const tableChunker = new TableChunker(100, null, null, "markdown");
const tableText = `| Name | Age | City |
|------|-----|------|
| Alice | 30 | London |
| Bob | 25 | Paris |
| Charlie | 35 | Tokyo |`;
const tableChunks = tableChunker.chunk(tableText);
assert(tableChunks.length >= 1);
console.log(`   ✓ HTML, JSON, and Table chunkers passed\n`);

// 7. TokenChunker (BPE cl100k_base)
console.log("7. Testing TokenChunker...");
const tokenChunker = new TokenChunker(15, 3, "cl100k_base");
const tokenText = "OpenAI tiktoken BPE tokenization inside WebAssembly running at native speed.";
const tokenChunks = tokenChunker.chunk(tokenText);
assert(tokenChunks.length >= 1);
const count = tokenChunker.countTokens(tokenText);
assert(count > 5, "Token count should be positive");
assert.strictEqual(countTokens(tokenText, "cl100k_base"), count);
console.log(`   ✓ TokenChunker passed (${count} tokens)\n`);

// 8. CodeChunker
console.log("8. Testing CodeChunker...");
const codeChunker = new CodeChunker("rust", 200, 30);
const rustCode = `fn main() {
    println!("Hello, world!");
}

fn add(a: i32, b: i32) -> i32 {
    a + b
}`;
const codeChunks = codeChunker.chunk(rustCode);
assert(codeChunks.length >= 1);
console.log(`   ✓ CodeChunker passed\n`);

// 9. SemanticChunker & LateChunker
console.log("9. Testing SemanticChunker & LateChunker...");
const semanticChunker = new SemanticChunker(85.0);
const semanticChunks = semanticChunker.chunk("Machine learning systems optimize objective functions. Deep neural networks learn hierarchical representations. Cooking requires fresh ingredients and precise temperature control.");
assert(semanticChunks.length >= 1);

const lateChunker = new LateChunker(100, 10, "cl100k_base");
const lateChunks = lateChunker.chunk("Late chunking embeds the full document first, preserving bidirectional context across span boundaries.");
assert(lateChunks.length >= 1);
console.log(`   ✓ Semantic & Late chunkers passed\n`);

// 10. PropositionChunker, HierarchicalChunker, QueryAwareChunker, StreamChunker
console.log("10. Testing Proposition, Hierarchical, QueryAware, Stream...");
const propChunker = new PropositionChunker(2, 0);
const propChunks = propChunker.chunk("The Eiffel Tower was built in Paris in 1889. It is made of wrought iron.");
assert(propChunks.length >= 1);

const hierChunker = new HierarchicalChunker(500, 50, 100, 10);
const hierChunks = hierChunker.chunk("First major topic with several detailed sub-points. Second major topic with further explanations and examples.");
assert(hierChunks.length >= 1);

const queryChunker = new QueryAwareChunker("neural network", 2, 4);
const queryChunks = queryChunker.chunk("Ancient history studied artifacts. A neural network is trained using backpropagation. Botany investigates plant life.");
assert(queryChunks.length >= 1);

const streamChunker = new StreamChunker(100, 15);
const streamChunks = streamChunker.chunkString("Streaming text chunks through WebAssembly cursor reader.");
assert(streamChunks.length >= 1);
console.log(`   ✓ Proposition, Hierarchical, QueryAware, Stream passed\n`);

console.log("11. Testing PDFLoader from memory bytes...");
const fs = require("fs");
const path = require("path");
const pdfLoader = new PDFLoader();
const samplePdfPath = path.join(__dirname, "test_files", "sample_doc.pdf");
const pdfBytes = fs.readFileSync(samplePdfPath);
const pdfDoc = pdfLoader.loadDocumentFromBytes(pdfBytes);
assert(pdfDoc !== null && typeof pdfDoc === "object");
assert(pdfDoc.content.length > 0, "PDF content should not be empty");
assert(pdfDoc.metadata.total_pages > 0, "PDF should have pages");

const pdfPages = pdfLoader.loadPagesFromBytes(pdfBytes);
assert(pdfPages.length > 0, "Expected PDF pages");
assert.strictEqual(pdfPages[0].metadata.page_number, 1);
console.log(`   ✓ In-memory PDFLoader parsed ${pdfPages.length} pages (${pdfDoc.content.length} chars)\n`);

// 12. ChunkPipeline
console.log("12. Testing ChunkPipeline (filter, dedup, pack, enrich)...");
const pipeline = new ChunkPipeline()
  .filterMinCharacters(5)
  .filterMaxCharacters(500)
  .deduplicateExact(true)
  .pack(150)
  .enrichMetadata()
  .withIdPrefix("test_");

const rawDocs = [
  { content: "Short", metadata: {} },
  { content: "This is a good chunk of text that should be kept.", metadata: {} },
  { content: "This is a good chunk of text that should be kept.", metadata: {} }, // duplicate
  { content: "Another unique chunk of text to be packed together.", metadata: {} },
];
const processed = pipeline.process(rawDocs);
assert(processed.length >= 1);
assert(processed[0].metadata.chunk_hash !== undefined, "Enricher should add SHA-256 chunk_hash");
assert(processed[0].metadata.chunk_id !== undefined, "Enricher should add chunk_id");
assert(processed[0].metadata.reading_time_secs !== undefined, "Enricher should add reading_time_secs");
console.log(`   ✓ ChunkPipeline processed ${rawDocs.length} -> ${processed.length} chunks with hash & metrics\n`);

// 13. Universal chunk() function
console.log("13. Testing universal chunk() function...");
const autoChunks = chunk("Universal chunk function works cleanly across all strategies.", "recursive", 30, 5);
assert(autoChunks.length >= 1);
console.log(`   ✓ Top-level chunk() helper passed\n`);

console.log("🎉 ALL WEBASSEMBLY TESTS PASSED SUCCESSFULLY!");
