"""End-to-end tests for the chunkr-tabular extension.

Run after `maturin develop --features python` in the repo root and
`pip install -e packages/chunkr-tabular`.
"""

from __future__ import annotations

import datetime
import subprocess
import sys
from pathlib import Path

import pyarrow as pa
import pyarrow.feather
import pyarrow.ipc
import pyarrow.parquet
import pytest

import chunkr
import chunkr_tabular
from chunkr_tabular import TableLoader

SCHEMA = pa.schema(
    [
        ("id", pa.int64()),
        ("name", pa.string()),
        ("score", pa.float64()),
        ("ok", pa.bool_()),
        ("when", pa.timestamp("ms")),
    ]
)


def sample_table(rows: int = 5) -> pa.Table:
    return pa.Table.from_pylist(
        [
            {
                "id": i,
                "name": f"row {i}",
                "score": i / 2,
                "ok": i % 2 == 0,
                "when": datetime.datetime(2024, 1, 1) + datetime.timedelta(hours=i),
            }
            for i in range(rows)
        ],
        schema=SCHEMA,
    )


@pytest.fixture(autouse=True)
def registered():
    """Every test runs with the packaged loader registered."""
    chunkr_tabular.register()
    yield
    chunkr_tabular.register()


def test_register_is_idempotent_and_widens_the_dispatcher():
    chunkr_tabular.register()
    chunkr_tabular.register()

    assert chunkr_tabular.is_available()
    assert "pyarrow" in chunkr.table_loaders()
    for extension in ("parquet", "feather", "arrow", "ipc"):
        assert extension in chunkr.supported_extensions()
    assert pyarrow_version_tuple() >= (14,)


def pyarrow_version_tuple():
    return tuple(int(part) for part in pa.__version__.split(".")[:2])


def test_parquet_loads_as_a_markdown_table_with_spans(tmp_path: Path):
    path = tmp_path / "events.parquet"
    pyarrow.parquet.write_table(sample_table(5), path)

    documents = chunkr.load_documents(str(path))
    assert len(documents) == 1

    document = documents[0]
    lines = document.content.split("\n")
    assert lines[0] == "| id | name | score | ok | when |"
    assert lines[1] == "| --- | --- | --- | --- | --- |"
    assert len(lines) == 2 + 5, "header, separator and every row"

    assert document.metadata["format"] == "table"
    assert document.metadata["table_format"] == "parquet"
    assert document.metadata["table_name"] == "batch_0"
    assert document.metadata["table_index"] == 0
    assert document.metadata["total_tables"] == 1
    assert document.metadata["row_offset"] == 0
    assert document.metadata["row_count"] == 5
    assert document.metadata["column_count"] == 5
    assert document.metadata["file_name"] == "events.parquet"


def test_cell_values_are_stringified(tmp_path: Path):
    path = tmp_path / "values.parquet"
    table = pa.Table.from_pylist(
        [
            {
                "text": "plain",
                "missing": None,
                "flag": True,
                "integer": 7,
                "real": 1.5,
                "list": [1, 2, 3],
                "nested": {"a": 1, "b": "two"},
                "day": datetime.date(2024, 3, 4),
                "pipe": "a|b",
            }
        ]
    )
    pyarrow.parquet.write_table(table, path)

    row = chunkr.load_documents(str(path))[0].content.split("\n")[2]
    assert row.startswith("| plain |  | true | 7 | 1.5 | 1, 2, 3 | a=1, b=two | 2024-03-04 |")
    assert row.endswith("a\\|b |"), "pipes are escaped so the table stays parseable"


def test_row_offsets_track_batches(tmp_path: Path):
    path = tmp_path / "many.parquet"
    pyarrow.parquet.write_table(sample_table(25), path)

    documents = chunkr.load_documents(str(path))

    # The packaged loader asks for 10 000 rows per table; this file is one table.
    assert len(documents) == 1, documents

    # A smaller batch size keeps offsets contiguous across tables.
    tables = list(TableLoader(rows_per_table=10).tables(str(path), "parquet"))
    assert [table["row_offset"] for table in tables] == [0, 10, 20]
    assert [table["name"] for table in tables] == ["batch_0", "batch_1", "batch_2"]
    assert [len(table["rows"]) for table in tables] == [10, 10, 5]

    # A larger row_group layout still yields whole, ordered rows.
    ids = [
        cell
        for table in tables
        for row in table["rows"]
        for index, cell in enumerate(row)
        if table["columns"][index] == "id"
    ]
    assert ids == [str(i) for i in range(25)]


