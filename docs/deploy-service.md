# Deploy a persistent container app

Use service mode for a process that must continue after an action returns or an
agent disconnects: a queue worker, watcher, indexer, or app-specific controller.
The image contains its language runtime and dependencies. No host Python or venv
is needed. Linux with a compatible local Docker Engine is tested; macOS remains
a feedback preview. Native Windows supervision is not implemented.

Service mode uses the same app manifest, actions, input/output validation and
three MCP tools as other Rhyven apps. Authors implement the
[service protocol](service-contract.md); arbitrary images without that adapter
cannot automatically expose Rhyven actions. No HTTP port or custom MCP server
is required inside the image.

## Build and run locally

```sh
rhyven app init acme/background-counter --runtime service --dir ./counter
# Edit app.json, main.py, rhyven_service.py and Dockerfile as needed.
docker build --iidfile ./counter-image.id ./counter
rhyven app package ./counter --image "$(cat ./counter-image.id)" --out ./counter.json
rhyven app validate ./counter.json
rhyven app test ./counter.json --allow-container
rhyven --collection demo install ./counter.json --accept-permissions
rhyven daemon start
rhyven --collection demo service start acme/background-counter
rhyven --collection demo call rhyven_call '{"category":"acme/background-counter","function":"action_increment","args":{"amount":2,"request_id":"first"}}'
rhyven --collection demo service status acme/background-counter
rhyven --collection demo service logs acme/background-counter
rhyven --collection demo service stop acme/background-counter
```

The scaffold persists a background tick counter and action counts under `/data`.
Its optional `remember` action calls `rhyven/project-knowledge` 0.3.0 through a
declared function grant. Install that app in **the same collection** to try the
callback. Other actions work without it. This is a developer example, not a
published official app or an agent harness.

Installation reviews the pinned image, `service.run`, resource limits and peer
grants; it never starts app code. `app test --allow-container` explicitly executes
the fixture in an isolated temporary supervisor. Permission acceptance in these
CLI examples means the user has reviewed the manifest.

## Lifecycle

One supervisor per Rhyven home manages separate instances per collection and app.
It runs independently of the TUI, MCP client and REST server. Use the same `--home`
when starting the daemon and calling an app. Legacy `--workspace` instances have
their own supervisor. `daemon status` reports the home, PID and budgets.

- `service start APP` enables the app and waits for readiness; repeated start of
  a ready instance reuses it. `service restart APP` stops it and starts a fresh
  generation. `service list` lists installed services in the selected collection.
- `service stop APP` durably disables it. Later calls and daemon restarts cannot
  silently undo that choice. Explicit start enables it again.
- `start_policy: manual` is the default. `on-demand` allows the first action call
  to enable a never-stopped instance. Both require a running supervisor.
- Closing a client does not stop enabled services. `daemon stop` requests graceful
  shutdown of that home's services; it returns before shutdown completes. A
  subsequent daemon start resumes enabled instances. SIGTERM/SIGINT follow the
  same shutdown path. SIGKILL recovery occurs when a new supervisor starts.
- Crash recovery uses bounded retries and backoff. Status reports a failed state
  after the retry budget is exhausted. Inspect logs/status and explicitly start
  or restart after addressing the failure.
- Logs return a bounded current-worker stderr tail, not a durable log archive.
  Status includes the selected collection and whether the supervisor is available.

Default aggregate admission limits are eight services, 4096 MiB of declared
memory, and four declared CPUs. To choose limits, stop the old daemon and run:

```sh
rhyven daemon run --max-services 4 --memory-budget-mb 2048 --cpu-budget 2
```

The Installed TUI view offers `b` start, `t` stop, `v` status and `l` logs.
Agents discover the `rhyven/runtime` category, then use functions such as:

```json
{"category":"rhyven/runtime","function":"action_service_start","args":{"app":"acme/background-counter"}}
```

Available functions are `action_service_list`, `action_service_start`,
`action_service_stop`, `action_service_restart`, `action_service_status`, and
`action_service_logs`. REST exposes them through the existing generic function
routes. Remote start/stop/restart requires the host's separate management token;
normal app calls still use the ordinary bearer token. An approved on-demand app
can start through an ordinary app call, as its manifest explicitly allows.

## Start with the operating system

```sh
rhyven daemon unit --out ./rhyven-supervisor.service
```

This generates a systemd user unit on Linux, or a launchd plist on macOS. It does
not install or enable the unit. Review its absolute binary/home paths, Docker
PATH/context and secret environment before placing it in your OS service manager.
On Linux, a user unit normally starts at login; boot without login requires the
user's systemd lingering configuration. Docker must also become available. If it starts later, enabled services retry
without consuming their app crash budget; status reports the host error until
recovery. Explicitly stopped apps stay stopped. Generated
macOS units have not been exercised on a Mac in this implementation.

## State, maintenance, and export

Backup and update fence callbacks, stop continuous writers, confirm termination,
then copy state. They resume previously enabled instances afterward. A forced
kill/OOM during a requested clean stop fails maintenance rather than reporting a
clean snapshot. Apps must handle SIGTERM and flush their own state. Restore
includes installed manifests, SQLite, receipts/audit and `/data`; restored services
start disabled. Images, secrets, live processes and supervisor registrations are
not in the archive. See [recovery](recovery-and-updates.md).

After an interrupted maintenance process, status can remain suspended. Inspect
the maintenance outcome, then use explicit `service start` to clear suspension;
ordinary app calls cannot clear it. Removal stops the service and retains data.
Reinstallation requires explicit start to resume it.

`app export-mcp` exports this same app contract. Its launcher starts an isolated
supervisor and service under the export's `state/` directory. Closing the exported
MCP connection leaves its service running. Stop it with
`./rhyven --workspace ./state daemon stop` from the export directory. Peer apps
and Docker images are not bundled; install any required peers in that workspace.

Distribution uses the existing image-digest and package registry flow. The hosted
validator and distributed binary must be upgraded to this source version before
publishing service manifests. No registry or image was published for this work.
