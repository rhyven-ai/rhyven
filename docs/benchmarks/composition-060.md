# 0.6.0 local validation

No release, registry change or website deployment was made.

- Workspace tests after the portable-library correction: 104 passed, zero failures/ignored tests.
- cargo fmt --all --check: passed.
- cargo clippy --workspace --all-targets --locked -- -D warnings: passed.
- Linux x86-64 release binary: built, reports 0.6.0.
- qa/universal_market_check.py: passed (three MCP tools, approval accept/decline,
  no-host fallback, live discovery, receipts, revisions and REST parity).
- qa/shared_runtime_check.py: passed (authentication, two clients, shared state
  and the REST-backed MCP adapter).
- Usage and composition skill validation: passed; skills are embedded in binary.

New tests exercise dependency pins and permission unions, references, iteration,
partial failure/retry blocking, actor-scoped reports, isolated conformance,
native ELF execution/checksums/approval/registry metadata, frame traversal and
overwrite rejection, classifier protocol/fallback boundaries, and a saved stack
across the three quality apps with persisted evidence visible to another agent.

The optional Laya/Jev adapters were tested with local mock HTTP servers.
No live model credentials or checkpoints were used. ARM64 artifacts are recognized
but were not executed on an ARM64 machine. Container lifecycle regression code
was covered by existing tests, not a new fresh-machine Docker acceptance run.

## Portable library acceptance

qa/portable_pallet_check.py passes against the rebuilt 0.6.0 binary. It verifies
source validation/test/save, three repeated calls to the same saved function,
cached descriptions, standalone ordinary Python import, scaffolding, and a
complete app built from vendored source. None registers a library as an app.

Six library tests additionally cover Python/JavaScript standalone launchers,
frames with separate app/library references, fresh-runtime app execution,
immutable versions, source tampering, execution consent, and collection
backup/restore. These checks demonstrate reuse mechanics, not measured model
token savings. Python/JavaScript have run/test automation; other languages have
source export and use their normal build toolchains.

The usage rule, composition skill and implementation docs now distinguish
portable source stacks from engine-managed app workflows.

## Retrieval fixture

Initial debug-build run, 30 manually authored queries against three quality apps
plus the bundled catalog:

| Measure | Result |
| --- | --- |
| Expected matching queries | 28 |
| Expected action in top three | 24/28 |
| No-match queries with empty shortlist | 2/2 |
| Mean response | 2,138 bytes |
| Cold median | 180 ms |
| Cached median | 55 ms |
| Model calls | 0 |

This is a small synthetic retrieval fixture, not an independent held-out model
evaluation. Timing varies with build/host/cache state. Bytes are not token counts.
It does not establish token savings or classifier quality. Reproduce with
cargo test -p agent-market-core --test discovery_benchmark -- --nocapture.
