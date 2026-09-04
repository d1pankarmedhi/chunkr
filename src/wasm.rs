//! # WebAssembly bindings for `chunkr`
//!
//! Exposes Chunkr's chunking algorithms, document loaders, and transformation pipelines
//! to JavaScript, TypeScript, Browsers, Cloudflare Workers, Node.js, Deno, and Bun.

use std::collections::HashMap;
use std::sync::Arc;
use wasm_bindgen::prelude::*;

use serde::Serialize;
use crate::prelude::*;

#[inline]
fn err_to_js(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

#[inline]
fn to_js_val<T: Serialize + ?Sized>(val: &T) -> Result<JsValue, JsValue> {
    let serializer = serde_wasm_bindgen::Serializer::json_compatible();
    val.serialize(&serializer).map_err(err_to_js)
}

#[inline]
fn docs_to_js(docs: &[Document]) -> Result<JsValue, JsValue> {
    to_js_val(docs)
}

#[inline]
fn js_to_docs(val: JsValue) -> Result<Vec<Document>, JsValue> {
    serde_wasm_bindgen::from_value(val).map_err(err_to_js)
}

fn parse_encoding(enc: Option<String>) -> Result<TokenEncoding, JsValue> {
    match enc.as_deref() {
        None | Some("cl100k_base") | Some("cl100k") => Ok(TokenEncoding::Cl100kBase),
        Some("o200k_base") | Some("o200k") => Ok(TokenEncoding::O200kBase),
        Some("p50k_base") | Some("p50k") => Ok(TokenEncoding::P50kBase),
        Some("r50k_base") | Some("r50k") => Ok(TokenEncoding::R50kBase),
        Some(other) => Err(JsValue::from_str(&format!(
            "Unsupported token encoding: '{}'. Supported: cl100k_base, o200k_base, p50k_base, r50k_base",
            other
        ))),
    }
}

fn parse_table_format(fmt: Option<String>) -> TableFormat {
    match fmt.as_deref() {
        Some("markdown") | Some("md") => TableFormat::Markdown,
        Some("csv") => TableFormat::Csv,
        Some("tsv") => TableFormat::Tsv,
        _ => TableFormat::Auto,
    }
}

fn parse_code_language(lang: Option<String>) -> CodeLanguage {
    match lang.as_deref().map(|s| s.to_lowercase()).as_deref() {
        Some("rust") | Some("rs") => CodeLanguage::Rust,
        Some("python") | Some("py") => CodeLanguage::Python,
        Some("javascript") | Some("js") => CodeLanguage::JavaScript,
        Some("typescript") | Some("ts") => CodeLanguage::TypeScript,
        Some("go") | Some("golang") => CodeLanguage::Go,
        Some("cpp") | Some("c++") | Some("c") => CodeLanguage::Cpp,
        Some("java") => CodeLanguage::Java,
        Some("html") | Some("htm") => CodeLanguage::Html,
        Some("sql") => CodeLanguage::Sql,
        Some("markdown") | Some("md") => CodeLanguage::Markdown,
        _ => CodeLanguage::Generic,
    }
}

// ============================================================================
// WasmDocument
// ============================================================================

/// Represents a document chunk with content and metadata
#[wasm_bindgen(js_name = Document)]
#[derive(Debug, Clone)]
pub struct WasmDocument {
    inner: Document,
}

#[wasm_bindgen(js_class = Document)]
impl WasmDocument {
    #[wasm_bindgen(constructor)]
    pub fn new(content: String, metadata: Option<JsValue>) -> Result<WasmDocument, JsValue> {
        let meta: HashMap<String, serde_json::Value> = match metadata {
            Some(v) if !v.is_null() && !v.is_undefined() => {
                serde_wasm_bindgen::from_value(v).map_err(err_to_js)?
            }
            _ => HashMap::new(),
        };
        Ok(Self {
            inner: Document::new(content, meta),
        })
    }

    #[wasm_bindgen(getter)]
    pub fn content(&self) -> String {
        self.inner.content.clone()
    }

    #[wasm_bindgen(setter)]
    pub fn set_content(&mut self, content: String) {
        self.inner.content = content;
    }

    #[wasm_bindgen(getter)]
    pub fn metadata(&self) -> Result<JsValue, JsValue> {
        to_js_val(&self.inner.metadata)
    }

    #[wasm_bindgen(js_name = addMetadata)]
    pub fn add_metadata(&mut self, key: String, value: JsValue) -> Result<(), JsValue> {
        let json_val: serde_json::Value = serde_wasm_bindgen::from_value(value).map_err(err_to_js)?;
        self.inner.add_metadata(key, json_val);
        Ok(())
    }

    #[wasm_bindgen(getter, js_name = charCount)]
    pub fn char_count(&self) -> usize {
        self.inner.content.chars().count()
    }

    #[wasm_bindgen(getter, js_name = wordCount)]
    pub fn word_count(&self) -> usize {
        self.inner.content.split_whitespace().count()
    }

    #[wasm_bindgen(js_name = toJSON)]
    pub fn to_json(&self) -> Result<JsValue, JsValue> {
        to_js_val(&self.inner)
    }
}

// ============================================================================
// RecursiveChunker
// ============================================================================

#[wasm_bindgen(js_name = RecursiveChunker)]
pub struct WasmRecursiveChunker {
    inner: RecursiveChunker,
}

#[wasm_bindgen(js_class = RecursiveChunker)]
impl WasmRecursiveChunker {
    #[wasm_bindgen(constructor)]
    pub fn new(chunk_size: Option<usize>, overlap: Option<usize>) -> Self {
        let cs = chunk_size.unwrap_or(1000);
        let ov = overlap.unwrap_or(150);
        Self {
            inner: RecursiveChunker::new()
                .with_chunk_size(cs)
                .with_overlap(ov),
        }
    }

    #[wasm_bindgen]
    pub fn chunk(&self, text: &str) -> Result<JsValue, JsValue> {
        let docs = self.inner.chunk(text).map_err(err_to_js)?;
        docs_to_js(&docs)
    }

    #[wasm_bindgen(js_name = chunkText)]
    pub fn chunk_text(&self, text: &str) -> Result<JsValue, JsValue> {
        let docs = self.inner.chunk(text).map_err(err_to_js)?;
        let strings: Vec<String> = docs.into_iter().map(|d| d.content).collect();
        to_js_val(&strings)
    }

    #[wasm_bindgen(js_name = chunkDocuments)]
    pub fn chunk_documents(&self, docs: JsValue) -> Result<JsValue, JsValue> {
        let doc_list = js_to_docs(docs)?;
        let chunks = self.inner.chunk_documents(&doc_list).map_err(err_to_js)?;
        docs_to_js(&chunks)
    }
}

// ============================================================================
// CharacterChunker & WordChunker
// ============================================================================

#[wasm_bindgen(js_name = CharacterChunker)]
pub struct WasmCharacterChunker {
    inner: CharacterChunker,
}

#[wasm_bindgen(js_class = CharacterChunker)]
impl WasmCharacterChunker {
    #[wasm_bindgen(constructor)]
    pub fn new(chunk_size: Option<usize>, overlap: Option<usize>) -> Self {
        let cs = chunk_size.unwrap_or(1000);
        let ov = overlap.unwrap_or(200);
        Self {
            inner: CharacterChunker::new()
                .with_chunk_size(cs)
                .with_overlap(ov),
        }
    }

    #[wasm_bindgen]
    pub fn chunk(&self, text: &str) -> Result<JsValue, JsValue> {
        let docs = self.inner.chunk(text).map_err(err_to_js)?;
        docs_to_js(&docs)
    }

    #[wasm_bindgen(js_name = chunkDocuments)]
    pub fn chunk_documents(&self, docs: JsValue) -> Result<JsValue, JsValue> {
        let doc_list = js_to_docs(docs)?;
        let chunks = self.inner.chunk_documents(&doc_list).map_err(err_to_js)?;
        docs_to_js(&chunks)
    }
}

#[wasm_bindgen(js_name = WordChunker)]
pub struct WasmWordChunker {
    inner: WordChunker,
}

#[wasm_bindgen(js_class = WordChunker)]
impl WasmWordChunker {
    #[wasm_bindgen(constructor)]
    pub fn new(chunk_size: Option<usize>, overlap: Option<usize>) -> Self {
        let cs = chunk_size.unwrap_or(200);
        let ov = overlap.unwrap_or(20);
        Self {
            inner: WordChunker::new()
                .with_chunk_size(cs)
                .with_overlap(ov),
        }
    }

    #[wasm_bindgen]
    pub fn chunk(&self, text: &str) -> Result<JsValue, JsValue> {
        let docs = self.inner.chunk(text).map_err(err_to_js)?;
        docs_to_js(&docs)
    }

    #[wasm_bindgen(js_name = chunkDocuments)]
    pub fn chunk_documents(&self, docs: JsValue) -> Result<JsValue, JsValue> {
        let doc_list = js_to_docs(docs)?;
        let chunks = self.inner.chunk_documents(&doc_list).map_err(err_to_js)?;
        docs_to_js(&chunks)
    }
}

// ============================================================================
// SentenceChunker & ParagraphChunker
// ============================================================================

#[wasm_bindgen(js_name = SentenceChunker)]
pub struct WasmSentenceChunker {
    inner: SentenceChunker,
}

#[wasm_bindgen(js_class = SentenceChunker)]
impl WasmSentenceChunker {
    #[wasm_bindgen(constructor)]
    pub fn new(
        sentences_per_chunk: Option<usize>,
        sentence_overlap: Option<usize>,
        max_characters: Option<usize>,
    ) -> Self {
        let spc = sentences_per_chunk.unwrap_or(3);
        let sov = sentence_overlap.unwrap_or(1);
        let mut inner = SentenceChunker::new()
            .with_sentences_per_chunk(spc)
            .with_sentence_overlap(sov);
        if let Some(max_chars) = max_characters {
            inner = inner.with_max_characters(max_chars);
        }
        Self { inner }
    }

    #[wasm_bindgen]
    pub fn chunk(&self, text: &str) -> Result<JsValue, JsValue> {
        let docs = self.inner.chunk(text).map_err(err_to_js)?;
        docs_to_js(&docs)
    }

    #[wasm_bindgen(js_name = chunkDocuments)]
    pub fn chunk_documents(&self, docs: JsValue) -> Result<JsValue, JsValue> {
        let doc_list = js_to_docs(docs)?;
        let chunks = self.inner.chunk_documents(&doc_list).map_err(err_to_js)?;
        docs_to_js(&chunks)
    }
}

#[wasm_bindgen(js_name = ParagraphChunker)]
pub struct WasmParagraphChunker {
    inner: ParagraphChunker,
}

#[wasm_bindgen(js_class = ParagraphChunker)]
impl WasmParagraphChunker {
    #[wasm_bindgen(constructor)]
    pub fn new(
        paragraphs_per_chunk: Option<usize>,
        paragraph_overlap: Option<usize>,
    ) -> Self {
        let ppc = paragraphs_per_chunk.unwrap_or(2);
        let pov = paragraph_overlap.unwrap_or(0);
        Self {
            inner: ParagraphChunker::new()
                .with_paragraphs_per_chunk(ppc)
                .with_paragraph_overlap(pov),
        }
    }

    #[wasm_bindgen]
    pub fn chunk(&self, text: &str) -> Result<JsValue, JsValue> {
        let docs = self.inner.chunk(text).map_err(err_to_js)?;
        docs_to_js(&docs)
    }

    #[wasm_bindgen(js_name = chunkDocuments)]
    pub fn chunk_documents(&self, docs: JsValue) -> Result<JsValue, JsValue> {
        let doc_list = js_to_docs(docs)?;
        let chunks = self.inner.chunk_documents(&doc_list).map_err(err_to_js)?;
        docs_to_js(&chunks)
    }
}

// ============================================================================
// MarkdownChunker, HtmlChunker, JsonChunker, TableChunker
// ============================================================================

#[wasm_bindgen(js_name = MarkdownChunker)]
pub struct WasmMarkdownChunker {
    inner: MarkdownChunker,
}

#[wasm_bindgen(js_class = MarkdownChunker)]
impl WasmMarkdownChunker {
    #[wasm_bindgen(constructor)]
    pub fn new(
        chunk_size: Option<usize>,
        overlap: Option<usize>,
        include_headers: Option<bool>,
    ) -> Self {
        let cs = chunk_size.unwrap_or(1000);
        let ov = overlap.unwrap_or(150);
        let mut inner = MarkdownChunker::new()
            .with_chunk_size(cs)
            .with_overlap(ov);
        if let Some(inc) = include_headers {
            inner = inner.with_include_header_in_content(inc);
        }
        Self { inner }
    }

    #[wasm_bindgen]
    pub fn chunk(&self, text: &str) -> Result<JsValue, JsValue> {
        let docs = self.inner.chunk(text).map_err(err_to_js)?;
        docs_to_js(&docs)
    }

    #[wasm_bindgen(js_name = chunkDocuments)]
    pub fn chunk_documents(&self, docs: JsValue) -> Result<JsValue, JsValue> {
        let doc_list = js_to_docs(docs)?;
        let chunks = self.inner.chunk_documents(&doc_list).map_err(err_to_js)?;
        docs_to_js(&chunks)
    }
}

#[wasm_bindgen(js_name = HtmlChunker)]
pub struct WasmHtmlChunker {
    inner: HtmlChunker,
}

#[wasm_bindgen(js_class = HtmlChunker)]
impl WasmHtmlChunker {
    #[wasm_bindgen(constructor)]
    pub fn new(chunk_size: Option<usize>, overlap: Option<usize>) -> Self {
        let cs = chunk_size.unwrap_or(1200);
        let ov = overlap.unwrap_or(150);
        Self {
            inner: HtmlChunker::new()
                .with_chunk_size(cs)
                .with_overlap(ov),
        }
    }

    #[wasm_bindgen]
    pub fn chunk(&self, text: &str) -> Result<JsValue, JsValue> {
        let docs = self.inner.chunk(text).map_err(err_to_js)?;
        docs_to_js(&docs)
    }

    #[wasm_bindgen(js_name = chunkDocuments)]
    pub fn chunk_documents(&self, docs: JsValue) -> Result<JsValue, JsValue> {
        let doc_list = js_to_docs(docs)?;
        let chunks = self.inner.chunk_documents(&doc_list).map_err(err_to_js)?;
        docs_to_js(&chunks)
    }
}

#[wasm_bindgen(js_name = JsonChunker)]
pub struct WasmJsonChunker {
    inner: JsonChunker,
}

#[wasm_bindgen(js_class = JsonChunker)]
impl WasmJsonChunker {
    #[wasm_bindgen(constructor)]
    pub fn new(max_chunk_size: Option<usize>, pretty: Option<bool>) -> Self {
        let mut inner = JsonChunker::new();
        if let Some(size) = max_chunk_size {
            inner = inner.with_max_chunk_size(size);
        }
        if let Some(p) = pretty {
            inner = inner.with_pretty(p);
        }
        Self { inner }
    }

    #[wasm_bindgen]
    pub fn chunk(&self, text: &str) -> Result<JsValue, JsValue> {
        let docs = self.inner.chunk(text).map_err(err_to_js)?;
        docs_to_js(&docs)
    }

    #[wasm_bindgen(js_name = chunkDocuments)]
    pub fn chunk_documents(&self, docs: JsValue) -> Result<JsValue, JsValue> {
        let doc_list = js_to_docs(docs)?;
        let chunks = self.inner.chunk_documents(&doc_list).map_err(err_to_js)?;
        docs_to_js(&chunks)
    }
}

#[wasm_bindgen(js_name = TableChunker)]
pub struct WasmTableChunker {
    inner: TableChunker,
}

#[wasm_bindgen(js_class = TableChunker)]
impl WasmTableChunker {
    #[wasm_bindgen(constructor)]
    pub fn new(
        chunk_size: Option<usize>,
        rows_per_chunk: Option<usize>,
        overlap_rows: Option<usize>,
        format: Option<String>,
    ) -> Self {
        let mut inner = TableChunker::new();
        if let Some(cs) = chunk_size {
            inner = inner.with_chunk_size(cs);
        }
        if let Some(rpc) = rows_per_chunk {
            inner = inner.with_rows_per_chunk(Some(rpc));
        }
        if let Some(or) = overlap_rows {
            inner = inner.with_overlap_rows(or);
        }
        inner = inner.with_format(parse_table_format(format));
        Self { inner }
    }

    #[wasm_bindgen]
    pub fn chunk(&self, text: &str) -> Result<JsValue, JsValue> {
        let docs = self.inner.chunk(text).map_err(err_to_js)?;
        docs_to_js(&docs)
    }

    #[wasm_bindgen(js_name = chunkDocuments)]
    pub fn chunk_documents(&self, docs: JsValue) -> Result<JsValue, JsValue> {
        let doc_list = js_to_docs(docs)?;
        let chunks = self.inner.chunk_documents(&doc_list).map_err(err_to_js)?;
        docs_to_js(&chunks)
    }
}

// ============================================================================
// TokenChunker & CodeChunker
// ============================================================================

#[wasm_bindgen(js_name = TokenChunker)]
pub struct WasmTokenChunker {
    inner: TokenChunker,
}

#[wasm_bindgen(js_class = TokenChunker)]
impl WasmTokenChunker {
    #[wasm_bindgen(constructor)]
    pub fn new(
        chunk_size: Option<usize>,
        overlap: Option<usize>,
        encoding: Option<String>,
    ) -> Result<WasmTokenChunker, JsValue> {
        let cs = chunk_size.unwrap_or(512);
        let ov = overlap.unwrap_or(50);
        let enc = parse_encoding(encoding)?;
        let inner = TokenChunker::with_encoding(cs, ov, enc).map_err(err_to_js)?;
        Ok(Self { inner })
    }

    #[wasm_bindgen]
    pub fn chunk(&self, text: &str) -> Result<JsValue, JsValue> {
        let docs = self.inner.chunk(text).map_err(err_to_js)?;
        docs_to_js(&docs)
    }

    #[wasm_bindgen(js_name = chunkDocuments)]
    pub fn chunk_documents(&self, docs: JsValue) -> Result<JsValue, JsValue> {
        let doc_list = js_to_docs(docs)?;
        let chunks = self.inner.chunk_documents(&doc_list).map_err(err_to_js)?;
        docs_to_js(&chunks)
    }

    #[wasm_bindgen(js_name = countTokens)]
    pub fn count_tokens(&self, text: &str) -> usize {
        self.inner.count_tokens(text)
    }
}

#[wasm_bindgen(js_name = CodeChunker)]
pub struct WasmCodeChunker {
    inner: CodeChunker,
}

#[wasm_bindgen(js_class = CodeChunker)]
impl WasmCodeChunker {
    #[wasm_bindgen(constructor)]
    pub fn new(
        language: Option<String>,
        chunk_size: Option<usize>,
        overlap: Option<usize>,
    ) -> Self {
        let lang = parse_code_language(language);
        let mut inner = CodeChunker::new(lang);
        if let Some(cs) = chunk_size {
            inner = inner.with_chunk_size(cs);
        }
        if let Some(ov) = overlap {
            inner = inner.with_overlap(ov);
        }
        Self { inner }
    }

    #[wasm_bindgen]
    pub fn chunk(&self, text: &str) -> Result<JsValue, JsValue> {
        let docs = self.inner.chunk(text).map_err(err_to_js)?;
        docs_to_js(&docs)
    }

    #[wasm_bindgen(js_name = chunkDocuments)]
    pub fn chunk_documents(&self, docs: JsValue) -> Result<JsValue, JsValue> {
        let doc_list = js_to_docs(docs)?;
        let chunks = self.inner.chunk_documents(&doc_list).map_err(err_to_js)?;
        docs_to_js(&chunks)
    }
}

// ============================================================================
// SemanticChunker & LateChunker
// ============================================================================

#[wasm_bindgen(js_name = SemanticChunker)]
pub struct WasmSemanticChunker {
    inner: SemanticChunker,
}

#[wasm_bindgen(js_class = SemanticChunker)]
impl WasmSemanticChunker {
    #[wasm_bindgen(constructor)]
    pub fn new(
        percentile: Option<f32>,
        min_chunk_size: Option<usize>,
        max_chunk_size: Option<usize>,
    ) -> Self {
        let mut inner = SemanticChunker::new();
        if let Some(p) = percentile {
            inner = inner.with_threshold(BreakpointThreshold::Percentile(p));
        }
        if let (Some(min_s), Some(max_s)) = (min_chunk_size, max_chunk_size) {
            inner = inner.with_size_bounds(min_s, max_s);
        }
        Self { inner }
    }

    #[wasm_bindgen]
    pub fn chunk(&self, text: &str) -> Result<JsValue, JsValue> {
        let docs = self.inner.chunk(text).map_err(err_to_js)?;
        docs_to_js(&docs)
    }

    #[wasm_bindgen(js_name = chunkDocuments)]
    pub fn chunk_documents(&self, docs: JsValue) -> Result<JsValue, JsValue> {
        let doc_list = js_to_docs(docs)?;
        let chunks = self.inner.chunk_documents(&doc_list).map_err(err_to_js)?;
        docs_to_js(&chunks)
    }
}

#[wasm_bindgen(js_name = LateChunker)]
pub struct WasmLateChunker {
    inner: LateChunker,
}

#[wasm_bindgen(js_class = LateChunker)]
impl WasmLateChunker {
    #[wasm_bindgen(constructor)]
    pub fn new(
        chunk_size: Option<usize>,
        overlap: Option<usize>,
        encoding: Option<String>,
    ) -> Result<WasmLateChunker, JsValue> {
        let enc = parse_encoding(encoding)?;
        let cs = chunk_size.unwrap_or(500);
        let ov = overlap.unwrap_or(50);
        let mut inner = LateChunker::new().with_encoding(enc).map_err(err_to_js)?;
        inner.base_chunker = Arc::new(
            RecursiveChunker::new()
                .with_chunk_size(cs)
                .with_overlap(ov),
        );
        Ok(Self { inner })
    }

    #[wasm_bindgen]
    pub fn chunk(&self, text: &str) -> Result<JsValue, JsValue> {
        let docs = self.inner.chunk(text).map_err(err_to_js)?;
        docs_to_js(&docs)
    }

    #[wasm_bindgen(js_name = chunkDocuments)]
    pub fn chunk_documents(&self, docs: JsValue) -> Result<JsValue, JsValue> {
        let doc_list = js_to_docs(docs)?;
        let chunks = self.inner.chunk_documents(&doc_list).map_err(err_to_js)?;
        docs_to_js(&chunks)
    }
}

// ============================================================================
// Proposition, Hierarchical, QueryAware, StreamChunker
// ============================================================================

#[wasm_bindgen(js_name = PropositionChunker)]
pub struct WasmPropositionChunker {
    inner: PropositionChunker,
}

#[wasm_bindgen(js_class = PropositionChunker)]
impl WasmPropositionChunker {
    #[wasm_bindgen(constructor)]
    pub fn new(
        propositions_per_chunk: Option<usize>,
        overlap: Option<usize>,
    ) -> Result<WasmPropositionChunker, JsValue> {
        let mut inner = PropositionChunker::new();
        if let (Some(ppc), Some(ov)) = (propositions_per_chunk, overlap) {
            inner = inner.with_grouping(ppc, ov).map_err(err_to_js)?;
        } else if let Some(ppc) = propositions_per_chunk {
            inner = inner.with_grouping(ppc, 0).map_err(err_to_js)?;
        }
        Ok(Self { inner })
    }

    #[wasm_bindgen]
    pub fn chunk(&self, text: &str) -> Result<JsValue, JsValue> {
        let docs = self.inner.chunk(text).map_err(err_to_js)?;
        docs_to_js(&docs)
    }
}

#[wasm_bindgen(js_name = HierarchicalChunker)]
pub struct WasmHierarchicalChunker {
    inner: HierarchicalChunker,
}

#[wasm_bindgen(js_class = HierarchicalChunker)]
impl WasmHierarchicalChunker {
    #[wasm_bindgen(constructor)]
    pub fn new(
        parent_chunk_size: Option<usize>,
        parent_overlap: Option<usize>,
        child_chunk_size: Option<usize>,
        child_overlap: Option<usize>,
    ) -> Result<WasmHierarchicalChunker, JsValue> {
        let p_cs = parent_chunk_size.unwrap_or(2000);
        let p_ov = parent_overlap.unwrap_or(200);
        let c_cs = child_chunk_size.unwrap_or(400);
        let c_ov = child_overlap.unwrap_or(50);
        let inner = HierarchicalChunker::with_sizes(p_cs, p_ov, c_cs, c_ov)
            .map_err(err_to_js)?;
        Ok(Self { inner })
    }

    #[wasm_bindgen]
    pub fn chunk(&self, text: &str) -> Result<JsValue, JsValue> {
        let docs = self.inner.chunk(text).map_err(err_to_js)?;
        docs_to_js(&docs)
    }
}

#[wasm_bindgen(js_name = QueryAwareChunker)]
pub struct WasmQueryAwareChunker {
    inner: QueryAwareChunker,
}

#[wasm_bindgen(js_class = QueryAwareChunker)]
impl WasmQueryAwareChunker {
    #[wasm_bindgen(constructor)]
    pub fn new(
        query: String,
        hotspot_sentences: Option<usize>,
        context_sentences: Option<usize>,
    ) -> Self {
        let h_s = hotspot_sentences.unwrap_or(2);
        let c_s = context_sentences.unwrap_or(5);
        let inner = QueryAwareChunker::new(query)
            .with_hotspot_sizing(h_s, 1)
            .with_context_sizing(c_s, 1);
        Self { inner }
    }

    #[wasm_bindgen]
    pub fn chunk(&self, text: &str) -> Result<JsValue, JsValue> {
        let docs = self.inner.chunk(text).map_err(err_to_js)?;
        docs_to_js(&docs)
    }
}

#[wasm_bindgen(js_name = StreamChunker)]
pub struct WasmStreamChunker {
    inner: StreamChunker,
}

#[wasm_bindgen(js_class = StreamChunker)]
impl WasmStreamChunker {
    #[wasm_bindgen(constructor)]
    pub fn new(chunk_size: Option<usize>, overlap: Option<usize>) -> Result<WasmStreamChunker, JsValue> {
        let cs = chunk_size.unwrap_or(1000);
        let ov = overlap.unwrap_or(150);
        let inner = StreamChunker::new(cs, ov).map_err(err_to_js)?;
        Ok(Self { inner })
    }

    #[wasm_bindgen(js_name = chunkString)]
    pub fn chunk_string(&self, text: &str) -> Result<JsValue, JsValue> {
        let cursor = std::io::Cursor::new(text.as_bytes());
        let reader = self.inner.chunk_reader(cursor);
        let docs: Result<Vec<Document>, ChunkrError> = reader.collect();
        let docs = docs.map_err(err_to_js)?;
        docs_to_js(&docs)
    }
}

// ============================================================================
// PDFLoader (In-Memory for Browser & Cloudflare Workers)
// ============================================================================

#[wasm_bindgen(js_name = PDFLoader)]
pub struct WasmPDFLoader {
    inner: PDFLoader,
}

#[wasm_bindgen(js_class = PDFLoader)]
impl WasmPDFLoader {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: PDFLoader::new(),
        }
    }

    #[wasm_bindgen(js_name = loadTextFromBytes)]
    pub fn load_text_from_bytes(&self, bytes: &[u8]) -> Result<String, JsValue> {
        self.inner.load_from_bytes(bytes).map_err(err_to_js)
    }

    #[wasm_bindgen(js_name = loadDocumentFromBytes)]
    pub fn load_document_from_bytes(&self, bytes: &[u8]) -> Result<JsValue, JsValue> {
        let doc = self.inner.load_document_from_bytes(bytes).map_err(err_to_js)?;
        to_js_val(&doc)
    }

    #[wasm_bindgen(js_name = loadPagesFromBytes)]
    pub fn load_pages_from_bytes(&self, bytes: &[u8]) -> Result<JsValue, JsValue> {
        let pages = self.inner.load_pages_from_bytes(bytes).map_err(err_to_js)?;
        docs_to_js(&pages)
    }
}

