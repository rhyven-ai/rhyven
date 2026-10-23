# Upgrade to Rhyven 0.9

The 0.9.0 source retains existing app package formats and the three universal
tool names. Apps and container images keep their independent versions.

Back up collections before upgrading. Restart long-running MCP bridges, shared
servers and supervisors after replacing the executable. Updating a CLI on PATH
does not update an already-running process or a client entry pinned to another
executable. Check `rhyven_categories.engine` from the actual agent session.

## Discovery and inspection

Discovery includes an additive `engine` identity and a new built-in category,
`rhyven/wobbegong`. Clients must discover app IDs rather than assume a fixed
category count. `apps()` still lists installed app packages; platform categories
are included by the universal category discovery operation.

New marketplace inspection actions are `action_connection_info`,
`action_request_report` and `action_changes`. They require ordinary authenticated
REST access, while package-management actions retain the existing separate
management credential. Actor labels and collections remain trusted-client state
boundaries, not per-user authorization.

A new additive `receipt_context` table identifies the encoding of new receipts.
Existing receipts and retry fingerprints are unchanged. Reports label older
receipts `recorded_unclassified` when metadata is unavailable rather than guessing
whether an app's `status` field describes execution. No receipt does not prove an
external action never ran. Reconcile uncertain effects before starting new work.

Audit cursors belong to their collection and app filter. Restart the scan from 0
after changing either, or when reconciling a rollback or restored collection.

## Wobbegong state

The index is created on first ingestion. Named collections use
`HOME/collections/NAME/.wobbegong/wobbegong.db`; legacy workspaces use
`WORKSPACE/.rhyven/.wobbegong/wobbegong.db`. It is private local state and belongs
outside source control and source exports.

Each ingestion replaces the complete selected source. Use separate source names
for independently refreshed file groups. Record ingestion is an explicit snapshot;
queries mark changed or missing records stale. Re-ingest after source changes.
Earlier backups retain their indexed copies even after source removal.

0.9 backups and staged app updates include Wobbegong. Backups containing the new
index path require 0.9 or later to restore; older runtimes reject that archive
path. 0.9 continues to restore older backups. Do not downgrade in place during a
staged update; restore a compatible pre-upgrade backup into a new collection.

The new `app check` command uses temporary Rhyven state. Executable tests still
require their existing execution consent flags and can cause real external
effects within the granted permissions. Normal installation review is unchanged.
