# ⚡ Browser Demo with Chunkr WebAssembly

A zero-backend client-side text chunking demonstration using Chunkr's WebAssembly build.

## Features

- Runs entirely in the client browser with 0 HTTP calls.
- Preserves user privacy: sensitive documents never leave the client device.
- Demonstrates Recursive, Markdown, Sentence, Paragraph, Code, Semantic, and OpenAI Token chunkers.
- Measures real-time execution duration in milliseconds.

## Running the Demo

Because modern browsers restrict ES modules and `.wasm` files from `file://` URLs due to CORS policies, serve the directory using any local web server:

```bash
# Using npx serve:
npx serve examples/browser

# Or using Python:
python -m http.server 8080 --directory examples/browser
```

Then open `http://localhost:8080` in your web browser.
