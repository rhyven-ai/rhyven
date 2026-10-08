# Composable capabilities — 0.6.0

Existing apps and the three-tool MCP contract
remain compatible. REST and MCP dispatch through the same runtime.

## Apps and portable code libraries

Apps are complete applications. [Portable pallets](portable-pallets.md) contain
ordinary reusable source: bricks (functions), mortar (adapters) and portable
stacks (composed functions). They have a separate format and local library store.
They are not installed apps.

This document covers engine-managed app workflows and execution. A workflow's
operation is still named stack, but it calls installed applications and depends
on the engine. Source pallets and portable stacks do not require that executor.

Frames now distinguish portable pallet references from installed-app references.

Python, JavaScript, native executables and containers share the JSON action
contract. They do not share language objects or imports. Python environments
continue to use existing immutable dependency identities; there is no mutable
global environment that one app can upgrade underneath another.

## Find capabilities without repeated browsing

Describe `rhyven/marketplace` and call `action_match_plan`:

```json
{
  "task": "project-quality",
  "revision": 1,
  "steps": [
    {"id": "check", "need": "Check supplied files for invalid JSON",
     "output_type": "object",
     "output_fields": [{"name": "passed", "type": "boolean"}]}
  ],
  "permissions": ["state.read", "state.write", "host.execute"],
  "backends": ["declarative", "script"]
}
```

Use `action_inspect_candidate` with the returned `session` and candidate `id`.
Only shortlisted IDs are accepted. Repeated inspections reuse saved descriptions.
`input_fields` optionally describes available top-level arguments;
`output_fields` describes required top-level results. These are type filters,
not proof that units, meanings, enum values or nested schemas agree.

The local SQLite FTS5/BM25 index covers installed app actions, cached catalog
packages and locally saved pallet exports. Use backend source for reusable code;
app actions are labeled app_action and remain separate from source exports. It never refreshes the network, installs an app, or executes a
candidate. Limits are 12 plan steps, 20 lexical hits examined per step, three
candidates per step, 12 overall, eight distinct contract inspections, and one
explicit retry (`retry:true`). Results are bounded to 16 KiB. Retrieval has a
15-second cooperative deadline; filesystem/SQLite admission is not forcibly
interrupted. Permission and backend filters apply before suggestions are returned.

Reuse the same task/revision for the same plan. Changes to the plan, catalog or
ranker configuration require an explicit revision increment. Do not increment
revisions just to evade the search budget. If no useful candidate is found,
build the missing capability or ask a focused question. Host readiness remains
`not_checked`; call `action_requirements` before proposing installation.

Only action metadata is indexed in this version. Public metadata requires an
explicit registry refresh through the existing workflow. There is no external
search crawler, embedding database or autonomous planning loop.

## Save an app workflow

A draft uses existing package fields plus dependency aliases:

```json
"dependencies": {"notes": "example/notes"}
```

After the user has approved and installed dependencies in the collection:

```sh
rhyven --collection project app compose --definition draft.json --out workflow.json
rhyven --collection project app validate workflow.json
rhyven --collection project app test workflow.json
rhyven app package workflow.json --out workflow.package.json
```

Compose replaces each alias with `app`, exact `version` and canonical package
`sha256`, and computes the conservative union of permissions. A package hash
pins all of its action contracts and implementation. No downloads, activation
or public publication occur. Existing output files are not overwritten.

Tests copy dependency contracts into an isolated collection, not their live
state. Script/native fixtures require `app test --allow-host`; container
fixtures require `--allow-container`. Persistent service dependency fixtures
need a separately managed integration test. Package creation does not silently
run stack dependencies. Activate using the existing reviewed installation flow.

A stack action has typed input/output, `operation:"stack"`, `steps` and
`result`. Each step names an `id`, dependency alias, action and argument mapping:

```json
{
  "id": "saved",
  "dependency": "notes",
  "action": "save",
  "args": {"title": {"$input": "/title"}}
}
```

Bindings use JSON pointers:

- `{"$input":"/title"}`: caller input.
- `{"$step":"saved","path":"/data/title"}`: earlier step result.
- `{"$item":"/title"}`: current element of a step's optional `foreach` array.
- `{"$expr":{"arg":"title"}}`: existing bounded engine expression over caller input.

Objects and arrays recursively bind values. `when` must resolve to a boolean.
False conditions skip the step and expose null. Missing pointers and unknown or
forward step references fail; Rhyven does not invent missing values. Concrete
child inputs and outputs are validated at execution. Complex adaptations use
an ordinary tested action. General static proof of nested binding compatibility
is not implemented.

Limits: 16 direct dependencies, graph size 32, depth four, 32 steps per action,
32 items per iteration, 64 total child calls across nested stacks, 256 KiB per
child result and 1 MiB of intermediate values. A five-minute shared dispatch
budget stops launching more children when their declared timeout cannot fit.
It is not a hard preemptive deadline for every declarative operation.

