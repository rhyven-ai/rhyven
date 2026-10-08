# Composable capabilities and bounded plan matching

Status: 0.6.0 local implementation, 2026-10-08. This document records the
agreed direction; its new contracts, commands and search budgets are not released
features. It extends the app platform rather than replacing it with a harness.
Implementation and local testing were authorized. Public publication is explicitly prohibited.

See [implemented contracts and limits](composable-capabilities.md). The bounded
first implementation covers stacks, pinned dependency permissions, local BM25
matching with cached descriptions, optional classifier wire adapters, native
ELF actions, local authoring, frames and the three-app quality fixture. Broader
semantic matching, live model comparisons, remote metadata-only function indexes,
retention management, ARM64 execution and public deployment remain follow-up
acceptance work. The sections below preserve the target design; proposals beyond
the implementation document are not claims of current behavior.

## Boundary correction: portable code and complete apps

The user clarified that bricks are ordinary reusable source functions, mortar is
adapter code, stacks can be ordinary function compositions, and pallets are
libraries used to build complete apps. They are not small installed apps.

This correction supersedes the earlier mappings below that equate pallets with
app packages or bricks with app actions. Portable pallets now use rhyven.pallet/1,
a separate local store, source export, compact discovery and optional app-source
bundling. Existing app-to-app composition remains an engine-managed app workflow.
See [portable pallets](portable-pallets.md) for implemented contracts and limits.

## 1. Outcome and product boundaries

An existing agent can find, create, test, retain and combine executable
capabilities. Another agent can discover and reuse the result without generating
the implementation again. A user can explicitly request publication of a useful
package. Rhyven continues to supply an app standard, runtime, package manager,
marketplace, collections and one universal agent interface.

Keep `rhyven_categories()`, `rhyven_describe(category)` and
`rhyven_call(category, function, args)`. Expose new discovery/authoring operations
as functions within categories, not as additional top-level MCP tools. MCP and
REST enter the same core service. Do not introduce an app-specific server per
capability or a mandatory planning/model service.

The calling agent owns open-ended reasoning and its task plan. Rhyven matches
capabilities to that plan, validates compositions and executes bounded operations.
The intended improvement is accumulated tested software, not model retraining.
Token, speed and reliability gains must be measured, not assumed.

## 2. Vocabulary and implementation mapping

| Concept | User meaning | Implementation |
| --- | --- | --- |
| Brick | Reusable code | Ordinary source function with typed input/output and tests |
| Mortar | Adapts values between code contracts | Ordinary adapter function; engine workflows separately retain bindings |
| Stack | Reusable code composition | Ordinary function composing bricks; engine app workflows remain separate |
| Pallet | Reusable source library | Separate versioned source package, typed exports and tests |
| Frame | Starting project structure | Template files, configuration and references to pallets/stacks |

Apps remain complete installable software. Pallets export related source functions
and do not own engine-managed app objects or require separate app installation.
Do not require a package or marketplace listing per tiny function. Existing apps
keep their IDs, contracts and behavior; new terminology is optional for ordinary
app consumers. Frames are starting points, not a second lifecycle manager.

## 3. Reuse the released foundation

The 0.5.5 baseline supplies declarative actions, Python/JavaScript scripts,
container actions, persistent services, collection-local state, immutable package
identities, validation, tests and marketplace approvals. The three quality apps
provide the first composition fixture: Preflight Checker, Failure-to-Regression
and Workflow Evaluator. Their current demonstration is coordinated externally;
a stored cross-app stack is new work.

Implement atop the current released source, not an old prototype checkout.
Audit existing contracts before adding fields. These are still required:

- First-class saved cross-app stacks and dependency resolution.
- Compact searchable capability metadata and plan-matching results.
- A native executable backend for precompiled artifacts.
- Local draft/activation tooling and frames.
- Optional matching-provider adapters and measured acceptance criteria.

Do not imply these are already supported in the registry validator or binaries.

## 4. Language-independent execution without managing every toolchain

Use the existing structured action request/result/error contract across backends.
A stack dispatches each action through the core, regardless of implementation
language. JSON values cross process boundaries; do not share language objects or
Python imports across packages. Design bounded, collection-scoped artifact
references for large files as a later extension; use current size limits first.

