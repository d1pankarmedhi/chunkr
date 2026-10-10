"""OCR engines for chunkr-pdf: registry adapters and a loopback OCR server.

liteparse talks to OCR servers over its documented HTTP API (``POST /ocr``,
multipart ``file`` + ``language``, JSON ``{"results": [...]}``). This module
implements that API locally so any Python callable — a packaged engine such as
RapidOCR, a model server client, or user code — can serve as the OCR engine
without an extra process to manage:

    chunkr.register_ocr_backend("house", my_engine)
    parser = PDFParser(ocr={"mode": "always", "backend": "house"})

Guarantees: loopback-only, stdlib server, one engine call per page image, and
the engine stays warm for the lifetime of the parser.
"""

from __future__ import annotations

import concurrent.futures
import inspect
import json
import struct
import threading
import zlib
from email.parser import BytesParser
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any, Callable, Dict, List, Optional, Tuple

import chunkr

__all__ = [
    "LoopbackOcrServer",
    "ADAPTERS",
    "adapter_available",
    "register_adapters",
    "resolve_engine",
    "normalize_results",
]

# ── engines ──────────────────────────────────────────────────────────────


def _bbox(polygon: Any) -> List[float]:
    """Axis-aligned `[x1, y1, x2, y2]` from a 4-point polygon or nested box."""
    points = list(polygon)
    xs = [float(point[0]) for point in points]
    ys = [float(point[1]) for point in points]
    return [min(xs), min(ys), max(xs), max(ys)]


def _polygon(points: Any) -> Optional[List[List[float]]]:
    """A 4x2 float polygon when the engine reports one, else None."""
    try:
        polygon = [[float(point[0]), float(point[1])] for point in points]
    except (TypeError, IndexError, ValueError):
        return None
    return polygon if len(polygon) == 4 else None


def _result(text: str, box: Any, confidence: Any, polygon: Any = None) -> Dict[str, Any]:
    """One OCR result in the liteparse OCR API shape."""
    bbox = _bbox(box) if _is_polygon(box) else [float(v) for v in box]
    item: Dict[str, Any] = {
        "text": str(text),
        "bbox": bbox,
        "confidence": float(confidence) if confidence is not None else 1.0,
    }
    four = _polygon(polygon if polygon is not None else (box if _is_polygon(box) else []))
    if four is not None:
        item["polygon"] = four
    return item


def _is_polygon(box: Any) -> bool:
    """True for `[[x, y], ...]` rather than a flat `[x1, y1, x2, y2]`."""
    try:
        first = box[0]
    except (TypeError, IndexError):
        return False
    return isinstance(first, (list, tuple)) or hasattr(first, "__len__")


def _as_list(value: Any) -> List[Any]:
    if value is None:
        return []
    if hasattr(value, "tolist"):
        return list(value.tolist())
    return list(value)


def _pick(source: Any, names: Tuple[str, ...], default: Any = None) -> Any:
    """Read the first present key/attribute among `names`."""
    for name in names:
        if isinstance(source, dict):
            if name in source:
                return source[name]
        elif hasattr(source, name):
            return getattr(source, name)
    return default


def normalize_results(raw: Any, image_size: Tuple[int, int]) -> List[Dict[str, Any]]:
    """Coerce an engine's output into liteparse OCR results.

    Accepts a plain string (one full-page block), a list of dicts with
    `text`/`bbox`/`confidence` (`box`, `score`, `conf`, `polygon`, `poly` are
    accepted as aliases), an EasyOCR-style list of `(polygon, text,
    confidence)` tuples, or an object exposing parallel `txts`/`scores`/`boxes`
    arrays (PaddleOCR/RapidOCR shape).
    """
    if raw is None:
        return []
    if isinstance(raw, str):
        width, height = image_size
        return [{"text": raw, "bbox": [0.0, 0.0, float(width), float(height)], "confidence": 1.0}]
    if isinstance(raw, dict) or hasattr(raw, "txts"):
        # Parallel-array shape (PaddleOCR/RapidOCR): txts/scores/boxes.
        raw_texts = _pick(raw, ("txts", "texts", "rec_texts", "text_lines"))
        if raw_texts is not None:
            texts = _as_list(raw_texts)
            scores = _as_list(_pick(raw, ("scores", "rec_scores", "confidence", "confidences")))
            raw_boxes = _pick(
                raw,
                ("boxes", "rec_boxes", "bboxes", "dt_polys", "rec_polys", "polygons"),
            )
            boxes = _as_list(raw_boxes)
            results = []
            for index, text in enumerate(texts):
                box = boxes[index] if index < len(boxes) else None
                if box is None:
                    continue
                score = scores[index] if index < len(scores) else 1.0
                item_text = text if isinstance(text, str) else _pick(text, ("text",), "")
                polygon = None if isinstance(text, str) else _pick(text, ("polygon", "poly"))
                if not isinstance(text, str):
                    box = _pick(text, ("bbox", "box", "boxes"), box)
                results.append(_result(item_text, box, score, polygon if polygon is not None else box))
            return results

    if not hasattr(raw, "__iter__") or isinstance(raw, (bytes, bytearray)):
        return []

    results = []
    for item in _as_list(raw):
        if isinstance(item, str):
            continue
        if isinstance(item, dict) or hasattr(item, "text"):
            text = _pick(item, ("text", "txt", "rec_text"), "")
            box = _pick(item, ("bbox", "box", "boxes", "polygon", "poly", "points"))
            score = _pick(item, ("confidence", "score", "conf", "rec_score"))
            polygon = _pick(item, ("polygon", "poly", "points"))
            if text and box is not None:
                results.append(_result(text, box, score, polygon if polygon is not None else box))
            continue
        # EasyOCR / PaddleOCR tuple: (polygon, text, confidence)
        values = list(item)
        if len(values) >= 3 and isinstance(values[1], str):
            results.append(_result(values[1], values[0], values[2], values[0]))
    return results


