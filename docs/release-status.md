# Release status

Rhyven 0.4.0-rc.9 adds on-demand Python and JavaScript apps with managed dependency
environments. Declarative apps, Docker actions and container services keep the
same three-tool MCP and REST interfaces. Schema alternatives and nullable values
support typed action contracts.

File RAG and the Razorback connection app exercise native execution without
Docker. Both require Python 3.10+; File RAG also requires Git. Native host.execute
grants OS-user access after review. It is not a sandbox. Razorback runs as a
separate shell; the user pairs a collection and approves actions in its terminal.

The runtime and registry validator must be promoted together. Signed Linux
x86-64 and ARM64 downloads remain the supported preview targets. Native Windows
is unsupported. Other platforms require separate acceptance testing.

The engine and apps are Apache-2.0. Website source stays in its private repository.
Corvid remains in progress as a separate harness.

## Unreleased merge/query branch

`feat/knowledge-merge-query` adds previewed, atomic knowledge export/import and
bounded OR queries, presence checks, string matching and field projection.
See [knowledge merge](knowledge-merge.md) and [declarative queries](declarative-engine.md).
The public binary and registry validator remain 0.4.0-rc.9; these additions need
a coordinated future release.
