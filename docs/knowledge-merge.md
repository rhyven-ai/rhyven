# Merge knowledge between collections or hosts

Available in Rhyven 0.4.0-rc.10. Both endpoints need rc.10 or newer.

Project Knowledge exposes three additional functions through the existing
`rhyven_call(category, function, args)` interface:

- `object_note_export`: export selected notes and all their referenced ancestors.
- `object_note_merge_preview`: validate a bundle and report additions, previously
  imported records, source-version conflicts and correction branches.
- `object_note_merge_apply`: apply the exact preview atomically.

These are generic runtime functions for local declarative immutable objects with
same-object relationships, no protected fields and no transition rules. App
packages and the three MCP tools are unchanged. Mutable records, cross-app
relationships and executable-app file storage are not merge targets.

## Agent workflow

Connect the source and destination as separate MCP connections. Local collections
and authenticated shared Rhyven hosts use the same functions. Ask the agent to
export selected knowledge, show the preview, then apply the reviewed merge.

On the source connection:

```json
{
  "category": "rhyven/project-knowledge",
  "function": "object_note_export",
  "args": {"where": {"labels": {"has": "operations"}}, "limit": 100}
}
```

The response includes `bundle`, `selected`, `total`, `offset` and `limit`.
Export supports the ordinary query filters, keyword search, sorting and paging,
but not field projection. Related ancestors are included even if they do not
match the selection. `current_only: true` selects current entries while retaining
ancestors needed to reconstruct their history.

On the destination connection, pass the returned bundle unchanged:

```json
{
  "category": "rhyven/project-knowledge",
  "function": "object_note_merge_preview",
  "args": {"bundle": "REPLACE WITH THE BUNDLE OBJECT"}
}
```

Preview returns `new_records`, `existing_records`, `conflicts`, `id_mapping` and
`preview_token`. The example placeholder must be replaced with an object, not a
string. Apply uses the same bundle and token:

```json
{
  "category": "rhyven/project-knowledge",
  "function": "object_note_merge_apply",
  "args": {
    "bundle": "REPLACE WITH THE BUNDLE OBJECT",
    "preview_token": "TOKEN FROM PREVIEW"
  }
}
```

If conflicts are present, review them and explicitly set `allow_conflicts: true`
to retain both versions. This never overwrites a destination note. Resolve
knowledge disagreements by writing a new correction after reviewing both entries.
The runtime checks the token and write permission; the token is not a separate
human-consent mechanism. Agents should request user approval before applying.

CLI calls accept `@file` for JSON arguments, allowing bundles to be saved and
moved between disconnected hosts:

```sh
rhyven --collection source call rhyven_call @export-request.json
rhyven --collection destination call rhyven_call @preview-request.json
rhyven --collection destination call rhyven_call @apply-request.json
```

## Identity, consistency and recovery

Original record identity and metadata are retained in `merge_provenance`.
Destination record IDs are remapped, and relationship fields point to the mapped
records. Destination timestamps and actor identify the import; provenance retains
original timestamps, revision and author label. Re-exporting and importing back
into the source does not duplicate unchanged records.

Deduplication follows source identity plus relationship-aware content. Identical
text authored independently remains separate knowledge. Different content under
the same source identity is a conflict. Independent corrections of the same note
are reported as branches, and both can remain current.

The preview token binds the actor, destination collection, package, destination
records and bundle. Changed destination state requires a fresh preview. Apply
inserts all new records and its audit event in one SQLite transaction. A repeated
old apply fails stale; previewing again reports already imported records. Normal
collection backup/restore includes these records and their provenance.

## Limits

- Same app ID, object name and exact object contract on both endpoints.
- At most 1,000 records including ancestors and 512 KiB per bundle. Use smaller
  filtered pages for larger knowledge bases. A relationship chain must fit in
  one bundle; cycles and paths of 256 records or more are rejected.
- No automatic synchronization, deletion propagation or semantic text merging.
- Sources are read only during export; destination write permission is required
  for apply. No credentials or source host connections are stored in bundles.
- Source identity, paths and author labels are descriptive claims, not signed
  attestations. Review bundles from untrusted sources. Exported provenance may
  contain the source workspace path.
