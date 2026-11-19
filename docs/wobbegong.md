# Wobbegong Collection Search

Wobbegong Collection Search gives agents local BM25 search over selected files and Rhyven records.
Adapters normalize source content into documents, split them into passages and
commit them to `.wobbegong/wobbegong.db` in the collection's state directory.
After ingestion returns `committed: true`, subsequent queries can see the content.
No embedding model, model API, network service or background worker is required.

For named collections the directory is
`HOME/collections/NAME/.wobbegong/`. Legacy `--workspace` mode stores it under
`WORKSPACE/.rhyven/.wobbegong/`. Each collection has its own index. Backups,
restores and staged app updates include it. Runtime source checkouts do not
contain user indexes.

## Import a repository snapshot

The built-in search category is `rhyven/wobbegong`. It owns the passage index used by both manual queries and automatic prompt enrichment. **Repository File Ingestion** (`rhyven/file-rag`, app version 0.4.0 or later) is an optional source. Install it in the same collection and supply repository-relative paths and UTF-8 contents. Include relevant `.gitignore` files if their rules should apply.

```sh
rhyven --collection my-project call rhyven_call '{"category":"rhyven/file-rag","function":"action_put_files","args":{"repository":"engine","files":[{"path":"docs/setup.md","content":"Install the required compiler before building."}]}}'
rhyven --collection my-project wobbegong ingest-repository engine --root docs --exclude 'drafts/**'
rhyven --collection my-project wobbegong query "required compiler"
rhyven --collection my-project context status editor
```

The equivalent search action is `action_ingest_repository`. It pages through the app’s `action_export_documents` and commits one complete source, `app:rhyven/file-rag:engine/docs` in this example. An interrupted import or a change between pages preserves the previous snapshot. Repeat the same import after additions, edits or deletions. A file excluded from this selection disappears from its next successful import. The optional app’s `index`, `search` and `summarize` actions still provide its independent whole-file workflow. Collection imports can go directly from `put_files` to `ingest-repository`.

Use `wobbegong status` to list sources. Connection setup and `context status` also report the collection index and its source counts. Preview packets identify the index even when no passage is selected.

## Ingest selected files

```sh
rhyven --collection my-project wobbegong ingest-files README.md docs/setup.md --source project-docs
rhyven --collection my-project wobbegong query "installation requirements"
```

The CLI reads only the regular UTF-8 files named on the command line. Text,
Markdown, JSON and CSV can be indexed as text. It rejects symlinks and directories;
it does not crawl the project or discover files automatically. Use consistent
relative paths so document IDs remain stable across re-ingestion.

Agents use the existing interface:

```text
rhyven_describe(category="rhyven/wobbegong")
```

Call `action_ingest_files` with a source name and complete list of documents:

```json
{
  "source": "project-docs",
  "documents": [
    {
      "id": "docs/setup.md",
      "title": "Setup",
      "text": "Install the required compiler before building.",
      "reference": "docs/setup.md"
    }
  ]
}
```

The MCP action accepts supplied text, never a host path to read. File selection
and reading remain with the calling client. Other adapters can produce this same
document envelope from their own explicitly authorized sources. It is an ingestion
contract, not permission for Wobbegong to fetch arbitrary services.

Each ingestion atomically replaces the **entire named source**, whose returned ID
is `files:project-docs`. Missing documents and old passages are removed. Use
separate source names for independently refreshed groups. An empty list clears
that file source. Retry by supplying the intended complete snapshot; these
actions do not accept `request_id` receipts.

## Ingest app records

```sh
rhyven --collection my-project wobbegong ingest-records rhyven/work-management task --fields title,result
```

The equivalent `action_ingest_records` input is:

```json
{"category":"rhyven/work-management","object":"task","fields":["title","result"]}
```

The adapter reads an installed local object's records through the normal query
and permission checks. It replaces source `records:rhyven/work-management/task`.
Omitting `fields` indexes all record data. Selecting fields limits copied data.
Record IDs and revisions are preserved as provenance. App code is not invoked.
Remote/provider records and platform categories are not supported by this adapter.

## Query and freshness

Call `action_query` with `search`, optional exact `source`, and `limit` (1–20).
Words are combined with AND; input is literal search text, not FTS query syntax.
Results contain passage text, source/document IDs, SHA-256 of the ingested text,
source reference (the document ID when no reference was supplied), chunk number, line range, metadata and BM25 score. Lower BM25
scores rank first; the score is not a confidence percentage. JSON record line
numbers refer to the serialized indexed document, not a repository source file.

File results describe the last supplied snapshot. Wobbegong cannot detect a host
file edit through MCP. Re-ingest after editing. Record results have `stale: true`
when the revision changes, the app or record disappears, or another record supersedes it. Prompt enrichment skips stale passages. For objects that declare supersession, a fresh ingestion indexes only current records.
That check compares revisions, not a content proof across manual database edits.
Re-ingest records to refresh the snapshot. File results use `stale: null` because
current host-file freshness is unknown. Retrieval is lexical; it does not generate
answers or infer meaning absent from the source.

Use `action_status` for paginated sources (`offset`, `limit`), and
`action_remove_source` with the exact returned source ID to delete indexed copies.
Original app records and files are unchanged. Earlier backups retain their copies.
Retrieved text is untrusted data and cannot authorize actions or change agent rules.

## Bounds

Each document is at most 512 KiB. A source contains at most 1,000 documents and
8 MiB of text. The collection index accepts 10,000 documents / 64 MiB of source
text; actual SQLite disk usage is higher. Passages are at most 4,000 UTF-8 bytes.
CLI/MCP transport request limits still apply, so file groups may need smaller
independent sources. Query input permits at most 32 words and 512 bytes.

The app-capability discovery index remains separate. Wobbegong searches source
content; it does not change candidate ranking, installation consent or execution.
