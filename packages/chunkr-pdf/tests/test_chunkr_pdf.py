"""End-to-end tests for the chunkr-pdf extension.

Run after `maturin develop --features python` in the repo root and
`pip install -e packages/chunkr-pdf`.
"""

from __future__ import annotations

import json
import re
from pathlib import Path

import pytest

import chunkr
import chunkr_pdf
from chunkr_pdf import PDFParser

FIXTURES = Path(__file__).resolve().parents[3] / "tests" / "test_files"
FINANCE = FIXTURES / "deck_16p.pdf"
LEBS = FIXTURES / "textbook_27p.pdf"
SAMPLE = FIXTURES / "sample_doc.pdf"

needs_finance = pytest.mark.skipif(not FINANCE.exists(), reason="deck_16p.pdf fixture missing")
needs_lebs = pytest.mark.skipif(not LEBS.exists(), reason="textbook_27p.pdf fixture missing")
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


# ── OCR engines: registry, proxy, gating, adapters ───────────────────────

import io
import time
import urllib.error
import urllib.request
import uuid

import chunkr_pdf
from chunkr_pdf import _ocr
from chunkr_pdf._ocr import LoopbackOcrServer, normalize_results, parse_multipart, png_size

DUMMY_PNG = bytes.fromhex(
    "89504e470d0a1a0a0000000d4948445200000001000000010802000000907753"
    "de0000000c4944415408d763f8ffff3f0005fe02fea735cb880000000049454e44ae426082"
)


def rasterize(pdf: Path, page: int = 1, dpi: float = 200.0) -> bytes:
    """Render one PDF page to PNG with liteparse, to act as a scanned page."""
    import liteparse

    parser = liteparse.LiteParse(extract_screenshots=True, dpi=dpi, ocr_enabled=False, quiet=True)
    result = parser.parse(str(pdf))
    return result.screenshots[page - 1].image_bytes


def word_engine(word: str = "ocrword"):
    """Engine returning one fixed word per page, recording its calls."""
    calls = []

    def engine(image, **kwargs):
        calls.append({"size": len(image), **kwargs})
        return [
            {"text": word, "bbox": [10.0, 120.0, 400.0, 160.0], "confidence": 0.9},
        ]

    engine.calls = calls
    return engine


def failing_engine():
    def engine(image):
        raise AssertionError("this engine must not be called")

    return engine


def multipart(fields: dict) -> tuple:
    boundary = uuid.uuid4().hex
    body = b""
    for name, value in fields.items():
        body += f"--{boundary}\r\n".encode()
        if isinstance(value, bytes):
            body += (
                f'Content-Disposition: form-data; name="{name}"; filename="page.png"\r\n'
                "Content-Type: image/png\r\n\r\n"
            ).encode()
            body += value + b"\r\n"
        else:
            body += f'Content-Disposition: form-data; name="{name}"\r\n\r\n{value}\r\n'.encode()
    body += f"--{boundary}--\r\n".encode()
    return body, f"multipart/form-data; boundary={boundary}"


def post(url: str, body: bytes, content_type: str, timeout: float = 30.0):
    request = urllib.request.Request(
        url, data=body, headers={"Content-Type": content_type}, method="POST"
    )
    return urllib.request.urlopen(request, timeout=timeout)


def test_ocr_registry_register_and_unregister():
    engine = word_engine()
    chunkr.register_ocr_backend("test-house", engine)
    assert "test-house" in chunkr.ocr_backends()
    assert chunkr.get_ocr_backend("test-house") is not None
    assert chunkr.unregister_ocr_backend("test-house") is True
    assert chunkr.unregister_ocr_backend("test-house") is False
    assert "test-house" not in chunkr.ocr_backends()


