# Connector input-token benchmark

Local feature-branch test, 2026-10-03. Nothing published or deployed.

The wrapper was tested against `@modelcontextprotocol/server-everything@2026.8.31`, the official [Everything MCP reference server](https://github.com/modelcontextprotocol/servers/tree/main/src/everything). This is a protocol test server, not a representative production workload. It advertised 13 tools to this client. We imported `echo` and `get-sum` and verified that direct MCP and Rhyven returned the same result for 17 + 25: 42.

## What was measured

The harness performs real initialization, tool discovery, import, installation into a temporary collection, Rhyven discovery and tool calls. It then builds a **fixed, synthetic sequence of model inputs** from the captured responses and counts canonical JSON with `tiktoken==0.12.0`, using `o200k_base` and `cl100k_base`.

No LLM was invoked. These numbers are exact tokenizer counts for the visible input fixtures, **not provider-reported usage, reasoning-token counts, or evidence of reasoning quality**. They exclude hidden harness/system prompts, provider-specific tool serialization, chat framing and generated reasoning text. Internal reasoning is generally output-side work; it cannot be measured by counting a guide. A live-model evaluation is still needed to measure actual usage and task reliability.

The inputs include:

- All exposed tool definitions on every modeled turn.
- Native MCP initialization instructions for the direct path.
- Rhyven initialization instructions for the wrapped path.
- The complete category and description responses, including the imported upstream guide and repeated full contract.
- Tool-call arguments/results and prior conversation context repeated at subsequent turns.
- A final response turn after the action result is read.

Cold Rhyven discovery takes four modeled turns: categories, describe, action, final answer. Direct MCP takes two: action, final answer. When the category is already known, Rhyven takes three. A subsequent task retains discovery in conversation history; it is not treated as free. No prompt-cache discount or context compaction is assumed. The native baseline does not assume it must read the MCP protocol specification or a setup README to call an already-connected tool.

## Results

Cumulative visible input tokens across the complete task:

| Scenario | o200k_base | cl100k_base |
|---|---:|---:|
| Direct MCP, all 13 advertised tools | 4,221 | 4,092 |
| Direct MCP, same two selected tools | 1,209 | 1,188 |
| Rhyven, categories → describe → call → answer | 5,264 | 5,197 |
| Rhyven, known category → describe → call → answer | 3,831 | 3,782 |
| Direct MCP, next task with all 13 tools and retained history | 4,385 | 4,250 |
| Direct MCP, next task with the same two tools and retained history | 1,373 | 1,346 |
| Rhyven, next task with retained discovery/history | 4,390 | 4,333 |

The two-tool direct baseline is important: comparing a selected wrapper with an entire upstream server otherwise confounds tool selection with transport/discovery efficiency. Some MCP clients already support tool filtering or lazy tool discovery; these results do not establish savings against those clients.

For `o200k_base`, the initial native tool definitions occupy 1,719 tokens (213 for the selected pair), versus 171 for Rhyven's three tools. The upstream guide adds 327 tokens; Rhyven's initialization guide adds 154. The Rhyven categories response is 326 tokens, and the description is 1,298 tokens including its guide and contract.

The smaller initial tool surface does **not** translate into a lower cold-task total here. Cold Rhyven discovery costs about 25% more than the full-server direct baseline, and much more than the same-two-tool baseline. Knowing the category reduces discovery overhead, but does not beat the filtered direct connection. Reusing an un-compacted conversation still retains the guide and descriptions.

A full 13-tool import was also attempted and rejected on an input property name outside Rhyven's supported schema subset. The importer did not silently rewrite that contract or expose unsupported tools. This test establishes two selected tools, not support for every Everything feature or arbitrary MCP server.

## Next improvements suggested by the measurement

Before claiming token savings, consider a compact description response that avoids repeating action schemas and guides inside `contract`, optional per-function details, and discovery caching with explicit freshness. Then benchmark real multi-app tasks with actual model usage, success rates and tool-filtered/lazy-discovery baselines. This was the initial recommendation. The follow-up below implements compact and selective descriptions; caching and cross-app search remain future work.

## Reproduce

Install and run the reference server **separately** in a disposable test environment:

```bash
mkdir -p /tmp/rhyven-connector-bench
npm install --prefix /tmp/rhyven-connector-bench --ignore-scripts --no-audit --no-fund \
  @modelcontextprotocol/server-everything@2026.8.31
PORT=38721 node /tmp/rhyven-connector-bench/node_modules/@modelcontextprotocol/server-everything/dist/index.js streamableHttp
```

The reference server is for isolated testing; stop it afterward. Rhyven does not execute these installation commands during import or app installation.

In another terminal, use the local feature-branch binary and a Python environment containing `tiktoken==0.12.0`:

```bash
cargo build --bin rhyven
python3 qa/connector_check.py target/debug/rhyven
python3 qa/connector_benchmark.py target/debug/rhyven \
  http://127.0.0.1:38721/mcp /tmp/rhyven-connector-results
```

The benchmark writes `trace.json` (captured definitions, guides and modeled inputs) and `results.json` (component/per-turn counts and trace hash). Temporary endpoint/workspace strings are normalized to stable placeholders. Metadata may vary across runs; the captured trace hash identifies the exact measurement. Treat upstream descriptions and instructions in traces as untrusted data.

[Recorded results](benchmarks/connector-tokens.json) contain both tokenizers and every modeled turn. The full raw capture remains in the local benchmark output rather than being distributed as Rhyven documentation.

## Follow-up: compact and selective discovery

The tables above record the initial wrapper implementation (`e91f5d6`). The local
follow-up removes duplicate schemas/guidance from default discovery and adds
`function`, `search`, and `full` options to the existing describe tool. It keeps the
same reference server, task, two selected tools, app guide and tokenizer methodology.
No LLM was invoked in this follow-up either.

| Rhyven path | Before, o200k_base | After, o200k_base | Reduction |
|---|---:|---:|---:|
| Cold categories → describe → call → answer | 5,264 | 4,258 | 19.1% |
| Known category → describe → call → answer | 3,831 | 2,781 | 27.4% |
| Next task, retaining discovery/history | 4,390 | 3,296 | 24.9% |

Known-category function search (`search: "sum"`) costs 2,689 cumulative input tokens;
selecting the known exact function costs 2,695. These paths return the complete
selected input schema and keep app guidance. They do not omit the guide to produce
a better score. With only two functions in this app, selecting one provides a modest
additional reduction; larger function catalogs should be measured separately.

The describe response itself falls from 1,298 to 707 tokens (45.5% smaller). The
three tool definitions increase from 171 to 195 tokens to describe the new options,
and initialization guidance increases from 154 to 173. Those costs are included
on every modeled turn.

Cold discovery is still slightly more expensive than direct MCP with all 13 tools
(4,221), and every wrapped path remains more expensive than a direct connection
filtered to the same two tools (1,209). This is an improvement over Rhyven's previous
discovery, not proof that wrappers are universally cheaper than native tools.

[Follow-up results](benchmarks/connector-tokens-compact.json) include both tokenizers,
all turn counts, and the capture hash. The current benchmark script produces these
additional selective-discovery scenarios. No database index was added: removing
repeated payloads and returning relevant function schemas reduces input tokens
without introducing another store to maintain.

## Multiple servers

The [three-server follow-up](multi-mcp-benchmark.md) tests actual Filesystem, Memory and Everything MCP servers, including both full-MCP-metadata and function-schema-only counts and matched filtered baselines.
