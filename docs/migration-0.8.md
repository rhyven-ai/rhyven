# Upgrade to 0.8

Use a normal forward upgrade. Back up collections before updating. Do not delete
saved libraries or downgrade an active collection to recover them.

0.8 removes `pallet`, `app bundle`, `app frame`, `--project`, pallet marketplace
actions and the TUI Pallets view. Pending pallet downloads cannot be applied.
Saved source, local evidence, old skills and historical releases are not deleted.
The old registry sidecar is retained for older clients; 0.8 does not fetch it.

If you need to export an old library, use the historical 0.7 binary against a
separate copy of that library/home, not the current live collection. Ordinary
source files can also be copied and used with their language's normal tools.

Complete apps containing vendored library source still validate and run without
a pallet store. The `libraries` package field is retained for those packages.
New apps should ship their complete implementation through ordinary package
files. Do not package every small helper function as a marketplace app.

App workflows still use `operation: "workflow"`; `stack` remains an accepted
legacy spelling. Pinned dependencies, partial-failure reports, script actions,
native binaries, containers, collections, approvals and backups remain available.

Restart agent MCP processes after upgrading and point them at the current
installed usage skill. Existing versioned skills are left intact.