def test_registered_adapter_names_are_reported():
    register = chunkr_pdf.register()
    assert register is None
    assert "liteparse" in chunkr.pdf_backends()
    assert set(chunkr_pdf.OCR_ADAPTERS) == {"rapidocr", "paddleocr", "easyocr", "surya"}
    # Only importable engines are advertised as available.
    for name in chunkr_pdf.ocr_backends():
        assert name in chunkr_pdf.OCR_ADAPTERS or name in chunkr.ocr_backends()


def test_unknown_and_unavailable_backends_error_clearly():
    with pytest.raises(ValueError, match="unknown OCR backend"):
        PDFParser(ocr={"mode": "always", "backend": "nope"}).payload(DUMMY_PNG)
    with pytest.raises(ValueError, match="pdf-ocr-ppocr"):
        PDFParser(ocr={"mode": "always", "backend": "ppocr"}).payload(DUMMY_PNG)
    with pytest.raises(ValueError, match="server_url"):
        PDFParser(ocr={"mode": "server", "backend": "server"}).payload(DUMMY_PNG)
    with pytest.raises(ValueError):
        PDFParser(ocr={"mode": "always", "backend": "paddleocr"}).payload(DUMMY_PNG) if not (
            _ocr.adapter_available("paddleocr")
        ) else (_ for _ in ()).throw(ValueError())


def test_loopback_server_speaks_the_liteparse_ocr_api():
    engine = word_engine("hello")
    server = LoopbackOcrServer(engine, language="en", warmup=False)
    url = server.start()
    try:
        body, content_type = multipart({"file": DUMMY_PNG, "language": "en"})
        with post(url, body, content_type) as response:
            payload = json.loads(response.read())
        assert response.status == 200
        assert payload["results"][0]["text"] == "hello"
        assert payload["results"][0]["bbox"] == [10.0, 120.0, 400.0, 160.0]
        assert payload["results"][0]["confidence"] == 0.9
        # The engine saw raw PNG bytes and the configured language.
        assert engine.calls[0]["size"] == len(DUMMY_PNG)
        assert engine.calls[0]["language"] == "en"
        assert engine.calls[0]["options"] == {}
        # /health and unknown paths behave.
        with urllib.request.urlopen(url.replace("/ocr", "/health")) as health:
            assert json.loads(health.read())["status"] == "healthy"
        with pytest.raises(urllib.error.HTTPError) as missing_file:
            post(url, *multipart({"language": "en"}))
        assert missing_file.value.code == 400
    finally:
        server.close()


def test_loopback_server_surfaces_engine_failures_and_timeouts():
    def broken(image):
        raise RuntimeError("model exploded")

    server = LoopbackOcrServer(broken, warmup=False)
    url = server.start()
    try:
        body, content_type = multipart({"file": DUMMY_PNG})
        with pytest.raises(urllib.error.HTTPError) as error:
            post(url, body, content_type)
        assert error.value.code == 500
        assert "model exploded" in json.loads(error.value.read())["error"]
    finally:
        server.close()

    def slow(image):
        time.sleep(2.0)
        return []

    server = LoopbackOcrServer(slow, timeout_ms=200, warmup=False)
    url = server.start()
    try:
        body, content_type = multipart({"file": DUMMY_PNG})
        with pytest.raises(urllib.error.HTTPError) as error:
            post(url, body, content_type)
        assert error.value.code == 504
        assert server.failures == 1
    finally:
        server.close()


def test_closing_the_server_releases_the_port():
    server = LoopbackOcrServer(word_engine(), warmup=False)
    url = server.start()
    server.close()
    with pytest.raises(urllib.error.URLError):
        post(url, *multipart({"file": DUMMY_PNG}))


