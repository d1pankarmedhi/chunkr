/**
 * Cloudflare Worker example powered by Chunkr WebAssembly
 *
 * Demonstrates:
 * 1. Synchronous Wasm initialization in Cloudflare Workers
 * 2. High-performance JSON text chunking with RecursiveChunker
 * 3. In-memory PDF document parsing and page extraction directly at the edge
 */

import wasmModule from "../../wasm/web/chunkr_bg.wasm";
import { initSync, RecursiveChunker, MarkdownChunker, PDFLoader, countTokens } from "../../wasm/web/chunkr.js";

// Initialize the WebAssembly module with zero network latency
initSync(wasmModule);

export default {
  async fetch(request, env, ctx) {
    const url = new URL(request.url);

    // Health check
    if (request.method === "GET" && url.pathname === "/") {
      return new Response(JSON.stringify({
        status: "ok",
        engine: "chunkr-wasm",
        endpoints: [
          { path: "/chunk", method: "POST", description: "Chunk raw text (JSON body: { text, strategy, chunkSize, overlap })" },
          { path: "/pdf", method: "POST", description: "Extract and chunk PDF bytes in memory" }
        ]
      }), {
        headers: { "Content-Type": "application/json" }
      });
    }

    // POST /chunk: Chunk text payload
    if (request.method === "POST" && url.pathname === "/chunk") {
      try {
        const body = await request.json();
        const text = body.text || "";
        const strategy = body.strategy || "recursive";
        const chunkSize = body.chunkSize || 500;
        const overlap = body.overlap || 50;

        if (!text.trim()) {
          return new Response(JSON.stringify({ error: "Empty text provided" }), {
            status: 400,
            headers: { "Content-Type": "application/json" }
          });
        }

        let chunks;
        if (strategy === "markdown") {
          const chunker = new MarkdownChunker(chunkSize, overlap, true);
          chunks = chunker.chunk(text);
        } else {
          const chunker = new RecursiveChunker(chunkSize, overlap);
          chunks = chunker.chunk(text);
        }

        const totalTokens = countTokens(text, "cl100k_base");

        return new Response(JSON.stringify({
          strategy,
          total_characters: text.length,
          total_tokens: totalTokens,
          chunk_count: chunks.length,
          chunks
        }), {
          headers: { "Content-Type": "application/json" }
        });
      } catch (err) {
        return new Response(JSON.stringify({ error: err.message || String(err) }), {
          status: 500,
          headers: { "Content-Type": "application/json" }
        });
      }
    }

    // POST /pdf: Parse raw PDF bytes in memory and chunk each page
    if (request.method === "POST" && url.pathname === "/pdf") {
      try {
        const arrayBuffer = await request.arrayBuffer();
        const bytes = new Uint8Array(arrayBuffer);

        if (bytes.length === 0) {
          return new Response(JSON.stringify({ error: "Empty PDF payload received" }), {
            status: 400,
            headers: { "Content-Type": "application/json" }
          });
        }

        const loader = new PDFLoader();
        const pages = loader.loadPagesFromBytes(bytes);

        // Chunk each page using RecursiveChunker
        const chunker = new RecursiveChunker(400, 50);
        const chunkedPages = pages.map(page => ({
          page_number: page.metadata.page_number,
          total_pages: page.metadata.total_pages,
          chunks: chunker.chunk(page.content)
        }));

        return new Response(JSON.stringify({
          page_count: pages.length,
          pages: chunkedPages
        }), {
          headers: { "Content-Type": "application/json" }
        });
      } catch (err) {
        return new Response(JSON.stringify({ error: err.message || String(err) }), {
          status: 500,
          headers: { "Content-Type": "application/json" }
        });
      }
    }

    return new Response("Not Found", { status: 404 });
  }
};
