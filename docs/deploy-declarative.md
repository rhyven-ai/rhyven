# Deploy a declarative app

The unpublished 0.6.0 branch adds [composable capabilities](composable-capabilities.md):
pinned cross-app stacks, bounded plan matching, local frames and native ELF actions.
That document specifies engine workflows. [Portable pallets](portable-pallets.md)
are separate source libraries used to build complete apps; saving one does not
install an app. Existing app behavior remains compatible.

Use this method for structured records, relationships and actions such as status
changes, assignments, arithmetic, guarded stock changes and string normalization.
See the [declarative expression and query contract](declarative-engine.md) for
`expressions`, Boolean `condition`, runtime values and typed query operators.
Only Rhyven is required on the user's machine.

```sh
rhyven app init acme/assets --dir ./assets
# Edit assets/app.json: objects, actions, permissions, guide and tests.
rhyven app validate ./assets
rhyven app test ./assets
rhyven app package ./assets --out ./assets.rhyven.json
rhyven --collection my-project install ./assets.rhyven.json --accept-permissions
rhyven --collection my-project connect --client codex
```

Reload the agent client if needed. The agent discovers
`acme/assets` through `rhyven_categories()`, reads its functions through
`rhyven_describe`, then invokes them through `rhyven_call`. All state belongs to
`my-project`. Omitting `--collection` uses the selected CLI collection (initially
`global`); an existing agent connection stays pinned to its configured collection.

`rhyven app publish ./assets.rhyven.json` adds it to the local catalog. For the
marketplace, upload the package to a release in your GitHub repository, generate
its entry with `rhyven registry-entry`, and submit a registry PR. See
[GitHub publishing](github-registry.md). GitHub submission is still separate
from `app publish`.

For REST access, set `RHYVEN_SERVE_TOKEN` and run
`rhyven --collection my-project serve --port 7421`. Then use
`rhyven mcp --server http://127.0.0.1:7421` for MCP through REST. Both methods
expose the same app functions. Installation alone does not open a network port.

## Package behavior tests

`tests` is a sequential array run in isolated state. Each case has `operation`
and `args`; `app` is filled in by the runner. Use `expect` for dotted result paths,
or `error` for the exact expected runtime error code. A failing operation also
occupies a result index. `{ "$result": "0.id" }` references an earlier result.

```json
[
  {"operation":"execute","args":{"action":"start","args":{"title":"Candidate","release":"v1","checks":2}},"expect":{"data.remaining":2}},
  {"operation":"execute","args":{"action":"mark_ready","args":{"id":{"$result":"0.id"},"expected_revision":1}},"error":"guard_failed"},
  {"operation":"get","args":{"object":"checklist","id":{"$result":"0.id"}},"expect":{"data.status":"open","data.remaining":2,"revision":1}}
]
```

This example assumes the checklist actions and guarded readiness described by
that app. It checks that rejection leaves both fields and revision unchanged.
Include successful calls, rejected inputs/conditions, retry behavior and state
read-back for your app. `app package` validates and runs declarative behavior
tests before writing the distributable JSON file.

Use `--home /path/to/test-home` before each command for an isolated package home;
combine it with `--collection project-name` for separate state. A prebuilt binary
needs no Rust or Python to author, validate or operate a declarative package.
