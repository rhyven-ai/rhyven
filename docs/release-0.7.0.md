# Rhyven 0.7.0

## Pallet test evidence

Explicit `pallet test` runs now store their latest result locally against the
complete package SHA-256, including source and examples. Reports include the
interpreter/version, platform, timestamp, example counts and per-export coverage.
Saving or downloading still executes nothing. Failures supersede prior passes;
changed code has no inherited evidence. Tests cover declared examples only and
are not a security guarantee or publisher certification.

CLI and MCP pallet descriptions expose `tests`, including when the contract hash
is unchanged. Discovery and cached inspections refresh local evidence. New
lexical searches use passing evidence to break relevance ties after compatibility
filters; existing bounded shortlists retain their ordering.

## App workflow naming

Use `operation: "workflow"` for engine-managed sequences of installed app calls.
`operation: "stack"` is a deprecated alias with identical validation, permissions,
execution and receipts. Existing installed packages remain unchanged. `app compose`
writes the preferred spelling to new drafts. `action_workflow_report` is the
preferred receipt lookup; `action_stack_report` remains available.

Portable stacks remain ordinary composed source functions in pallets.

## Validation

All 113 workspace tests pass, including hash changes, coverage, failure replacement,
local-home isolation, ranking ties, cached evidence, workflow alias execution and
new-draft normalization. Clippy passes with warnings denied. A CLI smoke test
verified persisted evidence through the universal interface with an unchanged hash.