// ============================================================================
// ChunkPipeline
// ============================================================================

#[wasm_bindgen(js_name = ChunkPipeline)]
pub struct WasmChunkPipeline {
    inner: ChunkPipeline,
}

#[wasm_bindgen(js_class = ChunkPipeline)]
impl WasmChunkPipeline {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: ChunkPipeline::new(),
        }
    }

    #[wasm_bindgen(js_name = filterMinCharacters)]
    pub fn filter_min_characters(mut self, min_chars: usize) -> Self {
        self.inner = self.inner.filter_min_characters(min_chars);
        self
    }

    #[wasm_bindgen(js_name = filterMaxCharacters)]
    pub fn filter_max_characters(mut self, max_chars: usize) -> Self {
        self.inner = self.inner.filter_max_characters(max_chars);
        self
    }

    #[wasm_bindgen(js_name = filterMinWords)]
    pub fn filter_min_words(mut self, min_words: usize) -> Self {
        self.inner = self.inner.filter_min_words(min_words);
        self
    }

    #[wasm_bindgen(js_name = filterMinAlphaRatio)]
    pub fn filter_min_alpha_ratio(mut self, ratio: f32) -> Self {
        self.inner = self.inner.filter_min_alpha_ratio(ratio);
        self
    }

    #[wasm_bindgen(js_name = deduplicateExact)]
    pub fn deduplicate_exact(mut self, case_sensitive: bool) -> Self {
        self.inner = self.inner.deduplicate_exact(case_sensitive);
        self
    }

    #[wasm_bindgen(js_name = deduplicateNormalized)]
    pub fn deduplicate_normalized(mut self, case_sensitive: bool) -> Self {
        self.inner = self.inner.deduplicate_normalized(case_sensitive);
        self
    }

    #[wasm_bindgen]
    pub fn pack(mut self, max_characters: usize) -> Self {
        self.inner = self.inner.pack(max_characters);
        self
    }

    #[wasm_bindgen(js_name = enrichMetadata)]
    pub fn enrich_metadata(mut self) -> Self {
        self.inner = self.inner.enrich_metadata();
        self
    }

    #[wasm_bindgen(js_name = withIdPrefix)]
    pub fn with_id_prefix(mut self, prefix: String) -> Self {
        self.inner = self.inner.with_id_prefix(prefix);
        self
    }

    #[wasm_bindgen]
    pub fn process(&self, docs: JsValue) -> Result<JsValue, JsValue> {
        let doc_list = js_to_docs(docs)?;
        let processed = self.inner.process(doc_list);
        docs_to_js(&processed)
    }
}

