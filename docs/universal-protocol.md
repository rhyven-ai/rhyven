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

## Compact and selective discovery (unreleased)

The local connector branch makes `rhyven_describe` compact by default. It returns
callable input/output schemas and Markdown guidance once. `contract` retains
hosting, permissions, execution settings, and object rules/relationships; it omits
repeated descriptions, action definitions, object schemas, tests and package files.
Execution validation still uses the complete installed package, not this view.

Optional arguments on the same tool:

```json
{"category":"acme/documents","function":"action_search_documents"}
```

```json
{"category":"acme/documents","search":"search documents"}
```

`function` selects an exact function name. `search` matches all terms, case-insensitively,
against function names, descriptions, and optional action `keywords`. Separators
are normalized; add/sum/total and find/search/lookup are recognized aliases. They are
mutually exclusive and limited to 128 bytes. Results include full callable schemas,
not just names; no extra detail request is required before calling a match. An
unknown exact name fails with up to three function suggestions; a search with no matches returns an empty function list.
Filtered responses include `total_functions` so absence is not mistaken for an
empty app. App guidance and security disclosures remain present even when filtering.

Use `{"category":"acme/documents","full":true}` when a client needs the complete
package contract (embedded files are still omitted). Clients that previously
consumed `contract.actions` or `contract.objects.*.schema` must request `full:true`
or use the unchanged `GET /apps/{publisher}/{app}` contract endpoint.

REST keeps `GET /categories/{publisher}/{app}` for default compact discovery and
adds `POST /categories/{publisher}/{app}/describe` with an optional JSON body of
`function`, `search`, `full`, `index`, and `if_hash`. REST-backed MCP forwards these options. All paths
use the same runtime filtering and collection scope; permissions and calls are unchanged.

When a category is already known in the active collection, request its relevant
function directly. Use categories to establish scope when it is unknown. Re-describe
after package updates or contract mismatches. Search currently scans manifest text;
there is no separate index or new storage dependency.


### Smaller listings, batched discovery, and reuse

`rhyven_categories` retains collection/workspace identity and returns only app
`name`, `description`, `version`, and `contract_hash`. Full branding, hosting and
permission metadata remain in `list_apps` / `GET /apps` and package descriptions;
installation review still exposes permissions and trust before approval.

Use `index:true` to omit argument/output schemas from the function list. The default
still returns schemas, so small apps do not require an extra lookup. Never invoke
an unfamiliar function from an index alone: retrieve its schema first.

Batch independent lookups in one tool call (1–16 entries):

```json
{"requests":[{"category":"acme/files","search":"read"},{"category":"acme/math","search":"add"}]}
```

The result contains ordered `descriptions`. `requests` cannot be mixed with
single-description arguments or nested; a failed entry fails the request. REST
uses `POST /categories/describe` with the same body. No new MCP tools are added.

Descriptions include `contract_hash`, a SHA-256 of the full manifest before
filtering. It includes guidance and contract metadata, excludes embedded files,
and does not change when app records change. It is a cache identity, not a package
signature or proof of trust. Use `if_hash` with a previously returned hash to
receive `{category, contract_hash, unchanged:true, scope}` when unchanged; a
mismatch returns the requested description. An unchanged response only confirms
freshness: it does not provide a schema that the agent has not already read.

Reuse known schemas within the same collection/session. After an app update,
contract mismatch, or schema error, refresh the description. Discovery caches do
not bypass runtime validation. No cross-session cache or new database is required.

Actions may declare up to 16 `keywords`, each a nonempty string of at most 64
bytes, for example `"keywords":["add","arithmetic"]`. This is optional metadata
for declarative, executable and connector actions; it does not alter execution.
