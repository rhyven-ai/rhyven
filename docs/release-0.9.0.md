# Rhyven 0.9.0

This release adds local retrieval and tools for inspecting and developing apps.

- **Wobbegong** indexes selected UTF-8 files and local app records into each
  collection's `.wobbegong/wobbegong.db`. Agents query committed passages through
  `rhyven/wobbegong`, using the existing three-tool interface. Results include
  source references, line ranges, content hashes and record-revision staleness.
  Source replacement is atomic; backups and staged updates retain the index.
- **Serving identity** identifies the actual engine executable, version, process,
  actor and collection. `connect --check --configured` probes a saved client
  entry. Reports distinguish a separate probe from the running agent session.
- **Request reports** inspect the current actor's receipts and workflow evidence
  without retrying work. Pending external outcomes remain unknown. New receipt
  encoding metadata prevents app result fields being confused with engine status.
- **Audit change queries** provide bounded, cursor-based summaries of existing
  engine audit events. This supports polling without adding a scheduler.
- **`app check`** validates and tests a draft in temporary state and can write
  the exact tested package. Failures identify the case and failed expectation.

Wobbegong uses SQLite FTS5/BM25 without embeddings or model calls. Both adapters
require explicit ingestion; file changes are not watched automatically. Its
content index is separate from app-capability discovery. The audit feed does not
automatically observe external services, internal app databases or index ingestion.
Action previews remain a design proposal and are not included in this release.

App contracts, existing record/retry behavior and installation consent remain
compatible. The new inspection calls are available to authenticated REST clients;
package management retains its separate management credential and human approval.

See [Wobbegong](wobbegong.md), [development and inspection](development-and-inspection.md),
and [upgrade notes](migration-0.9.md). Binary publication status is recorded in
[release status](release-status.md).