// ============================================================================
// Top-Level Convenience Functions
// ============================================================================

/// Universal one-line chunking helper
#[wasm_bindgen]
pub fn chunk(
    text: &str,
    strategy: Option<String>,
    chunk_size: Option<usize>,
    overlap: Option<usize>,
) -> Result<JsValue, JsValue> {
    let cs = chunk_size.unwrap_or(1000);
    let ov = overlap.unwrap_or(150);

    let docs = match strategy.as_deref().map(|s| s.to_lowercase()).as_deref() {
        None | Some("recursive") => {
            RecursiveChunker::new()
                .with_chunk_size(cs)
                .with_overlap(ov)
                .chunk(text)
        }
        Some("markdown") | Some("md") => {
            MarkdownChunker::new()
                .with_chunk_size(cs)
                .with_overlap(ov)
                .chunk(text)
        }
        Some("sentence") => {
            SentenceChunker::new()
                .with_sentences_per_chunk(cs)
                .with_sentence_overlap(ov)
                .chunk(text)
        }
        Some("paragraph") => {
            ParagraphChunker::new()
                .with_paragraphs_per_chunk(cs)
                .with_paragraph_overlap(ov)
                .chunk(text)
        }
        Some("token") => {
            TokenChunker::with_encoding(cs, ov, TokenEncoding::Cl100kBase)
                .map_err(err_to_js)?
                .chunk(text)
        }
        Some("html") => {
            HtmlChunker::new()
                .with_chunk_size(cs)
                .with_overlap(ov)
                .chunk(text)
        }
        Some("json") => {
            JsonChunker::new()
                .with_max_chunk_size(cs)
                .chunk(text)
        }
        Some("table") => {
            TableChunker::new()
                .with_chunk_size(cs)
                .chunk(text)
        }
        Some("code") => {
            CodeChunker::new(CodeLanguage::Generic)
                .with_chunk_size(cs)
                .with_overlap(ov)
                .chunk(text)
        }
        Some("character") | Some("char") => {
            CharacterChunker::new()
                .with_chunk_size(cs)
                .with_overlap(ov)
                .chunk(text)
        }
        Some("word") => {
            WordChunker::new()
                .with_chunk_size(cs)
                .with_overlap(ov)
                .chunk(text)
        }
        Some("semantic") => {
            SemanticChunker::new()
                .with_size_bounds(cs / 2, cs)
                .chunk(text)
        }
        Some(other) => {
            return Err(JsValue::from_str(&format!(
                "Unknown chunking strategy: '{}'. Supported: recursive, markdown, sentence, paragraph, token, html, json, table, code, character, word, semantic",
                other
            )));
        }
    }.map_err(err_to_js)?;

    docs_to_js(&docs)
}

/// Fast token count helper using OpenAI BPE
#[wasm_bindgen(js_name = countTokens)]
pub fn count_tokens(text: &str, encoding: Option<String>) -> Result<usize, JsValue> {
    let enc = parse_encoding(encoding)?;
    let chunker = TokenChunker::with_encoding(512, 50, enc).map_err(err_to_js)?;
    Ok(chunker.count_tokens(text))
}
