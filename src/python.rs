use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::collections::HashMap;

use crate::prelude::*;

fn json_to_py(py: Python, val: &serde_json::Value) -> PyResult<PyObject> {
    match val {
        serde_json::Value::Null => Ok(py.None()),
        serde_json::Value::Bool(b) => Ok(b.to_object(py)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.to_object(py))
            } else if let Some(u) = n.as_u64() {
                Ok(u.to_object(py))
            } else if let Some(f) = n.as_f64() {
                Ok(f.to_object(py))
            } else {
                Ok(py.None())
            }
        }
        serde_json::Value::String(s) => Ok(s.to_object(py)),
        serde_json::Value::Array(arr) => {
            let list = PyList::empty_bound(py);
            for item in arr {
                list.append(json_to_py(py, item)?)?;
            }
            Ok(list.into())
        }
        serde_json::Value::Object(map) => {
            let dict = PyDict::new_bound(py);
            for (k, v) in map {
                dict.set_item(k, json_to_py(py, v)?)?;
            }
            Ok(dict.into())
        }
    }
}

/// A document chunk holding text content and metadata
#[pyclass(name = "Document")]
#[derive(Debug, Clone)]
pub struct PyDocument {
    pub inner: Document,
}

#[pymethods]
impl PyDocument {
    #[new]
    #[pyo3(signature = (content, metadata=None))]
    pub fn new(content: String, metadata: Option<HashMap<String, String>>) -> Self {
        let mut inner = Document::from_text(content);
        if let Some(meta) = metadata {
            for (k, v) in meta {
                inner.add_metadata(k, serde_json::Value::String(v));
            }
        }
        Self { inner }
    }

    #[getter]
    pub fn content(&self) -> String {
        self.inner.content.clone()
    }

    #[getter]
    pub fn metadata(&self, py: Python) -> PyResult<PyObject> {
        let dict = PyDict::new_bound(py);
        for (k, v) in &self.inner.metadata {
            dict.set_item(k, json_to_py(py, v)?)?;
        }
        Ok(dict.into())
    }

    pub fn to_dict(&self, py: Python) -> PyResult<PyObject> {
        let dict = PyDict::new_bound(py);
        dict.set_item("content", &self.inner.content)?;
        dict.set_item("metadata", self.metadata(py)?)?;
        Ok(dict.into())
    }

    pub fn __repr__(&self) -> String {
        let preview: String = self.inner.content.chars().take(50).collect();
        format!("Document(content='{}...', len={})", preview.replace('\n', " "), self.inner.content.len())
    }

    pub fn __len__(&self) -> usize {
        self.inner.content.len()
    }
}

impl From<Document> for PyDocument {
    fn from(inner: Document) -> Self {
        Self { inner }
    }
}

fn wrap_docs(docs: Vec<Document>) -> Vec<PyDocument> {
    docs.into_iter().map(PyDocument::from).collect()
}

// 1. Recursive Chunker
#[pyclass(name = "RecursiveChunker")]
pub struct PyRecursiveChunker {
    inner: RecursiveChunker,
}

#[pymethods]
impl PyRecursiveChunker {
    #[new]
    #[pyo3(signature = (chunk_size=1000, overlap=200, separators=None))]
    pub fn new(chunk_size: usize, overlap: usize, separators: Option<Vec<String>>) -> Self {
        let mut chunker = RecursiveChunker::new()
            .with_chunk_size(chunk_size)
            .with_overlap(overlap);
        if let Some(seps) = separators {
            chunker = chunker.with_separators(seps);
        }
        Self { inner: chunker }
    }

    pub fn chunk(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        self.inner.chunk(text).map(wrap_docs).map_err(|e| PyValueError::new_err(e.to_string()))
    }
}

// 2. Token Chunker
#[pyclass(name = "TokenChunker")]
pub struct PyTokenChunker {
    inner: TokenChunker,
}

