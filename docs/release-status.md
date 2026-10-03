# Release status

Rhyven 0.4.0-rc.10 adds:

- Knowledge export, merge preview and atomic apply, preserving provenance and correction chains.
- Query OR branches, field-presence checks, case-insensitive substring/prefix matching and field selection.
- Background TUI marketplace sync with `r`, plus matching agent refresh and refresh-status functions.
- Host requirement checks in app details and installation reviews, and a dedicated agent function.
- `rhyven upgrade` and `rhyven upgrade --check`, using the signed-release installer.
- Automatic initial catalog discovery with offline fallback and clearer package compatibility errors.

The runtime and registry validator are released together. Signed Linux x86-64 and
ARM64 downloads are supported preview targets. Native Windows is unsupported.
Other platforms need separate acceptance testing.

The engine and apps are Apache-2.0. Website source remains private. Declarative,
Python/JavaScript script, on-demand container and persistent service apps keep the
same three-tool MCP and REST interfaces. Scripts require reviewed `host.execute`
permission and run unsandboxed as the OS user.

See [knowledge merge](knowledge-merge.md), [declarative queries](declarative-engine.md),
[shared hosting](shared-hosting.md) and [installation](installation.md).
