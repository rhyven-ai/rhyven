# Release status

Signed 0.8.0 downloads and app packages are published. See the
[GitHub release](https://github.com/rhyven-ai/rhyven/releases/tag/v0.8.0)
for its current release designation and assets.

This release adds declarative file actions, Design Review, Change Verifier and
Customer Onboarding Monitor. Pallet library tooling is retired without deleting
saved source. See [release notes](release-0.8.0.md) and [upgrade notes](migration-0.8.md).

Rust tests, lints, parser checks and app tests passed. The three new apps passed
MCP integration checks; both container apps passed Docker restart and security
checks. Upgrade checks preserved records and backups from 0.5.5, 0.6.0 and 0.7.0.
The exact signed packages passed ten Linux installation checks. Anonymous
public installs and MCP use passed for all three new apps and the existing
documentation and messaging apps. The live site passed browser and accessibility
checks. Fresh Ubuntu VM acceptance is maintained in the
[public installation workflow](https://github.com/rhyven-ai/rhyven/actions/workflows/public-app-acceptance.yml).
No model cost or quality improvement is claimed.

## Retained runtime features

- Bounded plan matching, cached contracts and app workflows.
- Python/JavaScript scripts, native executable actions, containers and services.
- Shared collections, approvals, backups, migration recovery and runtime upgrades.

## Earlier releases

New in 0.5.5:

- Default onboarding installs the runtime without requesting Docker setup.
- Setup reports runtime readiness separately from optional container setup.
- Connection output reports client executable detection without claiming a live session.
- Cline errors identify how to find the active settings file; headless approval and
  collection routing are documented explicitly.
- System packages install `/usr/bin/rhyven`; the runtime updater respects package
  ownership. Skills remain embedded and install per user during `rhyven setup`.

Included from 0.5.4:

- `connect --client hermes` configures Hermes Agent's YAML MCP settings.
- `connect --client openclaw` configures native OpenClaw MCP through JSON5.
- Both preserve unrelated settings, back up existing configuration, respect
  profile paths, and verify the three-tool server and collection before writing.
- Installer output and agent connection documentation include both clients.
- Bundled usage guidance now tells agents to look for reusable app capabilities,
  search existing apps first, and keep installation/publication consent explicit.

Included from 0.5.3:

- All six skills and the usage rule are embedded in the binary and installed
  offline under `RHYVEN_HOME/skills/0.5.4` during setup. `rhyven skills` lists
  their paths; `rhyven skills --install` restores missing files while preserving
  local edits. No agent configuration is changed.

Included from 0.5.2:

- TUI refresh is manual: press `r` to reload local state and fetch listings.
  The automatic refresh timer and elapsed sync counter are removed.
- The installer configures existing Bash login profiles and prints the command
  needed to use Rhyven immediately in the current terminal.
- App formats, installed apps and collection state are unchanged.

Included from 0.5.0:

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
