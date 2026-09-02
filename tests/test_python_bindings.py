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

    def test_table_chunker(self):
        md_table = (
            "| Quarter | Revenue | Profit |\n"
            "| :--- | :--- | :--- |\n"
            "| Q1 | $10M | $2M |\n"
            "| Q2 | $12M | $2.5M |\n"
            "| Q3 | $14M | $3M |\n"
            "| Q4 | $16M | $3.5M |"
        )
        chunker = chunkr.TableChunker(rows_per_chunk=2, overlap_rows=1)
        chunks = chunker.chunk(md_table)
        self.assertGreaterEqual(len(chunks), 2)
        for chunk in chunks:
            self.assertIn("| Quarter | Revenue | Profit |", chunk.content)
            self.assertTrue(chunk.metadata["is_table"])
            self.assertEqual(chunk.metadata["format"], "markdown")
            self.assertEqual(chunk.metadata["total_rows"], 4)
            self.assertEqual(chunk.metadata["columns"], ["Quarter", "Revenue", "Profit"])

        # Test CSV format
        csv_data = "Item,Qty,Price\nBook,5,10\nPen,20,2\nRuler,15,3\nNotebook,8,5"
        csv_chunker = chunkr.TableChunker(rows_per_chunk=2, overlap_rows=0, format="csv")
        csv_chunks = csv_chunker.chunk(csv_data)
        self.assertEqual(len(csv_chunks), 2)
        for chunk in csv_chunks:
            self.assertTrue(chunk.content.startswith("Item,Qty,Price"))
            self.assertEqual(chunk.metadata["format"], "csv")
            self.assertEqual(chunk.metadata["columns"], ["Item", "Qty", "Price"])

        # Test chunk_documents
        doc = chunkr.Document(csv_data, {"source": "inventory.csv"})
        doc_chunks = csv_chunker.chunk_documents([doc])
        self.assertEqual(len(doc_chunks), 2)
        self.assertEqual(doc_chunks[0].metadata["source"], "inventory.csv")

    def test_late_chunker(self):
        text = "Deep learning powers modern vision systems. Large language models excel at reasoning. Vector databases enable scalable retrieval."
        late_chunker = chunkr.LateChunker(chunk_size=50, overlap=10)
        chunks = late_chunker.chunk(text)
        self.assertGreaterEqual(len(chunks), 2)

        for chunk in chunks:
            self.assertIn("token_start", chunk.metadata)
            self.assertIn("token_end", chunk.metadata)
            self.assertIn("char_start", chunk.metadata)
            self.assertIn("char_end", chunk.metadata)
            self.assertGreater(chunk.metadata["token_end"], chunk.metadata["token_start"])

        # Test pooling
        # Create synthetic token embeddings
        num_tokens = max(c.metadata["token_end"] for c in chunks) + 5
        fake_embs = [[1.0, 0.5, 0.2] for _ in range(num_tokens)]

        pooled = late_chunker.pool_embeddings(fake_embs, chunks)
        self.assertEqual(len(pooled), len(chunks))
        self.assertEqual(len(pooled[0]), 3)

        # Norm should be approx 1.0
        norm = sum(x * x for x in pooled[0]) ** 0.5
        self.assertAlmostEqual(norm, 1.0, places=3)

    def test_directory_loader(self):
        loader = chunkr.DirectoryLoader(extensions=["pdf", "rs"])
        docs = loader.load_and_chunk("tests/test_files")
        self.assertGreaterEqual(len(docs), 1)
        self.assertIn("file_path", docs[0].metadata)
        self.assertEqual(docs[0].metadata["file_extension"], "pdf")

    def test_hf_token_chunker(self):
        tokenizer_json = """{
            "version": "1.0",
            "truncation": null,
            "padding": null,
            "added_tokens": [],
            "normalizer": null,
            "pre_tokenizer": { "type": "Whitespace" },
            "post_processor": null,
            "decoder": null,
            "model": {
                "type": "WordLevel",
                "unk_token": "[UNK]",
                "vocab": {
                    "[UNK]": 0,
                    "Hello": 1,
                    "world": 2,
                    "chunkr": 3,
                    "fast": 4
                }
            }
        }"""
        chunker = chunkr.HFTokenChunker.from_json(tokenizer_json, chunk_size=2, overlap=1)
        text = "Hello world chunkr fast"
        self.assertEqual(chunker.count_tokens(text), 4)
        chunks = chunker.chunk(text)
        self.assertGreaterEqual(len(chunks), 2)
        self.assertEqual(chunks[0].metadata["tokenizer"], "huggingface")

if __name__ == "__main__":
    unittest.main()
