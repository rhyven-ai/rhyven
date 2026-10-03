# Rhyven

Rhyven is an open-source marketplace, package manager and runtime for headless
apps that agents use through a shared interface. The engine, CLI, terminal
marketplace, MCP/REST adapters and Rhyven apps are licensed under Apache-2.0.

Apps run in your environment and keep their state in your collections. Use
schemas and bounded operations for declarative apps, Docker for custom actions,
or supervised containers for persistent services. Installing an app requires
no app-specific MCP server or changes to the runtime.

## Status

Version **0.5.3** supports Linux x86-64 and ARM64. Native Windows and macOS
are not supported release targets. Individual app images may support fewer architectures.

The signed **0.5.3** installer and platform binaries are available at
[rhyvenai.com](https://rhyvenai.com). App versions and container image digests
are released independently.
See [installation](docs/installation.md) and [release status](docs/release-status.md).

## Install

```sh
curl -fsSL https://rhyvenai.com/install.sh | bash -s -- --containers
rhyven
```

Omit `--containers` if you only need declarative apps. The installer verifies
signatures and checksums and requests approval for Docker dependency setup.
Open a new terminal after installation so PATH changes take effect.

## Build from source

Install Rust 1.90 or later, a C compiler and platform build tools. SQLite is
bundled; declarative apps do not require Docker or host Python.

```sh
git clone https://github.com/rhyven-ai/rhyven.git
cd rhyven
cargo build --release --locked --bin rhyven
./target/release/rhyven --version
./target/release/rhyven
```

Or install the CLI from a checkout with `cargo install --path crates/cli --locked`.
On Debian/Ubuntu the compiler prerequisites are `build-essential` and
`pkg-config`. See [contributing](CONTRIBUTING.md) for development checks.

## Connect an agent

```sh
rhyven --collection my-project connect --client generic --print
rhyven --collection my-project connect --check
```

Use a named client adapter when appropriate, or merge the printed MCP entry into
your client configuration. Reload the client and confirm the collection through
its actual connection. [Connection guide](docs/harnesses.md).

The universal MCP exposes three tools:

```text
rhyven_categories()
rhyven_describe(category)
rhyven_call(category, function, args)
```

A category is an exact app ID such as `rhyven/work-management`. Descriptions
return function names, argument schemas, permissions and agent guidance. REST
exposes the same contract; local MCP can call the core directly or bridge to a
shared REST server. [Architecture](docs/architecture.md).

Give your agent the [usage skill](skills/use-rhyven/SKILL.md) or merge the
shorter [usage rule](skills/use-rhyven/RULE.md) into its project instructions.

## Find and install apps

```sh
rhyven --collection my-project registry-refresh rhyven-ai/registry --anonymous
```

This fetches metadata without downloading apps. An agent can describe
`rhyven/marketplace`, search listings, show repository stars and permissions,
and prepare an install request. Downloads require human approval through the
host consent flow. [Agent marketplace](docs/agent-marketplace.md).

For terminal browsing, explicitly download the package catalog first:

```sh
rhyven registry-sync rhyven-ai/registry --anonymous
rhyven
```

The TUI supports browsing, installation, updates and removal. Collections have
separate app state; agents using the same collection can share records. Removal
retains data for compatible reinstallation. [Collections](docs/collections.md).

## Build an app

```sh
rhyven app init acme/checklist --dir ./checklist
rhyven app validate ./checklist
rhyven app test ./checklist
rhyven app package ./checklist --out ./checklist.rhyven.json
```

| App type | Behavior | Guide |
| --- | --- | --- |
| Declarative | Typed records, relationships, actions, calculations, rules and search | [Engine features](docs/declarative-engine.md) |
| Container action | Custom executable logic, bounded calls and persistent data | [Container deployment](docs/deploy-container.md) |
| Persistent service | Background work, readiness, heartbeats and supervised recovery | [Service deployment](docs/deploy-service.md) |

The [package format](docs/package-format.md) is shared by all three types.
`app publish` adds a package to a local catalog; submitting a marketplace listing
uses the [GitHub registry flow](docs/github-registry.md). The public registry is
[rhyven-ai/registry](https://github.com/rhyven-ai/registry), and the separate
app-only source repository is [rhyven-ai/apps](https://github.com/rhyven-ai/apps).

## State and recovery

Rhyven owns SQLite records, audit events and retry receipts for declarative apps.
Containers own their files under `/data`. Collections are state boundaries for
trusted clients, not per-agent security boundaries.

```sh
rhyven backup my-project --out project.rhyven
rhyven restore project.rhyven --collection recovered-project
```

Restore requires a new collection; container backups require permission review.
Updates migrate and validate staged state before activation. See
[backup, migrations and recovery](docs/recovery-and-updates.md).

## License and security

Rhyven is licensed under [Apache-2.0](LICENSE). See [NOTICE](NOTICE) and
[third-party licenses](THIRD_PARTY.md). Free personal and commercial use,
modification and redistribution are permitted under the license. It does not
grant general trademark rights. Binary notices are available with
`rhyven license --third-party`.

Report vulnerabilities privately using [SECURITY.md](SECURITY.md). Contributions
are welcome; see [CONTRIBUTING.md](CONTRIBUTING.md). Avoid uploading credentials,
local databases, customer data or signing keys in issues, commits or artifacts.

Rhyven 0.4.0-rc.9 supports native Python and JavaScript apps without Docker.
See [Python and JavaScript app deployment](docs/deploy-script.md) for its host-access
permission model and the native repository documentation tool.

Shared deployments: [hosting for groups](docs/shared-hosting.md). Knowledge transfer: [preview and merge](docs/knowledge-merge.md). Use `rhyven upgrade` for runtime updates; `r` in the TUI refreshes the marketplace catalog.

### External-service connectors (0.5+)

Rhyven 0.5 can generate apps from selected MCP tools or OpenAPI operations without installing upstream services. See [connector apps](docs/connector-apps.md) and the [token benchmark](docs/connector-benchmark.md).

## Optional starter for users without a harness

Existing harnesses extend their capabilities through Rhyven's three MCP tools;
they do not need an additional runner. For users with only a model API, the
[Starter Runner](apps/starter-runner/README.md) offers bounded planning, tasks,
knowledge and human questions. It requires Docker and your own model endpoint.
[User Questions](apps/user-questions/README.md) is a separate declarative app
usable by either path. General apps remain independent of the starter.