| Implementation | Backend | Dependency responsibility |
| --- | --- | --- |
| Declarative operations | Existing Rust engine | No extra interpreter |
| Python | Existing managed venv | Compatible host Python; pinned declared dependencies |
| JavaScript | Existing Node backend | Compatible host Node; pinned declared dependencies |
| Rust/Go and other compiled languages | Proposed native executable | Publisher builds matching artifacts; user supplies required system libraries |
| Languages or dependencies needing packaged OS support | Existing container backend | Publisher packages the runtime/dependencies in an image |
| Long-lived workloads | Existing service backend | Existing service lifecycle and permissions |

For native execution, select a declared OS/architecture/ABI artifact, verify its
immutable hash and launch it without a shell. Prefer package-owned binaries over
unversioned PATH lookup. Declare system-tool integrations separately, including
version checks and remedies. Do not run Cargo, install compilers, or modify OS
packages on ordinary app installation. Agent authoring can use an already
available compiler or a separately authorized build environment.

Current script packages embed bounded UTF-8 files; native binary assets need an
explicit reviewed artifact extension, size limits and matching validator support.
Do not smuggle executables through the existing text-file format. Linux x86-64
and ARM64 are the first acceptance targets. Static linking where practical does
not guarantee universal portability; validate libc/ABI and dynamic requirements.
Reject unsupported hosts with actionable diagnostics. Do not silently fall back
to a backend with different permissions or semantics.

Keep existing isolated/shared environment semantics: sharing requires compatible
interpreter identities and identical locked dependencies. Never maintain one
mutable global venv that new pallets can upgrade underneath existing apps.
Environment caches may be reused; collection state remains separate. Native
execution, including Python mortar, is unsandboxed OS-user execution unless a
separate verified sandbox applies. A dependency environment is not a sandbox.

## 5. Stack and mortar contract

A stack declares caller input, dependency aliases, ordered steps, bindings,
optional simple conditions, final output and execution limits. Each dependency
resolves to a package ID/version/hash and a selected function contract hash.
Expose the stack through a normal action schema. Validate the graph and bindings
before activation, then validate concrete values at every step boundary.

Start with sequential execution and stop-on-error. Add bounded iteration over
explicit input arrays for the quality fixture next. Exclude arbitrary while
loops, recursive stacks, unbounded dynamic discovery and hidden model calls.
Nested stacks need a depth cap, an acyclic dependency graph and shared run limits.
Do not acquire a parent write lock and deadlock by calling a child under it;
resolve existing core admission/locking semantics in the implementation design.

Mortar first uses existing expressions for renaming, filtering, defaults and
simple transformations. Use an ordinary tested script/native/container action
for transformations beyond that subset. Keep small mortar local to its pallet;
extract a library only when reuse warrants it. Field type compatibility does not
prove matching units or domain meanings: record semantic hints and test actual
examples. Reject ambiguous/missing required values instead of fabricating them.

Reuse the collection's package version rules. Initially reject incompatible
requirements for two versions of the same app in one collection; do not quietly
add side-by-side instances or change the installed version. An update that changes
a referenced hash marks affected stacks as needing explicit revalidation/rebind.
Never resolve a pinned stack against an unreviewed latest version at execution.

A stack is not a distributed transaction. A successful earlier step may have
changed state when a later step fails. Default to preserving evidence and
stopping, with no automatic retry of side effects. Reuse stable run/step request
identities and existing receipt behavior. An interrupted external call can be
ambiguous: mark it unknown and require reconciliation before resuming. Do not
claim exactly-once execution across external systems.

## 6. State, permissions and activation

Use collection SQLite for run and step records: package/contract hashes, actor,
status, timestamps, bounded input/result references and errors. Apps continue to
own their objects and executable data. Call app contracts, not their databases.
Integrate run state with backup/restore and maintenance admission. Do not write
secrets into logs, matching histories or public test fixtures. Define retention
and bounded result sizes before enabling durable payload capture by default.

Lifecycle: draft -> validate -> test within granted permissions -> review any new
execution scope -> activate immutable local version -> reuse. Drafts can live in
a project; activation uses existing package storage and collection installation.
An edited draft cannot mutate an installed package. New executable versions and
additional permissions go through the applicable installation/update review.

