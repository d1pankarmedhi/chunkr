"""chunkr-tabular — Parquet and Arrow IPC extraction for chunkr.

``chunkr-rs`` core stays free of columnar dependencies. This package is the
optional extension that reads columnar files with `pyarrow` and hands each
batch of rows to chunkr's format dispatcher, which renders the Markdown table
and attaches the span metadata itself. Extraction happens here; every mapping
rule stays in the Rust core, so these formats behave exactly like a spreadsheet.

    import chunkr, chunkr_tabular

    documents = chunkr.load_documents("events.parquet")   # one Document per batch
    chunks = chunkr.DirectoryLoader(chunk_size=1200).load_and_chunk("lake/")

Installing registers this loader automatically through the
``chunkr.table_loaders`` entry point.
"""

from __future__ import annotations

from typing import Any, Dict, Iterator, List, Optional

import chunkr

__all__ = [
    "EXTENSIONS",
    "DEFAULT_ROWS_PER_TABLE",
    "TableLoader",
    "load",
    "register",
    "is_available",
    "pyarrow_version",
]

#: Extensions this package claims, all read through pyarrow.
EXTENSIONS = ("parquet", "feather", "arrow", "ipc")

#: Rows per extracted table: enough rows that chunkr's table chunker decides the
#: final chunk boundaries, few enough that one extract never holds the file.
DEFAULT_ROWS_PER_TABLE = 10_000

try:  # pragma: no cover - pyarrow is a hard dependency
    import pyarrow as pa
except Exception:  # pragma: no cover - reported through is_available()
    pa = None  # type: ignore[assignment]


def pyarrow_version() -> str:
    """Installed pyarrow version, or ``"unavailable"``."""
    return getattr(pa, "__version__", "unavailable")


def is_available() -> bool:
    """True when pyarrow is importable and this loader is registered."""
    try:
        register()
    except Exception:
        return False
    return "pyarrow" in chunkr.table_loaders()


class TableLoader:
    """Reads columnar files into chunkr ``Document``s.

    ``rows_per_table`` bounds how many rows one extracted table holds.
    """

    def __init__(self, rows_per_table: int = DEFAULT_ROWS_PER_TABLE) -> None:
        if rows_per_table <= 0:
            raise ValueError("rows_per_table must be positive")
        self.rows_per_table = rows_per_table

    def tables(
        self, source: Any, extension: str, source_name: Optional[str] = None
    ) -> Iterator[Dict[str, Any]]:
        """Yield one table mapping per batch of rows, in file order.

        ``source`` is a path (preferred: pyarrow streams it) or a payload.
        """
        for index, (batch, row_offset) in enumerate(self._batches(source, extension)):
            columns = [str(field.name) for field in batch.schema]
            if not columns:
                continue
            yield {
                "name": f"batch_{index}",
                "columns": columns,
                "rows": [_row(batch, position) for position in range(batch.num_rows)],
                "row_offset": row_offset,
            }

    def _batches(self, source: Any, extension: str) -> Iterator[tuple]:
        """Yield ``(RecordBatch, row_offset)`` pairs for the payload."""
        self._require_pyarrow()
        if extension == "parquet":
            batches = self._parquet_batches(source)
        else:
            batches = self._arrow_batches(source)

        row_offset = 0
        for batch in batches:
            for sliced in _slices(batch, self.rows_per_table):
                yield sliced, row_offset
                row_offset += sliced.num_rows

    def _parquet_batches(self, source: Any) -> Iterator[Any]:
        # pyarrow hands Parquet over in whole row groups; `_slices` bounds them.
        yield from pa.parquet.ParquetFile(_handle(source)).iter_batches(
            batch_size=self.rows_per_table
        )

    def _arrow_batches(self, source: Any) -> Iterator[Any]:
        # Arrow IPC is either file format (with a footer) or stream format: the
        # file reader exposes batches by index, the stream reader is iterable.
        # The fallback opens a fresh handle so a failed attempt cannot leave it
        # positioned.
        try:
            reader = pa.ipc.open_file(_handle(source))
        except Exception:
            yield from pa.ipc.open_stream(_handle(source))
            return
        if hasattr(reader, "num_record_batches"):
            for index in range(reader.num_record_batches):
                yield reader.get_batch(index)
        else:
            yield from reader

    def _require_pyarrow(self) -> None:
        if pa is None:  # pragma: no cover - dependency is declared
            raise RuntimeError(
                "chunkr-tabular needs pyarrow: pip install 'chunkr-tabular'"
            )


def _handle(source: Any) -> Any:
    """pyarrow input for a path or an in-memory payload."""
    if isinstance(source, (bytes, bytearray, memoryview)):
        return pa.BufferReader(bytes(source))
    return str(source)


def _slices(batch: Any, rows_per_table: int) -> Iterator[Any]:
    """Split one batch into at most `rows_per_table` rows, keeping it whole when it fits."""
    if batch.num_rows <= rows_per_table:
        yield batch
        return
    for start in range(0, batch.num_rows, rows_per_table):
        yield batch.slice(start, rows_per_table)


def _row(batch: Any, position: int) -> List[Optional[str]]:
    """One row's cells as text, ``None`` for nulls (chunkr renders those empty)."""
    return [_cell(batch.column(index)[position].as_py()) for index in range(batch.num_columns)]


def _cell(value: Any) -> Optional[str]:
    """One cell as text."""
    if value is None:
        return None
    if isinstance(value, str):
        return value
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float)):
        return str(value)
    if isinstance(value, (bytes, bytearray)):
        return value.decode("utf-8", "replace")
    if isinstance(value, (list, tuple)):
        return ", ".join(_cell(item) or "" for item in value)
    if isinstance(value, dict):
        return ", ".join(f"{key}={_cell(item)}" for key, item in value.items())
    # datetimes, dates, decimals, UUIDs, ...
    return str(value)


def load(
    source: Any, extension: str, source_name: Optional[str] = None
) -> Iterator[Dict[str, Any]]:
    """Extract tables from ``source``; the entry point chunkr calls."""
    return TableLoader().tables(source, extension, source_name)


def register() -> None:
    """Register this package's loader with chunkr (idempotent)."""
    chunkr.register_table_loader("pyarrow", list(EXTENSIONS), load)