def test_normalize_results_accepts_every_engine_shape():
    size = (100, 200)
    # Plain text becomes one full-page block.
    assert normalize_results("all the text", size) == [
        {"text": "all the text", "bbox": [0.0, 0.0, 100.0, 200.0], "confidence": 1.0}
    ]
    # Dicts with aliases.
    boxes = normalize_results(
        [{"text": "a", "box": [1, 2, 3, 4], "score": 0.5, "poly": [[1, 2], [3, 2], [3, 4], [1, 4]]}],
        size,
    )
    assert boxes[0]["bbox"] == [1.0, 2.0, 3.0, 4.0]
    assert boxes[0]["confidence"] == 0.5
    assert boxes[0]["polygon"][0] == [1.0, 2.0]
    # EasyOCR tuples.
    tuples = normalize_results([([[0, 0], [10, 0], [10, 5], [0, 5]], "tuple", 0.8)], size)
    assert tuples[0]["text"] == "tuple"
    assert tuples[0]["bbox"] == [0.0, 0.0, 10.0, 5.0]
    # PaddleOCR/RapidOCR parallel arrays, including numpy-like `.tolist()`.
    class Output:
        txts = ("one", "two")
        scores = (0.9, 0.8)
        boxes = [[[0, 0], [4, 0], [4, 4], [0, 4]], [[5, 5], [9, 5], [9, 9], [5, 9]]]

    arrays = normalize_results(Output(), size)
    assert [item["text"] for item in arrays] == ["one", "two"]
    # Empty and unsupported inputs are empty, not errors.
    assert normalize_results(None, size) == []
    assert normalize_results([], size) == []
    assert normalize_results(object(), size) == []


def test_png_and_multipart_helpers():
    png = _ocr._blank_png(16)
    assert png[:8] == b"\x89PNG\r\n\x1a\n"
    assert png_size(png) == (16, 16)
    assert png_size(b"not a png") == (0, 0)
    body, content_type = multipart({"file": png, "language": "zh"})
    fields = parse_multipart(body, content_type)
    assert fields["file"] == png
    assert fields["language"] == b"zh"


def test_engine_call_styles_and_language_normalization():
    calls = []

    def simple(image):
        calls.append(("simple", len(image)))
        return []

    def rich(image, *, language="en", options=None):
        calls.append(("rich", language, options))
        return []

    for engine, expected in ((simple, "simple"), (rich, "rich")):
        server = LoopbackOcrServer(engine, language="de", warmup=False)
        assert server.recognize(DUMMY_PNG) == []
        assert calls[-1][0] == expected
    assert calls[-1][1] == "de"
    assert chunkr.ocr_language("eng") == "en"
    assert chunkr.ocr_language("en", "tesseract") == "eng"


@needs_finance
def test_auto_mode_makes_no_ocr_calls_on_digital_documents():
    parser = PDFParser(
        output="markdown",
        ocr={"mode": "auto", "backend": "test-never", "plugin": {"options": {"tag": "auto"}}},
    )
    chunkr.register_ocr_backend("test-never", failing_engine())
    try:
        auto = parser.load_pages(FINANCE)
        off = PDFParser(output="markdown", ocr={"mode": "off"}).load_pages(FINANCE)
        assert [doc.content for doc in auto] == [doc.content for doc in off]
        # The gate never opened, so no engine was started at all.
        assert parser.ocr_servers == {}
    finally:
        chunkr.unregister_ocr_backend("test-never")


@needs_finance
def test_auto_mode_ocrs_only_the_flagged_pages():
    engine = word_engine("gated")
    chunkr.register_ocr_backend("test-gate", engine)
    try:
        parser = PDFParser(
            output="markdown",
            ocr={"mode": "auto", "backend": "test-gate", "auto_reasons": ["sparse-text"]},
        )
        documents = parser.load_pages(FINANCE)
        assert len(documents) == 16
        # deck_16p.pdf: pages 1, 11, 12 and 14 carry `sparse-text`.
        pages = [call for call in engine.calls if call["size"] > 200]
        assert len(pages) == 4
        # Server calls include the warm-up probe on the blank image.
        assert parser.ocr_servers["test-gate"].calls == len(pages) + 1
        parser.close()
    finally:
        chunkr.unregister_ocr_backend("test-gate")


