# App workflows and discovery

An app workflow calls actions on installed apps in a declared sequence. It can
pass results between steps without asking the agent to repeat each call.
Dependencies are pinned by app ID, version and package hash. Permissions must
cover the declared operations; workflow composition does not grant new access.

Use `operation: "workflow"`. The older `stack` spelling remains a compatibility
alias. Workflows run in Rhyven and are separate from ordinary source-code helpers.
Use `rhyven app compose DEFINITION --out PACKAGE` to resolve installed dependency
versions and validate a draft. Review and install the resulting complete app
through the normal consent flow.

Execution records completed steps and partial failures. Read the report before
retrying: another app may have completed a side effect before a timeout. Do not
claim a distributed transaction or retry uncertain writes blindly.

For discovery, `rhyven/marketplace` exposes `action_match_plan`. Supply a task,
revision, missing steps, permission ceiling and permitted execution backends.
Each search returns a bounded shortlist; `action_inspect_candidate` retrieves a
selected contract. Reuse contracts rather than repeatedly searching. One retry,
12 candidates and eight distinct inspections keep discovery bounded. Optional
rankers do not grant permissions or prove compatibility.

Python, JavaScript and native executable apps can include ordinary libraries in
their complete packages. Pallet library tooling is retired in 0.8; see the
[upgrade notes](migration-0.8.md). Historical contracts remain in tagged releases.
