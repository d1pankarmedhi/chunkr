"""chunkr-pdf — high-fidelity PDF parsing for chunkr.

Runs layout-aware extraction (headings, lists, tables, reading order, page
labels, optional OCR) through `liteparse` and maps the result to
:class:`chunkr.Document` objects using chunkr's own mapping and sanitizing
rules, so every knob behaves exactly like the native Rust `pdf` feature.

    from chunkr_pdf import PDFParser

    parser = PDFParser(preset="structure", output="markdown", granularity="block")
    documents = parser.load("report.pdf")

Installing the ``chunkr-rs[pdf]`` extra registers this backend automatically and
``chunkr.PDFLoader(backend="auto")`` will use it. Pass ``preset="faithful"`` (or
``sanitize={"enabled": False}``) for raw, untouched backend output.
"""

from __future__ import annotations

import json
from typing import Any, Dict, Iterable, Iterator, List, Optional, Union

import chunkr

from . import _liteparse, _ocr, parsers
from ._liteparse import BACKEND_NAME, liteparse_version

__all__ = [
    "PDFParser",
    "parsers",
    "BACKEND_NAME",
    "liteparse_version",
    "register",
    "is_available",
    "ocr_backends",
    "OCR_ADAPTERS",
]

PathLike = Union[str, bytes]

PRESETS = ("faithful", "retrieval", "structure")


def _deep_merge(base: Dict[str, Any], patch: Dict[str, Any]) -> Dict[str, Any]:
    for key, value in patch.items():
        if isinstance(value, dict) and isinstance(base.get(key), dict):
            _deep_merge(base[key], value)
        else:
            base[key] = value
    return base


#: OCR engine adapters shipped with this package: name -> pip extra.
OCR_ADAPTERS = {name: extra for name, (_factory, extra, _description) in _ocr.ADAPTERS.items()}


def register() -> None:
    """Register this package's backend and OCR adapters with chunkr (idempotent)."""
    _liteparse.register()


def ocr_backends() -> List[str]:
    """OCR engines usable right now: packaged adapters plus user engines."""
    register()
    names = set(chunkr.ocr_backends())
    names.update(name for name in OCR_ADAPTERS if _ocr.adapter_available(name))
    return sorted(names)


def is_available() -> bool:
    """True when liteparse is importable and the backend is registered."""
    try:
        register()
    except Exception:
        return False
    return BACKEND_NAME in chunkr.pdf_backends()


