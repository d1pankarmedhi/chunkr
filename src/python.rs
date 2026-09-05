// NOTE: `clippy::useless_conversion` fires on every `#[pyfunction]` returning
// `PyResult` under current clippy — a known pyo3 0.22 macro-expansion false
// positive (the flagged `.into()` lives in generated wrapper code, not here).
// Scoped allow for this PyO3 boundary module only; revisit on pyo3 upgrade.
// `-D warnings` still applies to everything else in the crate.
#![allow(clippy::useless_conversion)]

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString};
use pyo3::IntoPyObjectExt; // .into_py_any(): pyo3 0.23+ replacement for removed .to_object()

use crate::prelude::*;

fn json_to_py(py: Python, val: &serde_json::Value) -> PyResult<PyObject> {
    match val {
        serde_json::Value::Null => Ok(py.None()),
        serde_json::Value::Bool(b) => Ok(b.into_py_any(py)?),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.into_py_any(py)?)
            } else if let Some(u) = n.as_u64() {
                Ok(u.into_py_any(py)?)
            } else if let Some(f) = n.as_f64() {
                Ok(f.into_py_any(py)?)
            } else {
                Ok(py.None())
            }
        }
        serde_json::Value::String(s) => Ok(s.into_py_any(py)?),
        serde_json::Value::Array(arr) => {
            let list = PyList::empty(py);
            for item in arr {
                list.append(json_to_py(py, item)?)?;
            }
            Ok(list.into())
        }
        serde_json::Value::Object(map) => {
            let dict = PyDict::new(py);
            for (k, v) in map {
                dict.set_item(k, json_to_py(py, v)?)?;
            }
            Ok(dict.into())
        }
    }
}