#[pymethods]
impl PyTokenChunker {
    #[new]
    #[pyo3(signature = (chunk_size=512, overlap=50, encoding="cl100k_base"))]
    pub fn new(chunk_size: usize, overlap: usize, encoding: &str) -> PyResult<Self> {
        let enc = match encoding.to_lowercase().replace('-', "_").as_str() {
            "cl100k_base" | "cl100k" | "gpt-4" | "gpt-3.5-turbo" => TokenEncoding::Cl100kBase,
            "o200k_base" | "o200k" | "gpt-4o" => TokenEncoding::O200kBase,
            "p50k_base" | "p50k" => TokenEncoding::P50kBase,
            "r50k_base" | "r50k" => TokenEncoding::R50kBase,
            other => return Err(PyValueError::new_err(format!("Unsupported encoding: {}", other))),
        };
        let inner = TokenChunker::with_encoding(chunk_size, overlap, enc)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self { inner })
    }

    pub fn chunk(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        self.inner.chunk(text).map(wrap_docs).map_err(|e| PyValueError::new_err(e.to_string()))
    }

    pub fn count_tokens(&self, text: &str) -> usize {
        self.inner.count_tokens(text)
    }
}

// 3. Sentence Chunker
#[pyclass(name = "SentenceChunker")]
pub struct PySentenceChunker {
    inner: SentenceChunker,
}

#[pymethods]
impl PySentenceChunker {
    #[new]
    #[pyo3(signature = (sentences_per_chunk=3, overlap=1, max_characters=None))]
    pub fn new(sentences_per_chunk: usize, overlap: usize, max_characters: Option<usize>) -> Self {
        let mut chunker = SentenceChunker::new()
            .with_sentences_per_chunk(sentences_per_chunk)
            .with_sentence_overlap(overlap);
        if let Some(max_chars) = max_characters {
            chunker = chunker.with_max_characters(max_chars);
        }
        Self { inner: chunker }
    }

    pub fn chunk(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        self.inner.chunk(text).map(wrap_docs).map_err(|e| PyValueError::new_err(e.to_string()))
    }
}

// 4. Paragraph Chunker
#[pyclass(name = "ParagraphChunker")]
pub struct PyParagraphChunker {
    inner: ParagraphChunker,
}

#[pymethods]
impl PyParagraphChunker {
    #[new]
    #[pyo3(signature = (paragraphs_per_chunk=2, overlap=0))]
    pub fn new(paragraphs_per_chunk: usize, overlap: usize) -> Self {
        Self {
            inner: ParagraphChunker::new()
                .with_paragraphs_per_chunk(paragraphs_per_chunk)
                .with_paragraph_overlap(overlap),
        }
    }

    pub fn chunk(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        self.inner.chunk(text).map(wrap_docs).map_err(|e| PyValueError::new_err(e.to_string()))
    }
}

// 5. Semantic Chunker
#[pyclass(name = "SemanticChunker")]
pub struct PySemanticChunker {
    inner: SemanticChunker,
}

#[pymethods]
impl PySemanticChunker {
    #[new]
    #[pyo3(signature = (percentile=90.0, min_size=100, max_size=2000))]
    pub fn new(percentile: f32, min_size: usize, max_size: usize) -> Self {
        Self {
            inner: SemanticChunker::new()
                .with_threshold(BreakpointThreshold::Percentile(percentile))
                .with_size_bounds(min_size, max_size),
        }
    }

    pub fn chunk(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        self.inner.chunk(text).map(wrap_docs).map_err(|e| PyValueError::new_err(e.to_string()))
    }
}

// 6. Proposition Chunker
#[pyclass(name = "PropositionChunker")]
pub struct PyPropositionChunker {
    inner: PropositionChunker,
}

#[pymethods]
impl PyPropositionChunker {
    #[new]
    #[pyo3(signature = (propositions_per_chunk=1, overlap=0))]
    pub fn new(propositions_per_chunk: usize, overlap: usize) -> PyResult<Self> {
        let inner = PropositionChunker::new()
            .with_grouping(propositions_per_chunk, overlap)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self { inner })
    }

    pub fn chunk(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        self.inner.chunk(text).map(wrap_docs).map_err(|e| PyValueError::new_err(e.to_string()))
    }
}