Resolve the complete dependency set before approval. Show a conservative union
of package permissions until finer action-level enforcement exists. Invoking a
child action cannot increase the approved scope or bypass its authorization.
No stack or matching provider can approve its own installation. Prefer existing
MCP elicitation for supported clients; any future harness approval fallback must
have an explicit trust model rather than an unrestricted self-approval command.

## 7. Plan-based discovery contract

The agent supplies a short structured plan, not its entire conversation or hidden
reasoning. Proposed request fields:

- Goal and stable task/plan revision identifier.
- Steps: ID, desired operation, available input shape and required output shape.
- Domain hints: concepts, formats, units, examples and acceptance checks.
- Explicit ordering/dependencies where relevant.
- Collection, available backends, privacy/offline constraints and permission ceiling.
- Discovery budget; default excludes public network refresh and hosted inference
  unless enabled by the operator's configuration.

Missing hard requirements produce one targeted clarification or an explicit
unknown-compatibility result. Do not repeatedly reformulate vague searches.
Keep the plan owned by the calling agent; discovery does not spawn agents or
expand it into a new open-ended planning task.

Proposed response: discovery session ID, catalog revision, compact candidates,
covered step IDs, unmet requirements, schema/contract references, permission and
host readiness, selection reasons, consumed budget, stop reason and recommended
next action (`reuse`, `adapt`, `build`, `clarify`, or `blocked`). Include the
selected source/version and evidence provenance. Names alone are not executable
contracts. These operations remain functions reached through the three tools.

## 8. Retrieval and matching algorithm

1. **Reuse the task's resolved manifest.** Check cached approved choices and their
   package/contract hashes. If still applicable, return them without searching.
2. **Normalize the plan once.** Extract operation terms and structured constraints.
   Batch all missing steps into one discovery session.
3. **Retrieve cheaply.** Search a local capability index using exact IDs/aliases,
   fielded keywords and BM25. Prefer an already suitable saved stack or frame
   before assembling many bricks. Include installed/local packages first; consult
   cached marketplace metadata for uncovered needs. A weak local match must not
   hide a substantially better compatible candidate.
4. **Filter hard constraints.** OS/backend, denied permissions, offline needs,
   input/output requirements, version conflicts and explicit trust policy. Separate
   ready candidates from installable candidates requiring setup/approval. Unknown
   compatibility is not the same as a verified match.
5. **Rank a bounded shortlist.** Rank by goal/step coverage, contract fit,
   descriptive relevance, applicable test evidence, installed readiness and cost
   of new dependencies/adapters. Treat test claims as attributed evidence, not
   certification. Stars are displayed popularity, not safety or correctness.
6. **Optionally rerank.** A configured provider can score the shortlist's semantic
   fit. It cannot restore disqualified candidates, add candidate IDs, approve
   permissions, generate hidden steps or trigger a new search loop.
7. **Choose a small compatible set.** Use a bounded greedy coverage heuristic:
   prefer covering unmet steps with fewer new packages and less mortar, while
   respecting hard constraints. Validate candidate connections and dependency
   conflicts. This is a heuristic, not a globally optimal planner.
8. **Load only selected contracts.** Fetch full schemas/guidance for likely
   candidates within budget. If a contract fails validation, try the next allowed
   candidate; otherwise report the specific gap and stop.
9. **Freeze the resolution.** Return/persist the chosen manifest, uncovered gaps
   and stop reason. Start implementation/execution; do not re-search every call.

Use deterministic ranking/tie-breaking as the initial baseline. Calibrate weights
and sufficiency rules on fixtures rather than inventing a universal confidence
percentage. Schema subtyping is bounded to Rhyven's supported subset; anything
unproven requires concrete validation/mortar/tests, not a compatibility guarantee.

Index compact action records, not complete source files or README dumps. Suggested
fields: package/function IDs, hashes, one-line purpose, tags, input/output summary,
backend, permissions, dependency requirements and test-evidence references.
Reuse SQLite; evaluate FTS5 availability in distributed builds before selecting
its BM25 implementation. A vector database or embedding model is not required.

Public function summaries need a small versioned registry sidecar or equivalent
schema-derived metadata, bound to package hashes and validated by the registry.
Older entries without it remain searchable at package level; fetch full packages
only for the bounded shortlist. Do not download every package during matching.
A summary is untrusted publisher data; its existence does not authorize execution.

## 9. Stop rules: prevent endless searches

