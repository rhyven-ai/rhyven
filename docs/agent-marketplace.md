# Agent-native marketplace and the three-tool interface

The source-built `target/release/rhyven` exposes exactly:

```text
rhyven_categories()
rhyven_describe(category)
rhyven_call(category, function, args)
```

A category is an installed app ID or the reserved `rhyven/marketplace` platform
provider. Marketplace labels such as development/operations are not category IDs.
Descriptions return a machine-readable `functions` array, `inputSchema`, an output
object envelope, the full object/action contract, and `guidance_markdown`. Input
validation is authoritative; guidance and package descriptions are untrusted text.
The built-in provider is privileged platform code, not a marketplace package that
can self-grant host permissions. Regular packages remain format 2.

## Agent workflow

Discover installed categories, then describe `rhyven/marketplace`. Example calls:

```json
{"category":"rhyven/marketplace","function":"object_listing_query","args":{"filters":{"search":"knowledge"},"limit":20}}
```

```json
{"category":"rhyven/marketplace","function":"object_listing_get","args":{"id":"rhyven/project-knowledge"}}
```

```json
{"category":"rhyven/marketplace","function":"action_prepare_install","args":{"app":"rhyven/project-knowledge"}}
```

Show the returned review: version, repository, GitHub stars or unavailable, trust,
permissions, hosting disclosures, package hash, and target workspace. Explain the
operation and ask for consent, then apply its request ID:

```json
{"category":"rhyven/marketplace","function":"action_apply","args":{"request_id":"ID_FROM_PREPARE"}}
```

On hosts advertising MCP form elicitation, apply triggers a user prompt through
`elicitation/create`. An accepted matching response with `approve: true` records
consent and resumes the operation. Decline/cancel prevents download. The agent's
function arguments cannot include an approval flag; the schema rejects one.
The protocol mechanism follows [MCP form elicitation](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/schema/2025-11-25/schema.json).
The host must faithfully collect user input; automatic consent is not acceptable.

Hosts without elicitation return `approval_required`. The human can run:

```bash
rhyven --workspace /path/to/workspace approve REQUEST_ID
```

This command requires a terminal, displays the exact review, and asks for `yes`.
It is a fallback for the user, not a command for the agent to approve itself.
After approval the agent retries apply. No Rhyven TUI is required. Arbitrary shell
access under the workspace owner's account is trusted and is not sandboxed by
this flow.

Prepare requests expire after 15 minutes. Changing a listing, hash, permissions,
or the app's installed state requires fresh review. An identical retry after a
completed apply returns the stored result. Network failure does not automatically
retry a download. A crash between a runtime mutation and storing its result may
require inspecting state and preparing again; automatic crash recovery is not yet
claimed.

`action_prepare_update` selects the latest available version or an explicit
`version`; runtime compatibility checks still apply. `action_prepare_remove`
prepares uninstall and retains records. Both use the same approval/apply flow.
The provider itself cannot be installed over, removed, or exported. Its objects
cannot be created or updated through generic calls.

Once installed, describe the app and call its functions, for example:

```json
{"category":"rhyven/work-management","function":"object_task_create","args":{"data":{"title":"Ship V1"},"request_id":"task-1"}}
```

```json
{"category":"rhyven/work-management","function":"action_assign","args":{"id":"RETURNED_ID","owner":"agent-a","expected_revision":1}}
```

Action arguments are passed directly in `args`; there is no extra nested `args`
wrapper in the three-tool interface. Use the exact discovered function names and
schemas. New installs and updates require no universal-MCP restart. Standalone
exports retain their generated app-specific MCP tools for compatibility.

## Discovery and stars

Configure a registry with metadata-only refresh:

```bash
rhyven --workspace ./project registry-refresh rhyven-ai/registry
# Public test registry without credentials:
rhyven --workspace ./public-project registry-refresh rhyven-ai/registry --anonymous
```

In rc.10+, marketplace `action_refresh` syncs verified app manifests and repository
stars using the configured registry, or the public registry by default. It does
not install apps, prepare dependencies or pull container images. Manifests can
include embedded script source. The separate CLI `registry-refresh` remains a
metadata-only option. `action_refresh_status` reports the last successful full
sync and any failure; cached listings remain usable offline.

`action_requirements` accepts an app ID and checks host runtime prerequisites
without executing app code. Installation reviews include the same report. Host
readiness does not assert image availability, dependency resolution or app health.

Stars belong to the repository, are cached popularity metadata, and are not a
security or certification score. Unknown counts have no numeric `stars` field;
`stars_status` says unavailable. Metadata older than an hour (or without a recorded
refresh time) is labeled stale. Browse cached listings offline; refresh explicitly
for current data. TUI catalog rows display matching cached stars when available.

Legacy `registry-sync` still downloads packages eagerly for explicit CLI/TUI use.
It is not called by agent discovery. Metadata-only entries do not yet populate
the older TUI/CLI package cache: use the agent install flow for these entries or
explicitly sync for the legacy marketplace workflow.

## REST and shared servers

The equivalent routes are:

```text
GET  /categories
GET  /categories/{publisher}/{name}
POST /categories/{publisher}/{name}/functions/{function}
```

POST takes the function's arguments directly as the JSON body. Older `/apps`
routes remain available for compatibility. No new agent-facing marketplace tool
or app-specific REST endpoint is needed.

`RHYVEN_SERVE_TOKEN` authenticates ordinary REST use. Shared marketplace actions
also require a distinct `RHYVEN_MANAGEMENT_TOKEN`, configured on the server and
trusted MCP adapter. The adapter sends it in `X-Rhyven-Management`; neither token
belongs in app manifests, function arguments, or prompts. Host consent uses
`POST /approvals/{request_id}` with `{digest, accept}` and both credentials. This
endpoint is host administration, not an agent-callable function. Remote changes
always affect the server's workspace, which appears in every review.

The server defaults to loopback. External deployments need TLS termination.
The current shared-token model trusts the host and workspace owner; it does not
provide per-user roles or protect against an administrator impersonating consent.