// 7. Contextual Chunker
#[pyclass(name = "ContextualChunker")]
pub struct PyContextualChunker {
    inner: ContextualChunker,
}

#[pymethods]
impl PyContextualChunker {
    #[new]
    #[pyo3(signature = (chunk_size=1000, overlap=200, max_context_chars=200))]
    pub fn new(chunk_size: usize, overlap: usize, max_context_chars: usize) -> Self {
        Self {
            inner: ContextualChunker::new()
                .with_base_chunker(RecursiveChunker::new().with_chunk_size(chunk_size).with_overlap(overlap))
                .with_context_generator(ExtractiveContextGenerator::new().with_max_chars(max_context_chars)),
        }
    }

    pub fn chunk(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        self.inner.chunk(text).map(wrap_docs).map_err(|e| PyValueError::new_err(e.to_string()))
    }
}

// 8. Query Aware Chunker
#[pyclass(name = "QueryAwareChunker")]
pub struct PyQueryAwareChunker {
    inner: QueryAwareChunker,
}

#[pymethods]
impl PyQueryAwareChunker {
    #[new]
    #[pyo3(signature = (query, hotspot_sentences=2, hotspot_overlap=1, context_sentences=5, relevance_threshold=0.1))]
    pub fn new(
        query: &str,
        hotspot_sentences: usize,
        hotspot_overlap: usize,
        context_sentences: usize,
        relevance_threshold: f64,
    ) -> Self {
        Self {
            inner: QueryAwareChunker::new(query)
                .with_hotspot_sizing(hotspot_sentences, hotspot_overlap)
                .with_context_sizing(context_sentences, 1)
                .with_relevance_threshold(relevance_threshold),
        }
    }

    pub fn chunk(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        self.inner.chunk(text).map(wrap_docs).map_err(|e| PyValueError::new_err(e.to_string()))
    }
}

// 9. Agentic Chunker
#[pyclass(name = "AgenticChunker")]
pub struct PyAgenticChunker {
    inner: AgenticChunker,
}

#[pymethods]
impl PyAgenticChunker {
    #[new]
    #[pyo3(signature = (min_chars=150, max_chars=1200))]
    pub fn new(min_chars: usize, max_chars: usize) -> Self {
        Self {
            inner: AgenticChunker::new().with_decision_maker(
                HeuristicAgenticDecisionMaker::new().with_size_limits(min_chars, max_chars),
            ),
        }
    }

    pub fn chunk(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        self.inner.chunk(text).map(wrap_docs).map_err(|e| PyValueError::new_err(e.to_string()))
    }
}

// 10. Hierarchical Chunker
#[pyclass(name = "HierarchicalChunker")]
pub struct PyHierarchicalChunker {
    inner: HierarchicalChunker,
}

#[pymethods]
impl PyHierarchicalChunker {
    #[new]
    #[pyo3(signature = (parent_size=2000, parent_overlap=200, child_size=400, child_overlap=50, include_parents=true))]
    pub fn new(
        parent_size: usize,
        parent_overlap: usize,
        child_size: usize,
        child_overlap: usize,
        include_parents: bool,
    ) -> PyResult<Self> {
        let inner = HierarchicalChunker::with_sizes(parent_size, parent_overlap, child_size, child_overlap)
            .map_err(|e| PyValueError::new_err(e.to_string()))?
            .with_include_parents(include_parents);
        Ok(Self { inner })
    }

    pub fn chunk(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        self.inner.chunk(text).map(wrap_docs).map_err(|e| PyValueError::new_err(e.to_string()))
    }
}

// 11. Markdown Chunker
#[pyclass(name = "MarkdownChunker")]
pub struct PyMarkdownChunker {
    inner: MarkdownChunker,
}

