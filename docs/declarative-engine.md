# Declarative engine: features, expressions and typed queries

The current source build adds calculations, Boolean conditions, runtime values,
string operations and richer queries to local declarative apps. No Docker,
app-supplied executable, extra MCP tool or app-specific runtime code is required.

## Feature overview

The website's **Docs → Declarative features** page (`#docs/declarative-features`)
provides the complete user-facing reference, including copyable examples and
the current limits. The engine executes these operations; app guidance does not
delegate the calculations or validation to the calling agent.

| Capability | Provided behavior |
| --- | --- |
| Typed records | Closed object schemas; object, array, string, integer, number and boolean fields; defaults, enums, required fields, bounds and calendar dates. |
| Record operations | Create, get, query and revision-checked top-level patches. Nested objects and arrays are whole-field replacements. |
| Named actions | Create/get/query/update with validated inputs, literal/argument mappings, configurable ID arguments, update guards, conditions and expressions. |
| Rules | Protected fields with defaults, string-enum lifecycle graphs and immutable append-only objects. |
| Relationships | String record IDs validated within an app or against another installed local app in the same collection. |
| Calculations | Arithmetic, comparisons, Boolean logic, array membership, string normalization/concatenation and runtime actor/time references. |
| Queries | Equality filters, typed comparisons, substring and array membership tests, keyword search, timestamp filters, sorting and pagination. |
| Knowledge history | Immutable correction records with a self-reference and optional current-only queries. |
| Durable state | SQLite record envelopes, generated IDs, revisions, timestamps and actor labels. Single-record mutations, audit events and retry receipts commit together. |
| Shared agent access | Collection-scoped state, the three-tool MCP interface, CLI calls, authenticated REST and generated standalone MCP adapters. |
| Permissions | Required state.read and optional state.write; no declarative host filesystem, process, secret or outbound network access. |
| Developer tools | Scaffolding, schema validation, isolated behavior tests, packaging, local publishing and GitHub registry distribution. |
| Lifecycle and recovery | Compatible version updates, explicit field-renaming/default migrations, staged activation, rollback, collection backup/restore and retained state after app removal. |

For record/schema details see [package format](package-format.md); for app
creation and tests see [deployment](deploy-declarative.md); for state preservation
see [recovery and updates](recovery-and-updates.md). The sections below detail
the expression and query contracts. Version notes record when extensions were
introduced, rather than the current public release status.

Package format remains **2**. New optional action fields are `expressions` and
`condition`. The existing `operation`, `object`, `input`, `set`, `guard` and
`id_arg` retain their meanings. These fields are supported from `0.4.0-rc.4`.
Older versions may reject them. See [release status](release-status.md) for the
current source and distributed runtime versions.

## Action contract

Actions still perform one `create`, `get`, `query` or `update` operation on one
object type. `expressions` and `condition` are supported on local `create` and
`update` actions only. A field must not appear in both `set` and `expressions`.

- `set`: existing literal/`{"$arg":"name"}` templates.
- `expressions`: map target field names to typed expression trees.
- `condition`: a Boolean expression that must evaluate to true.
- `guard`: existing update-only equality checks against the previous record.
  If both guard and condition exist, both must pass.

Here is an update action for an object with integer `quantity`, integer
`changed_at`, and string `changed_by` fields:

```json
{
  "description": "Withdraw stock only when sufficient quantity remains",
  "operation": "update",
  "object": "item",
  "input": {
    "type": "object",
    "properties": {
      "id": {"type": "string"},
      "expected_revision": {"type": "integer", "minimum": 1},
      "amount": {"type": "integer", "minimum": 1}
    },
    "required": ["id", "expected_revision", "amount"],
    "additionalProperties": false
  },
  "condition": {
    "op": "ge",
    "args": [{"field": "quantity"}, {"arg": "amount"}]
  },
  "expressions": {
    "quantity": {
      "op": "sub",
      "args": [{"field": "quantity"}, {"arg": "amount"}]
    },
    "changed_at": {"runtime": "now"},
    "changed_by": {"runtime": "actor"}
  }
}
```

