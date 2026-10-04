# Connect an agent harness

With runtime `0.4.0-rc.4` or later, use one command:

```sh
rhyven --collection my-project connect --client codex
```

Other adapters: `claude` (Claude Code user configuration), `cursor` (user MCP
configuration), `vscode` (the current project's `.vscode/mcp.json`), `hermes` (Hermes Agent), `openclaw` (native OpenClaw MCP), and `cline`
(pass `--config /path/to/cline_mcp_settings.json`). `generic` returns a portable
entry for any client supporting stdio MCP. The client does not need to be running.

`rhyven` with non-terminal input/output returns JSON connection instructions;
`rhyven --agent` does the same even in a terminal. A normal interactive launch
still opens the marketplace. Explicit `rhyven market` opens the TUI.

Setup pins the executable and collection, probes MCP initialization, lists the
three tools and calls discovery before writing settings. It preserves unrelated
configuration, is idempotent, and refuses to change an existing different entry
unless `--replace` is supplied. `--name rhyven-project` adds another connection.
`--config PATH` selects an explicit destination for any adapter. JSON files with
comments or invalid syntax are left untouched; merge `--print` output manually.
OpenClaw JSON5 is accepted and written as JSON; comments are normalized.
Hermes preserves text outside a plain top-level `mcp_servers` block. Complex YAML
anchors, tags or flow-style top-level mappings require a manual merge of `--print`
output. Existing Hermes/OpenClaw settings are backed up privately beside the file
as `<filename>.rhyven-backup-*` before changes. Codex TOML comments and unrelated entries are retained. Symlinked destination
files are rejected; pass their real path explicitly.

```sh
rhyven --collection my-project connect --client cursor --print
rhyven --collection my-project connect --check
```

The response distinguishes **configured**, **server_verified**, and
**client_session_verified**. The probe verifies the server, not whether an
already-running agent client has reloaded its configuration. Reload/restart the
client and call `rhyven_categories()` to confirm its collection. Installing more
apps afterward needs no MCP restart.

For a shared REST server, export its token in the client process environment:

```sh
rhyven connect --client codex --server https://your-server --expect-collection my-project
```

Verification reads the collection from the server and rejects a mismatch before
changing settings. Local collection flags do not override remote routing. Tokens
are not stored or printed; Codex `env_vars` forwards their names, and other clients
must inherit the token environment. A graphical client launched outside that
environment needs its own environment configuration. The URL is REST, not native
HTTP MCP. Authenticated `GET /` and `GET /connection` return connection
instructions; `serve` prints a readiness message to stderr after binding.

## Hermes Agent and OpenClaw

Rhyven 0.5.4 adds setup adapters for both clients. Install the client separately;
Rhyven does not install an agent or configure its model provider.

```sh
rhyven --collection my-project connect --client hermes
rhyven --collection my-project connect --client openclaw
```

Hermes uses `~/.hermes/config.yaml` or `$HERMES_HOME/config.yaml`. For another
profile, pass its file with `--config`. In Hermes, use `/reload-mcp` or start a
new session, then ask it to call `rhyven_categories()` and confirm `my-project`.

OpenClaw uses `~/.openclaw/openclaw.json`, respecting `OPENCLAW_STATE_DIR` and
`OPENCLAW_CONFIG_PATH` (explicit `--config` takes precedence). It must support
native `mcp.servers`; the adapter does not modify an older mcporter registry.
Run `openclaw mcp doctor rhyven --probe`, then reload/restart the Gateway that
owns the agent connection. Check discovery in the actual agent session.

Both use the same three stdio MCP tools and existing marketplace consent flow.
Do not configure automatic approval of app installs. A Gateway running elsewhere
needs Rhyven on that host, with paths accessible to that process. Use `--server`
for a shared Rhyven REST instance; Rhyven still provides the local stdio bridge.
A server probe does not prove a running client has loaded its configuration.

Configuration references: [Hermes MCP](https://hermes-agent.nousresearch.com/docs/user-guide/features/mcp),
[OpenClaw MCP](https://docs.openclaw.ai/tools/mcp).

## Existing configuration command

The server is ordinary stdio MCP. Start it with an absolute executable and workspace path:

```text
/absolute/path/rhyven --workspace /absolute/path/project --actor assistant mcp
```

Run `rhyven --workspace /absolute/path/project config CLIENT` to print a config for `claude`, `cline`, `cursor`, `vscode`, `codex`, `hermes` or `openclaw`. It prints only; it does not overwrite existing harness configuration. Merge the generated entry into your harness's MCP settings. Do not start the terminal marketplace from an MCP configuration.

Claude/Cline/Cursor use the familiar `mcpServers` entry with `command` and `args`. VS Code uses `servers` and `type: "stdio"`. Codex output is TOML under `[mcp_servers.rhyven]`. The command should point to the provided binary or your compiled `target/release/rhyven`.

The default server exposes `rhyven_categories`, `rhyven_describe`, and `rhyven_call`. Newly installed apps appear immediately through `rhyven_categories`; no restart is needed. Run `rhyven tools` to inspect the same definitions without an agent. `docs/agent-guide.md` is optional project guidance.

## Shared runtime

To serve one customer-controlled workspace to multiple trusted clients, set a
16–512 character bearer token and start the loopback HTTP adapter:

```bash
export RHYVEN_SERVE_TOKEN='a-long-random-secret-at-least-16-characters'
rhyven --workspace /absolute/path/project serve --port 7421
```

Point each harness's ordinary stdio MCP entry to
`rhyven --workspace /absolute/path/project mcp --server http://127.0.0.1:7421`.
The MCP adapter calls the REST interface, so every harness retains the same
three tools and operates the same SQLite state. For another host, use
`serve --host 0.0.0.0 --allow-insecure-network` only behind a TLS reverse proxy.
The token authenticates a shared trusted-client group; do not treat the optional
actor header as user authentication.

## Standalone mode

Install/review an app, then export it:

```bash
rhyven app export-mcp rhyven/work-management --out ./work-mcp
```

Point a harness at the absolute path to `work-mcp/launch.sh`, with no arguments. Or merge `work-mcp/mcp.json`; that generated JSON uses absolute paths and must be adjusted if the folder moves. The launcher discovers its own directory and is relocatable. It includes the runtime binary, starts separate state and requires no external Rhyven installation. It validates the exported package digest before startup. No original data or credentials are copied.

Generated tools include `object_task_create`, `object_task_get`, `object_task_query`, `object_task_update`, `action_assign`, `action_complete`, and `describe_app`. Create accepts `{data: {...}}`; actions accept `{args: {...}}`. Immutable objects have no update tool. Schema changes require re-export and restart for standalone mode. `rhyven mcp --app rhyven/work-management` gives the same generated interface for an already installed app without exporting.

## Validation and compatibility

The stdio implementation supports MCP revisions 2025-11-25, 2025-06-18, 2025-03-26 and 2024-11-05, initialization, ping and tools. It returns structuredContent on newer revisions and JSON text content on all supported revisions. MCP form elicitation obtains marketplace consent on supporting hosts. This prototype does not implement resources, prompts, sampling, OAuth, or a network MCP transport; remote MCP mode is still local stdio connected to the REST runtime. A real Codex client has searched, prepared and applied installations after explicit human approval through the host approval flow. Client-native elicitation dialogs and other agent products still need interactive acceptance. Protocol interoperability is also tested with the official MCP SDK; see the [candidate report](release-status.md).

References: [MCP tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools), [Claude Code](https://code.claude.com/docs/en/mcp), [Cline](https://docs.cline.bot/mcp/mcp-overview), [Cursor](https://cursor.com/docs/mcp), [VS Code](https://code.visualstudio.com/docs/agent-customization/mcp-servers), [Codex](https://learn.chatgpt.com/docs/extend/mcp?surface=cli).

Marketplace management on a shared server additionally requires a distinct
`RHYVEN_MANAGEMENT_TOKEN` on the server and trusted local MCP adapter. It is not
an agent tool argument. Without it, the adapter can browse but cannot prepare,
approve or apply package changes. Never configure automatic acceptance of
elicitation requests. See [agent marketplace](agent-marketplace.md).
