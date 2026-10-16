"""chunkr — Rust document chunking for LLMs, RAG and agents.

The native extension lives in :mod:`chunkr._core`; this module re-exports it,
discovers installed PDF parser backends (``pip install "chunkr-rs[pdf]"``) so
``PDFLoader(backend="auto")`` picks the high-fidelity extractor automatically,
and discovers tabular format extensions (``pip install chunkr-tabular``) that
register Parquet/Arrow extractors with the format dispatcher.

Set ``CHUNKR_NO_PDF_PLUGINS=1`` (or ``CHUNKR_NO_TABLE_PLUGINS=1``) to skip that
discovery entirely.
"""

from __future__ import annotations

import os as _os
from . import _core as _core
from ._core import *  # noqa: F401,F403  (public API lives in the extension)

__version__ = getattr(_core, "__version__", "unknown")

_BACKENDS_LOADED = False
_TABLES_LOADED = False


def _load_entry_points(group: str) -> None:
    """Register every installed entry point in `group`."""
    try:
        from importlib.metadata import entry_points
    except Exception:  # pragma: no cover - importlib.metadata always present on 3.8+
        return
    try:
        discovered = entry_points()
        selected = (
            discovered.select(group=group)
            if hasattr(discovered, "select")
            else discovered.get(group, [])
        )
    except Exception:  # pragma: no cover - a broken metadata cache must not break chunkr
        return
    for entry_point in selected:
        try:
            register = entry_point.load()
            register()
        except Exception:  # a broken plugin must not break `import chunkr`
            continue


def _load_pdf_backends() -> None:
    """Register every installed ``chunkr.pdf_backends`` entry point, once."""
    global _BACKENDS_LOADED
    if _BACKENDS_LOADED or _os.environ.get("CHUNKR_NO_PDF_PLUGINS"):
        _BACKENDS_LOADED = True
        return
    _BACKENDS_LOADED = True
    _load_entry_points("chunkr.pdf_backends")


def _load_table_loaders() -> None:
    """Register every installed ``chunkr.table_loaders`` entry point (Parquet, ...)."""
    global _TABLES_LOADED
    if _TABLES_LOADED or _os.environ.get("CHUNKR_NO_TABLE_PLUGINS"):
        _TABLES_LOADED = True
        return
    _TABLES_LOADED = True
    _load_entry_points("chunkr.table_loaders")


_load_pdf_backends()
_load_table_loaders()

__all__ = [name for name in dir(_core) if not name.startswith("_")]
