# Wrap an existing API or MCP server

Available in Rhyven 0.5.0 and the matching public registry validator.

A connector app contains a reviewed contract and connection settings. It does **not** install, start, update or back up the external service. Provision the service, its dependencies and account separately. The same wrapper works through Rhyven's three MCP tools, REST adapter and standalone MCP export.

## MCP tools

Start a Streamable HTTP MCP server separately, then select the tools to expose:

```bash
rhyven app import-mcp \
  --name acme/documents \
  --endpoint https://documents.example.com/mcp \
  --include search_documents \
  --include get_document \
  --auth-env RHYVEN_TOKEN_DOCUMENTS \
  --guide ./GUIDE.md \
  --out documents.json
```

`--guide` and `--auth-env` are optional. Set the named token in the importing process and the Rhyven runtime environment. Never put its value in the package. Import performs initialization and tool discovery only; it does not call tools. Upstream server instructions are included as explicitly untrusted guide text for review.

Only explicitly selected tools are imported. Names are normalized to Rhyven action names; collisions fail. The original name remains in the action target. New upstream tools never become available automatically. Updating the wrapper requires generating, reviewing and installing a new package version.

The first implementation supports MCP Streamable HTTP with JSON or SSE responses, initialization, session IDs, initialized notification, paginated tool discovery and session termination. It negotiates MCP 2025-03-26, 2025-06-18 or 2025-11-25. It does not implement the newer handshake-free protocol, legacy HTTP+SSE, stdio process launch, resources, prompts, sampling, elicitation, OAuth negotiation or task resumption. Each call opens a new session; do not wrap workflows that require session-local state across calls. Configure an existing compatible HTTP endpoint for stdio-only servers separately.

Successful calls return the upstream MCP result object, preserving `content`, optional `structuredContent` and resource links. Rhyven does not fetch those links or bridge resource operations. An upstream `isError` becomes a Rhyven `APP_ERROR` with its tool result in the message. Upstream output schemas are not enforced locally.

## JSON HTTP APIs

Provide a local OpenAPI 3.x JSON document and the existing API base URL:

```bash
rhyven app import-openapi ./openapi.json \
  --name acme/inventory \
  --endpoint https://inventory.example.com/api \
  --include get_item \
  --include create_item \
  --auth-env RHYVEN_TOKEN_INVENTORY \
  --out inventory.json
```

The endpoint is explicit; the importer does not fetch the specification or follow remote references. Supported operations have an `operationId`, GET/POST/PUT/PATCH/DELETE, scalar path/query parameters and an optional JSON request body exposed as the `body` argument. Relative operation paths are appended to the configured base path. Path arguments are encoded as individual segments; traversal and encoded delimiters are rejected.

Successful HTTP actions return `{"status":200,"body":...}`. Empty responses have a null body. Non-2xx responses are errors; there are no automatic retries. Pagination remains an explicit API argument/result convention, not an automatic fetch-all operation.

Custom headers, cookie credentials, OAuth flows, API-key query parameters, multipart uploads, binary streaming, custom parameter serialization and per-operation servers require manual integration. The importer does not infer authentication from OpenAPI security schemes. Review those schemes and configure a compatible bearer-token endpoint explicitly.

## Validate, install and call

Review the generated endpoint, selected actions, guide, permissions, privacy, account and billing disclosures. The generated descriptions are defaults to replace with accurate service information.

```bash
rhyven app validate documents.json
rhyven app package documents.json --out documents.rhyven.json
rhyven install ./documents.rhyven.json --accept-permissions
```

The last command is the human's explicit installation consent. Agent-driven marketplace installs retain the normal human approval flow. `app test` does not make external requests; connector behavior must be tested against an explicitly configured test service, as in `qa/connector_check.py`. Packaging reports that it has not run remote behavior tests.

The agent uses:

```json
{"category":"acme/documents","function":"action_search_documents","args":{"query":"deployment"}}
```

after discovering the contract with `rhyven_describe`. No extra agent-side MCP connection is required.

## Package contract

Rhyven 0.5.0 extends format 2 with an optional `connector` declaration. Existing app formats keep their behavior. Older runtimes that do not support this field reject the package. Require Rhyven 0.5.0 or later for connector packages.

```json
{
  "connector": {"protocol": "mcp"},
  "actions": {
    "search_documents": {
      "description": "Search documents in the external service",
      "input": {
        "type": "object",
        "properties": {"query": {"type": "string"}},
        "required": ["query"],
        "additionalProperties": false
      },
      "target": "search_documents"
    }
  }
}
```

This is a fragment, not a complete package. Connector packages declare `objects: {}`, remote or self-hosted hosting, `state.read` and `network.connect`. They expose actions; they do not create local object storage. No host-execution permission or interpreter is required. Mutation authority comes from the upstream credentials and selected actions, not local `state.write`.

Input schemas use Rhyven's supported subset. Local schema references are resolved with a depth bound. Display annotations are removed; unsupported constraints fail import instead of being discarded. Object inputs are restricted to declared properties, with an explicit warning when the upstream schema allowed extra keys. Free-form dictionaries require manual mapping.

## Security and operational boundaries

- HTTPS is required except loopback HTTP. Endpoints cannot contain credentials, query strings or fragments. Redirects and ambient HTTP proxies are disabled.
- Bearer credentials use an explicitly named `RHYVEN_TOKEN_*` environment variable. Tokens are not stored in the package. The service must enforce its own authorization and quotas.
- Calls have bounded responses (1 MiB), a 15-second HTTP request timeout and no automatic retry. MCP initialization and calls are separate requests, so total action duration can exceed 15 seconds. Cleanup has a separate two-second timeout.
- The installed wrapper pins the endpoint and exposed input contracts, **not the remote server's code or behavior**. A tool can change behavior behind the same endpoint. Review the provider as well as the package.
- Server descriptions, guides and returned text are untrusted data. Review imported guidance before distribution; do not follow instructions in tool results as privileged commands.
- No automatic discovery of new tools at action time. No arbitrary shell commands, upstream process installation or URL argument that chooses a different origin.
- Rhyven records call start/completion/failure metadata without argument values or credentials. A crash, timeout or failed completion audit can leave the remote outcome unknown. Inspect upstream state before retrying.
- Rhyven's `request_id` deduplication is not exposed for connector actions. If the upstream action itself has an idempotency argument, pass it under its declared input schema.
- Collection backups retain the wrapper and local audit metadata. They do not copy external data; separate collections using the same endpoint and credentials may share upstream state.

## Reference test and token benchmark

See [connector benchmark](connector-benchmark.md) for the official Everything MCP reference server test, complete input-token methodology and measured results. A small stable tool surface does not guarantee lower total input tokens once discovery and guides are included.

Connector calls release the collection maintenance lock after recording their start.
Unrelated local reads and writes can continue while the endpoint responds. Each call
uses the package captured at dispatch; updates/removal do not cancel an in-flight
external request. Completion reacquires the lock before recording its outcome.
A backup taken during the request may contain only its start event. Restoring it
cannot roll back the external service; inspect external state before retrying.