fn py_to_json(val: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    if val.is_none() {
        Ok(serde_json::Value::Null)
    } else if let Ok(b) = val.downcast::<PyBool>() {
        Ok(serde_json::Value::Bool(b.is_true()))
    } else if let Ok(i) = val.downcast::<PyInt>() {
        let n: i64 = i.extract()?;
        Ok(serde_json::Value::Number(n.into()))
    } else if let Ok(f) = val.downcast::<PyFloat>() {
        let fl: f64 = f.extract()?;
        if let Some(num) = serde_json::Number::from_f64(fl) {
            Ok(serde_json::Value::Number(num))
        } else {
            Ok(serde_json::Value::Null)
        }
    } else if let Ok(s) = val.downcast::<PyString>() {
        let st: String = s.extract()?;
        Ok(serde_json::Value::String(st))
    } else if let Ok(dict) = val.downcast::<PyDict>() {
        let mut map = serde_json::Map::new();
        for (k, v) in dict.iter() {
            let key_str: String = k.extract()?;
            map.insert(key_str, py_to_json(&v)?);
        }
        Ok(serde_json::Value::Object(map))
    } else if let Ok(list) = val.downcast::<PyList>() {
        let mut arr = Vec::with_capacity(list.len());
        for item in list.iter() {
            arr.push(py_to_json(&item)?);
        }
        Ok(serde_json::Value::Array(arr))
    } else {
        let s = val.to_string();
        Ok(serde_json::Value::String(s))
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
    pub fn new(content: String, metadata: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        let mut inner = Document::from_text(content);
        if let Some(meta) = metadata {
            for (k, v) in meta.iter() {
                let key: String = k.extract()?;
                let val = py_to_json(&v)?;
                inner.add_metadata(key, val);
            }
        }
        Ok(Self { inner })
    }

    #[getter]
    pub fn content(&self) -> String {
        self.inner.content.clone()
    }

    #[getter]
    pub fn metadata(&self, py: Python) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        for (k, v) in &self.inner.metadata {
            dict.set_item(k, json_to_py(py, v)?)?;
        }
        Ok(dict.into())
    }

    pub fn to_dict(&self, py: Python) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        dict.set_item("content", &self.inner.content)?;
        dict.set_item("metadata", self.metadata(py)?)?;
        Ok(dict.into())
    }

    pub fn to_langchain(&self, py: Python) -> PyResult<PyObject> {
        let lc_mod = py.import("langchain_core.documents")
            .or_else(|_| py.import("langchain.schema"))
            .map_err(|_| PyValueError::new_err("Could not import langchain_core.documents or langchain.schema. Ensure langchain is installed."))?;
        let doc_cls = lc_mod.getattr("Document")?;
        let dict = PyDict::new(py);
        dict.set_item("page_content", &self.inner.content)?;
        dict.set_item("metadata", self.metadata(py)?)?;
        let obj = doc_cls.call((), Some(&dict))?;
        Ok(obj.into())
    }

    #[staticmethod]
    pub fn from_langchain(doc: &Bound<'_, PyAny>) -> PyResult<Self> {
        let content: String = doc.getattr("page_content")?.extract()?;
        let mut inner = Document::from_text(content);
        if let Ok(meta_obj) = doc.getattr("metadata") {
            if let Ok(dict) = meta_obj.downcast::<PyDict>() {
                for (k, v) in dict.iter() {
                    let key: String = k.extract()?;
                    let val = py_to_json(&v)?;
                    inner.add_metadata(key, val);
                }
            }
        }
        Ok(Self { inner })
    }

    pub fn to_llamaindex(&self, py: Python) -> PyResult<PyObject> {
        let li_mod = py
            .import("llama_index.core.schema")
            .or_else(|_| py.import("llama_index.schema"))
            .map_err(|_| {
                PyValueError::new_err(
                    "Could not import llama_index.core.schema. Ensure llama-index is installed.",
                )
            })?;
        let node_cls = li_mod.getattr("TextNode")?;
        let dict = PyDict::new(py);
        dict.set_item("text", &self.inner.content)?;
        dict.set_item("metadata", self.metadata(py)?)?;
        let obj = node_cls.call((), Some(&dict))?;
        Ok(obj.into())
    }

    #[staticmethod]
    pub fn from_llamaindex(node: &Bound<'_, PyAny>) -> PyResult<Self> {
        let content: String = if let Ok(t) = node.getattr("text") {
            t.extract()?
        } else if let Ok(t) = node.call_method0("get_content") {
            t.extract()?
        } else {
            return Err(PyValueError::new_err(
                "Expected LlamaIndex node with 'text' attribute or get_content() method",
            ));
        };
        let mut inner = Document::from_text(content);
        if let Ok(meta_obj) = node.getattr("metadata") {
            if let Ok(dict) = meta_obj.downcast::<PyDict>() {
                for (k, v) in dict.iter() {
                    let key: String = k.extract()?;
                    let val = py_to_json(&v)?;
                    inner.add_metadata(key, val);
                }
            }
        }
        Ok(Self { inner })
    }

    pub fn __repr__(&self) -> String {
        let preview: String = self.inner.content.chars().take(50).collect();
        format!(
            "Document(content='{}...', len={})",
            preview.replace('\n', " "),
            self.inner.content.len()
        )
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

fn chunk_docs_helper<C: Chunker>(
    chunker: &C,
    docs: Vec<PyRef<'_, PyDocument>>,
) -> PyResult<Vec<PyDocument>> {
    let rust_docs: Vec<Document> = docs.iter().map(|d| d.inner.clone()).collect();
    chunker
        .chunk_documents(&rust_docs)
        .map(wrap_docs)
        .map_err(|e| PyValueError::new_err(e.to_string()))
}

fn par_chunk_docs_helper<C: Chunker>(
    chunker: &C,
    docs: Vec<PyRef<'_, PyDocument>>,
) -> PyResult<Vec<PyDocument>> {
    let rust_docs: Vec<Document> = docs.iter().map(|d| d.inner.clone()).collect();
    chunker
        .par_chunk_documents(&rust_docs)
        .map(wrap_docs)
        .map_err(|e| PyValueError::new_err(e.to_string()))
}

fn par_chunk_texts_helper<C: Chunker>(
    chunker: &C,
    texts: Vec<String>,
) -> PyResult<Vec<Vec<PyDocument>>> {
    let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
    let res = chunker
        .par_chunk_texts(&text_refs)
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    Ok(res.into_iter().map(wrap_docs).collect())
}

fn node_to_py(py: Python, node: &HierarchyNode) -> PyResult<PyObject> {
    let dict = PyDict::new(py);
    dict.set_item("id", &node.id)?;
    if let Some(ref pid) = node.parent_id {
        dict.set_item("parent_id", pid)?;
    } else {
        dict.set_item("parent_id", py.None())?;
    }
    dict.set_item("depth", node.depth)?;
    dict.set_item(
        "document",
        Py::new(py, PyDocument::from(node.document.clone()))?,
    )?;
    let children = PyList::empty(py);
    for child in &node.children {
        children.append(node_to_py(py, child)?)?;
    }
    dict.set_item("children", children)?;
    Ok(dict.into())
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
        self.inner
            .chunk(text)
            .map(wrap_docs)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    pub fn chunk_documents(&self, docs: Vec<PyRef<'_, PyDocument>>) -> PyResult<Vec<PyDocument>> {
        chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_documents(
        &self,
        docs: Vec<PyRef<'_, PyDocument>>,
    ) -> PyResult<Vec<PyDocument>> {
        par_chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_texts(&self, texts: Vec<String>) -> PyResult<Vec<Vec<PyDocument>>> {
        par_chunk_texts_helper(&self.inner, texts)
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
            other => {
                return Err(PyValueError::new_err(format!(
                    "Unsupported encoding: {}",
                    other
                )))
            }
        };
        let inner = TokenChunker::with_encoding(chunk_size, overlap, enc)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self { inner })
    }

    pub fn chunk(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        self.inner
            .chunk(text)
            .map(wrap_docs)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    pub fn chunk_documents(&self, docs: Vec<PyRef<'_, PyDocument>>) -> PyResult<Vec<PyDocument>> {
        chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_documents(
        &self,
        docs: Vec<PyRef<'_, PyDocument>>,
    ) -> PyResult<Vec<PyDocument>> {
        par_chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_texts(&self, texts: Vec<String>) -> PyResult<Vec<Vec<PyDocument>>> {
        par_chunk_texts_helper(&self.inner, texts)
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
        self.inner
            .chunk(text)
            .map(wrap_docs)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    pub fn chunk_documents(&self, docs: Vec<PyRef<'_, PyDocument>>) -> PyResult<Vec<PyDocument>> {
        chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_documents(
        &self,
        docs: Vec<PyRef<'_, PyDocument>>,
    ) -> PyResult<Vec<PyDocument>> {
        par_chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_texts(&self, texts: Vec<String>) -> PyResult<Vec<Vec<PyDocument>>> {
        par_chunk_texts_helper(&self.inner, texts)
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
        self.inner
            .chunk(text)
            .map(wrap_docs)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    pub fn chunk_documents(&self, docs: Vec<PyRef<'_, PyDocument>>) -> PyResult<Vec<PyDocument>> {
        chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_documents(
        &self,
        docs: Vec<PyRef<'_, PyDocument>>,
    ) -> PyResult<Vec<PyDocument>> {
        par_chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_texts(&self, texts: Vec<String>) -> PyResult<Vec<Vec<PyDocument>>> {
        par_chunk_texts_helper(&self.inner, texts)
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
    #[pyo3(signature = (percentile=90.0, min_size=100, max_size=2000, buffer_size=1))]
    pub fn new(percentile: f32, min_size: usize, max_size: usize, buffer_size: usize) -> Self {
        Self {
            inner: SemanticChunker::new()
                .with_threshold(BreakpointThreshold::Percentile(percentile))
                .with_size_bounds(min_size, max_size)
                .with_buffer_size(buffer_size),
        }
    }

    pub fn chunk(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        self.inner
            .chunk(text)
            .map(wrap_docs)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    pub fn chunk_documents(&self, docs: Vec<PyRef<'_, PyDocument>>) -> PyResult<Vec<PyDocument>> {
        chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_documents(
        &self,
        docs: Vec<PyRef<'_, PyDocument>>,
    ) -> PyResult<Vec<PyDocument>> {
        par_chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_texts(&self, texts: Vec<String>) -> PyResult<Vec<Vec<PyDocument>>> {
        par_chunk_texts_helper(&self.inner, texts)
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
        self.inner
            .chunk(text)
            .map(wrap_docs)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    pub fn chunk_documents(&self, docs: Vec<PyRef<'_, PyDocument>>) -> PyResult<Vec<PyDocument>> {
        chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_documents(
        &self,
        docs: Vec<PyRef<'_, PyDocument>>,
    ) -> PyResult<Vec<PyDocument>> {
        par_chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_texts(&self, texts: Vec<String>) -> PyResult<Vec<Vec<PyDocument>>> {
        par_chunk_texts_helper(&self.inner, texts)
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
                .with_base_chunker(
                    RecursiveChunker::new()
                        .with_chunk_size(chunk_size)
                        .with_overlap(overlap),
                )
                .with_context_generator(
                    ExtractiveContextGenerator::new().with_max_chars(max_context_chars),
                ),
        }
    }

    pub fn chunk(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        self.inner
            .chunk(text)
            .map(wrap_docs)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    pub fn chunk_documents(&self, docs: Vec<PyRef<'_, PyDocument>>) -> PyResult<Vec<PyDocument>> {
        chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_documents(
        &self,
        docs: Vec<PyRef<'_, PyDocument>>,
    ) -> PyResult<Vec<PyDocument>> {
        par_chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_texts(&self, texts: Vec<String>) -> PyResult<Vec<Vec<PyDocument>>> {
        par_chunk_texts_helper(&self.inner, texts)
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
        self.inner
            .chunk(text)
            .map(wrap_docs)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    pub fn chunk_documents(&self, docs: Vec<PyRef<'_, PyDocument>>) -> PyResult<Vec<PyDocument>> {
        chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_documents(
        &self,
        docs: Vec<PyRef<'_, PyDocument>>,
    ) -> PyResult<Vec<PyDocument>> {
        par_chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_texts(&self, texts: Vec<String>) -> PyResult<Vec<Vec<PyDocument>>> {
        par_chunk_texts_helper(&self.inner, texts)
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
        self.inner
            .chunk(text)
            .map(wrap_docs)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    pub fn chunk_documents(&self, docs: Vec<PyRef<'_, PyDocument>>) -> PyResult<Vec<PyDocument>> {
        chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_documents(
        &self,
        docs: Vec<PyRef<'_, PyDocument>>,
    ) -> PyResult<Vec<PyDocument>> {
        par_chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_texts(&self, texts: Vec<String>) -> PyResult<Vec<Vec<PyDocument>>> {
        par_chunk_texts_helper(&self.inner, texts)
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
        let inner =
            HierarchicalChunker::with_sizes(parent_size, parent_overlap, child_size, child_overlap)
                .map_err(|e| PyValueError::new_err(e.to_string()))?
                .with_include_parents(include_parents);
        Ok(Self { inner })
    }

    pub fn chunk(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        self.inner
            .chunk(text)
            .map(wrap_docs)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    pub fn chunk_hierarchical(&self, py: Python, text: &str) -> PyResult<PyObject> {
        let pairs = self
            .inner
            .chunk_hierarchical(text)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        let list = PyList::empty(py);
        for pair in pairs {
            let dict = PyDict::new(py);
            dict.set_item("parent", Py::new(py, PyDocument::from(pair.parent))?)?;
            let children = PyList::empty(py);
            for child in pair.children {
                children.append(Py::new(py, PyDocument::from(child))?)?;
            }
            dict.set_item("children", children)?;
            list.append(dict)?;
        }
        Ok(list.into())
    }

    pub fn chunk_tree(&self, py: Python, text: &str) -> PyResult<PyObject> {
        let root = self
            .inner
            .chunk_tree(text)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        node_to_py(py, &root)
    }

    pub fn chunk_documents(&self, docs: Vec<PyRef<'_, PyDocument>>) -> PyResult<Vec<PyDocument>> {
        chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_documents(
        &self,
        docs: Vec<PyRef<'_, PyDocument>>,
    ) -> PyResult<Vec<PyDocument>> {
        par_chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_texts(&self, texts: Vec<String>) -> PyResult<Vec<Vec<PyDocument>>> {
        par_chunk_texts_helper(&self.inner, texts)
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
    #[pyo3(signature = (chunk_size=1000, overlap=150, include_header_in_content=true))]
    pub fn new(chunk_size: usize, overlap: usize, include_header_in_content: bool) -> Self {
        Self {
            inner: MarkdownChunker::new()
                .with_chunk_size(chunk_size)
                .with_overlap(overlap)
                .with_include_header_in_content(include_header_in_content),
        }
    }

    pub fn chunk(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        self.inner
            .chunk(text)
            .map(wrap_docs)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    pub fn chunk_documents(&self, docs: Vec<PyRef<'_, PyDocument>>) -> PyResult<Vec<PyDocument>> {
        chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_documents(
        &self,
        docs: Vec<PyRef<'_, PyDocument>>,
    ) -> PyResult<Vec<PyDocument>> {
        par_chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_texts(&self, texts: Vec<String>) -> PyResult<Vec<Vec<PyDocument>>> {
        par_chunk_texts_helper(&self.inner, texts)
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
        self.inner
            .chunk(text)
            .map(wrap_docs)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    pub fn chunk_documents(&self, docs: Vec<PyRef<'_, PyDocument>>) -> PyResult<Vec<PyDocument>> {
        chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_documents(
        &self,
        docs: Vec<PyRef<'_, PyDocument>>,
    ) -> PyResult<Vec<PyDocument>> {
        par_chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_texts(&self, texts: Vec<String>) -> PyResult<Vec<Vec<PyDocument>>> {
        par_chunk_texts_helper(&self.inner, texts)
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
        self.inner
            .chunk(text)
            .map(wrap_docs)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    pub fn chunk_documents(&self, docs: Vec<PyRef<'_, PyDocument>>) -> PyResult<Vec<PyDocument>> {
        chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_documents(
        &self,
        docs: Vec<PyRef<'_, PyDocument>>,
    ) -> PyResult<Vec<PyDocument>> {
        par_chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_texts(&self, texts: Vec<String>) -> PyResult<Vec<Vec<PyDocument>>> {
        par_chunk_texts_helper(&self.inner, texts)
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
        self.inner
            .chunk(text)
            .map(wrap_docs)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    pub fn chunk_documents(&self, docs: Vec<PyRef<'_, PyDocument>>) -> PyResult<Vec<PyDocument>> {
        chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_documents(
        &self,
        docs: Vec<PyRef<'_, PyDocument>>,
    ) -> PyResult<Vec<PyDocument>> {
        par_chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_texts(&self, texts: Vec<String>) -> PyResult<Vec<Vec<PyDocument>>> {
        par_chunk_texts_helper(&self.inner, texts)
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
        self.inner
            .chunk(text)
            .map(wrap_docs)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    pub fn chunk_documents(&self, docs: Vec<PyRef<'_, PyDocument>>) -> PyResult<Vec<PyDocument>> {
        chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_documents(
        &self,
        docs: Vec<PyRef<'_, PyDocument>>,
    ) -> PyResult<Vec<PyDocument>> {
        par_chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_texts(&self, texts: Vec<String>) -> PyResult<Vec<Vec<PyDocument>>> {
        par_chunk_texts_helper(&self.inner, texts)
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
        self.inner
            .chunk(text)
            .map(wrap_docs)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    pub fn chunk_documents(&self, docs: Vec<PyRef<'_, PyDocument>>) -> PyResult<Vec<PyDocument>> {
        chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_documents(
        &self,
        docs: Vec<PyRef<'_, PyDocument>>,
    ) -> PyResult<Vec<PyDocument>> {
        par_chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_texts(&self, texts: Vec<String>) -> PyResult<Vec<Vec<PyDocument>>> {
        par_chunk_texts_helper(&self.inner, texts)
    }
}

#[pyclass(name = "TableChunker")]
pub struct PyTableChunker {
    inner: TableChunker,
}

#[pymethods]
impl PyTableChunker {
    #[new]
    #[pyo3(signature = (chunk_size=1000, rows_per_chunk=None, overlap_rows=1, format="auto"))]
    pub fn new(
        chunk_size: usize,
        rows_per_chunk: Option<usize>,
        overlap_rows: usize,
        format: &str,
    ) -> PyResult<Self> {
        let fmt = match format.to_lowercase().as_str() {
            "auto" => TableFormat::Auto,
            "markdown" | "md" => TableFormat::Markdown,
            "csv" => TableFormat::Csv,
            "tsv" => TableFormat::Tsv,
            other => {
                return Err(PyValueError::new_err(format!(
                    "Unknown table format '{}'. Supported formats: 'auto', 'markdown', 'csv', 'tsv'",
                    other
                )));
            }
        };

        Ok(Self {
            inner: TableChunker::new()
                .with_chunk_size(chunk_size)
                .with_rows_per_chunk(rows_per_chunk)
                .with_overlap_rows(overlap_rows)
                .with_format(fmt),
        })
    }

    pub fn chunk(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        self.inner
            .chunk(text)
            .map(wrap_docs)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    pub fn chunk_documents(&self, docs: Vec<PyRef<'_, PyDocument>>) -> PyResult<Vec<PyDocument>> {
        chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_documents(
        &self,
        docs: Vec<PyRef<'_, PyDocument>>,
    ) -> PyResult<Vec<PyDocument>> {
        par_chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_texts(&self, texts: Vec<String>) -> PyResult<Vec<Vec<PyDocument>>> {
        par_chunk_texts_helper(&self.inner, texts)
    }
}

#[pyclass(name = "LateChunker")]
pub struct PyLateChunker {
    inner: LateChunker,
}

#[pymethods]
impl PyLateChunker {
    #[new]
    #[pyo3(signature = (encoding="cl100k_base", chunk_size=500, overlap=50, normalize=true))]
    pub fn new(
        encoding: &str,
        chunk_size: usize,
        overlap: usize,
        normalize: bool,
    ) -> PyResult<Self> {
        let enc = match encoding.to_lowercase().as_str() {
            "cl100k_base" => TokenEncoding::Cl100kBase,
            "o200k_base" => TokenEncoding::O200kBase,
            "p50k_base" => TokenEncoding::P50kBase,
            "r50k_base" => TokenEncoding::R50kBase,
            other => {
                return Err(PyValueError::new_err(format!(
                    "Unsupported encoding '{}'. Supported: cl100k_base, o200k_base, p50k_base, r50k_base",
                    other
                )));
            }
        };

        let base = RecursiveChunker::new()
            .with_chunk_size(chunk_size)
            .with_overlap(overlap);

        let inner = LateChunker::new()
            .with_encoding(enc)
            .map_err(|e| PyValueError::new_err(e.to_string()))?
            .with_base_chunker(base)
            .with_normalize(normalize);

        Ok(Self { inner })
    }

    pub fn chunk(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        self.inner
            .chunk(text)
            .map(wrap_docs)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    pub fn chunk_spans(&self, text: &str) -> PyResult<Vec<(PyDocument, (usize, usize))>> {
        let pairs = self
            .inner
            .chunk_spans(text)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(pairs
            .into_iter()
            .map(|(doc, span)| (PyDocument::from(doc), span))
            .collect())
    }

    pub fn pool_span(&self, token_embeddings: Vec<Vec<f32>>, start: usize, end: usize) -> Vec<f32> {
        LateChunker::pool_span(&token_embeddings, start, end, self.inner.normalize)
    }

    pub fn pool_embeddings(
        &self,
        token_embeddings: Vec<Vec<f32>>,
        chunks: Vec<PyRef<'_, PyDocument>>,
    ) -> Vec<Vec<f32>> {
        let docs: Vec<Document> = chunks.iter().map(|c| c.inner.clone()).collect();
        self.inner.pool_embeddings(&token_embeddings, &docs)
    }

    pub fn pool_spans(
        &self,
        token_embeddings: Vec<Vec<f32>>,
        spans: Vec<(usize, usize)>,
    ) -> Vec<Vec<f32>> {
        self.inner.pool_spans(&token_embeddings, &spans)
    }

    pub fn chunk_documents(&self, docs: Vec<PyRef<'_, PyDocument>>) -> PyResult<Vec<PyDocument>> {
        chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_documents(
        &self,
        docs: Vec<PyRef<'_, PyDocument>>,
    ) -> PyResult<Vec<PyDocument>> {
        par_chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_texts(&self, texts: Vec<String>) -> PyResult<Vec<Vec<PyDocument>>> {
        par_chunk_texts_helper(&self.inner, texts)
    }
}

#[pyclass(name = "HFTokenChunker")]
pub struct PyHFTokenChunker {
    inner: HFTokenChunker,
}

#[pymethods]
impl PyHFTokenChunker {
    #[new]
    #[pyo3(signature = (json_or_path, is_file=false, chunk_size=512, overlap=50))]
    pub fn new(
        json_or_path: &str,
        is_file: bool,
        chunk_size: usize,
        overlap: usize,
    ) -> PyResult<Self> {
        let inner = if is_file {
            HFTokenChunker::from_file(json_or_path, chunk_size, overlap)
        } else {
            HFTokenChunker::from_json(json_or_path, chunk_size, overlap)
        }
        .map_err(|e| PyValueError::new_err(e.to_string()))?;

        Ok(Self { inner })
    }

    #[staticmethod]
    #[pyo3(signature = (path, chunk_size=512, overlap=50))]
    pub fn from_file(path: &str, chunk_size: usize, overlap: usize) -> PyResult<Self> {
        HFTokenChunker::from_file(path, chunk_size, overlap)
            .map(|inner| Self { inner })
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    #[staticmethod]
    #[pyo3(signature = (json_str, chunk_size=512, overlap=50))]
    pub fn from_json(json_str: &str, chunk_size: usize, overlap: usize) -> PyResult<Self> {
        HFTokenChunker::from_json(json_str, chunk_size, overlap)
            .map(|inner| Self { inner })
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    #[staticmethod]
    #[pyo3(signature = (tokenizer, chunk_size=512, overlap=50))]
    pub fn from_tokenizer(
        tokenizer: &Bound<'_, PyAny>,
        chunk_size: usize,
        overlap: usize,
    ) -> PyResult<Self> {
        let json_str: String = if let Ok(s) = tokenizer.call_method0("to_str") {
            s.extract()?
        } else {
            return Err(PyValueError::new_err(
                "Expected Hugging Face tokenizer with a to_str() method or a JSON string",
            ));
        };
        Self::from_json(&json_str, chunk_size, overlap)
    }

    pub fn chunk(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        self.inner
            .chunk(text)
            .map(wrap_docs)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    pub fn count_tokens(&self, text: &str) -> PyResult<usize> {
        self.inner
            .count_tokens(text)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    pub fn chunk_documents(&self, docs: Vec<PyRef<'_, PyDocument>>) -> PyResult<Vec<PyDocument>> {
        chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_documents(
        &self,
        docs: Vec<PyRef<'_, PyDocument>>,
    ) -> PyResult<Vec<PyDocument>> {
        par_chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_texts(&self, texts: Vec<String>) -> PyResult<Vec<Vec<PyDocument>>> {
        par_chunk_texts_helper(&self.inner, texts)
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

// 15. Directory Loader
#[pyclass(name = "DirectoryLoader")]
pub struct PyDirectoryLoader {
    inner: DirectoryLoader,
}

#[pymethods]
impl PyDirectoryLoader {
    #[new]
    #[pyo3(signature = (recursive=true, extensions=None, excludes=None, chunk_size=1000, overlap=150))]
    pub fn new(
        recursive: bool,
        extensions: Option<Vec<String>>,
        excludes: Option<Vec<String>>,
        chunk_size: usize,
        overlap: usize,
    ) -> Self {
        let mut loader = DirectoryLoader::new()
            .with_recursive(recursive)
            .with_chunk_size(chunk_size)
            .with_overlap(overlap);

        if let Some(exts) = extensions {
            loader = loader.with_extensions(exts);
        }
        if let Some(excl) = excludes {
            loader = loader.with_excludes(excl);
        }

        Self { inner: loader }
    }

    #[pyo3(signature = (path))]
    pub fn load(&self, path: &str) -> PyResult<Vec<PyDocument>> {
        self.inner
            .load_files(path)
            .map(wrap_docs)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    #[pyo3(signature = (path))]
    pub fn load_and_chunk(&self, path: &str) -> PyResult<Vec<PyDocument>> {
        self.inner
            .load_and_chunk(path)
            .map(wrap_docs)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    #[pyo3(signature = (path))]
    pub fn load_files_lenient(&self, path: &str) -> (Vec<PyDocument>, Vec<(String, String)>) {
        let (docs, errors) = self.inner.load_files_lenient(path);
        let docs = wrap_docs(docs);
        let errors = errors
            .into_iter()
            .map(|(path, err)| (path.display().to_string(), err.to_string()))
            .collect();
        (docs, errors)
    }

    #[pyo3(signature = (path))]
    pub fn load_and_chunk_lenient(&self, path: &str) -> (Vec<PyDocument>, Vec<(String, String)>) {
        let (docs, errors) = self.inner.load_and_chunk_lenient(path);
        let docs = wrap_docs(docs);
        let errors = errors
            .into_iter()
            .map(|(path, err)| (path.display().to_string(), err.to_string()))
            .collect();
        (docs, errors)
    }
}

// 16. AST Code Chunker
#[pyclass(name = "AstCodeChunker")]
pub struct PyAstCodeChunker {
    inner: AstCodeChunker,
}

#[pymethods]
impl PyAstCodeChunker {
    #[new]
    #[pyo3(signature = (language="rust", max_chunk_size=1500))]
    pub fn new(language: &str, max_chunk_size: usize) -> PyResult<Self> {
        let lang = match language.to_lowercase().as_str() {
            "rust" | "rs" => AstLanguage::Rust,
            "python" | "py" => AstLanguage::Python,
            other => {
                return Err(PyValueError::new_err(format!(
                    "Unsupported AST language '{}'. Supported languages are 'rust' and 'python'",
                    other
                )))
            }
        };

        Ok(Self {
            inner: AstCodeChunker::new(lang).with_max_chunk_size(max_chunk_size),
        })
    }

    pub fn chunk(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        self.inner
            .chunk(text)
            .map(wrap_docs)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    pub fn chunk_documents(&self, docs: Vec<PyRef<'_, PyDocument>>) -> PyResult<Vec<PyDocument>> {
        chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_documents(
        &self,
        docs: Vec<PyRef<'_, PyDocument>>,
    ) -> PyResult<Vec<PyDocument>> {
        par_chunk_docs_helper(&self.inner, docs)
    }

    pub fn par_chunk_texts(&self, texts: Vec<String>) -> PyResult<Vec<Vec<PyDocument>>> {
        par_chunk_texts_helper(&self.inner, texts)
    }
}

// 17. Chunk Packer
#[pyclass(name = "ChunkPacker")]
pub struct PyChunkPacker {
    inner: ChunkPacker,
}

#[pymethods]
impl PyChunkPacker {
    #[new]
    #[pyo3(signature = (max_characters=1000, separator="\n\n"))]
    pub fn new(max_characters: usize, separator: &str) -> Self {
        Self {
            inner: ChunkPacker::new(max_characters).with_separator(separator),
        }
    }

    pub fn pack(&self, docs: Vec<PyRef<'_, PyDocument>>) -> Vec<PyDocument> {
        let unwrap_docs: Vec<Document> = docs.iter().map(|d| d.inner.clone()).collect();
        let packed = self.inner.pack(&unwrap_docs);
        wrap_docs(packed)
    }
}

// 18. Chunk Pipeline
#[pyclass(name = "ChunkPipeline")]
#[derive(Default)]
pub struct PyChunkPipeline {
    inner: ChunkPipeline,
}

#[pymethods]
impl PyChunkPipeline {
    #[new]
    pub fn new() -> Self {
        Self {
            inner: ChunkPipeline::new(),
        }
    }

    #[pyo3(signature = (min_chars))]
    pub fn filter_min_chars(mut slf: PyRefMut<'_, Self>, min_chars: usize) -> PyRefMut<'_, Self> {
        slf.inner = slf.inner.clone().filter_min_characters(min_chars);
        slf
    }

    #[pyo3(signature = (max_chars))]
    pub fn filter_max_chars(mut slf: PyRefMut<'_, Self>, max_chars: usize) -> PyRefMut<'_, Self> {
        slf.inner = slf.inner.clone().filter_max_characters(max_chars);
        slf
    }

    #[pyo3(signature = (min_words))]
    pub fn filter_min_words(mut slf: PyRefMut<'_, Self>, min_words: usize) -> PyRefMut<'_, Self> {
        slf.inner = slf.inner.clone().filter_min_words(min_words);
        slf
    }

    #[pyo3(signature = (ratio))]
    pub fn filter_min_alpha_ratio(mut slf: PyRefMut<'_, Self>, ratio: f32) -> PyRefMut<'_, Self> {
        slf.inner = slf.inner.clone().filter_min_alpha_ratio(ratio);
        slf
    }

    #[pyo3(signature = (exact=true, case_sensitive=true))]
    pub fn deduplicate(
        mut slf: PyRefMut<'_, Self>,
        exact: bool,
        case_sensitive: bool,
    ) -> PyRefMut<'_, Self> {
        slf.inner = if exact {
            slf.inner.clone().deduplicate_exact(case_sensitive)
        } else {
            slf.inner.clone().deduplicate_normalized(case_sensitive)
        };
        slf
    }

    #[pyo3(signature = (max_characters))]
    pub fn pack(mut slf: PyRefMut<'_, Self>, max_characters: usize) -> PyRefMut<'_, Self> {
        slf.inner = slf.inner.clone().pack(max_characters);
        slf
    }

    #[pyo3(signature = (id_prefix=None))]
    pub fn enrich(mut slf: PyRefMut<'_, Self>, id_prefix: Option<String>) -> PyRefMut<'_, Self> {
        let mut p = slf.inner.clone().enrich_metadata();
        if let Some(prefix) = id_prefix {
            p = p.with_id_prefix(prefix);
        }
        slf.inner = p;
        slf
    }

    pub fn process(&self, docs: Vec<PyRef<'_, PyDocument>>) -> Vec<PyDocument> {
        let unwrap_docs: Vec<Document> = docs.iter().map(|d| d.inner.clone()).collect();
        let processed = self.inner.process(unwrap_docs);
        wrap_docs(processed)
    }

    pub fn par_process(&self, docs: Vec<PyRef<'_, PyDocument>>) -> Vec<PyDocument> {
        let unwrap_docs: Vec<Document> = docs.iter().map(|d| d.inner.clone()).collect();
        let processed = self.inner.par_process(unwrap_docs);
        wrap_docs(processed)
    }
}

// 19. Stream Chunker
#[pyclass(name = "StreamChunker")]
pub struct PyStreamChunker {
    inner: StreamChunker,
}

#[pymethods]
impl PyStreamChunker {
    #[new]
    #[pyo3(signature = (chunk_size=1000, overlap=150))]
    pub fn new(chunk_size: usize, overlap: usize) -> PyResult<Self> {
        let inner = StreamChunker::new(chunk_size, overlap)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self { inner })
    }

    #[pyo3(signature = (path))]
    pub fn chunk_file(&self, path: &str) -> PyResult<Vec<PyDocument>> {
        let iter = self
            .inner
            .chunk_file(path)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        let mut docs = Vec::new();
        for item in iter {
            let doc = item.map_err(|e| PyValueError::new_err(e.to_string()))?;
            docs.push(PyDocument::from(doc));
        }
        Ok(docs)
    }

    #[pyo3(signature = (text))]
    pub fn chunk_text(&self, text: &str) -> PyResult<Vec<PyDocument>> {
        let cursor = std::io::Cursor::new(text);
        let iter = self.inner.chunk_reader(cursor);
        let mut docs = Vec::new();
        for item in iter {
            let doc = item.map_err(|e| PyValueError::new_err(e.to_string()))?;
            docs.push(PyDocument::from(doc));
        }
        Ok(docs)
    }

    #[pyo3(signature = (lines))]
    pub fn chunk_lines(&self, lines: Vec<String>) -> PyResult<Vec<PyDocument>> {
        let combined = lines.join("\n");
        self.chunk_text(&combined)
    }
}

#[pyfunction]
#[pyo3(signature = (docs))]
pub fn to_langchain(py: Python, docs: Vec<PyRef<'_, PyDocument>>) -> PyResult<PyObject> {
    let list = PyList::empty(py);
    for doc in docs {
        list.append(doc.to_langchain(py)?)?;
    }
    Ok(list.into())
}

#[pyfunction]
#[pyo3(signature = (docs))]
pub fn from_langchain(docs: &Bound<'_, PyList>) -> PyResult<Vec<PyDocument>> {
    let mut result = Vec::with_capacity(docs.len());
    for item in docs.iter() {
        result.push(PyDocument::from_langchain(&item)?);
    }
    Ok(result)
}

#[pyfunction]
#[pyo3(signature = (docs))]
pub fn to_llamaindex(py: Python, docs: Vec<PyRef<'_, PyDocument>>) -> PyResult<PyObject> {
    let list = PyList::empty(py);
    for doc in docs {
        list.append(doc.to_llamaindex(py)?)?;
    }
    Ok(list.into())
}

#[pyfunction]
#[pyo3(signature = (nodes))]
pub fn from_llamaindex(nodes: &Bound<'_, PyList>) -> PyResult<Vec<PyDocument>> {
    let mut result = Vec::with_capacity(nodes.len());
    for item in nodes.iter() {
        result.push(PyDocument::from_llamaindex(&item)?);
    }
    Ok(result)
}

#[pyfunction]
#[pyo3(signature = (docs))]
pub fn to_dict_list(py: Python, docs: Vec<PyRef<'_, PyDocument>>) -> PyResult<PyObject> {
    let list = PyList::empty(py);
    for doc in docs {
        list.append(doc.to_dict(py)?)?;
    }
    Ok(list.into())
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
    m.add_class::<PyHFTokenChunker>()?;
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
    m.add_class::<PyAstCodeChunker>()?;
    m.add_class::<PyJsonChunker>()?;
    m.add_class::<PyHtmlChunker>()?;
    m.add_class::<PyTableChunker>()?;
    m.add_class::<PyLateChunker>()?;
    m.add_class::<PyCharacterChunker>()?;
    m.add_class::<PyWordChunker>()?;
    m.add_class::<PyChunkPacker>()?;
    m.add_class::<PyChunkPipeline>()?;
    m.add_class::<PyStreamChunker>()?;
    m.add_class::<PyPDFLoader>()?;
    m.add_class::<PyDirectoryLoader>()?;
    m.add_function(wrap_pyfunction!(load_pdf, m)?)?;
    m.add_function(wrap_pyfunction!(load_pdf_pages, m)?)?;
    m.add_function(wrap_pyfunction!(to_langchain, m)?)?;
    m.add_function(wrap_pyfunction!(from_langchain, m)?)?;
    m.add_function(wrap_pyfunction!(to_llamaindex, m)?)?;
    m.add_function(wrap_pyfunction!(from_llamaindex, m)?)?;
    m.add_function(wrap_pyfunction!(to_dict_list, m)?)?;
    Ok(())
}
