# Persistent service contract

Format 2 adds an opt-in container execution mode. Omitted mode or `mode: action`
retains the one-shot [container protocol](container-contract.md).

```json
{
  "driver": "container",
  "mode": "service",
  "protocol": "rhyven.service/1",
  "image": "registry/owner/app@sha256:<64 lowercase hex characters>",
  "memory_mb": 128,
  "cpus": 1,
  "timeout_seconds": 30,
  "startup_timeout_seconds": 30,
  "shutdown_timeout_seconds": 10,
  "restart_limit": 3,
  "start_policy": "manual",
  "calls": [
    {"category":"rhyven/project-knowledge","function":"action_remember","version":"0.3.0"}
  ]
}
```

Permissions include `state.read`, `container.execute`, and `service.run`.
Add `state.write` for writable `/data`. Nonempty `calls` requires `app.call`;
neither may be declared without the other. At most 32 unique category/function
grants are allowed. Existing named secrets and `network.connect` rules still
apply. Images are immutable, root filesystems read-only, capabilities dropped,
privilege escalation blocked and PIDs/CPU/memory limited. `/data` is private to
the installed app in its collection. Docker socket, host networking, arbitrary
mounts, privileged mode and package-supplied Docker flags are not allowed.

There is one instance per collection/app, protected by a single supervisor owner
lock and deterministic Docker name. Each launch receives a fresh generation.
Docker automatic restart and on-disk container logs are disabled; the supervisor
owns recovery and retains a 32 KiB stderr tail per worker. Admission budgets sum
declared resource limits; Docker enforces the per-instance limits.

## Framed stdin/stdout

Messages are newline-delimited UTF-8 JSON, at most 1 MiB including the newline.
Stdout is protocol-only; stderr is logs. The runtime sends initialization:

```json
{"type":"initialize","protocol":"rhyven.service/1","context":{"category":"owner/app","collection":"project","generation":"opaque-id","data_dir":"/data"}}
```

Load persistent state and return `{"type":"ready","protocol":"rhyven.service/1"}`.
Readiness timeout is 1–300 seconds (default 30). Do not make peer callbacks before
readiness. Runtime heartbeat `{"type":"ping"}` requires `{"type":"pong"}`; the
input pump must remain responsive during actions and background work. Ping is
sent every five seconds and twenty seconds without a pong ends the session.

An action arrives as:

```json
{"type":"call","id":"correlation-id","category":"owner/app","function":"action_increment","args":{"amount":2},"context":{"actor":"agent","request_id":"optional-retry-id","generation":"opaque-id","collection":"project"}}
```

Reply with exactly one of:

```json
{"type":"response","id":"correlation-id","result":{"count":2}}
{"type":"response","id":"correlation-id","error":{"code":"APP_SPECIFIC_CODE","message":"Explanation"}}
```

Results must match the declared output schema. App errors use the runtime's
`app_error` envelope. Each instance executes one foreground action at a time;
the supervisor queue holds at most eight requests. Queue wait and execution share
the action's 1–300 second deadline. Startup waiting is separate. An expired queued
request is rejected before execution. A timed-out executing action ends the
session; the instance must be stopped before a replacement starts. Background
jobs and their persistence/cancellation policies belong to the app.

## Scoped app callbacks

The app may send a callback while an action is pending or while idle:

```json
{"type":"callback","id":"callback-id","category":"rhyven/project-knowledge","function":"action_remember","args":{"title":"Finding","body":"Evidence","request_id":"stable-note-id"}}
```

The runtime returns `{"type":"callback_result","id":"callback-id","result":{...}}`
or `{"type":"callback_result","id":"callback-id","error":{...}}`.
Keep callback IDs distinct from action IDs. The supplied Python adapter provides
an independent input pump and correlated callback replies.

Callbacks are limited to the exact declared function, exact installed version,
and same collection. The target must be a local declarative app. Platform
management, other services/containers, remote targets, other collections and
undeclared functions are rejected. Installation does not automatically install
peers. Callback checks include the current package hash and active generation;
stop/update/backup revoke the old session before stopping its process.

The runtime assigns a reserved `_rhyven_service_…` actor stable for the source
app/collection. CLI and HTTP clients cannot claim that identity. A container
cannot supply an actor or select a different collection, and receives no broad
REST bearer token. Normal schema, rules, revisions, audit and retry receipts
apply to peer mutations. The ordinary client actor label remains caller-selected;
this is not a general multi-user identity system.

## Recovery and guarantees

Declare shutdown time 1–30 seconds (default 10). Handle SIGTERM, stop background
work, atomically persist data and exit. Runtime verifies ownership and termination
before removing a container or copying data. If the engine cannot prove the old
writer stopped, a replacement is refused. Supervisor restart reconciles registered
collections and replaces orphan instances with a new generation.

Failure retries use exponential backoff (2, 4, …, capped at 30 seconds).
`restart_limit` is 0–10 (default 3), counted cumulatively until explicit reset;
exhaustion requires user/agent intervention through start/restart. Stop is durable.
Service status separates desired state from observed state and includes errors.

Foreground calls with a request ID write a pending receipt before execution and
save validated results/errors with audit on completion. A lost/invalid response
leaves an uncertain receipt: retry returns `service_incomplete`, never a blind
repeat. `/data` writes, callbacks and external effects are not one atomic runtime
transaction. Apps should persist their own job identity and make background
effects idempotent; Rhyven does not guarantee exactly-once execution.

Backup/update use the collection gate to fence sessions and stop continuous
writers. Foreign action execution holds no long-lived collection gate, so scoped
callbacks can enter the normal runtime without deadlocking. Callback admission
has a bounded lock wait. Staged migration/health checks use disposable supervisors
with network, secrets and peer grants removed. They never call production peers.

OS startup configuration is generated for review, not enabled automatically.
This implementation does not include Compose stacks, public app ports, native
process execution, an agent scheduler or a Codex subprocess controller. Those
can be separate future capabilities/apps. See [deployment](deploy-service.md).

## Host availability during startup/shutdown

Since 0.4.0-rc.3, shutdown suspension is distinct from maintenance suspension.
Enabled services resume after a supervisor/OS restart even if Docker stopped
before the supervisor could confirm container removal. While the host is
unavailable, status reports `unavailable`; retries wait five seconds and do not
consume the app's crash budget. Each retry must confirm that the old writer has
stopped before creating a replacement. Pending action receipts are preserved.

An explicit stop remains disabled. An interrupted/failed backup or update remains
suspended until an explicit start after reviewing the maintenance outcome.
Old unclassified suspensions (including ones left by rc.2) also require an
explicit start. Runtime recovery never repairs Docker configuration or weakens
required host capabilities.

Since 0.4.0-rc.4, every backup retry verifies container shutdown even if a prior
attempt already marked the service suspended. A failed stop is never accepted
as proof that the app stopped writing. Existing suspended intent is retained.
