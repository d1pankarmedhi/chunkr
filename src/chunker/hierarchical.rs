use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::chunker::base::{BaseChunker, Chunker};
use crate::chunker::recursive::RecursiveChunker;
use crate::error::ChunkrError;
use crate::structures::document::Document;

/// Structure representing a parent chunk and its associated child chunks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HierarchicalChunkPair {
    pub parent: Document,
    pub children: Vec<Document>,
}

/// Recursive node representing an arbitrary-depth hierarchy of document chunks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HierarchyNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub depth: usize,
    pub document: Document,
    pub children: Vec<HierarchyNode>,
}

impl HierarchyNode {
    /// Flatten all nodes in the subtree into a list of documents
    pub fn flatten(&self) -> Vec<Document> {
        let mut docs = vec![self.document.clone()];
        for child in &self.children {
            docs.extend(child.flatten());
        }
        docs
    }
}

/// Splits text into a multi-tier hierarchy of parent (broad context) and child (focused retrieval) chunks.
#[derive(Debug, Clone)]
pub struct HierarchicalChunker {
    pub parent_chunker: RecursiveChunker,
    pub child_chunker: RecursiveChunker,
    pub include_parents_in_output: bool,
}

impl HierarchicalChunker {
    /// Create a new HierarchicalChunker with defaults:
    /// - Parent: chunk_size 2000, overlap 200
    /// - Child: chunk_size 400, overlap 50
    pub fn new() -> Self {
        Self {
            parent_chunker: RecursiveChunker::new()
                .with_chunk_size(2000)
                .with_overlap(200),
            child_chunker: RecursiveChunker::new()
                .with_chunk_size(400)
                .with_overlap(50),
            include_parents_in_output: true,
        }
    }

    /// Custom parent and child chunk sizes
    pub fn with_sizes(
        parent_size: usize,
        parent_overlap: usize,
        child_size: usize,
        child_overlap: usize,
    ) -> Result<Self, ChunkrError> {
        if child_size >= parent_size {
            return Err(ChunkrError::Generic(
                "child_size must be smaller than parent_size".to_string(),
            ));
        }
        Ok(Self {
            parent_chunker: RecursiveChunker::new()
                .with_chunk_size(parent_size)
                .with_overlap(parent_overlap),
            child_chunker: RecursiveChunker::new()
                .with_chunk_size(child_size)
                .with_overlap(child_overlap),
            include_parents_in_output: true,
        })
    }

    /// Set whether output includes both parent and child chunks, or only child chunks tagged with parent_id
    pub fn with_include_parents(mut self, include: bool) -> Self {
        self.include_parents_in_output = include;
        self
    }

    /// Split text into structured pairs of parent documents with their corresponding children
    pub fn chunk_hierarchical(&self, text: &str) -> Result<Vec<HierarchicalChunkPair>, ChunkrError> {
        let parent_docs = self.parent_chunker.chunk(text)?;
        let mut pairs = Vec::new();

        for (p_idx, mut parent_doc) in parent_docs.into_iter().enumerate() {
            let parent_id = format!("parent-{}", p_idx);
            let child_docs = self.child_chunker.chunk(&parent_doc.content)?;

            let child_count = child_docs.len();
            parent_doc.add_metadata("chunk_type", Value::from("parent"));
            parent_doc.add_metadata("parent_id", Value::from(parent_id.clone()));
            parent_doc.add_metadata("child_count", Value::from(child_count));
            parent_doc.add_metadata("depth", Value::from(0));

            let mut enriched_children = Vec::new();
            for (c_idx, mut child_doc) in child_docs.into_iter().enumerate() {
                child_doc.add_metadata("chunk_type", Value::from("child"));
                child_doc.add_metadata("parent_id", Value::from(parent_id.clone()));
                child_doc.add_metadata("child_index", Value::from(c_idx));
                child_doc.add_metadata("depth", Value::from(1));
                child_doc.add_metadata(
                    "parent_preview",
                    Value::from(
                        parent_doc
                            .content
                            .chars()
                            .take(100)
                            .collect::<String>(),
                    ),
                );
                enriched_children.push(child_doc);
            }

            pairs.push(HierarchicalChunkPair {
                parent: parent_doc,
                children: enriched_children,
            });
        }

        Ok(pairs)
    }

    /// Generate an N-level hierarchy tree
    pub fn chunk_tree(&self, text: &str) -> Result<HierarchyNode, ChunkrError> {
        let pairs = self.chunk_hierarchical(text)?;
        let mut root_doc = Document::from_text(text);
        root_doc.add_metadata("chunk_type", Value::from("root"));
        root_doc.add_metadata("depth", Value::from(0));

        let mut parent_nodes = Vec::new();
        for pair in pairs {
            let mut child_nodes = Vec::new();
            let parent_id = pair
                .parent
                .metadata
                .get("parent_id")
                .and_then(|v| v.as_str())
                .unwrap_or("parent")
                .to_string();

            for (c_idx, child_doc) in pair.children.into_iter().enumerate() {
                let child_id = format!("{}-child-{}", parent_id, c_idx);
                child_nodes.push(HierarchyNode {
                    id: child_id,
                    parent_id: Some(parent_id.clone()),
                    depth: 2,
                    document: child_doc,
                    children: Vec::new(),
                });
            }

            parent_nodes.push(HierarchyNode {
                id: parent_id,
                parent_id: Some("root".to_string()),
                depth: 1,
                document: pair.parent,
                children: child_nodes,
            });
        }

        Ok(HierarchyNode {
            id: "root".to_string(),
            parent_id: None,
            depth: 0,
            document: root_doc,
            children: parent_nodes,
        })
    }
}

impl Default for HierarchicalChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl Chunker for HierarchicalChunker {
    fn chunk(&self, text: &str) -> Result<Vec<Document>, ChunkrError> {
        let pairs = self.chunk_hierarchical(text)?;
        let mut all_docs = Vec::new();

        for pair in pairs {
            if self.include_parents_in_output {
                all_docs.push(pair.parent);
            }
            all_docs.extend(pair.children);
        }

        Ok(all_docs)
    }
}

impl BaseChunker<Result<Vec<Document>, String>> for HierarchicalChunker {
    fn chunk_text(
        &self,
        text: &str,
        chunk_size: usize,
        overlap: usize,
    ) -> Result<Vec<Document>, String> {
        let child_size = chunk_size / 4;
        let child_overlap = overlap / 4;
        let chunker = HierarchicalChunker::with_sizes(
            chunk_size,
            overlap,
            child_size.max(50),
            child_overlap.min(child_size.max(50) / 2),
        )
        .map_err(|e| e.to_string())?;
        chunker.chunk(text).map_err(|e| e.to_string())
    }
}
