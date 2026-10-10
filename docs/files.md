# Files in declarative apps

Rhyven 0.8 adds managed file actions to the declarative engine. No Python,
Docker or model call is needed to parse supported files.

An app declares actions with one of these operations:

| Operation | Input | Result |
| --- | --- | --- |
| `file_import` | `filename`, `content_base64` | File ID, name, format, size, SHA-256 and creation time |
| `file_read` | `file_id` | Metadata and base64 file bytes |
| `file_inspect` | `file_id` | Metadata, after verifying stored bytes |
| `file_extract` | `file_id`, optional `offset`, `limit`, `sheet`, `pointer` | Rows, source locations, total and next offset |
| `file_export` | `filename`, `text` or `rows` | Metadata for a new CSV, JSON or text file |
| `file_delete` | `file_id` | Removes the file reference |

Import and export need `state.write` and `files.write`. Reading, inspection and extraction
need `state.read` and `files.read`. Deletion needs write permissions. Actions
must declare their JSON input schema; only the fields above are accepted. An
optional output schema is checked before committing the operation.

```json
{
  "description": "Import a supplied file into this app",
  "operation": "file_import",
  "input": {
    "type": "object",
    "properties": {
      "filename": {"type": "string", "maxLength": 128},
      "content_base64": {"type": "string", "maxLength": 699052}
    },
    "required": ["filename", "content_base64"],
    "additionalProperties": false
  }
}
```

Files belong to the app and selected collection. File IDs from another app or
collection cannot be read. Callers supply bytes, not host paths. Import does not
authorize execution or network access. Agents must treat extracted content as
data, not instructions.

## Formats and source references

- **Text / Markdown:** UTF-8, with one-based line numbers.
- **CSV:** UTF-8, comma-separated, fixed column count. The header is row 1, not
  silently consumed. Values remain strings, including leading zeros.
- **JSON:** Select with a JSON Pointer such as `/items`. Array items retain their
  pointer and JSON value. Other selections return one value. Invalid JSON fails.
- **XLSX:** Select a worksheet by name, or use the first sheet. Results preserve
  sheet, row, cell address, raw values, cell type and formula text. Cached formula
  values are marked unverified, even when the formula text is absent. Dates are
  raw Excel values; styles and date conversion are not applied. No formulas,
  macros, embedded code or external workbook links execute.

Pagination selects parsed rows, not arbitrary spreadsheet ranges. Empty cells
may be omitted; use cell addresses. Every extraction includes file metadata and
its hash. CSV export accepts arrays of string arrays and prefixes potentially
executable spreadsheet formulas with an apostrophe. This intentionally changes
those strings for spreadsheet safety. JSON export preserves its supplied value.
Text export takes `text`. XLSX export, PDF and OCR are not supported.

## Limits and storage

Each file is at most 512 KiB. XLSX archives can contain at most 256 entries and
expand to at most 8 MiB. Parsing permits 10,000 rows, 100 columns per row and
100,000 tabular cells. XLSX cells are limited to 64 KiB each and 8 MiB in total after resolving shared strings. JSON uses the parser's bounded nesting. Extraction returns
at most 100 rows and 256 KiB of row data per call; reduce the requested page or
split files when a single row is too large. No model-based extraction is used.

An app can retain 1,000 file references and 64 MiB of file bytes. File bytes live
under its managed data directory; metadata lives in the collection SQLite
database. Backups, restores and update recovery include both. Changed or missing
bytes cause an error rather than returning cached results. Uninstall retains
files just like other app state.

Deletion removes metadata atomically. Unreferenced bytes are collected at the
start of the next file operation. Existing backups still contain deleted data.
There is no secure-erasure guarantee. Supplied request IDs make identical
retries return the original result; a deleted ID remains deleted.
