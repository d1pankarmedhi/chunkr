import unittest
import chunkr

class TestPythonBindings(unittest.TestCase):
    def test_document_metadata_types(self):
        meta = {
            "int_val": 42,
            "float_val": 3.14,
            "bool_val": True,
            "str_val": "chunkr",
            "list_val": ["apple", "banana"],
            "dict_val": {"nested_key": 100},
        }
        doc = chunkr.Document("Sample content", meta)
        self.assertEqual(doc.content, "Sample content")
        self.assertEqual(doc.metadata["int_val"], 42)
        self.assertAlmostEqual(doc.metadata["float_val"], 3.14, places=2)
        self.assertIs(doc.metadata["bool_val"], True)
        self.assertEqual(doc.metadata["str_val"], "chunkr")
        self.assertEqual(doc.metadata["list_val"], ["apple", "banana"])
        self.assertEqual(doc.metadata["dict_val"]["nested_key"], 100)

        # Test to_dict
        d_dict = doc.to_dict()
        self.assertEqual(d_dict["content"], "Sample content")
        self.assertEqual(d_dict["metadata"]["int_val"], 42)

    def test_batch_and_parallel_chunking(self):
        chunker = chunkr.RecursiveChunker(chunk_size=50, overlap=10)
        docs = [
            chunkr.Document(f"Document {i} with some paragraph content to split into pieces.", {"doc_id": i})
            for i in range(5)
        ]

        # 1. chunk_documents
        chunked = chunker.chunk_documents(docs)
        self.assertGreaterEqual(len(chunked), 5)
        self.assertIn("doc_index", chunked[0].metadata)
        self.assertIn("chunk_index", chunked[0].metadata)
        self.assertIn("doc_id", chunked[0].metadata)

        # 2. par_chunk_documents (Rayon multi-threaded)
        par_chunked = chunker.par_chunk_documents(docs)
        self.assertEqual(len(chunked), len(par_chunked))

        # 3. par_chunk_texts
        texts = ["Text one with words to chunk.", "Text two with more words to chunk."]
        par_texts = chunker.par_chunk_texts(texts)
        self.assertEqual(len(par_texts), 2)
        self.assertGreaterEqual(len(par_texts[0]), 1)
        self.assertGreaterEqual(len(par_texts[1]), 1)

    def test_hierarchical_methods(self):
        chunker = chunkr.HierarchicalChunker(parent_size=120, parent_overlap=20, child_size=40, child_overlap=5)
        text = "Section A: Overview of architecture.\n\nSection B: Database and storage layer."

        # chunk_hierarchical
        pairs = chunker.chunk_hierarchical(text)
        self.assertGreaterEqual(len(pairs), 1)
        self.assertIn("parent", pairs[0])
        self.assertIn("children", pairs[0])
        self.assertIsInstance(pairs[0]["parent"], chunkr.Document)
        self.assertIsInstance(pairs[0]["children"], list)
        self.assertGreaterEqual(len(pairs[0]["children"]), 1)

        # chunk_tree
        tree = chunker.chunk_tree(text)
        self.assertEqual(tree["id"], "root")
        self.assertEqual(tree["depth"], 0)
        self.assertIn("children", tree)
        self.assertGreaterEqual(len(tree["children"]), 1)
        self.assertEqual(tree["children"][0]["depth"], 1)

    def test_token_chunker_batch(self):
        chunker = chunkr.TokenChunker(chunk_size=20, overlap=5)
        docs = [chunkr.Document("Large language models are fascinating.", {"id": 1})]
        res = chunker.chunk_documents(docs)
        self.assertGreaterEqual(len(res), 1)
        self.assertEqual(res[0].metadata["id"], 1)

    def test_proposition_chunker_correctness(self):
        chunker = chunkr.PropositionChunker(propositions_per_chunk=1, overlap=0)
        sentence = "The Eiffel Tower, which was constructed in 1889, is located in Paris and welcomes millions of tourists every year."
        props = chunker.chunk(sentence)
        self.assertEqual(len(props), 2)
        self.assertIn("is located in Paris", props[0].content)
        self.assertIn("was constructed in 1889", props[1].content)

if __name__ == "__main__":
    unittest.main()
