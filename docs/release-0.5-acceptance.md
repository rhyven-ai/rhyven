# 0.5.0 acceptance

Publication authorized after acceptance. Linux runtime binaries and the optional
starter image have passed hosted tests; production delivery is verified separately below.

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

## Hosted and app acceptance

- Linux x86-64 and ARM64 musl builds passed on native hosted runners:
  [binary build](https://github.com/rhyven-ai/rhyven/actions/runs/37140092816).
  The runtime sources are from c8559a7; subsequent commits change app/test/docs
  files, not compiled runtime code.
- Starter Runner's real Docker test passed using the signed distributed binary:
  model HTTP, restricted callbacks into Work Management/Project Knowledge/User
  Questions, waiting without model polling, human answer, service restart,
  phase completion and backup/restore. The image security scan passed:
  [image acceptance](https://github.com/rhyven-ai/rhyven/actions/runs/37140619527).
- Six local runner tests include crash replay after a peer commit, no duplicate
  notes, budget exhaustion, cancellation and unsupported model operations.
  User Questions includes two packaged conformance cases; negative tests cover
  expiry, actor separation, protected fields and stale revisions.
- Website browser/accessibility and security regressions pass with eleven apps
  and the optional starter guide. General harness connections remain unchanged.

The x86-64 distributed binary SHA-256 is
`5c8cd35f03ae18da2b0448401e79ba0bdf2717b09ab445be0b8988ea38c0c9bc`.
The registry validator pins these exact bytes. Downloads use the existing
production release key; private signing material is not in the repository.

## Limits

The runner uses a deterministic OpenAI-compatible model fixture in acceptance,
not paid provider credentials. Its initial image is Linux x86-64 only. Its token
and time budgets are soft limits, not financial guarantees. It is a planning and
knowledge starter, not a shell-capable autonomous agent or multiagent harness.

The hosted tests use fresh Linux runners with Docker already installed. This
release does not claim a new physical host reboot test or a fresh-machine Docker
installation test. Existing service lifecycle and installer fixture coverage is
retained. Production DNS, download integrity and anonymous registry access are
checked during publication.
