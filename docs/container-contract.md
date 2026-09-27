# Container execution contract

Format 2 retains the existing declarative format by default. An optional
`execution` field selects `driver: declarative` or `driver: container`.
This document describes `mode: action` (the default). `mode: service` uses the
[persistent service contract](service-contract.md) and `rhyven.service/1`.
A container manifest uses local hosting, requests `container.execute`, and
defines actions with `description`, `input` and `output` schemas. Both schemas
must describe objects using the supported Rhyven schema subset. `objects` may
be empty. If objects are declared, their CRUD operations still use the Rhyven
SQLite runtime; container actions use their own `/data` storage and cannot
directly access the runtime database.

```json
{
  "driver": "container",
  "protocol": "rhyven.container/1",
  "image": "ghcr.io/owner/app@sha256:<64 lowercase hex characters>",
  "timeout_seconds": 30,
  "memory_mb": 512,
  "cpus": 1,
  "secrets": ["RHYVEN_SECRET_API_KEY"]
}
```

Immutable image references are mandatory. A local `sha256:<image ID>` also works
for development and offline loads. The image includes all code, dependencies and
an ENTRYPOINT; runtime-controlled Docker flags cannot be supplied by a package.
Image `VOLUME` declarations are rejected; persistent files belong in `/data`.

Each invocation receives exactly one newline-terminated JSON request:

```json
{"protocol":"rhyven.container/1","category":"owner/app","function":"action_analyze","args":{"text":"hello"},"context":{"actor":"agent","request_id":"optional-id","collection":"my-project","data_dir":"/data"}}
```

Read stdin, run the function, emit one JSON response, and exit successfully:

```json
{"result":{"words":1}}
```

Or return `{"error":{"code":"failed","message":"Explanation"}}`; Rhyven exposes
this as `app_error` with the app's error details. Put logs on stderr. Stdout and
stderr are each capped at 1 MiB. Results must match the action's output schema.
Invalid input is rejected before a container starts. No custom MCP server or
public app HTTP port is needed.

Permissions and execution:

- `container.execute` permits code and subprocesses inside the container.
- `state.read` is required. `/data` is read-only unless `state.write` is granted.
- `network.connect` enables Docker's bridge network; otherwise networking is
  disabled. This is broad network access, not a domain allowlist.
- `secrets.read` plus `execution.secrets` passes only the named `RHYVEN_SECRET_*`
  variables from the runtime's environment. They are never stored in packages.
- The root filesystem is read-only, capabilities are dropped, privilege
  escalation is disabled, and `/tmp` is a 64 MiB non-executable tmpfs. The
  process limit is 128. Default memory is 512 MiB, CPU is one, timeout is 30s;
  manifests can request 1–32768 MiB, 1–32 CPUs and 1–300 seconds.
- On Unix, a rootful daemon runs with the state directory owner's UID/GID;
  a rootless daemon uses its mapped root identity. Remote Docker daemons are
  unsupported because data is bound from the runtime host. Native Windows
  execution is unverified; Linux/WSL is the tested target.

Rhyven serializes collection operations under a maintenance lock. Updates stage
`/data` and can run declared migration and health actions with network/secrets
disabled before activation. Authors must declare required migrations. Driver
changes require a separate installation. `backup` includes container files;
`snapshot` remains a partial export. See [recovery and updates](recovery-and-updates.md).

Successful calls with request IDs return saved results on retry. Before starting
code, Rhyven records a pending receipt. If execution fails, times out or crashes,
reusing that ID yields `container_incomplete`, preventing a blind repeat of
possible side effects. Inspect state before deliberately using a new request ID.
External effects and private files are not transactional with runtime receipts.
Rhyven attempts forced container removal after timeouts/errors; a hard runtime or
daemon crash may require operator cleanup of containers named `rhyven-*`.

MCP, REST and CLI use identical schemas and dispatch. Local MCP can call the
core directly; `mcp --server` uses REST. Long calls also depend on client/harness
timeouts. Containers share the host kernel; these controls are not a claim of
complete isolation for hostile code. Registry checks validate container packages
without executing publisher code; independent certification remains future work.