@needs_finance
def test_always_mode_uses_the_registered_engine_end_to_end():
    """A scan-shaped page round-trips: raster -> engine -> liteparse -> documents."""
    if not FINANCE.exists():
        pytest.skip("fixture missing")
    page_png = rasterize(FINANCE, page=1, dpi=150.0)
    engine = word_engine("proxyword")
    chunkr.register_ocr_backend("test-e2e", engine)
    try:
        parser = PDFParser(ocr={"mode": "always", "backend": "test-e2e"}, output="markdown")
        documents = parser.load_pages(page_png)
        assert len(documents) == 1
        assert engine.calls, "engine should have been called for the raster page"
        assert engine.calls[-1]["size"] > 1_000
        assert parser.ocr_servers["test-e2e"].calls >= 1
        parser.close()
    finally:
        chunkr.unregister_ocr_backend("test-e2e")


def test_surya_block_mapping_without_inference():
    """Surya emits block HTML + polygons; the mapping must drop empty blocks."""

    class Block:
        def __init__(self, html, polygon, confidence=0.9, skipped=False, error=False):
            self.html, self.polygon, self.confidence = html, polygon, confidence
            self.skipped, self.error = skipped, error

    class Page:
        blocks = [
            Block(
                "<p>Heading <b>with</b> markup</p>",
                [[168, 409], [1892, 409], [1892, 561], [168, 561]],
            ),
            Block("<div>skip me</div>", [[0, 0], [1, 0], [1, 1], [0, 1]], skipped=True),
            Block("<div>errored</div>", [[0, 0], [1, 0], [1, 1], [0, 1]], error=True),
            Block("<div></div>", [[0, 0], [1, 0], [1, 1], [0, 1]]),
            Block("<h1>Section</h1>", [[431, 1092], [1200, 1092], [1200, 1273], [431, 1273]], None),
        ]

    results = _ocr.surya_blocks([Page()])
    assert [item["text"] for item in results] == ["Heading with markup", "Section"]
    assert results[0]["bbox"] == [168.0, 409.0, 1892.0, 561.0]
    assert results[0]["confidence"] == 0.9
    assert len(results[0]["polygon"]) == 4
    assert results[1]["confidence"] == 1.0  # None becomes 1.0
    assert _ocr.html_text("<p>a<br>b</p><div>c</div>") == "a b c"
    assert _ocr.surya_blocks(None) == []


@pytest.mark.parametrize("name", ["rapidocr", "paddleocr", "easyocr"])
@needs_finance
def test_installed_adapters_read_a_rasterized_page(name):
    """Real engines, real merge: each adapter recovers a known line."""
    if not _ocr.adapter_available(name):
        pytest.skip(f"{name} is not installed")
    # Use the page carrying the most text: covers and slides are mostly artwork,
    # so their text layers are not a fair reference for OCR.
    reference_pages = PDFParser(output="markdown", ocr={"mode": "off"}).load_pages(FINANCE)
    densest = max(
        range(len(reference_pages)),
        key=lambda index: len(re.findall(r"[0-9A-Za-z]+", reference_pages[index].content)),
    )
    page_png = rasterize(FINANCE, page=densest + 1, dpi=200.0)
    parser = PDFParser(ocr={"mode": "always", "backend": name}, output="markdown")
    try:
        documents = parser.load_pages(page_png)
        assert len(documents) == 1
        # Score against the page's own text layer: no hardcoded expectations.
        reference = reference_pages[densest].content
        expected = set(re.findall(r"[0-9A-Za-z]+", reference.lower()))
        seen = set(re.findall(r"[0-9A-Za-z]+", documents[0].content.lower()))
        recall = len(expected & seen) / max(len(expected), 1)
        assert recall > 0.5, f"{name} recovered {recall:.2f} of the page's words"
        assert parser.ocr_servers[name].calls >= 1
        assert parser.ocr_servers[name].failures == 0
    finally:
        parser.close()


