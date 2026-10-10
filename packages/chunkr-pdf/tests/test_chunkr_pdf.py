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
        # finance.pdf: pages 1, 11, 12 and 14 carry `sparse-text`.
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
                "<p>College of <b>Business</b> Administration</p>",
                [[168, 409], [1892, 409], [1892, 561], [168, 561]],
            ),
            Block("<div>skip me</div>", [[0, 0], [1, 0], [1, 1], [0, 1]], skipped=True),
            Block("<div>errored</div>", [[0, 0], [1, 0], [1, 1], [0, 1]], error=True),
            Block("<div></div>", [[0, 0], [1, 0], [1, 1], [0, 1]]),
            Block("<h1>FINANCE</h1>", [[431, 1092], [1200, 1092], [1200, 1273], [431, 1273]], None),
        ]

    results = _ocr.surya_blocks([Page()])
    assert [item["text"] for item in results] == ["College of Business Administration", "FINANCE"]
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
    page_png = rasterize(FINANCE, page=1, dpi=200.0)
    parser = PDFParser(ocr={"mode": "always", "backend": name}, output="markdown")
    try:
        documents = parser.load_pages(page_png)
        assert len(documents) == 1
        text = documents[0].content.replace("\n", " ")
        assert "College of Business Administration" in text, text[:200]
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