def _filter_kwargs(call: Any, options: Dict[str, Any]) -> Dict[str, Any]:
    """Keep options the called object accepts (forward-compatible adapters)."""
    try:
        parameters = inspect.signature(call).parameters
    except (TypeError, ValueError):
        return dict(options)
    if any(p.kind is inspect.Parameter.VAR_KEYWORD for p in parameters.values()):
        return dict(options)
    return {key: value for key, value in options.items() if key in parameters}


def rapidocr_engine(**options: Any) -> Callable[..., Any]:
    """RapidOCR (PP-OCRv6 ONNX models, CPU-friendly)."""
    from rapidocr import RapidOCR

    return _wrap_engine(RapidOCR(**_filter_kwargs(RapidOCR.__init__, options)))


def paddleocr_engine(**options: Any) -> Callable[..., Any]:
    """PaddleOCR 3.x text detection + recognition."""
    from paddleocr import PaddleOCR

    kwargs = dict(
        lang=options.pop("lang", options.pop("language", "en")),
        use_doc_orientation_classify=False,
        use_doc_unwarping=False,
        use_textline_orientation=bool(options.pop("textline_orientation", False)),
    )
    kwargs.update(options)
    return _wrap_engine(PaddleOCR(**_filter_kwargs(PaddleOCR.__init__, kwargs)))


def easyocr_engine(**options: Any) -> Callable[..., Any]:
    """EasyOCR, one Reader per language."""
    import easyocr

    languages = options.pop("languages", None) or options.pop("language", None)
    if isinstance(languages, str):
        languages = [languages]
    reader = easyocr.Reader(languages or ["en"], **_filter_kwargs(easyocr.Reader.__init__, options))
    return _wrap_engine(reader)


def surya_engine(**options: Any) -> Callable[..., Any]:
    """Surya OCR 2 (VLM; set ``SURYA_INFERENCE_BACKEND``/URL beforehand)."""
    from surya.inference import SuryaInferenceManager
    from surya.recognition import RecognitionPredictor

    manager = SuryaInferenceManager(**_filter_kwargs(SuryaInferenceManager.__init__, options))
    return _wrap_engine(RecognitionPredictor(manager))


def _wrap_engine(engine: Any) -> Callable[..., Any]:
    """Adapt a library object's call/`predict` into our engine contract.

    The engine receives the page PNG, decodes it with numpy/PIL when the library
    needs an array, and returns liteparse-shaped results.
    """
    call = getattr(engine, "predict", None) or engine
    if not callable(call):
        raise TypeError("OCR engine must be callable or expose `predict`")

    def engine_call(image: bytes, **kwargs: Any) -> List[Dict[str, Any]]:
        import numpy as np
        from PIL import Image
        import io

        array = np.array(Image.open(io.BytesIO(image)).convert("RGB"))
        raw = call(array)
        size = (array.shape[1], array.shape[0])
        if hasattr(raw, "__iter__") and not isinstance(raw, (dict, str)):
            raw = list(raw)
            if len(raw) == 1 and hasattr(raw[0], "txts"):
                return normalize_results(raw[0], size)
        return normalize_results(raw, size)

    return engine_call


