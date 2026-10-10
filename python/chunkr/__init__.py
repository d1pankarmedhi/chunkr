"""chunkr — Rust document chunking for LLMs, RAG and agents.

The native extension lives in :mod:`chunkr._core`; this module re-exports it and
discovers installed PDF parser backends (``pip install "chunkr-rs[pdf]"``) so
``PDFLoader(backend="auto")`` picks the high-fidelity extractor automatically.

Set ``CHUNKR_NO_PDF_PLUGINS=1`` to skip backend discovery entirely.
"""

from __future__ import annotations

import os as _os
from . import _core as _core
from ._core import *  # noqa: F401,F403  (public API lives in the extension)

__version__ = getattr(_core, "__version__", "unknown")

_BACKENDS_LOADED = False


def _load_pdf_backends() -> None:
    """Register every installed ``chunkr.pdf_backends`` entry point, once."""
    global _BACKENDS_LOADED
    if _BACKENDS_LOADED or _os.environ.get("CHUNKR_NO_PDF_PLUGINS"):
        _BACKENDS_LOADED = True
        return
    _BACKENDS_LOADED = True
    try:
        from importlib.metadata import entry_points
    except Exception:  # pragma: no cover - importlib.metadata always present on 3.8+
        return
    try:
        discovered = entry_points()
        selected = (
            discovered.select(group="chunkr.pdf_backends")
            if hasattr(discovered, "select")
            else discovered.get("chunkr.pdf_backends", [])
        )
    except Exception:  # pragma: no cover - a broken metadata cache must not break chunkr
        return
    for entry_point in selected:
        try:
            register = entry_point.load()
            register()
        except Exception:  # a broken plugin must not break `import chunkr`
            continue


_load_pdf_backends()

__all__ = [name for name in dir(_core) if not name.startswith("_")]