Initial defaults are tunable engineering starting points, not performance claims:

| Budget | Initial default |
| --- | --- |
| Steps per matching request | 12; larger plans explicitly split into subplans |
| Discovery rounds | One batched round plus one targeted retry for remaining gaps |
| Internal retrieved candidates | At most 20 per step; deduplicate by package/function identity |
| Agent-visible candidates | At most 3 per step and 12 overall, with uncovered steps reported |
| Full contracts loaded | At most 8 per discovery session |
| Optional reranker | One bounded batch, at most 12 candidates, 5-second timeout |
| Total discovery deadline | 15 seconds; return partial results/fallback on timeout |
| Response size | Default 12 KB compact UTF-8; report omissions, never silently truncate schemas |

The deadline includes remote work; provider timeouts cannot reset it. Permission
or installation waits happen after matching and do not cause repeated searches.
Track rounds/inspection budgets across calls using a persisted discovery session.
Repeated equivalent requests reuse the result. Budget expansion is explicit and
recorded, not an agent's automatic reaction to a low score. This prevents the
built-in matcher looping; a harness must also follow the skill because Rhyven
cannot stop an unrestricted agent using unrelated search tools.

Stop when the required steps have a suitable validated candidate, the single
retry has no useful new candidates, or any session budget is exhausted. Return
`build` for a clear missing capability within granted development scope; return
`clarify` for an ambiguous requirement and `blocked` for unavailable authority or
host prerequisites. Building is not permission to bypass an execution restriction.
Do not insist on searching the whole catalog to prove that no better brick exists.

Record rejected candidates and reasons. Do not reconsider them within the same
plan/catalog/constraint revision. Reopen discovery only for a concrete trigger:
changed requirement, invalidated dependency, observed incompatibility, explicit
user request or relevant catalog change. A runtime error alone first enters
ordinary diagnosis; it does not launch endless replacement searches.

Cache keys include collection visibility, normalized plan/constraints, catalog
revision, host capability fingerprint, permission/trust policy and ranker version.
Negative results have a bounded TTL and are invalidated by relevant changes.
Do not share private plans or private capability metadata across collections.

## 10. Optional Laya and other matching providers