Dependencies must already be installed at their exact pins. Changes stop the
stack; re-compose, test and review a new version. Platform management actions
cannot be stack dependencies. Stacks cannot grant approval.

## State and failure recovery

Stack runs and step outcomes live in the collection SQLite database, alongside
ordinary backup/restore state. Records include actor, package pins, input/output
hashes, child request IDs, status and errors. The final result is retained;
intermediate payloads are not copied into the run log.

Supply a stable `request_id`. A completed identical retry returns its saved
result. A changed request with that ID conflicts. A failed or interrupted run
does not automatically replay possible side effects. Read
`action_stack_report` with the request ID using the same actor, then reconcile
app state. Child app state remains accessible to other authorized agents in
the collection.

This is not a distributed transaction. Earlier effects survive later failures.
Run records remain with collection state; there is no automatic retention
purge yet. Do not return secrets in results intended to be persisted.

## Native executables

Linux x86-64 and ARM64 ELF64 targets are recognized. The local execution test
runs on x86-64; ARM64 execution still needs an ARM64 acceptance host.

```sh
rhyven app native-artifact ./my-action
```

Insert its hash/hex object into an ordinary executable app manifest:

```json
"execution": {
  "driver": "native",
  "protocol": "rhyven.action/1",
  "timeout_seconds": 30,
  "artifacts": {
    "linux-x86_64": {"sha256": "...", "hex": "..."}
  }
}
```

Publishers build binaries. Rhyven verifies checksums, ELF class/architecture and
executable type, then launches the package-owned binary without a shell.
It supplies one JSON request on stdin and expects `{"result": {...}}` on stdout.
Use stderr for diagnostics. Inputs and outputs are objects, as for scripts.
Collection data is available through `RHYVEN_DATA_DIR`.

Artifacts are limited to 450 KB each and the entire package to the existing
1 MiB. Descriptions and approval reviews show hashes, not binary bytes.
Use containers for larger binaries or packaged OS dependencies. Dynamic linker,
libc and library compatibility remain the publisher/operator's responsibility;
a matching ELF header alone does not prove ABI compatibility.

Native execution requires `host.execute` and is **unsandboxed OS-user code**.
Rhyven supplies an empty inherited environment with explicit HOME, PATH and
data directory, and uses the existing timeout/output/process-group controls.
It does not install Cargo, compilers, OS packages or missing libraries.

## Frames

```sh
rhyven app frame examples/quality-stack/frame.json --dir ./quality-project
rhyven app frame examples/quality-stack/frame.json --dir ./quality-project --apply
```

The default previews files, exact pallet@version references and separate app IDs. Applying requires a new
directory, rejects traversal/conflicting paths, and writes provenance. Dependencies
are references, not implicit installation permission.

## Optional Laya / Jev-compatible ranking

No model is required or downloaded. Configure an operator-managed endpoint in
the collection state directory's `discovery-ranker.json` (normally
`~/.rhyven/collections/PROJECT/discovery-ranker.json`; legacy workspaces use
their `.rhyven` state directory):

```json
{
  "enabled": true,
  "provider": "laya",
  "endpoint": "http://127.0.0.1:8000/v1/systemone"
}
```

`laya` and `jev` use the typed questions/answers choice protocol; `generic`
sends `{plan,candidates}` and accepts `{"order":["c1","c0"]}`. Match the endpoint
to the server's route. These wire adapters have mock-server tests; no live model
accuracy comparison has been performed.

Remote ranking requires HTTPS and `allow_remote:true`. Optional bearer
credentials come from an `auth_env` named `RHYVEN_RANKER_*`.
Only plan-step text and shortlisted descriptions are sent, not source or app
records. There is at most one classifier call per plan revision, with a maximum
five-second timeout. Invalid/unavailable responses fall back to lexical ordering.
The classifier can only reorder offered candidates; it cannot run tools,
change permissions, install apps or initiate publication.

## Worked example and measurements

[Quality stack](../examples/quality-stack/README.md) composes three existing apps.
The 30-query retrieval fixture is in
`crates/core/tests/fixtures/discovery-plans.json`. Run:

```sh
cargo test -p agent-market-core --test discovery_benchmark -- --nocapture
cargo test -p agent-market-core --test quality_stack
```

The initial local debug run found the expected action in the top three for
24/28 matching queries, with 2/2 no-match abstentions. Mean response size was
2,138 bytes; median cold/warm lookup was 180/55 ms. These are a small synthetic
fixture and one host, not held-out model evaluations or token-saving claims.

Public registry rollout, website deployment, ARM64 executable acceptance and
live classifier quality evaluation remain release checks. Nothing in this branch
authorizes publication.
