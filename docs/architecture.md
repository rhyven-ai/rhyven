# Architecture

Rhyven 0.6.0 introduced [composable capabilities](composable-capabilities.md):
app workflows, bounded app discovery and native executable actions. See
[0.8 upgrade notes](migration-0.8.md) for retired library tooling.

Rhyven loads installable app contracts into a transport-independent runtime.
Local declarative apps, container actions and persistent services share discovery
and invocation contracts. Remote/provider-hosted execution is experimental.
Rhyven does not host customer workloads or data.

| Boundary | Responsibility |
|---|---|
| `crates/core/src/catalog.rs` | Fail-closed format-2 package validator, immutable local registry, package resolution |
| `crates/core/src/runtime.rs` | Generic object CRUD, relationships, action interpreter, rules, local transaction boundary, remote adapter |
| `crates/core/src/store.rs` | SQLite state, audit, receipts and package integrity digests |
| `crates/core/src/schema.rs` | Bounded JSON Schema subset and default materialization |
| `crates/core/src/tools.rs` | Three universal tools, generated function manifests, or standalone app-specific tools |
| `crates/core/src/http.rs` | Generic authenticated REST adapter and REST client for shared runtimes |
| `crates/core/src/conformance.rs` | Isolated package behavior cases through public operations |
| `crates/mcp` | MCP lifecycle and stdio JSON-RPC framing; no domain logic |
| `crates/tui` | Human discovery, package inspection, install permission review |
| `crates/cli` | Developer/user commands, harness config and standalone export |

No app names appear in the runtime. Catalog embedding is a distribution convenience, not mandatory installation. All packages can be removed while the three-tool interface continues working. A Git checkout can supply additional immutable package versions; no provider abstraction/service layer is needed before a real hosted registry exists.

## Shared runtime transport

Execution is a separate choice from transport. `container.rs` dispatches custom
actions to a digest-pinned Docker image using one JSON stdin/stdout exchange per
call. The ordinary object engine continues to serve declarative CRUD. Both expose
the same three tools and REST function routes. See [execution contract](container-contract.md).

The core owns package validation and generic operations. The REST adapter maps
category discovery, manifests and function invocation to `/categories` routes
and retains earlier `/apps` routes for compatibility, and the stdio MCP adapter
can either call the core directly or use that REST client. No app-specific HTTP
route or transport-specific package format exists. `serve` uses a required
bearer token and loopback binding by default; an external plaintext listener
requires an explicit flag and belongs behind TLS. This is shared-token access
for trusted clients, not per-user authorization.

## Mutation invariants

1. Installed package digest and bounded schema are checked before every operation.
2. Only declared objects, actions, fields and supported permissions are accepted.
3. Local writes acquire an immediate SQLite transaction. Read/validate/write/audit/receipt is atomic.
4. Updates require the exact current positive revision. Defaults are materialized on complete records, never injected into partial patches.
5. Immutable objects reject updates, including updates through actions. Protected fields require an action. Action guards and allowed transitions apply before commit.
6. Local relationship targets must exist in the declared app/object. Cross-remote relationships cannot be verified and are rejected on local writes.
7. An actor + request_id pair identifies one exact operation/argument envelope. An identical retry returns the original result; changed arguments fail. Receipts persist across process restarts.
8. Uninstall hides an app and preserves data/contract. Reinstallation checks compatibility with the retained contract. Updates use staged migrations and recovery points; see [recovery](recovery-and-updates.md).

Single-record actions keep this atomicity boundary simple. Multi-record transactions, leases, dependency scheduling and arbitrary native code require new contracts and tests rather than hardcoded domain extensions.

## Remote execution

The same discovery and tools apply to remote packages. Input shapes and declared permissions are checked locally; execution goes to one reviewed endpoint with redirects off, a 15-second timeout and a 1 MiB response cap. The provider is responsible for its rules, authorization, revisions, durability, idempotency and response correctness. Network timeouts are ambiguous for writes; no automatic write retry is performed. Remote credentials are read only from the package's disclosed `RHYVEN_TOKEN_*` variable and are never serialized into packages or exports.

## Storage tradeoff

The earlier prototype's JSON event directories have been replaced with SQLite as authoritative local state so state, audit and retry receipts share one transaction. JSON snapshots preserve inspectability and can be committed to ordinary Git. The database is not safe for file-level multi-machine synchronization; Each collection owns a local SQLite database. Queries scan scoped records and support typed filters, keyword search, sorting and pagination. This is suitable for prototype data volumes, not a scale claim.

## Extension points

Add storage backends beneath the runtime, richer validated action primitives, explicit migration implementations, a signed registry/trust authority, scoped OS sandbox execution and verified remote conformance later. MCP remains an adapter; package behavior does not depend on transport. Standalone export reuses the runtime and contract rather than producing a separate implementation that could drift.

## Platform marketplace provider

`crates/core/src/marketplace.rs` implements the reserved `rhyven/marketplace`
category. Its object/action schema generates the same function manifest as other
categories. Its privileged handlers call package-management services; installable
apps cannot register such handlers or replace the reserved category. This is a
platform extension, not a claim that package management can be expressed using
the current declarative object mutations alone.

Request state pins the source/hash, permissions, hosting and workspace. MCP host
elicitation or explicit local approval records consent outside model-visible
functions. Shared serving checks a separate management credential. Runtime
validation and integrity checks still govern activation. The local threat model
trusts the harness and account with filesystem access; it does not sandbox an
agent that has arbitrary shell access as that account.