Assumption: "Laya" means the open-source typed-decision model in
[NandhaKishorM/laya](https://github.com/NandhaKishorM/laya), not the unrelated
project-management product with the same name. Its documented choice/score/yes-no
operations make shortlist evaluation a plausible adapter use. This is a design
proposal, not verified integration or an accuracy/latency claim (reviewed 2026-10-08).

Define a provider-independent `rank_candidates` contract: minimal plan summary,
candidate IDs/descriptions/contract summaries -> bounded scores or ordering,
coverage judgments and abstention. Validate the returned IDs, output shape and
bounds. Treat provider scores as advisory and not comparable across models without
calibration. Failures, invalid responses and timeouts fall back to baseline ranking.

Ship providers as optional apps/adapters with explicit activation. Pin model and
adapter versions; document hardware, license and resource requirements. Do not
bundle model weights or download them on first discovery without consent. Local
inference is preferred where available; hosted inference requires opt-in, minimal
redacted input, disclosed endpoint/costs and configured credentials. No requirement
for the Rhyven engine to manage PyTorch/CUDA or host a model service.

Treat candidate descriptions as untrusted data, not instructions. A ranker cannot
bypass permission filters or reveal private source. Keep full code, secrets and
conversation history out of ranking requests. Evaluate malicious descriptions
and provider abstention explicitly. Optional embeddings can later improve
retrieval recall; a reranker alone cannot recover candidates never retrieved.

## 11. Agent authoring, frames and publication

Provide scaffolding for one typed action, a stack, a pallet and a frame. Generate
a small entrypoint adapter and tests rather than a new programming language.
Keep canonical existing app commands; choose aliases only when they improve
usability. Full new command names are deferred to implementation design.

Update usage skills/rules to instruct agents: plan once, batch discovery, inspect
only selected schemas, honor stop reasons, prefer a sufficient existing solution,
write only missing behavior, test and retain locally. Do not package every one-off
snippet: create reusable capabilities when repetition or clear future reuse
justifies the extra work. Save what worked and why, scoped to the project.

A frame supplies template files, starter stacks, dependency references, examples,
optional initial records and customization instructions. Applying one previews
changes, preserves existing files/state and sends dependencies through normal
review. It creates an editable local project with recorded frame provenance.
No automatic template merge/update system initially. Start with a project-quality
frame; add documentation or data-processing frames only after validating them.

Publishing is exclusively user-initiated. Agents may prepare and perform it after
an explicit user request, but must not independently upload/push to public repos,
submit marketplace entries or turn private state into fixtures. Reuse existing
open-source submission rules, immutable versions, hashes and publisher ownership.
Review selected source, licenses, tests, dependency locks and permission changes;
exclude secrets, customer inputs, collection databases and private run histories.
Public sharing is optional; local capability accumulation is the default outcome.

## 12. Ordered implementation and release gates

| Phase | Deliverable | Required acceptance |
| --- | --- | --- |
| 0. Contract design | Baseline audit, fixtures, versioned manifest extensions, permission/dependency semantics | Existing app contracts unchanged; old clients reject unsupported formats clearly |
| 1. Bounded discovery | Local index, plan request/result, deterministic matcher, persistent budgets/cache | Finds existing actions; stops on no-match; no model/network required |
| 2. Sequential composition | Typed stacks, declarative mortar, dependency pins, run/step records | Mixed declarative/Python/JS actions through same interface; no lock deadlocks or approval bypass |
| 3. Reusable local authoring | Scaffolding, tests, draft/activation, bounded iteration, quality pallet | Three-app example executes as a saved stack; another agent reuses it without rediscovery |
| 4. Native artifacts | Linux executable backend, artifact validation and host diagnostics | Rust/Go fixtures run on supported architectures without compiler installation; unsupported ABI rejected |
| 5. Frames and optional rankers | Quality frame, provider contract, optional Laya adapter | Frame preserves user changes; baseline works without provider; reranker survives bad/late responses |
| 6. Distribution and docs | Registry capability metadata, validator/runtime alignment, skills/site updates | Fresh user discovers, reviews, installs and uses a published pallet/frame with unchanged three-tool MCP |

Phases 1–3 do not wait for native execution or Laya. Define the public-index format
in phase 0; ship its remote path with the coordinated validator in phase 6. Package
features must never be accepted by the marketplace before the distributed runtime
supports them. Update approvals, backup/update recovery and supported-client paths
alongside new contracts. Do not silently upgrade today's package format.

Before public release, test functional failures, permission denial, contract
mismatches, removed/updated dependencies, cycles, bounded iteration/nesting,
partial writes, interruption/restart and collection isolation. Test malformed and
oversized indexes, malicious candidate text, stale caches, unsupported providers,
missing runtimes, tampered native artifacts and conflicting dependency versions.

## 13. Evaluation and release proof

Create at least 30 held-out plan fixtures covering exact reuse, synonyms, multi-step
stacks, ambiguous goals, permission/backend conflicts, no match, required mortar,
stale versions and adversarial metadata. Vary catalog size with distractor entries;
report tested sizes rather than claiming unmeasured marketplace-scale performance.

Compare four modes: existing agent-led search; deterministic bounded matching;
matching plus optional Laya; repeated tasks using saved resolutions/stacks. Use
the same available capabilities and task acceptance checks. Separate cold-cache,
warm-cache, one-time setup/approval and recurring execution costs.

Measure retrieval recall@k and valid-candidate selection separately, task success,
false-compatible selections, no-match abstention, searches, schemas loaded, bytes,
actual model input tokens, discovery latency, package/setup overhead and regression
outcomes. Count plan construction, guides, tool schemas, candidate descriptions,
reranker calls and retries. Do not infer token savings from tool count alone.

Non-negotiable gates: zero known hard-constraint bypasses in the fixture suite;
all calls respect configured bounds and explicit stop reasons; old apps remain
compatible; ranker failure preserves a usable baseline; no public publication
without a user request. Compare task success and search overhead against the
baseline before claiming an improvement. Freeze numeric quality targets after
collecting baseline measurements, before tuning against the held-out evaluation.

Demonstration: an agent finds existing quality capabilities, writes only missing
mortar, saves a tested stack/pallet locally, then another agent runs it using saved
contracts. A new failure adds a fixture and leads to a tested version update. Show
what changed, what was reused and what approval was required. Public posting is a
separate, explicitly requested final operation.
