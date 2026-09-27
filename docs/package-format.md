# Rhyven package format 2

`app.json` (or `*.rhyven.json`) defines a complete app contract. By default it is a
declarative app containing executable rules/actions, guidance and contract cases.
The optional `execution` field also supports container apps: a digest-pinned image
supplies custom code, with action input/output schemas declared in the manifest.
See [container contract](container-contract.md). The sections below describe the
declarative driver unless stated otherwise.

Required top-level fields: `format: 2`, `name: "publisher/app"`, numeric `version: "major.minor.patch"`, `publisher`, `description`, `hosting`, `permissions`, `objects`, `actions`, `guide`, `tests`. Optional `source` is a repository reference; optional `execution` selects the driver. Unknown fields are rejected. Publisher and source are metadata, not verified identity.

## Objects and relationships

Each named object has `schema`, optional `immutable`, `relationships`, `protected_fields`, `transitions`, `search_fields`, and `supersession_field`. Names begin with a lowercase letter, contain lowercase letters/digits/underscores/hyphens, and are at most 64 characters. There may be 1–32 objects. Example:

```json
{
  "schema": {
    "type": "object",
    "properties": {
      "title": {"type": "string", "minLength": 1},
      "status": {"type": "string", "enum": ["open", "done"], "default": "open"},
      "project_id": {"type": "string"}
    },
    "required": ["title"],
    "additionalProperties": false
  },
  "protected_fields": ["status"],
  "transitions": {"status": {"open": ["done"], "done": []}},
  "relationships": {"project_id": {"object": "project"}}
}
```

Supported schema types: object, array, string, integer, number, boolean. Supported keywords: properties, required, additionalProperties=false, items, enum, default, minLength, maxLength, minimum, maximum, maxItems, description, format=date. `$ref`, unions, regex patterns and other JSON Schema features are not supported. Nesting is capped at eight levels, packages and requests at 1 MiB, arrays at 1,000 by default and strings at 100,000 characters by default.

Defaults are materialized on creation. Patches only replace explicitly supplied top-level fields; nested objects are whole-field replacements, not JSON Merge Patch. Unknown fields and wrong types fail. Protected fields need defaults and cannot change via ordinary create/update. Immutable objects reject every update. Transitions restrict changes from each previous state; missing outgoing edges allow no state change.

Relationship values are string IDs; omitted or empty optional values are unlinked. Targets name an object and optionally another namespaced app. Local writes require the target app installed locally and the target record present in exactly that app/object. Fetch it with the universal `get` operation. No cascade deletion, automatic joins or remote referential-integrity guarantee is provided.

## Actions

An action has `description`, `input` (object schema), `object`, `operation`, optional `set`, `guard`, and `id_arg`. Operations: create, update, get, query. Example update:

```json
{
  "description": "Complete an open item",
  "input": {
    "type": "object",
    "properties": {
      "id": {"type": "string"},
      "expected_revision": {"type": "integer", "minimum": 1}
    },
    "required": ["id", "expected_revision"],
    "additionalProperties": false
  },
  "object": "item",
  "operation": "update",
  "guard": {"status": "open"},
  "set": {"status": "done"}
}
```

`{"$arg":"owner"}` in set/guard substitutes a validated action argument; other values are literals. `id_arg` defaults to `id`. Update actions require expected_revision and can change protected fields, but cannot bypass immutability, transitions, schema validation or relationships. Guards are equality preconditions on the previous record and only apply to update. `set` supplies create data, update patch or query equality filters. Get actions return the complete target record. Each mutation affects one record atomically; typed expressions and Boolean conditions are supported through optional `expressions` and `condition` fields; arbitrary scripts, process execution and multi-record workflow actions are not provided. See [declarative engine](declarative-engine.md).

## Contract cases and developer checks

Container packages validate without execution. Run their behavior cases explicitly
with `rhyven app test PACKAGE --allow-container`; this can pull images and execute
publisher code. `app package` only runs the automatic cases for local declarative
apps. Registry validation never automatically runs container tests.

`tests` is an ordered array of operations and assertions in an isolated workspace. Each case contains `operation`, `args`, and optional `expect` (dot-path to expected value) or `error` (expected code). `{"$result":"0.id"}` references an earlier successful result. The current app name is supplied automatically. See the five packages in `catalog/` for runnable examples.

`rhyven app validate` checks structure and supported capabilities. `rhyven app test` executes declared local cases. `rhyven app package` validates and runs local cases before creating an immutable-output bundle. `rhyven app publish` validates and adds a version to the local registry; it does not assert independent testing or certification. Remote test cases are not automatically sent to external services; the remote conformance service is not implemented.

## Versions and trust

Publishing cannot replace an existing name/version with different bytes after canonical JSON serialization. Installation stores the canonical SHA-256 digest. Upgrades require a strictly newer numeric version and permission approval. Without a migration, only additions of objects/actions and optional fields without defaults are supported; existing fields/rules and hosting must remain compatible. Explicit staged declarative migrations can rename fields and materialize defaults, with every retained record revalidated. They may extend transition graphs while preserving existing edges, and add protected/transition fields only when those fields are new. Existing immutability, relationships and protected fields cannot be relaxed. Changed action definitions in a newer version are allowed and should have regression tests. Incompatible changes fail with `migration_required`; see [recovery and updates](recovery-and-updates.md) for staging, rollback and retained backups.

Community means schema-valid with declared metadata/permissions. Verified is intended to require publisher identity and signatures. Rhyven Certified is intended to require independent conformance, security, permissions, data and migration review. Current registry entries use Unverified, with Community accepted as a legacy spelling. Publisher branding is separate from trust. The validator rejects extra manifest fields such as `trust: "Rhyven Certified"`.

## Display name and publisher

Format-2 packages may include `display_name`, a human-readable name of 1–120
characters without control characters. The canonical `name` remains the routing
ID. Registry entries preserve and verify display names against the exact package.
The current Rhyven apps use publisher `rhyven` and `rhyven/...` IDs. Publisher
identity is separate from independent certification: new entries use trust
`Unverified`; the legacy `Community` spelling remains readable. These metadata
additions require runtime/validator `0.4.0-rc.6` or newer.