# name -> (factory, extra, description)
ADAPTERS: Dict[str, Tuple[Callable[..., Any], str, str]] = {
    "rapidocr": (rapidocr_engine, "ocr-rapid", "RapidOCR / PP-OCRv6 ONNX (CPU-friendly)"),
    "paddleocr": (paddleocr_engine, "ocr-paddle", "PaddleOCR 3.x (best CJK)"),
    "easyocr": (easyocr_engine, "ocr-easyocr", "EasyOCR (80+ languages)"),
    "surya": (surya_engine, "ocr-surya", "Surya OCR 2 (VLM, GPU recommended)"),
}


def adapter_available(name: str) -> bool:
    """True when the package behind an adapter can be imported."""
    import importlib.util

    module = {
        "rapidocr": "rapidocr",
        "paddleocr": "paddleocr",
        "easyocr": "easyocr",
        "surya": "surya",
    }.get(name)
    if module is None:
        return False
    try:
        return importlib.util.find_spec(module) is not None
    except (ImportError, ValueError):
        return False


def resolve_engine(backend: str, options: Optional[Dict[str, Any]] = None) -> Tuple[Any, Dict[str, Any]]:
    """The engine for `backend`, plus options to hand it on each call.

    Packaged adapters are built here (the plugin options configure the model),
    while user-registered engines are called as they are and receive the plugin
    options per call.
    """
    options = dict(options or {})
    if backend in ADAPTERS:
        factory, extra, description = ADAPTERS[backend]
        try:
            return factory(**options), {}
        except ImportError as error:
            raise ValueError(
                f"OCR backend {backend!r} ({description}) failed to import: {error}. "
                f'Install it with: pip install "chunkr-pdf[{extra}]"'
            ) from None
        except TypeError as error:
            raise ValueError(
                f"OCR backend {backend!r} ({description}) rejected options {sorted(options)}: {error}"
            ) from None
    engine = chunkr.get_ocr_backend(backend)
    if engine is not None:
        return engine, options
    names = set(chunkr.ocr_backends())
    names.update(name for name in ADAPTERS if adapter_available(name))
    available = ", ".join(sorted(names)) or "none"
    raise ValueError(
        f"unknown OCR backend {backend!r}; available engines: {available}. "
        "Register one with chunkr.register_ocr_backend(name, engine), point "
        "ocr.server_url at an OCR server, or use tesseract."
    )


# ── loopback server ──────────────────────────────────────────────────────


def _blank_png(size: int = 8) -> bytes:
    """Minimal white PNG, used to warm the engine up."""

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    raw = b"".join(b"\x00" + b"\xff" * size * 3 for _ in range(size))
    header = struct.pack(">IIBBBBB", size, size, 8, 2, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw))
        + chunk(b"IEND", b"")
    )


def png_size(data: bytes) -> Tuple[int, int]:
    """`(width, height)` of a PNG, without decoding it."""
    if len(data) >= 24 and data[:8] == b"\x89PNG\r\n\x1a\n" and data[12:16] == b"IHDR":
        width, height = struct.unpack(">II", data[16:24])
        return width, height
    return 0, 0


def parse_multipart(body: bytes, content_type: str) -> Dict[str, bytes]:
    """Field name -> bytes for a `multipart/form-data` body (stdlib only)."""
    header = f"Content-Type: {content_type}\r\nMIME-Version: 1.0\r\n\r\n".encode()
    message = BytesParser().parsebytes(header + body)
    fields: Dict[str, bytes] = {}
    if not message.is_multipart():
        return fields
    for part in message.walk():
        if part.get_content_maintype() == "multipart":
            continue
        name = part.get_param("name", header="content-disposition")
        if name:
            fields[name] = part.get_payload(decode=True) or b""
    return fields


class _EngineCaller:
    """Calls an engine with the richest signature it supports."""

    def __init__(self, engine: Callable[..., Any]) -> None:
        function = engine
        if not callable(function) and hasattr(function, "recognize"):
            function = function.recognize
        if not callable(function):
            raise ValueError("OCR engine must be callable or expose `recognize`")
        self.function = function
        try:
            parameters = inspect.signature(function).parameters
        except (TypeError, ValueError):
            parameters = {}
        names = set(parameters)
        var_keyword = any(
            parameter.kind is inspect.Parameter.VAR_KEYWORD for parameter in parameters.values()
        )
        self.rich = var_keyword or bool(names & {"language", "options"})

    def __call__(self, image: bytes, language: str, options: Dict[str, Any]) -> Any:
        if self.rich:
            return self.function(image, language=language, options=options)
        return self.function(image)