@needs_finance
def test_adapter_without_its_package_gives_an_install_hint():
    """An adapter that is known but not installed must say how to install it."""
    missing = [name for name in chunkr_pdf.OCR_ADAPTERS if not _ocr.adapter_available(name)]
    if not missing:
        pytest.skip("all adapters are installed here")
    with pytest.raises(ValueError, match=r"pip install \"chunkr-pdf\[ocr-"):
        PDFParser(ocr={"mode": "always", "backend": missing[0]}).payload(DUMMY_PNG)


# ── page-level parser backends (docling-serve, Mistral OCR, OpenAI-compatible VLM) ──

from chunkr_pdf import parsers as parser_backends


def mock_server(handler):
    """Run `handler(path, body) -> (status, payload)` on loopback for one test."""
    import threading
    from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *_: Any) -> None:
            pass

        def _respond(self, status: int, payload: dict) -> None:
            body = json.dumps(payload).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_POST(self) -> None:  # noqa: N802
            length = int(self.headers.get("Content-Length") or 0)
            body = self.rfile.read(length)
            status, payload = handler(self.path, body)
            self._respond(status, payload)

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server.daemon_threads = True
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server, f"http://127.0.0.1:{server.server_address[1]}"


def test_markdown_blocks_structure_and_plain_text():
    markdown = "# Title\n\nIntro paragraph\ncontinued here.\n\n- one\n- two\n\n1. first\n\n| a | b |\n| - | - |\n"
    blocks = parser_backends.markdown_blocks(markdown)
    assert blocks[0] == {"kind": "heading", "text": "Title", "level": 1}
    assert blocks[1]["kind"] == "paragraph" and "continued here." in blocks[1]["text"]
    assert [b["text"] for b in blocks if b["kind"] == "list_item"] == ["one", "two", "first"]
    assert blocks[-1]["kind"] == "paragraph" and blocks[-1]["text"].startswith("| a |")
    assert parser_backends._markdown_text("# T\n\n- x\n1. y\n") == "T\n\nx\ny"


@needs_sample
def test_docling_backend_maps_structured_pages():
    def handler(path, body):
        assert path == "/v1/convert/file"
        assert b"document.pdf" in body  # the PDF travelled as multipart
        return 200, {
            "status": "success",
            "document": {
                "md_content": "# Ignored when json_content exists",
                "json_content": {
                    "texts": [
                        {"text": "Report Title", "label": "title", "level": 1, "prov": [{"page_no": 1}]},
                        {"text": "Section", "label": "section_header", "level": 2, "prov": [{"page_no": 1}]},
                        {"text": "First paragraph.", "label": "text", "prov": [{"page_no": 1}]},
                        {"text": "Bullet", "label": "list_item", "prov": [{"page_no": 2}]},
                        {"text": "  ", "label": "text", "prov": [{"page_no": 2}]},
                    ]
                },
            },
        }

    server, url = mock_server(handler)
    try:
        chunkr.register_pdf_backend("test-docling", parser_backends.docling(url))
        loader = chunkr.PDFLoader(backend="test-docling")
        documents = loader.load_pages(SAMPLE)
        assert len(documents) == 2
        assert documents[0].metadata["page_number"] == 1
        assert documents[0].content.startswith("Report Title")
        blocks = documents[0].metadata["blocks"]
        assert [b["kind"] for b in blocks] == ["heading", "heading", "paragraph"]
        # Backend levels survive when they differ; a page whose headings are all
        # one level gets flattened by `sanitize.headings.levels="auto"` (h2).
        assert [b["level"] for b in blocks[:2]] == [1, 2]
        assert documents[0].content.splitlines()[0] == "Report Title"
        assert documents[1].content.strip() == "Bullet"
    finally:
        chunkr.unregister_pdf_backend("test-docling")
        server.shutdown()
        server.server_close()


