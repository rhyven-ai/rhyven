# Release status

Rhyven 0.5.0 provides signed Linux x86-64 and ARM64 releases, with a matching
registry validator and website installer.

New in 0.5.0:

- Import selected tools from an existing HTTP MCP server or supported OpenAPI JSON
  document as a connector app. Upstream services and credentials remain user-managed.
- Smaller discovery responses, keyword/alias search, batch descriptions, optional
  function indexes, contract hashes and actionable function lookup errors.
- Read-only MCP annotations on discovery; calls retain normal approval controls.
- Slow connector requests no longer hold the collection maintenance lock.
- Optional Starter Runner for users without a harness, plus a separate declarative
  User Questions app. Existing harness connections and general apps remain independent.
  See [starter setup](../apps/starter-runner/README.md).

All existing declarative, script, on-demand container and persistent service
backends remain available. Engine and apps are Apache-2.0. Linux x86-64 and ARM64
are distribution targets; acceptance evidence is recorded per tested architecture.
Scripts run as the OS user with reviewed host.execute permission, not a sandbox.

See [0.5 acceptance](release-0.5-acceptance.md), [connector apps](connector-apps.md),
[discovery compatibility](universal-protocol.md) and [live benchmarks](codex-discovery-benchmark.md).
