# Remote and self-hosted app contract

Remote/provider-hosted execution is experimental and is not the launch focus. `hosting.mode` is `local`, `self-hosted` or `remote`. The latter two use the same adapter; the label communicates who operates the endpoint. Rhyven does not host these apps.

Remote metadata must include:

```json
{
  "mode": "self-hosted",
  "endpoint": "https://apps.example.com/rhyven",
  "auth": "Bearer token provisioned by your administrator",
  "auth_env": "RHYVEN_TOKEN_EXAMPLE",
  "privacy": "Inputs and records stay on your organization's server; logs retained 30 days",
  "account": "Organization account required",
  "billing": "Customer-operated infrastructure; no Rhyven execution fee",
  "domains": ["apps.example.com"]
}
```

These are developer declarations, not independently verified guarantees. They are visible in inspect, install review and describe_app. Auth may omit auth_env for an unauthenticated endpoint, but the auth disclosure is still required. Credentials may not be embedded in URL usernames/passwords or query strings. Endpoint fragments are rejected. Permission `network.connect` is mandatory in addition to state permissions. The only credential variable allowed is an explicitly named `RHYVEN_TOKEN_*` variable, read at call time. Tokens are not part of the package, database or standalone export.

## Wire protocol

The runtime POSTs one JSON operation to the exact endpoint. Redirects are disabled, ambient proxy configuration is ignored, HTTPS uses standard Web PKI certificate validation, and plain HTTP is limited to loopback development. There is no OAuth negotiation, arbitrary REST mapping or general-purpose fetch tool.

```json
{
  "protocol": "rhyven/1",
  "operation": "query",
  "arguments": {
    "app": "acme/incidents",
    "object": "incident",
    "filters": {"status": "open"}
  },
  "actor": "agent",
  "package_sha256": "canonical-package-digest"
}
```

For execute, the original app/action/args envelope is sent; the provider executes that declared action. Responses must be HTTP 2xx with exactly one of:

```json
{"result": {"items": [], "total": 0, "offset": 0, "limit": 100}}
```

```json
{"error": {"code": "revision_conflict", "message": "Reread current record"}}
```

Record results follow the local record shape: id, app, object, revision, data, created_at, updated_at, updated_by. Query results contain items/total/offset/limit. The adapter currently validates the response envelope and size, not independent semantic conformance of provider results. Provider errors are surfaced as remote_app errors containing the supplied error object.

The provider must authenticate the request, authorize access to its own data, validate the package contract, enforce revisions/rules and implement request_id deduplication. `actor` is a caller-supplied audit label, not proof of identity. Provider authorization must rely on real authentication. Network requests have a 15-second timeout and 1 MiB response limit. There is no automatic write retry or local remote-result receipt. A timeout may follow a committed write; use provider-supported idempotency before retrying.

`examples/remote-inventory.rhyven.json` is a complete package configured for a loopback endpoint. It intentionally needs a server implementation; no provider workload is silently launched. The automated integration test supplies a local mock HTTP server and verifies the actual request/response exchange. TLS, external providers, OAuth, domain ownership and remote certification have not been integration-tested.

Self-hosting the local runtime itself is also possible on a customer-controlled machine through its authenticated generic REST adapter, with a harness connecting through `rhyven mcp --server URL`; an SSH-launched stdio process remains an alternative. This prototype ships no hosted service or provider workload.

## External API and MCP wrappers (0.5+)

Packages with `connector.protocol` set to `mcp` or `http` use a native wrapper instead of the Rhyven remote wire protocol above. They expose reviewed actions from an existing service and do not install that service. See [connector apps](connector-apps.md).
