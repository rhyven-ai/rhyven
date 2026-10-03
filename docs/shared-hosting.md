# Hosting Rhyven for larger groups

Use one hosted collection when several agents need the same knowledge, work
items or inboxes. Clients connect to the host's authenticated REST service through
local Rhyven MCP adapters. The host owns SQLite and app files; clients do not
synchronize database copies. Use runtime 0.4.0-rc.10 or newer for the features below.

## Create the shared collection

On a Linux server controlled by your organization, use a dedicated OS account
and a persistent local disk. Install Rhyven, then:

```sh
rhyven --home /srv/rhyven --collection team registry-sync rhyven-ai/registry --anonymous
rhyven --home /srv/rhyven --collection team inspect rhyven/project-knowledge
rhyven --home /srv/rhyven --collection team install rhyven/project-knowledge --accept-permissions
```

Review the manifest and permissions before accepting. The account running Rhyven
must own `/srv/rhyven`. Install any additional apps into the same collection to
share relationships. Containers need a compatible local Docker engine; declarative
knowledge and work apps do not need Docker.

## Expose one authenticated endpoint

Generate a random bearer token of 16–512 characters and store it in an OS-managed
secret or a file readable only by the service account. Set `RHYVEN_SERVE_TOKEN` in
the server's environment, then run:

```sh
rhyven --home /srv/rhyven --collection team serve --host 127.0.0.1 --port 7421
```

Put an HTTPS reverse proxy in front of this loopback endpoint. Keep port 7421
private; clients use the HTTPS address. A proxy on a different host needs an
appropriately protected private connection. Non-loopback binding requires the
explicit `--allow-insecure-network` flag because Rhyven itself serves plain HTTP.
Do not expose that connection directly to the public internet.

Run `serve` under your OS service manager with the same account, home, collection
and token. The `rhyven daemon unit` command supervises persistent app containers;
it does **not** create a service for the REST listener. If you use both, supervise
both processes. Configure startup order, restart behavior and log retention for
your deployment.

## Connect each agent

Install Rhyven on each client. Supply the shared token through the client process's
`RHYVEN_SERVE_TOKEN` environment variable, then:

```sh
rhyven connect --client codex --server https://knowledge.example.com --expect-collection team
```

Use the appropriate adapter for Claude Code, Cursor, VS Code, Cline or another
MCP client. Graphical clients need their environment configured separately. Reload
the client and call `rhyven_categories()` to confirm the collection. All clients
use the same three tools; installing another app does not add another MCP server.

The remote connection owns its collection selection. A local `--collection` flag
does not reroute a remote server. Use a separate endpoint/connection for another
team's collection. Project collections do not inherit global apps.

## Access boundaries

The bearer token admits a trusted group to one server. Rhyven currently has no
individual user accounts, document ACLs, SSO, role hierarchy or per-agent quotas.
Actor labels are audit annotations, not authenticated identities. Collections
separate state, but are not OS security boundaries.

Keep unrelated or mutually untrusted groups in separate deployments with different
OS accounts, homes and credentials. Do not offer one shared token as enterprise
multi-tenant authorization. Native script apps run with the server account's OS
permissions, so review them carefully before installing them on a shared host.

Remote package installation/removal and service management require a separate
`RHYVEN_MANAGEMENT_TOKEN`. Keep it off ordinary clients and provide it only to
administrators. Human installation consent is still required. Catalog refresh and
host-requirement checks do not install apps or grant execution permissions.

## Capacity and operations

Start with a small pilot and measure your actual workload; there is no validated
agent-count capacity claim. SQLite is on the server, and writes and maintenance
are coordinated per collection. Queries currently scan and sort scoped records in
memory. Use selective filters and pages; `select` reduces response size but does
not create indexes. The maximum ordinary query page is 1,000 records.

Shared request bodies are limited to 1 MiB. Each persistent service handles one
foreground action at a time, with a bounded queue. Default aggregate service
admission budgets are eight services, 4 GiB of declared memory and four CPUs.
Container limits are enforced separately from REST-process memory. Do not assume
adding clients adds independent database writers or service capacity.

Monitor disk space, request latency, error rates, Docker health, service queues and
backup success. Use `rhyven doctor`, `rhyven service status` and your service manager's
logs. An unavailable Docker engine does not prevent declarative apps from working.
Split independent workloads into separate collections/deployments when contention
rises. There is no built-in load balancer, database replication or automatic failover.
Do not share one live SQLite file between hosts over a network filesystem.

## Backups, upgrades and knowledge transfer

```sh
rhyven --home /srv/rhyven backup team --out team.rhyven
rhyven --home /srv/rhyven --collection restore-test restore team.rhyven --accept-permissions
rhyven upgrade --check
rhyven upgrade
```

Test restoration into a new collection before relying on backups. Backup includes
SQLite, app metadata and managed app files. Secrets, container images and running
processes are not included. Store backups on a separate protected system under your
organization's retention policy. Restored services start disabled.

Schedule runtime upgrades and restart REST servers, supervisors and MCP client
processes afterward. `rhyven upgrade` updates the executable at its own installation
path; it does not upgrade app packages, migrate every collection or restart active
services automatically. Test app updates and their migrations separately.

Use [knowledge merge](knowledge-merge.md) to combine selected notes from another
collection or host. Export, preview, review conflicts and apply. This is explicit
one-way transfer, not continuous replication. Both endpoints must support the
same object contract. Keep a backup before major imports or app migrations.
