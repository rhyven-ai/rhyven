# Collection recovery and staged updates

Select a shell default and inspect the target:

```sh
rhyven collection list
rhyven collection use my-project
rhyven collection current
rhyven --collection another-project list
```

An explicit `--collection` wins over the saved CLI default. `rhyven config codex`
and the other configuration generators pin the selected collection. Changing
`collection use` does not reroute an already configured agent. A REST client
uses the server's collection, regardless of its local CLI default.

## Backup and restore

Persistent services are fenced and stopped before a backup/update copies state.
Previously enabled instances resume afterward; forced kill/OOM on a requested
clean stop fails maintenance. Restored services stay stopped until explicitly
started. Supervisor process state, OS startup units and credentials are not
backed up. After interrupted maintenance, inspect its outcome and use explicit
`service start` to clear a remaining suspension. See [service recovery](service-contract.md#recovery-and-guarantees).

```sh
rhyven backup my-project --out project.rhyven
rhyven restore project.rhyven --collection recovered-project
# For a reviewed backup containing container apps:
rhyven restore project.rhyven --collection recovered-project --accept-permissions
```

Restore requires a new collection name and refuses to overwrite existing state.
To roll back an update later, restore the `recovery_backup` returned by update
into a new collection, inspect it, and explicitly reconfigure agents to that
collection. This preserves both versions for comparison.

Archive format 3 includes a consistent SQLite snapshot (records, installed and
removed app versions, audit, retry receipts, approval history), full immutable
package definitions, and regular files in container `/data` directories. Image
references are in the packages; Docker images and runtime environment variables
are not copied. Container restore checks/pulls pinned images after explicit
permission acceptance, without starting app code. Registry caches and CLI default
selection are machine configuration and are not restored.

Restore verifies paths, file lengths, SHA-256 hashes, SQLite integrity, and package
contracts before activation. Pending/approved unconsumed marketplace requests are
invalidated. Completed approvals and receipts remain historical records. Hashes
provide corruption detection, not publisher authentication: restore only trusted
backups. Archives contain private app data, including any secrets an app itself
saved in `/data`. Store them with the same care as the original state.

New backups use a streaming binary format: length-prefixed JSON metadata, raw
file bytes, per-file SHA-256 checksums, and a final checksum covering metadata
and contents. File data is copied in 64 KiB chunks, and restore's path/mode index
lives in temporary SQLite storage. There is no fixed total-content, archive-size,
or entry-count cap. Individual metadata frames are limited to 64 KiB to reject
malformed headers without unbounded allocation; this does not limit file sizes.

Backup and staged updates need sufficient disk space for the consistent SQLite
snapshot, archive, and staged state. Disk/IO failures abort before activation;
failed backups do not publish a partial archive. Put backup output outside the
container data tree so it cannot include itself. Archives are uncompressed, so
on-disk size is approximately the data size plus metadata, without JSON byte-array
expansion.

File contents, empty directories and Unix permission bits are preserved.
Symlinks, special files and non-UTF-8 paths are rejected. Ownership, setuid/setgid
bits, timestamps and hard-link identity are not preserved. Container data must
be readable by the runtime user. Existing JSON format 1 and 2 backups remain
restorable through a compatibility reader retaining their original safety bounds;
all new backups and update recovery archives use streaming format 3. Older Rhyven
binaries cannot read format 3.

Runtime operations use one cross-process maintenance lock per collection.
Backups wait for in-flight actions; mutations wait for backup/update completion.
Do not edit SQLite or container files through tools outside Rhyven during these
operations. Calls in different collections remain independent.

## Updates

In the terminal marketplace, select an installed app and press `u` (**Update app**).
Review the installed and available versions, added/removed permissions and hosting,
then confirm with `y` or cancel with `n`/`Esc`. `?` explains the operation. This
updates the selected app in the current collection, not the Rhyven executable or
copies installed in other collections.

Press `r` in the TUI to reload local state and fetch marketplace listings in the
background. There is no automatic refresh timer. Refreshing never installs an
app or applies an update. Approval screens remain fixed, and confirmation rejects an app
whose installed contract changed while the review was open.

Updates preserve the live version until a staged copy has migrated and passed
validation. A recovery archive is retained under the collection's `recovery/`
directory. Activation records a durable journal; an uncommitted switch is rolled
back at the next runtime operation. Permission acceptance is required again.
Backups are not automatically deleted.

Compatible additive updates still work without a migration. Unsupported changes
fail closed. Changing execution driver, hosting, removing objects or changing
relationships or removing existing protection/transition edges is unsupported.
In source candidate `0.4.0-rc.5`, an explicit migration can extend a transition
graph and add protected/transition fields that did not exist in the old schema.
Every retained record is still revalidated in staging.

Declarative migrations are version-specific maps. For example, adding a required
`source` field to an object's schema:

```json
"migrations": [{
  "protocol": 1,
  "from": "0.1.0",
  "steps": [{
    "object": "task",
    "defaults": {"source": "imported"}
  }]
}]
```

A step can also contain `"rename": {"old_field": "new_field"}`. Renames reject
existing destination fields. Defaults only fill missing values. Transformed
records must pass the new schema; revisions advance so old writers cannot use
stale revisions. Migrations are direct from the declared source version, not an
automatically composed chain. Arbitrary scripts are not declarative migrations.

Container migrations name a declared action:

```json
"migrations": [{"protocol": 1, "from": "0.1.0", "action": "migrate"}],
"health_action": "health"
```

`migrate` receives `{"from_version":"0.1.0"}`. `health` receives `{}` and must
return a schema-valid result containing `"ok": true`. Both use the normal
container request/response contract, against staged `/data`, with network and
secrets disabled. A failure discards the staged change; the old live package and
state remain usable. Health checks are optional unless declared. External side
effects from earlier normal app calls cannot be undone by restoring local state.

## Streaming regression checks

`cargo test --test recovery` covers a collection larger than the former 64 MiB
limit with more than 10,000 entries, including staged update preservation,
legacy restore, corruption/truncation and path rejection. On Linux,
`python3 qa/streaming_backup_check.py target/release/rhyven` verifies a 128 MiB
backup and restore while limiting each runtime process to 96 MiB of address space.