class LoopbackOcrServer:
    """Serve one Python OCR engine over the liteparse OCR API on 127.0.0.1."""

    def __init__(
        self,
        engine: Callable[..., Any],
        *,
        language: str = "en",
        options: Optional[Dict[str, Any]] = None,
        timeout_ms: int = 60_000,
        concurrency: int = 1,
        port: int = 0,
        warmup: bool = True,
        name: str = "plugin",
    ) -> None:
        self.engine = _EngineCaller(engine)
        self.language = language
        self.options = options or {}
        self.timeout_ms = timeout_ms
        self.concurrency = max(1, concurrency)
        self.warmup = warmup
        self.name = name
        self.calls = 0
        self.failures = 0
        self._port = port
        self._server: Optional[ThreadingHTTPServer] = None
        self._thread: Optional[threading.Thread] = None
        self._pool = concurrent.futures.ThreadPoolExecutor(
            max_workers=self.concurrency, thread_name_prefix="chunkr-ocr"
        )

    # -- lifecycle -------------------------------------------------------
    def start(self) -> str:
        """Start the server (idempotent) and return its URL."""
        if self._server is not None:
            return self.url
        server = ThreadingHTTPServer(("127.0.0.1", self._port), self._handler())
        server.daemon_threads = True
        self._server = server
        self._port = server.server_address[1]
        self._thread = threading.Thread(
            target=self._serve, args=(server,), name="chunkr-ocr-server", daemon=True
        )
        self._thread.start()
        if self.warmup:
            try:
                self.recognize(_blank_png())
            except Exception:
                self.close()
                raise
        return self.url

    def _serve(self, server: ThreadingHTTPServer) -> None:
        try:
            server.serve_forever(poll_interval=0.2)
        except Exception:  # closed under us; nothing left to serve
            pass

    def close(self) -> None:
        """Stop the server and release the engine thread pool."""
        server, self._server = self._server, None
        thread, self._thread = self._thread, None
        if server is not None:
            # `shutdown()` waits for the serve loop, so only wait when it runs.
            if thread is not None and thread.is_alive():
                server.shutdown()
                thread.join(timeout=2.0)
            server.server_close()
        self._pool.shutdown(wait=False, cancel_futures=True)

    @property
    def url(self) -> str:
        return f"http://127.0.0.1:{self._port}/ocr"

    def __enter__(self) -> "LoopbackOcrServer":
        self.start()
        return self

    def __exit__(self, *_: Any) -> None:
        self.close()

    def __del__(self) -> None:  # backstop: never leak the listening socket
        server, self._server = getattr(self, "_server", None), None
        if server is not None:
            try:
                server.server_close()
            except Exception:
                pass

    # -- engine ----------------------------------------------------------
    def recognize(self, image: bytes) -> List[Dict[str, Any]]:
        """Run the engine over one page image, honoring the timeout."""
        self.calls += 1
        future = self._pool.submit(self.engine, image, self.language, self.options)
        try:
            return normalize_results(future.result(timeout=self.timeout_ms / 1000), png_size(image))
        except concurrent.futures.TimeoutError:
            self.failures += 1
            raise TimeoutError(
                f"OCR engine {self.name!r} exceeded ocr.plugin.timeout_ms={self.timeout_ms}"
            ) from None
        except Exception:
            self.failures += 1
            raise

    # -- http ------------------------------------------------------------
    def _handler(self) -> type:
        server = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, *_: Any) -> None:  # keep the console clean
                pass

            def _send(self, status: int, payload: Dict[str, Any]) -> None:
                body = json.dumps(payload).encode()
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def do_GET(self) -> None:  # noqa: N802 (http.server API)
                if self.path.startswith("/health"):
                    self._send(200, {"status": "healthy", "engine": server.name})
                else:
                    self._send(404, {"error": f"unknown path {self.path}"})

            def do_POST(self) -> None:  # noqa: N802 (http.server API)
                if not self.path.startswith("/ocr"):
                    self._send(404, {"error": f"unknown path {self.path}"})
                    return
                length = int(self.headers.get("Content-Length") or 0)
                body = self.rfile.read(length)
                content_type = self.headers.get("Content-Type") or ""
                try:
                    fields = parse_multipart(body, content_type)
                    image = fields.get("file")
                    if not image:
                        self._send(400, {"error": "missing `file` field"})
                        return
                    # The engine is built for the configured language; the
                    # request's `language` field is informational.
                    results = server.recognize(image)
                except TimeoutError as error:
                    self._send(504, {"error": str(error)})
                except Exception as error:  # surface the engine's message
                    self._send(500, {"error": f"{type(error).__name__}: {error}"})
                else:
                    self._send(200, {"results": results})

        return Handler
