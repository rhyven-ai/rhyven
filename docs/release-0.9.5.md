# Working with multiple agents in Rhyven 0.9.5

These instructions apply to the source build. Run `cargo build --release --locked --bin rhyven` and use the resulting executable for the CLI and the agent connection. Confirm the executable and version through discovery in the actual client.

## Tasks and handoffs

Install Work Management in the collection and give each connected worker a distinct actor label. Existing unclaimed tasks retain their existing actions. Press **t** in the terminal marketplace for a read-only list of tasks, current lease holders and status. Press **r** to refresh, use the arrow keys to scroll, and Escape to return.

Use `rhyven_describe` for the exact function inputs and outputs of `rhyven/work-management`. All operations below use the ordinary universal call or the equivalent REST function route.

| Action | Behavior |
| --- | --- |
| `action_ownership` | Returns the assigned actor and current owner separately. Read pages using offset and limit. An expired claim has a null current owner. |
| `action_claim` | Atomically claims a task and returns a private attempt token, expiry and handoff. Default lease duration is 300 seconds. Supply a stable request ID when retrying a lost response. |
| `action_renew_claim`, `action_release_claim` | Extend or release the current actor's live attempt. |
| `action_save_checkpoint`, `action_handoff` | Save completed work, references, questions, uncertain operations and the suggested next step. A successor can read the checkpoint and provider evidence after a restart. |
| `action_transfer_claim` | Invalidate the old token and reserve the task for the recipient. The recipient calls claim to obtain a new token. A local operator can use `reassign-task TASK_ID --owner ACTOR --expected-revision REV`. Read the revision with `object_task_get`. |
| `action_complete_claim` | Record the result and close the task, checking the live token in the same transaction. Existing task actions such as block and resume accept an additional `claim_token` on managed tasks. |

To observe changes, call `rhyven/marketplace.action_changes` with `app: "rhyven/work-management"`. The version 1 feed returns cursor, operation, action, actor, record ID, revision and time. Read all pages and save the returned cursor together with the collection and filter. Task lifecycle events identify the task and the action taken. Read the public ownership and handoff operations for current state. Expiry is evaluated against the clock at read time.

### External operations

Before contacting a provider, call `action_operation_intent` with the task ID, claim token, a logical key such as `create-ticket`, and a stable serialization of the intended action and arguments. Persist the intent before dispatch. When `next_action` is `dispatch`, use the returned operation ID as the provider idempotency key. The OpenAPI importer maps a declared `Idempotency-Key` header to the string input `idempotency_key`. After verifying the provider outcome, call `action_operation_resolve` with the logical key and provider reference.

On takeover, an existing intent returns the same operation ID. When `next_action` is `use_recorded_result`, use the saved reference. When it is `reconcile`, use provider lookup, a provider-supported idempotent replay or human reconciliation. A retry of the intent alone does not repeat the external operation. Preserve these decisions in the checkpoint. Providers without lookup or deduplication may require manual investigation.

## Automatic context

Automatic enrichment is off by default. Choose a collection and a distinct connection name for each harness. Wobbegong Collection Search (`rhyven/wobbegong`) supplies the cited passages. Use `wobbegong ingest-files` for selected files, `ingest-records` for local app records, or `ingest-repository` for snapshots supplied by Repository File Ingestion (`rhyven/file-rag`, 0.4.0+). Query generation uses local keyword matching.

```sh
rhyven --collection project context enable rhyven-project
rhyven --collection project context status rhyven-project
rhyven --collection project context preview '{"connection":"rhyven-project","turn":"example","prompt":"How are release tickets reconciled?","tool":"mcp__rhyven-project__rhyven_call"}'
rhyven --collection project context disable rhyven-project
```

Export the adapter with `rhyven context export-adapter /absolute/new/adapter.py`. Use the same actor, collection and MCP entry name for the adapter and the server connection. Pin the executable path. The adapter derives the tool alias from the client and entry name; use `--tool` for a custom alias or an older Hermes release.

**Codex and Claude Code.** In Codex's hooks.json or Claude Code's settings.json, merge a UserPromptSubmit command into the existing hooks. In this example use `codex` or `claude` as the adapter mode to match the host. Review the hook through the client's normal hook controls. Use a WSL launcher if the host runs on Windows and the binary is in Ubuntu.

```json
{"hooks":{"UserPromptSubmit":[{"hooks":[{"type":"command","command":"python3 /absolute/new/adapter.py claude --binary /absolute/path/to/rhyven --collection project --actor worker-a --connection rhyven-project"}]}]}}
```

**Hermes.** Add the pre-LLM shell hook to the active profile's config.yaml and review its first-use consent.

```yaml
hooks:
  pre_llm_call:
    - command: "python3 /absolute/new/adapter.py hermes --binary /absolute/path/to/rhyven --collection project --actor worker-a --connection rhyven-project"
```

**Verify the actual connection.** Start with a question that matches an indexed document. The following command probes the saved MCP entry and returns a two-line prompt under `diagnostics.enrichment.submit_in_harness`.

```sh
rhyven connect --client hermes --name rhyven-project --check --configured --test-prompt "How are release tickets reconciled?"
```

Submit both lines in the connected harness. On a retrieval hit the excerpt carries a one-time confirmation call. Run the same check without `--test-prompt` to see its status and confirmation time. Each new test replaces the prior challenge for that connection and actor. Challenges expire after ten minutes. A normal MCP probe reports the serving version, executable, actor, collection, enrichment setting and expected tool alias. A live confirmation also records the alias submitted by the adapter. A pending challenge records its latest preparation outcome, including invalid submissions, retrieval misses and successful matches. Only a successful proof confirms delivery. The adapter gives oversized native turn IDs a stable SHA-256 identifier so retries keep the same delivery identity.

Markdown retrieval keeps excerpts inside a heading and includes the heading in the citation, including for data indexed before this update. A standalone operation name can match an exact identifier. Ordinary multiword prompts continue to use the relevance gate.

For a custom harness, invoke the same adapter in `reference` mode with an envelope containing `turn_id` and `messages`. Forward the returned messages when assembling the request. It only handles an original text user submission. A fresh turn ID is required for each user message. Codex receives excerpts through its additional-context hook. Enable this for sources appropriate to the chosen harness's context channel.

The CLI preview and `rhyven/wobbegong.action_prepare_context` return the same version 1 packet. Each identifies the collection index and includes a selection reason, excerpts with references, hashes and line ranges, and a bounded addition. Automatic adapters take a packet once per actor, connection and turn. The default adapter timeout is two seconds. Disabled connections, empty or weak results, errors and timeouts leave the submitted text intact. A failed delivery after the packet is taken can omit the optional addition. A manual preview can still retrieve it. Display settings or disable enrichment for one harness without changing manual search.

## Extend with ordinary apps

Use `examples/team-customization` for a script app containing two actions and tests. The context filter receives candidate excerpt IDs, text and source metadata. It returns an ordered subset in `selected`; an empty list suppresses enrichment. Install the reviewed app, then select the action for a connection:

```sh
rhyven --collection project context enable rhyven-project --app example/team-customization --action select_releases
rhyven --collection project context enable rhyven-project --clear-filter
```

Selection pins the installed action contract. Review and select it again after an update. A missing or invalid selection suppresses the addition. To build the ownership report, install Work Management and the example app, then compose `examples/team-customization/ownership-workflow.json` against those dependencies. Install the resulting package and call `action_group`. It reads a page of tasks through the public ownership action and groups them by current owner.
