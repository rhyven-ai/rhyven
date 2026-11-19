# Rhyven 0.9.5 release plan

**Status:** Implemented in the local source build. See [verification results](verification-0.9.5.md). The release goal is for a second agent to finish interrupted work in a shared collection using evidence recorded by the first.

## 1. Atomic task claims

Add claim, renew, release and complete actions to Work Management. Record the actor, attempt, expiry and a distinct claim token when a worker acquires a task. Check the current token and expiry atomically with each ownership-dependent write, including checkpoints. A takeover or human reassignment invalidates the previous claim. Existing unclaimed tasks and callers retain their current behavior until a task enters the claim flow. Messaging continues to deliver notifications independently of task ownership.

**Acceptance:** Race two workers for the same task. Exactly one acquires it. After expiry a second worker can take over. Attempts by the original worker to renew, change progress or complete the task fail without modifying the new owner's work.

## 2. Durable handoffs

Store a checkpoint on the task with the last completed step, record or artifact references, open questions, uncertain external operations and the suggested next action. Persist the checkpoint across process restarts and ownership changes. Expose the evidence needed by a successor through the task, preserving the actor scope of private request reports.

**Acceptance:** Restart the first worker after it checkpoints. A different actor can open the task, see the saved evidence and resume the unfinished step without the first worker's chat history.

## 3. External operations across attempts

Give each logical external action a durable operation ID scoped to the collection and task. Persist its intent before dispatch. Reuse the ID across retries and ownership changes, pass an idempotency key where the provider supports it, and save the external reference and outcome when available. Bind the ID to the intended operation and arguments. When the provider outcome is unknown, expose a needs-reconciliation state and evidence for resolving it before another dispatch.

**Acceptance:** Interrupt a worker after a test provider accepts a request but before Rhyven records the result. With provider lookup or idempotency support, the successor recovers the existing result. With neither capability, takeover surfaces the uncertainty and does not automatically repeat the action.

## 4. Basic task ownership view

Show three columns in the selected collection: task, current owner and status. Derive current owner from the active claim and show Unclaimed when no lease is active. Use the same documented read operation available to installed apps. Keep the built-in screen read-only. Reassignment remains a Work Management action.

**Acceptance:** Claim, expire and reassign a task. On refresh, the screen and an independent consumer of the public contract agree on its owner and status, including when an assigned task has no active lease.

## 5. Automatic prompt enrichment

When enabled for a collection and harness, run retrieval once for each new user prompt before the first model call. Users type normally. XML tags are optional output formatting. Process only original user input so tool results and retrieved examples cannot trigger another search.

Provide a user-visible Automatic prompt enrichment on/off setting for each harness connection, with the same control available from the CLI. Show the effective setting and apply changes on the next prompt. Turning it off bypasses the automatic path before prompt processing, database queries or model calls. The adapter sends the original prompt unchanged. Manual Wobbegong search remains available.

**Disable and re-enable test:** With a known matching source, turn enrichment off and submit the matching prompt. Verify zero enrichment calls, identical outgoing user text and no injected excerpt or reminder. Verify manual search still finds the source. Turn enrichment on again and verify that the next matching turn receives the cited excerpt once.

1. Add a shared Rhyven context-preparation operation with CLI, MCP and REST access. Bind requests to the caller's collection, source permissions and turn. Return either a bounded context addition or no addition. Scope retry reuse to the same actor, turn, prompt and settings.
2. Query the existing Wobbegong index. Start with the prototype's proposed 3–9 short keyword searches, using local keyword extraction as the baseline. Merge overlapping passages and apply one relevance gate and size budget across all results. Evaluate the gate with labeled prompts, including misleading keyword matches. Skip stale records and preserve source references, line ranges and available version or release-status labels.
3. On a strong match, add cited evidence as reference material and end the addition with a reminder showing the actual session tool name, category, function and a usable example query. The adapter resolves the tool name for that harness. On a weak match, empty index, error or timeout, forward the original prompt exactly as submitted, with no wrapper or reminder.
4. Implement a Codex submission-hook adapter and a reference adapter for a harness that controls its model request. Both use the same preparation operation. Use the harness's supported context channel and delimit retrieved text as source data. Offer the same operation on demand to MCP-only clients. Provide a CLI preview showing the proposed addition, sources and selection outcome.
5. Verify both adapters with the same fixtures. A known-answer prompt receives a cited passage and working tool reminder before the first model call. Unrelated, misleading-overlap and timeout cases preserve the original prompt. Concurrent agents receive only their own turn's results. A repeated delivery adds the packet once. Compare with manual RAG on the same tasks and index, recording answer correctness, false injections, latency and total tokens including any query-generation calls.

## 6. Customization through installed apps

Extend the existing app model. Publish versioned, discoverable input and output schemas, examples and contract tests for task, claim and checkpoint reads and for their lifecycle events. Include stable record IDs and distinguish the assigned actor from the current lease holder. Expose changes through the existing collection-scoped cursor feed. Document how an app reads current state, polls and resumes, and requests an ownership change through the same validated action as a built-in client. Return the fields needed to display claims without exposing lease tokens. Observers run independently of task transitions.

For the first decision hook, let a collection operator select a compatible action from an installed app to filter proposed prompt context before attachment. Use the existing installation, app discovery, action dispatch, schema validation and permission mechanisms. Show the app, action and data it will receive when enabling it for the collection. Supply eligible excerpt IDs, text and source metadata. A valid result is an ordered subset of those IDs, including an empty list. Rhyven retains control of citations, size limits and the tool reminder. Raw user prompts are outside this first hook's input. If enrichment is disabled, skip the hook. If an enabled filter fails validation or times out, send the original prompt without an addition.

Ship a small example app with contract cases: select released documentation, omit plan excerpts and return an empty selection when only plans match. Verify that it can be installed, enabled and replaced using normal app tooling and a collection setting. Verify observation separately with an app that groups tasks by current owner through public operations. Neither example requires engine changes by its author. Include the active filter identity and version in enrichment retry settings so changing the selected app invalidates earlier selections.

## Release acceptance scenario

Deliver the same task notification to two agents. The first claims the task and creates a ticket in a test service. Stop it before it records the ticket number. After the lease expires, the second agent takes over, recovers the ticket using the operation ID and completes the task. Reconnect the first agent and verify its late completion is rejected. The terminal view shows one completed task. Its checkpoint and lifecycle events expose the original ticket reference and the history of both attempts through the public contract. Verify that the test service contains exactly one ticket.

Keep these agent-facing operations discoverable through the existing three-tool interface. Scope the release to cooperating agents in a trusted collection.