Declare `quantity` as a protected field with a default if ordinary record updates
must not bypass the withdrawal action. The resulting record is still subject to
its schema, relationships, transitions, permissions and immutability rules.

## Expression syntax

Every expression is an object with exactly one reference/literal key, or with
`op` and `args`. Argument and field references address top-level declared fields.

| Expression | Meaning |
| --- | --- |
| `{"literal": 10}` | JSON literal, including arrays or objects |
| `{"arg": "amount"}` | Validated action input, including input defaults |
| `{"field": "quantity"}` | Previous record's data; update actions only |
| `{"runtime": "now"}` | Runtime Unix timestamp in seconds |
| `{"runtime": "actor"}` | Runtime actor identity, not an action argument |
| `{"op":"add","args":[EXPR,EXPR]}` | Typed operation |

| Operators | Arity | Behavior |
| --- | --- | --- |
| `add`, `sub`, `mul` | 2 | Integer or floating-point arithmetic |
| `div` | 2 | Floating-point division |
| `mod` | 2 | Signed integer remainder |
| `eq`, `ne` | 2 | Equality/inequality; numeric values compare numerically |
| `lt`, `le`, `gt`, `ge` | 2 | Numeric or case-sensitive lexical string comparison |
| `and`, `or` | 2–16 | Boolean operands, evaluated left-to-right with short-circuiting |
| `not` | 1 | Boolean negation |
| `in` | 2 | Test whether the first value equals an element of the second array |
| `concat` | 2–16 | Concatenate strings, with no implicit conversions |
| `trim`, `lower`, `upper` | 1 | Unicode string trimming/case conversion |

The validator checks operator names, arity, reference existence, operand types,
assignment result types, and Boolean condition results. Final values still pass
normal record validation. Referencing an absent optional value fails; it does
not silently become zero, false or null.

Integer arithmetic uses checked signed 64-bit operations. Overflow, division by
zero and non-finite floating-point results fail the action. `div` returns a
`number`, so it cannot be assigned to an `integer` field. Mixed integer/float
operations reject integer operands outside ±(2^53−1) to avoid silent conversion
loss. Integer-to-integer comparisons and integer schema bounds remain exact,
including integers above 2^53. Floating-point values otherwise use IEEE-754 f64;
there is no implicit decimal arithmetic.

Each expression is limited to 12 nesting levels and 16 KiB of serialized source.
Operator string results are limited to 100 KB; destination schema limits also
apply. The interpreter has no loops, scripts, filesystem/network access or
process execution.

## Transaction and retry semantics

Update expressions run after the revision check, inside the same SQLite write
transaction as the mutation. Every expression sees the **same previous record**;
assignments do not read one another's new values. All runtime timestamp references
within one action use the same timestamp.

Condition, expression, or final validation failure leaves the record, revision,
audit events and receipt unchanged. Existing request-ID retries return the saved
result before reevaluating expressions. Concurrent updates still require the
current `expected_revision`; a conflicting action does not silently recalculate.

## Rich object queries

The generic object query function accepts optional `where` and `order_by` alongside
existing `filters`, `limit` and `offset`. They are exposed with field-specific
schemas in `rhyven_describe`, standalone MCP exports, and the same REST service.
No new universal MCP tool is introduced.

```json
{
  "category": "acme/inventory",
  "function": "object_item_query",
  "args": {
    "filters": {"status": "active"},
    "where": {
      "quantity": {"ge": 1, "lt": 10},
      "location": {"in": ["workshop", "warehouse"]},
      "label": {"contains": "bolt"}
    },
    "order_by": [
      {"field": "quantity", "direction": "asc"},
      {"field": "label", "direction": "asc"}
    ],
    "limit": 20,
    "offset": 0
  }
}
```