#[pymethods]
impl PyMarkdownChunker {
    #[new]
    #[pyo3(signature = (chunk_size=1000, overlap=150))]
    pub fn new(chunk_size: usize, overlap: usize) -> Self {
        Self {
            inner: MarkdownChunker::new()
                .with_chunk_size(chunk_size)
                .with_overlap(overlap),
        }
    }

    pub fn chunk(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        self.inner.chunk(text).map(wrap_docs).map_err(|e| PyValueError::new_err(e.to_string()))
    }
}

// 12. Code Chunker
#[pyclass(name = "CodeChunker")]
pub struct PyCodeChunker {
    inner: CodeChunker,
}

#[pymethods]
impl PyCodeChunker {
    #[new]
    #[pyo3(signature = (language="rust", chunk_size=1500, overlap=200))]
    pub fn new(language: &str, chunk_size: usize, overlap: usize) -> Self {
        let lang = match language.to_lowercase().as_str() {
            "rust" | "rs" => CodeLanguage::Rust,
            "python" | "py" => CodeLanguage::Python,
            "javascript" | "js" => CodeLanguage::JavaScript,
            "typescript" | "ts" => CodeLanguage::TypeScript,
            "go" | "golang" => CodeLanguage::Go,
            "cpp" | "c++" | "c" => CodeLanguage::Cpp,
            "java" => CodeLanguage::Java,
            "html" | "htm" => CodeLanguage::Html,
            "sql" => CodeLanguage::Sql,
            "markdown" | "md" => CodeLanguage::Markdown,
            _ => CodeLanguage::Generic,
        };
        Self {
            inner: CodeChunker::new(lang)
                .with_chunk_size(chunk_size)
                .with_overlap(overlap),
        }
    }

    pub fn chunk(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        self.inner.chunk(text).map(wrap_docs).map_err(|e| PyValueError::new_err(e.to_string()))
    }
}

// 13. Fixed Char & Word Chunkers
#[pyclass(name = "CharacterChunker")]
pub struct PyCharacterChunker {
    inner: CharacterChunker,
}

#[pymethods]
impl PyCharacterChunker {
    #[new]
    #[pyo3(signature = (chunk_size=1000, overlap=200))]
    pub fn new(chunk_size: usize, overlap: usize) -> Self {
        Self {
            inner: CharacterChunker::new()
                .with_chunk_size(chunk_size)
                .with_overlap(overlap),
        }
    }

    pub fn chunk(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        self.inner.chunk(text).map(wrap_docs).map_err(|e| PyValueError::new_err(e.to_string()))
    }
}

#[pyclass(name = "WordChunker")]
pub struct PyWordChunker {
    inner: WordChunker,
}

#[pymethods]
impl PyWordChunker {
    #[new]
    #[pyo3(signature = (chunk_size=200, overlap=20))]
    pub fn new(chunk_size: usize, overlap: usize) -> Self {
        Self {
            inner: WordChunker::new()
                .with_chunk_size(chunk_size)
                .with_overlap(overlap),
        }
    }

    pub fn chunk(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        self.inner.chunk(text).map(wrap_docs).map_err(|e| PyValueError::new_err(e.to_string()))
    }
}

#[pyclass(name = "JsonChunker")]
pub struct PyJsonChunker {
    inner: JsonChunker,
}

#[pymethods]
impl PyJsonChunker {
    #[new]
    #[pyo3(signature = (max_size=1500))]
    pub fn new(max_size: usize) -> Self {
        Self {
            inner: JsonChunker::new().with_max_chunk_size(max_size),
        }
    }

    pub fn chunk(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        self.inner.chunk(text).map(wrap_docs).map_err(|e| PyValueError::new_err(e.to_string()))
    }
}

#[pyclass(name = "HtmlChunker")]
pub struct PyHtmlChunker {
    inner: HtmlChunker,
}

#[pymethods]
impl PyHtmlChunker {
    #[new]
    #[pyo3(signature = (chunk_size=1200, overlap=150))]
    pub fn new(chunk_size: usize, overlap: usize) -> Self {
        Self {
            inner: HtmlChunker::new()
                .with_chunk_size(chunk_size)
                .with_overlap(overlap),
        }
    }

