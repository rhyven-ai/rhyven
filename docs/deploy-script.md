# Deploy a Python or JavaScript script app

Available in Rhyven 0.4.0-rc.9 and its matching registry validator. Linux has
been tested. Other Unix hosts need acceptance testing; native Windows execution
is rejected.

Scripts use the same category/function/args interface and action schemas as
containers. Each call starts a process, receives one JSON request on stdin,
returns one JSON result/error on stdout, and exits. No app-specific HTTP or MCP
server is needed. Persistent services continue to use the container backend.

## Create and exercise

The host needs Python 3.10+ for Python apps or Node 20+ for JavaScript apps.
Rhyven creates dependency environments; it does not install system interpreters,
compilers or operating-system packages. Missing tools produce structured errors.

```sh
rhyven app init acme/text-tools --runtime python --dir ./text-tools
# Use --runtime javascript for a Node app.
rhyven app validate ./text-tools
# Review the code: this explicitly permits execution with your OS user's access.
rhyven app test ./text-tools --allow-host
rhyven app package ./text-tools --out ./text-tools.rhyven.json
rhyven --collection script-trial install ./text-tools.rhyven.json --accept-permissions
rhyven --collection script-trial call rhyven_describe '{"category":"acme/text-tools"}'
rhyven --collection script-trial call rhyven_call '{"category":"acme/text-tools","function":"action_analyze","args":{"text":"hello world"}}'
```

Validation and packaging never execute publisher code or install dependencies.
Script behavior tests require `--allow-host`; `--allow-container` does not grant
host execution. Installation prepares dependencies after permission acceptance.

## Manifest and files

An authoring directory contains `app.json`, source files and optional dependency
lockfiles. The new `files` field is an explicit array of relative source paths:

```json
{
  "execution": {
    "driver": "script",
    "protocol": "rhyven.action/1",
    "language": "python",
    "entrypoint": "main.py",
    "python_version": "3.10",
    "environment": "isolated",
    "timeout_seconds": 30
  },
  "permissions": ["state.read", "state.write", "host.execute"],
  "files": ["main.py", "helpers.py"]
}
```

Use `language: javascript`, `entrypoint: main.mjs` and `node_version: 20.0` for
Node. Version fields are minimum `major.minor` versions. No shell command or
arbitrary launch flags are accepted. Arguments travel as JSON, never shell text.

`app package` embeds the listed UTF-8 files into the distributable JSON manifest.
All code and lockfiles participate in its checksum. File-based packages must be
self-contained; only an explicitly selected authoring directory resolves file
paths. Traversal, source symlinks, conflicting file/directory paths and dependency
directories are rejected. The current limits are 128 files and a 1 MiB package.
Large applications and binary assets should use containers.

## Dependencies and environment sharing

Declare `execution.dependencies` when dependencies are needed:

```json
{"dependencies":{"pip":"requirements.lock","npm":"package-lock.json"}}
```

Both managers may be used by either language. The repository documentation tool
uses Python for app logic and npm for Pyright and TypeScript language servers.
Include all referenced lockfiles in `files`, plus `package.json` when using npm.

- Python uses a venv and pip. Each noncomment lockfile line must contain exactly
  one pinned requirement, `name==version --hash=sha256:HASH`, with additional hash
  tokens permitted. All transitive dependencies must be pinned and hashed. Only
  wheels are installed; source builds, editable installs, URLs and pip option
  directives are rejected. The host needs Python venv/pip bootstrap support.
- Node uses `npm ci --ignore-scripts` and a version 3 lockfile. Dependency entries
  must identify HTTPS artifacts with SHA-512 integrity. Local/git dependencies
  are rejected. Packages requiring install scripts/native builds need a container
  or an explicitly designed future installation capability.

Rhyven stores environments under `RHYVEN_HOME/script-runtime/environments/` and
shares download caches under `script-runtime/cache/`. A legacy workspace keeps
these under its `.rhyven` directory.

`environment: isolated` is the default: an environment is tied to the exact app
package and interpreter identities. Collections installing the same package can
reuse it while keeping separate app state. `environment: shared` allows different
apps with identical interpreter identities, dependency locks and npm manifests
to reuse an environment. Choose the mode in the manifest before packaging; the
documentation tool's source generator also accepts `--environment shared`.

The runtime never upgrades a prepared shared environment in place. Changed
dependencies produce a new environment. Interrupted preparation is rebuilt before
use; a missing environment or changed interpreter requires explicit reinstallation.
Environments are rebuildable caches, not backup contents. Automatic cache garbage
collection is not provided yet.

## Invocation and state

The request shape matches on-demand container actions, with protocol
`rhyven.action/1`. `context.data_dir` and `RHYVEN_DATA_DIR` name the app's persistent
directory for the selected collection. Replies are `{"result":{...}}` or
`{"error":{"code":"APP_ERROR","message":"Explanation"}}`. Logs go to stderr.

Input/output validation, action locks, audit records and retry receipts use the
shared executable-action lifecycle. Successful retries return the saved result.
Failed or interrupted calls leave an incomplete receipt; reusing that request ID
returns `script_incomplete`. Inspect state before retrying with a new ID.

App files under the supplied data directory participate in existing backup/restore
and staged updates. Removal retains them. The existing internal `containers/`
state subtree is reused for executable-app data to preserve archive compatibility.
Code and dependency environments are reconstructed from the stored package.

Changing a published app from container to script is not an in-place update.
Use a separate collection/installation and explicitly migrate state. Script apps
can use the existing declarative migrations for SQLite objects, but script-based
migration/health hooks are intentionally unsupported: host code cannot guarantee
the restricted side effects required by the container maintenance contract.

## Host access and limits

`host.execute` means **unsandboxed execution as the runtime's OS user**. This grants
host filesystem, network and subprocess access. A venv or `node_modules` directory
isolates dependency versions only. `state.write` still controls the generic SQLite
API, but it does not restrict what native code can write through the OS.

Rhyven clears inherited environment variables and supplies only execution-related
values. This avoids accidental credential inheritance; host code can still access
files readable by the OS user. Do not use this backend for untrusted publishers.

Calls have a 1–300 second timeout and 1 MiB stdout/stderr limits. Each process gets
a process group; ordinary descendants are killed when the invocation finishes,
times out or fails. Deliberately detached processes and hard runtime crashes are
outside that cleanup guarantee. CPU/memory quotas, filesystem/network sandboxes,
systemd supervision, background services and scheduled jobs are not implemented
by this backend. Container isolation remains available for those requirements.

## Marketplace publication

For now, public marketplace apps must provide the release source in a public
repository with an OSI-approved license. Include the manifest, app logic and
build files. A downloadable binary alone is insufficient. This submission policy
does not restrict private local apps; it is not an automated license audit.
