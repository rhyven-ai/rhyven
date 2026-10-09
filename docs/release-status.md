# Release status

Development version 0.7.0 adds local hash-bound pallet test evidence and the
preferred `workflow` operation alias. See [0.7.0 changes](release-0.7.0.md).
The currently published downloads remain 0.6.0.

Rhyven 0.6.0 adds portable source libraries alongside complete headless apps.

New in 0.6.0:

- Bricks, mortar and portable stacks are ordinary reusable functions, grouped
  into versioned pallets. Python and JavaScript have built-in test launchers;
  other source languages use their own toolchains.
- Workspace and user-global libraries, explicit promotion and scope-aware
  discovery. Bundled app dependencies retain exact versions and content hashes.
- A separate Pallets marketplace view with approved source downloads through
  the TUI or the existing three-tool MCP interface.
- Bounded plan-based discovery, cached contracts, frames and engine app workflows.
- Native Linux executable actions with explicit host-execution permission.
- A bundled composition skill and updated usage guidance.

See [portable libraries](portable-pallets.md), [app workflows](composable-capabilities.md)
and [live pallet acceptance](pallet-marketplace-acceptance.md). The live acceptance
used a minimal MCP client and the actual TUI on Linux; it did not claim a live
Codex session or measured model-token savings.

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
