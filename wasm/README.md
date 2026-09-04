# ⚡ `chunkr-wasm`: Blazingly Fast Chunking in WebAssembly

WebAssembly build of **Chunkr**, engineered for **Browsers**, **Cloudflare Workers**, **Vercel Edge**, **Node.js**, **Deno**, and **Bun**.

Perform client-side and edge text chunking, token counting, and in-memory PDF extraction with zero server roundtrips.

---

## 📦 Installation

```bash
npm install chunkr-wasm
# or
pnpm add chunkr-wasm
# or
yarn add chunkr-wasm
```

---

## 🚀 Quickstart

### 1. Cloudflare Workers (Edge Serverless)

In Cloudflare Workers, import the `.wasm` file directly and initialize synchronously:

```javascript
import wasm from "chunkr-wasm/wasm";
import { initSync, RecursiveChunker, PDFLoader } from "chunkr-wasm/web";

// Initialize Wasm module with zero network latency
initSync(wasm);

export default {
  async fetch(request, env, ctx) {
    const url = new URL(request.url);

    // Text chunking endpoint
    if (request.method === "POST" && url.pathname === "/chunk") {
      const { text, chunkSize, overlap } = await request.json();
      const chunker = new RecursiveChunker(chunkSize || 500, overlap || 50);
      const chunks = chunker.chunk(text);
      return Response.json({ count: chunks.length, chunks });
    }

    // In-memory PDF chunking directly at the edge
    if (request.method === "POST" && url.pathname === "/pdf") {
      const arrayBuffer = await request.arrayBuffer();
      const bytes = new Uint8Array(arrayBuffer);
      const loader = new PDFLoader();
      const pages = loader.loadPagesFromBytes(bytes);
      return Response.json({ pages: pages.length, pages });
    }

    return new Response("Chunkr Cloudflare Worker ready. POST /chunk or /pdf");
  }
};
```

---

### 2. Browser (Vite / Next.js / Webpack)

Modern bundlers handle `.wasm` imports automatically via the bundler entry point:

```typescript
import { RecursiveChunker, MarkdownChunker, TokenChunker } from "chunkr-wasm";

// 1. Recursive Chunking
const chunker = new RecursiveChunker(1000, 150);
const chunks = chunker.chunk("Text to split into chunks...");

// 2. Token-Aware Chunking (OpenAI BPE cl100k_base)
const tokenChunker = new TokenChunker(512, 50, "cl100k_base");
const tokenChunks = tokenChunker.chunk("LLM context tokens...");
```

---

### 3. Vanilla Browser (ESM / CDN)

No build step required:

```html
<script type="module">
  import init, { RecursiveChunker, MarkdownChunker } from "./node_modules/chunkr-wasm/web/chunkr.js";

  async function start() {
    // Initializes the WebAssembly module
    await init();

    const chunker = new RecursiveChunker(500, 50);
    const chunks = chunker.chunk("High performance client-side chunking in browser memory.");
    console.log(chunks);
  }

  start();
</script>
```

---

### 4. Node.js (CommonJS & ESM)

```javascript
const { RecursiveChunker, ChunkPipeline, countTokens } = require("chunkr-wasm");

const chunker = new RecursiveChunker(800, 100);
const chunks = chunker.chunk("Large document text...");

// Post-processing pipeline (filter, dedup, bin-pack, SHA-256 metadata enrichment)
const pipeline = new ChunkPipeline()
  .filterMinCharacters(10)
  .deduplicateExact(true)
  .pack(1000)
  .enrichMetadata();

const optimized = pipeline.process(chunks);
```

---

## 🛠️ Supported Strategies in Wasm

| Strategy | Class | Description |
| :--- | :--- | :--- |
| **Recursive** | `RecursiveChunker` | Splits on natural boundaries (paragraphs, sentences, words) |
| **Markdown** | `MarkdownChunker` | Heading hierarchy awareness (`#` to `######`) with header paths |
| **Token BPE** | `TokenChunker` | OpenAI token boundaries (`cl100k_base`, `o200k_base`, etc.) |
| **Code** | `CodeChunker` | Language syntax splits (`rust`, `python`, `javascript`, `typescript`, `go`, `sql`, etc.) |
| **HTML** | `HtmlChunker` | DOM element boundaries (`<article>`, `<section>`, `<div>`, `<p>`) |
| **JSON** | `JsonChunker` | Structural JSON tree splits preserving valid JSON payloads |
| **Table** | `TableChunker` | Markdown/CSV/TSV table row chunking preserving column headers |
| **Sentence** | `SentenceChunker` | Sentence boundary splits with abbreviation handling |
| **Paragraph** | `ParagraphChunker` | Double-newline paragraph grouping |
| **Character** | `CharacterChunker` | Fixed character sliding window |
| **Word** | `WordChunker` | Fixed word sliding window |
| **Semantic** | `SemanticChunker` | Lexical embedding distance breakpoint clustering |
| **Late** | `LateChunker` | Long-context attention span pooling |
| **Proposition** | `PropositionChunker` | Atomic factual claim decomposition |
| **Hierarchical** | `HierarchicalChunker` | Multi-scale parent-child chunk trees |
| **Query-Aware** | `QueryAwareChunker` | Query hotspot resolution adaptation |
| **Stream** | `StreamChunker` | Windowed stream chunker for string buffers |
| **PDF Loader** | `PDFLoader` | In-memory byte extraction for PDFs without server dependencies |
| **Pipeline** | `ChunkPipeline` | Composable filter, dedup, pack, and metadata enrichment |

---

## 📄 In-Memory PDF Extraction

```typescript
import { PDFLoader, RecursiveChunker } from "chunkr-wasm";

// In browser File input, Cloudflare Worker request, or Node buffer:
const fileBytes = new Uint8Array(await file.arrayBuffer());

const loader = new PDFLoader();
const pages = loader.loadPagesFromBytes(fileBytes);

// Each page is a Document with page_number and total_pages metadata:
console.log(`Page 1: ${pages[0].content}`);
console.log(`Page number: ${pages[0].metadata.page_number}`);
```

---

## 📜 License

MIT License. Engineered with ❤️ by Dipankar Medhi.
