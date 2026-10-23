# Develop and inspect apps

## Identify the actual serving engine

`rhyven_categories` now includes `engine`: the serving runtime's version,
executable path, process ID, OS, architecture, actor and collection scope.
`rhyven/marketplace` also exposes `action_connection_info` with this identity.
REST-backed MCP returns the REST server's identity, not the local bridge's version.
The MCP initialization `serverInfo` still identifies the transport process.

```sh
rhyven --collection my-project connect --check
rhyven connect --client codex --check --configured
```

The first command probes a generated entry using the current CLI. The second
reads and probes the saved named client entry. `--config FILE` selects a specific
client profile; `--name NAME` selects its entry. Configured checks support stdio
entries in the existing supported client formats and never rewrite the settings.
Disabled entries fail. Environment values are not printed. Probes use the current
process environment plus the entry's explicit `env` overrides; the real client
may inherit different variables.

Use `--expect-collection NAME` to fail on a different serving collection. An old
server can omit the new identity; reports then say `identity_available: false`
rather than substituting the CLI version. Neither probe verifies a running agent
session: call `rhyven_categories` from that session after reloading its connection.

## Inspect a request without retrying it

Describe `rhyven/marketplace`, then call `action_request_report` with the original
`request_id`. The report reads evidence for the current actor and collection:

| Status | Meaning |
| --- | --- |
| `complete` | A successful result was durably recorded |
| `error` | An execution error was durably recorded |
| `unknown` | An executable invocation has no recorded final outcome |
| `partial_or_unknown` | A workflow is unfinished or failed; inspect its steps |
| `recorded_unclassified` | An older receipt lacks explicit encoding metadata |
| `not_recorded` | No matching durable evidence was found |

No evidence does not prove that an external call never ran. A pending receipt
may belong to a live call or an interrupted call. This operation never retries,
resumes or reconciles effects. Workflow details retain the completed steps and
uncertain outcomes from the existing workflow report. The report omits oversized
details explicitly above 256 KiB. Calls without request IDs, remote-provider calls,
connectors and Wobbegong snapshots may have no receipt.

New receipts include separate encoding metadata so an app result containing
`status`, `result` or `error` cannot be mistaken for an execution envelope. Existing
receipts remain readable and preserve their retry behavior.

## Read changes since a cursor

Call `action_changes` on `rhyven/marketplace` with optional `after` (initially 0),
`limit` (1–100) and exact `app` filter. Store the returned `next_cursor` together
with the collection scope and filter. Continue while `has_more` is true.

Pages contain ordered audit summaries: operation, app, actor, request ID, record
ID/object/revision, file ID and time where recorded. Record bodies are omitted;
read the relevant record to obtain current data. A fixed SQLite snapshot provides
each page's `through_cursor`; later commits remain available on the next poll.
An exhausted filtered page advances through unrelated events too. Restart from
0 when changing the filter or collection. Cursors beyond the current history fail.

Coverage is the engine's existing audit events, including app removal. It is not
a complete change feed for internal container databases, external providers or
Wobbegong ingestion. There is no scheduler, subscription worker or automatic agent
wakeup. A rollback/restore can invalidate a saved cursor; re-establish scope and
restart the scan when reconciling restored state.

## Check a local app draft

```sh
rhyven --collection my-project app check ./checklist
rhyven --collection my-project app check ./checklist --out ./checklist-tested.json
```

This validates the package and runs its behavior cases in temporary state. A
successful optional output is the exact tested package, identified by its hash.
An existing output is never overwritten, and failed checks write no package.
Edit the draft and repeat the same check. Installed app contracts and records
remain separate. Failures identify the case, operation and failed expectation.

Workflow tests use installed dependency contracts as fixtures, not live app data.
Executable dependencies still need explicit `--allow-host` or `--allow-container`.
Native code is unsandboxed and containers can have real authorized external
effects; temporary Rhyven state is not a guarantee of side-effect-free execution.
Review and install the finished app through the usual consent flow.
