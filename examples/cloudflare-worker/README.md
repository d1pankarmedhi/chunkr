# ⚡ Cloudflare Worker Example with Chunkr WebAssembly

This example demonstrates how to deploy Chunkr inside a **Cloudflare Worker** serverless function.

## Features Demonstrated

1. **Zero-Latency In-Memory Wasm Loading**: Synchronous initialization of `chunkr_bg.wasm` bundled with the worker.
2. **Text Chunking API**: `POST /chunk` accepts raw text and returns structured chunks with metadata and OpenAI token counts.
3. **In-Memory PDF Parsing**: `POST /pdf` accepts raw binary PDF bytes (`application/pdf`) and extracts and chunks pages at the edge without external dependencies.

## Running Locally

1. Install Wrangler if you haven't already:
   ```bash
   npm install -g wrangler
   ```

2. Start the local development server:
   ```bash
   npx wrangler dev
   ```

3. Test text chunking:
   ```bash
   curl -X POST http://localhost:8787/chunk \
     -H "Content-Type: application/json" \
     -d '{"text": "Chunkr running inside Cloudflare Workers at the edge.", "chunkSize": 30, "overlap": 5}'
   ```

4. Test PDF chunking:
   ```bash
   curl -X POST http://localhost:8787/pdf \
     --data-binary @../../tests/test_files/sample_doc.pdf \
     -H "Content-Type: application/pdf"
   ```

## Deploying to Cloudflare

```bash
npx wrangler deploy
```