def test_docling_backend_falls_back_to_markdown_and_reports_errors():
    def handler(path, body):
        return 200, {"document": {"md_content": "# Only markdown\n\nBody text"}}

    server, url = mock_server(handler)
    try:
        parser = parser_backends.docling(url)
        payload = json.loads(parser(b"%PDF-1.4 fake", '{"output": "markdown"}'))
        assert payload["pages"][0]["markdown"].startswith("# Only markdown")
        assert payload["pages"][0]["blocks"] == [
            {"kind": "heading", "text": "Only markdown", "level": 1},
            {"kind": "paragraph", "text": "Body text"},
        ]
    finally:
        server.shutdown()
        server.server_close()

    def failing(path, body):
        return 500, {"error": "conversion failed"}

    server, url = mock_server(failing)
    try:
        with pytest.raises(RuntimeError, match="HTTP 500"):
            parser_backends.docling(url)(b"%PDF-1.4", "")
    finally:
        server.shutdown()
        server.server_close()

    with pytest.raises(RuntimeError, match="unreachable"):
        parser_backends.docling("http://127.0.0.1:1")(b"%PDF-1.4", "")


def test_mistral_backend_keeps_page_markdown():
    def handler(path, body):
        assert path == "/v1/ocr"
        request = json.loads(body)
        assert request["model"] == "mistral-ocr-latest"
        assert request["document"]["document_url"].startswith("data:application/pdf;base64,")
        return 200, {
            "pages": [
                {"index": 0, "markdown": "# Page one\n\nText"},
                {"index": 1, "markdown": "- a\n- b"},
            ]
        }

    server, url = mock_server(handler)
    try:
        parser = parser_backends.mistral("test-key", base_url=url)
        payload = json.loads(parser(b"%PDF-1.4", ""))
        assert payload["version"] == "mistral-ocr:mistral-ocr-latest"
        assert [page["page_number"] for page in payload["pages"]] == [1, 2]
        assert payload["pages"][0]["markdown"].startswith("# Page one")
        assert payload["pages"][1]["text"] == "a\nb"
    finally:
        server.shutdown()
        server.server_close()


@needs_sample
def test_vlm_backend_renders_pages_and_sends_them():
    prompts = []

    def handler(path, body):
        assert path == "/chat/completions"
        request = json.loads(body)
        content = request["messages"][0]["content"]
        prompts.append(content[0]["text"])
        assert content[1]["image_url"]["url"].startswith("data:image/png;base64,")
        return 200, {"choices": [{"message": {"content": f"# Page from {request['model']}"}}]}

    def render(data, dpi):
        assert data.startswith(b"%PDF")
        assert dpi == 150.0
        return [b"\x89PNG\r\n\x1a\nfake-page-1", b"\x89PNG\r\n\x1a\nfake-page-2"]

    server, url = mock_server(handler)
    try:
        chunkr.register_pdf_backend(
            "test-vlm", parser_backends.vlm(url, "olmocr-2", dpi=150.0, render=render)
        )
        documents = chunkr.PDFLoader(backend="test-vlm").load_pages(SAMPLE)
        assert len(documents) == 2
        assert documents[0].content.startswith("Page from olmocr-2")
        assert documents[0].metadata["blocks"][0]["kind"] == "heading"
        assert prompts and "Transcribe this page to markdown" in prompts[0]

        # Raw markdown passthrough keeps the model's own formatting.
        raw = chunkr.PDFLoader(
            backend="test-vlm", config={"output": "markdown", "sanitize": {"enabled": False}}
        ).load_pages(SAMPLE)
        assert raw[0].content.startswith("# Page from olmocr-2")
    finally:
        chunkr.unregister_pdf_backend("test-vlm")
        server.shutdown()
        server.server_close()


@needs_sample
def test_vlm_backend_renders_for_real_with_liteparse():
    """The default renderer is liteparse screenshots: no mocks on that path."""
    page_pngs = parser_backends._render_pages(SAMPLE.read_bytes(), 150.0)
    assert len(page_pngs) == 10
    assert all(png[:8] == b"\x89PNG\r\n\x1a\n" for png in page_pngs)