def test_bytes_payload_is_accepted(tmp_path: Path):
    path = tmp_path / "payload.parquet"
    pyarrow.parquet.write_table(sample_table(3), path)
    data = path.read_bytes()

    documents = chunkr.load_documents_from_bytes(data, "parquet", source="memory.parquet")
    assert len(documents) == 1
    assert documents[0].metadata["row_count"] == 3
    assert documents[0].metadata["format"] == "table"


def test_arrow_ipc_file_and_stream_are_accepted(tmp_path: Path):
    table = sample_table(4)

    file_path = tmp_path / "table.arrow"
    with pa.OSFile(str(file_path), "wb") as sink:
        with pa.ipc.new_file(sink, table.schema) as writer:
            writer.write_table(table)

    stream_path = tmp_path / "table.feather"
    with pa.OSFile(str(stream_path), "wb") as sink:
        with pa.ipc.new_stream(sink, table.schema) as writer:
            writer.write_table(table)

    for path, extension in ((file_path, "arrow"), (stream_path, "feather")):
        documents = chunkr.load_documents(str(path))
        assert len(documents) == 1, path
        assert documents[0].metadata["table_format"] == extension
        assert documents[0].metadata["row_count"] == 4
        assert documents[0].content.startswith("| id | name | score | ok | when |")


def test_directory_loader_chunks_columnar_files_in_parallel(tmp_path: Path):
    # Four files exercise the parallel directory path, where a registered Python
    # loader runs on a worker thread: a GIL deadlock would hang here.
    for index in range(4):
        pyarrow.parquet.write_table(sample_table(40), tmp_path / f"part_{index}.parquet")

    chunks = chunkr.DirectoryLoader(chunk_size=200, overlap=0).load_and_chunk(str(tmp_path))

    assert chunks, "no chunks produced"
    sources = {chunk.metadata["file_name"] for chunk in chunks}
    assert sources == {f"part_{index}.parquet" for index in range(4)}
    for chunk in chunks:
        # Routed chunks are Markdown tables; source provenance stays in table_format.
        assert chunk.metadata["table_format"] == "parquet"
        assert chunk.metadata["row_count"] > 0
        assert "row_offset" in chunk.metadata
        assert chunk.content.startswith("| id | name |"), "header repeats per chunk"


def test_corrupt_payload_reports_the_source(tmp_path: Path):
    path = tmp_path / "broken.parquet"
    path.write_bytes(b"definitely not parquet")

    with pytest.raises(ValueError) as error:
        chunkr.load_documents(str(path))
    assert "broken.parquet" in str(error.value)


def test_unregistering_restores_the_binary_fallback(tmp_path: Path):
    path = tmp_path / "events.parquet"
    pyarrow.parquet.write_table(sample_table(2), path)

    assert chunkr.unregister_table_loader("pyarrow")
    try:
        assert "pyarrow" not in chunkr.table_loaders()
        assert "parquet" not in chunkr.supported_extensions()
        with pytest.raises(ValueError) as error:
            chunkr.load_documents(str(path))
        assert "binary payload" in str(error.value)
    finally:
        chunkr_tabular.register()


def test_entry_point_registration_loads_with_a_fresh_interpreter():
    # `import chunkr` must discover the installed entry point on its own.
    probe = (
        "import chunkr;"
        "assert 'parquet' in chunkr.supported_extensions(), chunkr.supported_extensions();"
        "assert 'pyarrow' in chunkr.table_loaders(), chunkr.table_loaders();"
        "print('ok')"
    )
    result = subprocess.run(
        [sys.executable, "-c", probe], capture_output=True, text=True, timeout=120
    )
    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == "ok"
