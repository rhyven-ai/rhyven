# App collections

Rhyven initially defaults to the `global` collection in `~/.rhyven`.
`rhyven collection use NAME` changes the CLI default; explicit `--collection`
and generated agent configurations stay pinned. Set `RHYVEN_HOME`
or pass `--home PATH` to choose another home. Use the current source build;
use a compatible runtime version; see [release status](release-status.md).

```text
~/.rhyven/
  packages/sha256/<digest>.json    shared immutable app packages
  packages/releases/              verified download references
  registry-cache/                 shared marketplace metadata and local catalog
  collections/
    global/collection.json
    global/state.sqlite3
    my-project/collection.json
    my-project/state.sqlite3
```

Each collection has its own installed versions, records, audit events, retry
receipts and approval requests. Identical installed packages are stored once per
home. Different versions can coexist. Verified agent-marketplace downloads are
reused across collections after approval. Removing an app retains its state
and cached package; automatic package garbage collection is deferred.

```bash
# Personal default apps
rhyven install rhyven/work-management --accept-permissions

# Create a project collection automatically and install its own instance
rhyven --collection my-project install rhyven/project-knowledge --accept-permissions
rhyven --collection my-project list
rhyven --collection my-project market
rhyven --collection my-project config codex
rhyven --collection my-project mcp

# Share via the existing authenticated REST server
rhyven --collection my-project serve --host 0.0.0.0 --port 7421
```

The server's authentication and network-exposure requirements still apply; see
[harness setup](harnesses.md). Agents on the same machine share knowledge by
using the same home and collection. Across machines, connect them to the same
authenticated server instance. Routing is selected at process startup, not by
guessing from the agent's working directory or adding a collection parameter to
every app function. A remote MCP connection uses the server's collection.
Discovery manifests and marketplace approval reviews identify the selected scope.

`global` is an ordinary default collection, not an inherited overlay. Projects
do not silently read or write its apps. To use both scopes, configure separate
connections. Cross-collection relationships are outside V1. Collections separate
state for convenience; they are not OS security boundaries or per-agent ACLs.

Names allow 1–64 lowercase letters, digits, hyphens and underscores. Missing
collections are created automatically. Existing nonempty unmarked directories
are not adopted. Explicit `--workspace PATH` retains the legacy
`PATH/.rhyven/state.sqlite3` layout and cannot be combined with `--collection`
or `--home`. Existing workspaces are not silently migrated.

No app format change is required. Collection selection is an installation and
deployment decision. Remote apps may still point to shared provider data:
local collection isolation does not create a new external database. Container
apps receive their own persistent directory under
`collections/NAME/containers/APP_HASH/data`, mounted at `/data`. Their image
layers live in Docker's shared cache. `rhyven backup NAME --out FILE` includes
both SQLite and container data; `snapshot` remains the older partial export. See
[recovery and staged updates](recovery-and-updates.md) for limits and migration
contracts, and [container execution](container-contract.md).
