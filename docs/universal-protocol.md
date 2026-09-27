# Universal agent protocol 1

MCP exposes exactly `rhyven_categories()`, `rhyven_describe(category)` and
`rhyven_call(category, function, args)`. MCP and REST route to the same Rust runtime;
validation, execution, collection locking, permission checks and audit are shared.
Local MCP does not require an HTTP server. REST-backed MCP uses that server's
collection and authentication. REST is an alternative adapter.

Discovery now returns an object, not the previous prototype array:

```json
{
  "rhyven_protocol": 1,
  "collection": "my-project",
  "workspace": "/home/user/.rhyven/collections/my-project",
  "apps": []
}
```

The platform marketplace is included in `apps`. Legacy workspace sessions have
`collection: null` and an explicit workspace path. `rhyven_describe` continues to
include `scope`. Legacy `list_apps` and REST `GET /apps` retain their array return;
`GET /categories` uses the new versioned discovery object. Clients must read
`apps` after checking the protocol. CLI/MCP success results retain their existing
app-specific JSON shape; MCP wraps them in its standard result envelope.

Errors retain the detailed legacy `code` and `message`, with an additive stable
classification and protocol version:

```json
{"rhyven_protocol":1,"kind":"PERMISSION_DENIED","code":"approval_required","message":"User approval is required"}
```

Stable `kind` values: `INVALID_ARGUMENT`, `NOT_FOUND`, `PERMISSION_DENIED`,
`TIMEOUT`, `APP_ERROR`, `UNAVAILABLE`. Use `code` for precise cases such as revision
conflicts, approval states and retry ambiguity; existing clients keep working.
MCP JSON-RPC transport errors still use MCP's own numeric codes. Container apps
continue to use `rhyven.container/1` on stdin and exactly one `result` or `error`
on stdout. Action inputs and outputs remain schema-validated.

Parity gates: `qa/universal_market_check.py`, `qa/shared_runtime_check.py`,
`qa/collections_check.py` and `qa/container_check.py`. They cover discovery,
function manifests, schema errors, revision/idempotency semantics, approvals,
collection routing, persisted state and direct versus REST-backed MCP.