class PDFParser:
    """Configurable high-fidelity PDF parser built on liteparse.

    Every option mirrors chunkr's `ParserConfig`: pass a `preset` and override
    any nested group (`output`, `granularity`, `extract`, `sanitize`, `ocr`,
    `scope`, `on_error`, ...) with plain dicts. Nothing is hidden and nothing is
    forced: `sanitize={"enabled": False}` returns backend output untouched.
    """

    def __init__(self, preset: Optional[str] = "retrieval", **overrides: Any) -> None:
        if preset is not None and preset not in PRESETS:
            raise ValueError(f"unknown preset {preset!r}; expected one of {PRESETS}")
        base = json.loads(chunkr.ParserConfig(preset).to_json())
        self._config: Dict[str, Any] = _deep_merge(base, overrides)
        # Fail fast on unknown keys or bad values instead of at parse time.
        chunkr.ParserConfig(self._config)
        self._ocr_servers: Dict[str, _ocr.LoopbackOcrServer] = {}

    # ── configuration ────────────────────────────────────────────────────
    @property
    def config(self) -> Dict[str, Any]:
        """The resolved config as a plain dict."""
        return json.loads(json.dumps(self._config))

    def to_parser_config(self) -> Any:
        """The same config as a `chunkr.ParserConfig`."""
        return chunkr.ParserConfig(self._config)

    def __repr__(self) -> str:
        cfg = self._config
        return (
            f"PDFParser(backend={BACKEND_NAME} v{liteparse_version()}, "
            f"output={cfg.get('output')!r}, granularity={cfg.get('granularity')!r}, "
            f"sanitize={cfg.get('sanitize', {}).get('enabled')}, "
            f"tables={(cfg.get('extract') or {}).get('tables')!r}, "
            f"ocr={(cfg.get('ocr') or {}).get('mode')!r})"
        )

    # ── OCR ---------------------------------------------------------------
    def ocr_settings(self) -> Dict[str, Any]:
        """The resolved `ocr` group, with backend defaults filled in."""
        return dict(self._config.get("ocr") or {})

    def _ocr_overrides(self, *, enabled: Optional[bool] = None) -> Dict[str, Any]:
        """liteparse kwargs that depend on the OCR backend.

        `tesseract` is built into liteparse, `server` needs a URL, and every
        other name is an engine served by a loopback proxy. Returns the override
        dict for `_liteparse`, starting the proxy on first use.
        """
        ocr = self.ocr_settings()
        mode = ocr.get("mode", "off")
        if enabled is None:
            enabled = mode in ("auto", "always", "server")
        if not enabled:
            return {"ocr_enabled": False}
        backend = str(ocr.get("backend") or "tesseract").strip().lower()
        if mode == "server" or backend in ("server", "http", "http-server"):
            if not ocr.get("server_url"):
                raise ValueError('ocr.backend="server" needs ocr.server_url')
            return {"ocr_enabled": True, "ocr_language": self._language(iso=True)}
        if backend in ("ppocr", "pp-ocr", "oar", "oar-ocr"):
            raise ValueError(
                'ocr.backend="ppocr" needs the Rust `pdf-ocr-ppocr` feature, which is not built '
                'yet. Use backend="server" with a PP-OCR service, or one of the Python '
                "engines: " + ", ".join(sorted(OCR_ADAPTERS))
            )
        if backend in ("", "tesseract"):
            return {"ocr_enabled": True, "ocr_language": self._language(iso=False)}
        plugin = ocr.get("plugin") or {}
        engine, call_options = _ocr.resolve_engine(backend, plugin.get("options"))
        server = self._ensure_ocr_server(backend, engine, call_options)
        return {
            "ocr_enabled": True,
            "ocr_server_url": server.url,
            "ocr_language": self._language(iso=True),
            "ocr_server_headers": None,
        }

    def _language(self, *, iso: bool) -> str:
        language = str(self.ocr_settings().get("language") or "eng")
        return chunkr.ocr_language(language, "iso" if iso else "tesseract")

    def _ensure_ocr_server(
        self, backend: str, engine: Any, call_options: Optional[Dict[str, Any]] = None
    ) -> "_ocr.LoopbackOcrServer":
        """Start (once) the loopback server serving `engine`."""
        server = self._ocr_servers.get(backend)
        if server is None:
            plugin = self.ocr_settings().get("plugin") or {}
            server = _ocr.LoopbackOcrServer(
                engine,
                language=self._language(iso=True),
                options=dict(call_options or {}),
                timeout_ms=int(plugin.get("timeout_ms", 60_000)),
                concurrency=int(plugin.get("concurrency", 1)),
                port=int(plugin.get("port", 0)),
                warmup=bool(plugin.get("warmup", True)),
                name=backend,
            )
            # Warm-up failures (missing package, bad credentials) surface here.
            server.start()
            self._ocr_servers[backend] = server
        return server

    @property
    def ocr_servers(self) -> Dict[str, "_ocr.LoopbackOcrServer"]:
        """Loopback servers currently serving an engine, by backend name."""
        return dict(self._ocr_servers)

    def close(self) -> None:
        """Stop every loopback OCR server this parser started."""
        for server in self._ocr_servers.values():
            server.close()
        self._ocr_servers.clear()

    def __enter__(self) -> "PDFParser":
        return self

    def __exit__(self, *_: Any) -> None:
        self.close()

    def __del__(self) -> None:
        try:
            self.close()
        except Exception:
            pass

    # ── parsing ──────────────────────────────────────────────────────────
    def payload(self, source: PathLike) -> str:
        """Raw backend payload (JSON) before chunkr maps it."""
        return self._payload(source)

    def documents(self, source: PathLike) -> List[chunkr.Document]:
        """Documents for `source`, honoring `granularity`."""
        return self._documents(source, self._config)

    def load(self, source: PathLike) -> List[chunkr.Document]:
        """Alias for :meth:`documents`."""
        return self.documents(source)

    def load_pages(self, source: PathLike) -> List[chunkr.Document]:
        """One document per page, whatever `granularity` says."""
        return self._documents(source, {**self._config, "granularity": "page"})

    def load_document(self, source: PathLike) -> chunkr.Document:
        """A single document holding every page."""
        documents = self._documents(source, {**self._config, "granularity": "document"})
        if documents:
            return documents[0]
        return chunkr.Document("", {"parser_backend": BACKEND_NAME})

    def _documents(self, source: PathLike, config: Dict[str, Any]) -> List[chunkr.Document]:
        """Extraction uses this parser's config; only mapping differs per call."""
        return self._map(self._payload(source), source, config)

    def text(self, source: PathLike) -> str:
        """Plain text of the document, page by page."""
        return "\n\n".join(doc.content for doc in self.load_pages(source) if doc.content.strip())

    def blocks(self, source: PathLike) -> List[Dict[str, Any]]:
        """Every layout block with its page number and type, in reading order."""
        out: List[Dict[str, Any]] = []
        for document in self.load_pages(source):
            for block in document.metadata.get("blocks") or []:
                out.append(
                    {
                        "page_number": document.metadata.get("page_number"),
                        "page_label": document.metadata.get("page_label"),
                        **block,
                    }
                )
        return out

    def stream(self, source: PathLike, batch_size: int = 25) -> Iterator[List[chunkr.Document]]:
        """Yield documents batch by batch, bounded memory.

        `ocr.mode="auto"` needs two passes (find broken pages, then OCR them),
        which the batched reader cannot express; those parses are chunked in
        Python instead, so only the output stays streaming.
        """
        from pathlib import Path

        data = source if isinstance(source, bytes) else str(Path(source))
        if self._auto_gating():
            documents = self._documents(source, self._config)
            for start in range(0, len(documents), batch_size):
                yield documents[start : start + batch_size]
            return
        overrides = self._ocr_overrides()
        for payload in _liteparse.parse_batches(data, self._config, batch_size, overrides):
            yield self._map(payload, source)

    def is_complex(self, source: PathLike) -> List[Dict[str, Any]]:
        """Per-page OCR/layout signals, cheap pre-parse pass."""
        from pathlib import Path

        data = source if isinstance(source, bytes) else str(Path(source))
        return _liteparse.complexity_rows(data, self._config)

    # ── internals ────────────────────────────────────────────────────────
    def _auto_gating(self) -> bool:
        return self.ocr_settings().get("mode") == "auto"

    def _payload(self, source: PathLike) -> str:
        """Backend payload, running the two-pass OCR gate for `mode="auto"`."""
        from pathlib import Path

        data = source if isinstance(source, bytes) else str(Path(source))
        if not self._auto_gating():
            return _liteparse.payload(data, self._config, self._ocr_overrides())

        # Pass 1: text layer only, with complexity signals for the gate.
        first = json.loads(
            _liteparse.payload(
                data,
                self._config,
                {"ocr_enabled": False, "include_complexity": True},
            )
        )
        selected = chunkr.pages_needing_ocr(first, self.to_parser_config())
        if not selected:
            return json.dumps(self._clean_complexity(first), ensure_ascii=False)

        # Pass 2: OCR only the gated pages, then splice them back in.
        replacements = json.loads(
            _liteparse.payload(
                data,
                self._config,
                {
                    **self._ocr_overrides(enabled=True),
                    "include_complexity": True,
                    "target_pages": chunkr.format_page_range(selected),
                },
            )
        )
        by_page = {page["page_number"]: page for page in replacements.get("pages", [])}
        merged = [
            by_page.get(page["page_number"], page) for page in first.get("pages", [])
        ]
        return json.dumps(
            self._clean_complexity({"version": first.get("version"), "pages": merged}),
            ensure_ascii=False,
        )

    def _clean_complexity(self, payload: Dict[str, Any]) -> Dict[str, Any]:
        """Drop the gate's complexity fields unless the user asked for them."""
        if self._config.get("include_complexity"):
            return payload
        for page in payload.get("pages", []):
            page.pop("complexity", None)
        return payload

    def _map(
        self, payload: str, source: PathLike, config: Optional[Dict[str, Any]] = None
    ) -> List[chunkr.Document]:
        name = None if isinstance(source, bytes) else str(source)
        return chunkr.pages_to_documents(
            payload,
            config=chunkr.ParserConfig(config or self._config),
            source=name,
            backend=BACKEND_NAME,
        )
