"""End-to-end tests for the chunkr-pdf extension.

Run after `maturin develop --features python` in the repo root and
`pip install -e packages/chunkr-pdf`.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest

import chunkr
import chunkr_pdf
from chunkr_pdf import PDFParser

FIXTURES = Path(__file__).resolve().parents[3] / "tests" / "test_files"
FINANCE = FIXTURES / "finance.pdf"
LEBS = FIXTURES / "lebs201.pdf"
SAMPLE = FIXTURES / "sample_doc.pdf"

needs_finance = pytest.mark.skipif(not FINANCE.exists(), reason="finance.pdf fixture missing")
needs_lebs = pytest.mark.skipif(not LEBS.exists(), reason="lebs201.pdf fixture missing")
needs_sample = pytest.mark.skipif(not SAMPLE.exists(), reason="sample_doc.pdf fixture missing")


def test_register_is_idempotent():
    chunkr_pdf.register()
    chunkr_pdf.register()
    assert chunkr_pdf.BACKEND_NAME in chunkr.pdf_backends()
    assert chunkr_pdf.is_available()


def test_config_defaults_are_retrieval_shaped():
    parser = PDFParser()
    config = parser.config
    assert config["output"] == "text"
    assert config["granularity"] == "page"
    assert config["keep_blocks"] is True
    assert config["sanitize"]["enabled"] is True
    assert config["extract"]["tables"] == "sanitized"
    assert config["ocr"]["mode"] == "off"


def test_config_overrides_and_unknown_keys():
    parser = PDFParser(
        preset="structure",
        output="markdown",
        granularity="block",
        extract={"tables": "raw"},
        sanitize={"headings": {"levels": "derive"}},
        ocr={"mode": "server", "server_url": "http://localhost:8828/ocr"},
    )
    config = parser.config
    assert config["granularity"] == "block"
    assert config["extract"]["tables"] == "raw"
    assert config["sanitize"]["headings"]["levels"] == "derive"
    assert config["ocr"]["server_url"] == "http://localhost:8828/ocr"

    with pytest.raises(Exception):
        PDFParser(tables="raw")  # typos must fail loudly
    with pytest.raises(ValueError):
        PDFParser(preset="nope")


@needs_finance
def test_page_documents_carry_structure_metadata():
    parser = PDFParser(keep_blocks=True, include_page_labels=True)
    documents = parser.load_pages(FINANCE)
    assert len(documents) == 16
    first = documents[0]
    assert first.metadata["parser_backend"] == "liteparse"
    assert first.metadata["page_number"] == 1
    kinds = {block["kind"] for block in first.metadata["blocks"]}
    assert kinds, "blocks should be attached to page documents"


@needs_finance
def test_block_granularity_and_markdown():
    parser = PDFParser(granularity="block", output="markdown")
    documents = parser.load(FINANCE)
    assert documents
    assert all("block_type" in doc.metadata for doc in documents)
    headings = [d for d in documents if d.metadata.get("block_type") == "heading"]
    assert headings and headings[0].content.startswith("#")


@needs_lebs
def test_sanitized_markdown_drops_prose_grids():
    sanitized = PDFParser(output="markdown", granularity="page").load(LEBS)
    raw = PDFParser(preset="faithful", granularity="page").load(LEBS)

    def table_count(documents):
        return sum(
            1
            for doc in documents
            for block in doc.metadata.get("blocks") or []
            if block["kind"] in ("table", "merged_table")
        )

    assert table_count(sanitized) < table_count(raw)
    demoted = sum(
        len((doc.metadata.get("sanitize_report") or {}).get("notes", [])) for doc in sanitized
    )
    assert demoted > 0


@needs_lebs
def test_raw_passthrough_has_no_sanitizing():
    parser = PDFParser(preset="faithful", output="markdown")
    documents = parser.load_pages(LEBS)
    assert all("sanitize_report" not in doc.metadata for doc in documents)
    assert all(doc.metadata["parser_sanitized"] is False for doc in documents)


@needs_lebs
def test_junk_free_and_full_text():
    documents = PDFParser().load_pages(LEBS)
    text = "\n".join(doc.content for doc in documents)
    assert "Unimplemented" not in text
    assert len(text) > 80_000


@needs_lebs
def test_stream_batches_match_full_parse():
    parser = PDFParser()
    streamed = [doc for batch in parser.stream(LEBS, batch_size=5) for doc in batch]
    full = parser.load_pages(LEBS)
    assert len(streamed) == len(full)
    assert streamed[0].content.strip() == full[0].content.strip()


@needs_lebs
def test_is_complex_reports_layout_signals():
    rows = PDFParser().is_complex(LEBS)
    assert len(rows) == 27
    assert any(row.get("layout", {}).get("column_count", 1) > 1 for row in rows)


@needs_finance
def test_chunkr_pdfloader_auto_uses_the_backend():
    loader = chunkr.PDFLoader()
    documents = loader.load_pages(FINANCE)
    assert len(documents) == 16
    assert documents[0].metadata["parser_backend"] == "liteparse"


@needs_sample
def test_fast_backend_still_available():
    loader = chunkr.PDFLoader(backend="fast")
    assert "Sample PDF Document" in loader.load(SAMPLE)


@needs_sample
def test_loader_accepts_bytes_and_config():
    loader = chunkr.PDFLoader(config=chunkr.ParserConfig("faithful"))
    text = loader.load_from_bytes(SAMPLE.read_bytes())
    assert "Sample PDF Document" in text


@needs_finance
def test_loader_accepts_callable_backend():
    calls = {"n": 0}

    def backend(source, config_json):
        calls["n"] += 1
        return json.dumps({"version": "custom-1.0", "pages": [{"page_number": 1, "text": "hello"}]})

    loader = chunkr.PDFLoader(backend=backend)
    documents = loader.load_pages(FINANCE)
    assert calls["n"] == 1
    assert documents[0].content.strip() == "hello"
    assert documents[0].metadata["parser_version"] == "custom-1.0"


@needs_sample
def test_pages_to_documents_is_public_api():
    payload = json.dumps(
        [
            {
                "page_number": 1,
                "text": "page one",
                "blocks": [{"kind": "heading", "text": "Title", "level": 2}],
            }
        ]
    )
    documents = chunkr.pages_to_documents(payload, source="x.pdf", backend="test")
    assert documents[0].metadata["page_number"] == 1
    assert documents[0].metadata["blocks"][0]["kind"] == "heading"