- All filters, fields and conditions are combined with **AND**.
- `eq`, `ne`, and `in` support declared field types; `in` allows at most 100 values.
- `lt`, `le`, `gt`, `ge` support numeric and string fields.
- `contains` is a case-sensitive literal substring match on strings, not regex.
- Values are validated against the field schema. Unknown fields/operators and
  incorrect operand types fail before scanning records.
- A missing field fails every `where` condition, including `ne`.
- Up to three scalar sort fields are allowed. Direction defaults to `asc`;
  missing fields sort last in either direction. Ties use record ID ascending.
- Sorting occurs before pagination; `total` counts all matching records.
- Existing equality-only `filters` behavior and unsorted ID order are unchanged.
- Named declarative query actions still use their existing `set` equality-filter
  mapping. Rich query arguments are available through the generic object query.
- Marketplace discovery retains its specialized listing filters. Remote-provider
  forwarding does not support these extensions yet. Local REST service access does.

Queries currently filter/sort runtime records in memory. These additions do not
introduce SQL aggregation, joins, full-text indexing or a query optimizer.

## Not included in this increment

Multi-record/multi-step actions, aggregation, unique constraints, native executable
execution, and a package-validator action are not implemented by this change.

## Verification

`crates/core/tests/declarative_expressions.rs` checks transaction/retry behavior,
concurrent withdrawals, type rejection, arithmetic failures, expression limits,
large-integer correctness, string normalization and typed query pagination.

`python3 qa/declarative_engine_check.py target/release/rhyven` exercises installation
and the new action/query behavior through actual stdio MCP and authenticated REST.

## Search, dates and current knowledge (source candidate 0.4.0-rc.5)

These extensions require runtime/validator `0.4.0-rc.5` or newer. Advance release
and validator pins together before publishing apps that depend on newer features.

An object may declare `search_fields: ["title", "body", "labels"]`. Each entry must
name a string or string-array field. Its query function then accepts `search`:
1–32 whitespace-separated literal keywords, up to 1,000 characters. Keywords are
lowercased and ANDed across all declared fields. They do not need to occur in the
same field. There is no regex, query language, stemming, relevance rank or vector
search. This uses the existing scoped record scan, not a full-text index.

An immutable object may declare `supersession_field: "supersedes"`, where the field
is a local relationship to that same object. Queries then accept
`current_only: true`. Every record referenced by a successor is excluded **before**
other filters and pagination, even if its successor does not match the search.
Correction branches can yield multiple current records. Omit the option to query
history, or fetch any original record by ID.

Every local object query accepts `metadata` comparisons on `created_at` and
`updated_at` (engine-owned Unix seconds): `eq`, `lt`, `le`, `gt`, `ge`. `order_by`
accepts `$created_at` and `$updated_at` as well as ordinary data fields. String-array
fields support `where: {"labels": {"has": "release"}}` for exact membership.

```json
{
  "category": "rhyven/project-knowledge",
  "function": "object_note_query",
  "args": {
    "search": "release recovery",
    "current_only": true,
    "where": {"labels": {"has": "operations"}},
    "order_by": [{"field": "$updated_at", "direction": "desc"}]
  }
}
```

String schemas may declare `format: "date"`. This validates actual Gregorian
calendar dates in `YYYY-MM-DD` form, including leap years (years 0001–9999).
No other `format` values are supported. Calendar dates sort chronologically as
strings. Optional due/document/review dates have no default; absent values do not
match range filters. These are annotations, not automatic reminders.

```json
{
  "category": "rhyven/work-management",
  "function": "object_issue_query",
  "args": {
    "search": "login",
    "where": {
      "status": {"in": ["open", "blocked"]},
      "due_date": {"le": "2026-10-01"}
    },
    "order_by": [{"field": "due_date"}]
  }
}
```

The three MCP tools remain unchanged. REST and standalone exported MCP functions
use the same query schemas, validation and runtime implementation.
