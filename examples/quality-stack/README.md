# Saved quality workflow

This engine-managed app workflow is distinct from a portable source stack.
See ../pallet-text for independent code libraries.

This fixture composes Preflight Checker, Failure-to-Regression and Workflow
Evaluator. Their Apache-2.0 source snapshots are under dependencies/.

The stack records a failing JSON example, saves a two-case suite, checks it
with an exists-only policy and a JSON-validity policy, compares results, then
marks the failure fixed only when the comparison improves without regressions.
All results stay in the selected collection. Another agent can inspect them.

Use a temporary collection. Review the two Python apps' host.execute permission
before installing them through the usual consent flow. Then:

```sh
rhyven --collection quality-demo app compose --definition examples/quality-stack/draft.json --out /tmp/quality-workflow.json
rhyven --collection quality-demo app test /tmp/quality-workflow.json --allow-host
```

Tests copy dependency packages into a fresh isolated collection and execute
their code. They do not test against or modify your live app records.
After reviewed installation of the resulting workflow app, call action_verify in
example/quality-stack with a stable request_id.

The automated acceptance test installs reviewed fixture copies only in temporary
test homes:

```sh
cargo test -p agent-market-core --test quality_stack
```

It proves composition and saved evidence, not autonomous model improvement.
No publication is part of this example.