    pub fn chunk(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        self.inner.chunk(text).map(wrap_docs).map_err(|e| PyValueError::new_err(e.to_string()))
    }
}

// 14. PDF Loader
#[pyclass(name = "PDFLoader")]
#[derive(Default)]
pub struct PyPDFLoader {
    inner: PDFLoader,
}

#[pymethods]
impl PyPDFLoader {
    #[new]
    pub fn new() -> Self {
        Self {
            inner: PDFLoader::new(),
        }
    }

    #[pyo3(signature = (path))]
    pub fn load(&self, path: &str) -> PyResult<String> {
        self.inner
            .load_from_file(path)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    #[pyo3(signature = (path))]
    pub fn load_from_file(&self, path: &str) -> PyResult<String> {
        self.load(path)
    }

    #[pyo3(signature = (bytes))]
    pub fn load_from_bytes(&self, bytes: &[u8]) -> PyResult<String> {
        self.inner
            .load_from_bytes(bytes)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    #[pyo3(signature = (path))]
    pub fn load_document(&self, path: &str) -> PyResult<PyDocument> {
        self.inner
            .load_document(path)
            .map(PyDocument::from)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    #[pyo3(signature = (bytes))]
    pub fn load_document_from_bytes(&self, bytes: &[u8]) -> PyResult<PyDocument> {
        self.inner
            .load_document_from_bytes(bytes)
            .map(PyDocument::from)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    #[pyo3(signature = (path))]
    pub fn load_pages(&self, path: &str) -> PyResult<Vec<PyDocument>> {
        self.inner
            .load_pages_from_file(path)
            .map(wrap_docs)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    #[pyo3(signature = (path))]
    pub fn load_pages_from_file(&self, path: &str) -> PyResult<Vec<PyDocument>> {
        self.load_pages(path)
    }

    #[pyo3(signature = (bytes))]
    pub fn load_pages_from_bytes(&self, bytes: &[u8]) -> PyResult<Vec<PyDocument>> {
        self.inner
            .load_pages_from_bytes(bytes)
            .map(wrap_docs)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }
}

#[pyfunction]
#[pyo3(signature = (path))]
pub fn load_pdf(path: &str) -> PyResult<String> {
    PDFLoader::new()
        .load_from_file(path)
        .map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pyfunction]
#[pyo3(signature = (path))]
pub fn load_pdf_pages(path: &str) -> PyResult<Vec<PyDocument>> {
    PDFLoader::new()
        .load_pages_from_file(path)
        .map(wrap_docs)
        .map_err(|e| PyValueError::new_err(e.to_string()))
}

/// The `chunkr` Python native extension module
#[pymodule]
pub fn chunkr(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyDocument>()?;
    m.add_class::<PyRecursiveChunker>()?;
    m.add_class::<PyTokenChunker>()?;
    m.add_class::<PySentenceChunker>()?;
    m.add_class::<PyParagraphChunker>()?;
    m.add_class::<PySemanticChunker>()?;
    m.add_class::<PyPropositionChunker>()?;
    m.add_class::<PyContextualChunker>()?;
    m.add_class::<PyQueryAwareChunker>()?;
    m.add_class::<PyAgenticChunker>()?;
    m.add_class::<PyHierarchicalChunker>()?;
    m.add_class::<PyMarkdownChunker>()?;
    m.add_class::<PyCodeChunker>()?;
    m.add_class::<PyJsonChunker>()?;
    m.add_class::<PyHtmlChunker>()?;
    m.add_class::<PyCharacterChunker>()?;
    m.add_class::<PyWordChunker>()?;
    m.add_class::<PyPDFLoader>()?;
    m.add_function(wrap_pyfunction!(load_pdf, m)?)?;
    m.add_function(wrap_pyfunction!(load_pdf_pages, m)?)?;
    Ok(())
}
