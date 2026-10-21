# chunkr-tabular

Parquet and Arrow IPC extraction for [chunkr](https://github.com/d1pankarmedhi/chunkr).

`chunkr-rs` ships extractors for text, office and caption formats but no
columnar reader, so a `.parquet` file would fail as an unsupported binary
payload. This package is the optional extension that reads those files with
[pyarrow](https://arrow.apache.org/docs/python/) and hands each batch of rows to
chunkr's format dispatcher. Extraction happens here; Markdown rendering, cell
escaping and the span metadata stay in chunkr's Rust core, so a Parquet
document is indistinguishable from a spreadsheet one.

```bash
pip install chunkr-tabular
```

## Quick start

Once installed, the loader registers itself on `import chunkr` and every entry
point that already exists works on columnar files:

```python
import chunkr

documents = chunkr.load_documents("events.parquet")     # one Document per batch
chunks = chunkr.DirectoryLoader(chunk_size=1200).load_and_chunk("lake/")
```

Each extracted batch becomes a Markdown table with the same metadata shape as a
worksheet, plus the fields that describe where it came from:

| Metadata | Meaning |
| :--- | :--- |
| `format` | `"table"` |
| `table_format` | source extension (`parquet`, `feather`, `arrow`, `ipc`) |
| `table_name` | batch name, e.g. `batch_3` |
| `table_index`, `total_tables` | position of the batch inside the file |
| `row_offset` | 0-based row index of the batch's first row in the file |
| `row_count`, `column_count` | batch shape |

`row_offset` and `row_count` are what make retrieval citable: a chunk can be
traced back to exact rows of the source file, and chunkr's packer reconciles
them rather than inheriting another batch's span.

```python
for chunk in chunkr.DirectoryLoader(chunk_size=1200).load_and_chunk("lake/"):
    print(chunk.metadata["table_name"], chunk.metadata["row_offset"], chunk.content[:60])
```

## Using it directly

```python
from chunkr_tabular import TableLoader

for table in TableLoader(rows_per_table=50_000).tables("events.parquet", "parquet"):
    print(table["name"], table["columns"], table["row_offset"], len(table["rows"]))
```

Those mappings are exactly what chunkr's dispatcher expects, and how the package
registers itself:

```python
import chunkr, chunkr_tabular

chunkr.register_table_loader("pyarrow", list(chunkr_tabular.EXTENSIONS), chunkr_tabular.load)
chunkr.unregister_table_loader("pyarrow")
chunkr.table_loaders()          # ["pyarrow"]
```

## Notes

- `rows_per_table` (default 10 000) bounds one extracted table. Chunkr's table
  chunker then splits rows into chunks of `chunk_size` characters, repeating the
  header, so this only needs to be large enough to give the chunker rows to work
  with.
- Parquet is read batch by batch, so file size is not bound by memory; Arrow IPC
  file and stream formats are both accepted.
- Nulls render as empty cells, booleans as `true`/`false`, nested lists and
  structs as comma-separated text, timestamps and decimals as their `str()`.
- `CHUNKR_NO_TABLE_PLUGINS=1` skips registration, e.g. to keep a process lean.
