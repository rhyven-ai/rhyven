# 0.5.0 release preparation

Status: prepared locally, not published. Publication remains on hold.

## Implemented

- Read-only, non-destructive, idempotent discovery hints. Generic execution is
  not marked read-only; marketplace host approval remains mandatory.
- Connector requests release the collection maintenance gate after the start
  audit event. Completion reacquires it; dispatch keeps its captured package.
- Discovery client migration notes and optional metadata documented.
- Engine/Cargo lockfile, installer release workflow, registry validator pin,
  website release text and agent skills prepared for 0.5.0 together.
- Version-dependent acceptance tests updated for the stable version bump.

## Verified locally

- 81 Rust tests; Clippy with warnings denied; formatting and source-export tests.
- Actual Codex CLI discovery through the 0.5 build, without per-tool approval
  overrides: categories and describe succeeded. Execution approval was not relaxed.
- Linux x86-64 musl release artifact, packaged with installer/license/checksums.
- Signed ephemeral local HTTPS installation using the exact portable binary.
- Upgrade from the actual published 0.4.0-rc.10 binary: a task created by rc.10
  remains byte-for-byte equivalent when read after installation of 0.5.
- Installer corruption/signature rejection, retained collections, catalog setup,
  dependency setup fixtures and failed-upgrade preservation.
- Local MCP and authenticated REST wrappers, batched/indexed discovery, missing
  credentials, server errors and redirects; no real remote writes performed.
- Controlled paused upstream call while an unrelated collection read AND task
  creation complete; the connector then finishes successfully.
- Three-tool marketplace consent acceptance/decline and no-host fallback;
  collection isolation, shared runtime, knowledge merge/query parity.
- Python and JavaScript execution tests, including child credential isolation.
- All nine existing public registry packages validated by the candidate binary;
  publisher code/container workloads were not executed by registry validation.
- Website staging unit tests, JS syntax, desktop/mobile browser checks, automated
  WCAG A/AA checks and security regressions; no external requests, cookies or storage.

## Staged repositories and artifact

Engine: local feat/connector-wrappers branch. Registry and private website:
local prepare/0.5.0 branches. Nothing pushed, merged, deployed or released.

Artifact directory: `dist/release-0.5.0-linux-x86_64/` (ignored build output).
The x86-64 binary SHA-256 is
`bf7c06478f227fc2a27e7b98d5c8ef66067ccb2442214f2ae8d3996ec37e18ba`.
The registry workflow and engine validator checksum refer to those exact bytes.
If CI rebuilds the artifact, update both pins to that build's verified digest
before publishing/merging. Never copy a checksum from a different build.

## Remaining publication gates

These are not represented as completed by local tests:

- ARM64 build and execution acceptance on that architecture.
- Real Docker/container isolation and persistent-service/reboot acceptance on a
  suitable host. This execution environment has no Docker or VM runner; installer
  dependency tests use controlled fixtures, not a fresh-machine Docker install.
- Production signing, uploading both platform artifacts, verifying the public
  one-command download, then activating the matching validator and website.
  Signing tests used temporary test keys, not the production release key.

Do not publish the staged website claiming both Linux architectures before the
ARM64 artifact is available. No publication is authorized by this preparation.
