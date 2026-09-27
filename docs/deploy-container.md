# Deploy a container app

Use this method for custom Python, Node, Rust or other application code.
The user needs Rhyven and a local Docker Engine running Linux containers.
Rhyven starts the image for each action call, supplies JSON on stdin, validates
the JSON response, and retains app data between calls.

```sh
rhyven app init acme/analyzer --runtime container --dir ./analyzer
# Edit main.py, Dockerfile, and the input/output schemas in app.json.
docker build --iidfile ./analyzer-image.id ./analyzer
rhyven app package ./analyzer --image "$(cat ./analyzer-image.id)" --out ./analyzer.rhyven.json
rhyven app test ./analyzer.rhyven.json --allow-container
rhyven --collection my-project install ./analyzer.rhyven.json --accept-permissions
rhyven --collection my-project call rhyven_call '{"category":"acme/analyzer","function":"action_analyze","args":{"text":"hello world"}}'
rhyven --collection my-project config codex
```

The Python scaffold uses the standard library. Add any language/runtime and its
dependencies to your Dockerfile. Only the image is executed on recipient hosts;
they do not need Python or your build toolchain. `app package` validates the
manifest without running container code. `app test --allow-container` explicitly
permits pulling its image and executing tests in a temporary state directory.

To distribute, push your image to your own container registry (for example GHCR):

```sh
docker tag "$(cat ./analyzer-image.id)" ghcr.io/YOUR_GITHUB_OWNER/analyzer:0.1.0
docker push ghcr.io/YOUR_GITHUB_OWNER/analyzer:0.1.0
# Use the immutable digest printed by push, replacing DIGEST below.
rhyven app package ./analyzer --image ghcr.io/YOUR_GITHUB_OWNER/analyzer@sha256:DIGEST --out ./analyzer-release.json
```

Upload `analyzer-release.json` as a GitHub Release asset and submit its
`registry-entry` through the normal [registry PR flow](github-registry.md).
Local `sha256:image-id` references are for development; marketplace entries need
a distributable repository digest. Images must be public or users must have
Docker credentials for that registry. Publish images for the recipients' CPU
architectures. The registry's validator binary must include container support
before it can accept these entries.

Install approval covers the image, permissions and secret names. Rhyven pulls
missing images after approval; installation never starts app code. Calls use
the installed image without an automatic pull. Data is mounted at `/data`, with
one directory per app per collection. Deleting an installation retains its data.
Docker shares image layers across collections. Docker's image cache is separate
from the Rhyven manifest store.

Agents use the same three MCP tools and REST routes as declarative apps. To use
REST, set `RHYVEN_SERVE_TOKEN`, start `rhyven --collection my-project serve`, and
connect using `rhyven mcp --server http://127.0.0.1:7421`.

This mode runs bounded actions. For background processes use the opt-in
[persistent service mode](deploy-service.md), with its supervised lifecycle and
scoped peer callbacks. Multi-container stacks remain unsupported. See the
[container protocol and limits](container-contract.md) for the one-shot contract.

## Host compatibility diagnosis

Run `rhyven doctor` for a JSON report. It reports declarative availability separately
from container engine prerequisites; inspect `container.ready` (the diagnosis
command itself succeeds even when the engine is unavailable). It does not install
software, pull images, change Docker contexts, or execute publisher code.

Agents use the existing universal interface:

```json
{"category":"rhyven/marketplace","function":"action_doctor","args":{}}
```

Docker must expose a local endpoint, Linux containers and CPU/memory/PID controls.
Rootless mode additionally requires systemd/cgroup v2. Install checks occur before
image pulls; calls recheck the current engine before recording a pending receipt.
A `container_unavailable` preflight failure can be retried with the same request ID
after repairing the environment. A `container_incomplete` result still requires
inspection because execution may have begun.

Publish pinned images supporting the target engine architecture (for example a
multi-platform index covering linux/amd64 and linux/arm64). Automatic emulation
is not enabled. Engine capability reports do not certify effective isolation,
registry connectivity or Docker Desktop mount behavior.
