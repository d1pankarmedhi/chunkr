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

from . import _liteparse
from ._liteparse import BACKEND_NAME, liteparse_version

__all__ = ["PDFParser", "BACKEND_NAME", "liteparse_version", "register", "is_available"]

PathLike = Union[str, bytes]

PRESETS = ("faithful", "retrieval", "structure")


def _deep_merge(base: Dict[str, Any], patch: Dict[str, Any]) -> Dict[str, Any]:
    for key, value in patch.items():
        if isinstance(value, dict) and isinstance(base.get(key), dict):
            _deep_merge(base[key], value)
        else:
            base[key] = value
    return base


def register() -> None:
    """Register this package's backend with chunkr (idempotent)."""
    _liteparse.register()


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

    # ── parsing ──────────────────────────────────────────────────────────
    def payload(self, source: PathLike) -> str:
        """Raw backend payload (JSON) before chunkr maps it."""
        from pathlib import Path

        data = source if isinstance(source, bytes) else str(Path(source))
        return _liteparse.parse_payload(data, json.dumps(self._config))

    def documents(self, source: PathLike) -> List[chunkr.Document]:
        """Documents for `source`, honoring `granularity`."""
        return self._map(self.payload(source), source)

    def load(self, source: PathLike) -> List[chunkr.Document]:
        """Alias for :meth:`documents`."""
        return self.documents(source)

    def load_pages(self, source: PathLike) -> List[chunkr.Document]:
        """One document per page, whatever `granularity` says."""
        page_config = _deep_merge(self.config, {"granularity": "page"})
        parser = type(self)(preset=None, **page_config)
        return parser.documents(source)

    def load_document(self, source: PathLike) -> chunkr.Document:
        """A single document holding every page."""
        parser = type(self)(preset=None, **self.config, granularity="document")
        documents = parser.documents(source)
        if documents:
            return documents[0]
        return chunkr.Document("", {"parser_backend": BACKEND_NAME})

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
        """Yield documents batch by batch, bounded memory."""
        from pathlib import Path

        data = source if isinstance(source, bytes) else str(Path(source))
        for payload in _liteparse.parse_batches(data, self._config, batch_size):
            yield self._map(payload, source)

    def is_complex(self, source: PathLike) -> List[Dict[str, Any]]:
        """Per-page OCR/layout signals, cheap pre-parse pass."""
        from pathlib import Path

        data = source if isinstance(source, bytes) else str(Path(source))
        return _liteparse.complexity_rows(data, self._config)

    # ── internals ────────────────────────────────────────────────────────
    def _map(self, payload: str, source: PathLike) -> List[chunkr.Document]:
        name = None if isinstance(source, bytes) else str(source)
        return chunkr.pages_to_documents(
            payload,
            config=self.to_parser_config(),
            source=name,
            backend=BACKEND_NAME,
        )
