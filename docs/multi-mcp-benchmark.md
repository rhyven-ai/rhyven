# Three-server MCP benchmark

Local test, 2026-10-03. No changes published. Runtime: local `7e64eff` discovery implementation.

We ran three distinct official reference MCP servers, not copies of the same schema:

| Server | npm package | Version | Advertised tools |
|---|---|---|---:|
| Filesystem | `@modelcontextprotocol/server-filesystem` | 2026.8.31 | 14 |
| Memory | `@modelcontextprotocol/server-memory` | 2026.8.31 | 9 |
| Everything | `@modelcontextprotocol/server-everything` | 2026.8.31 | 13 |

Sources: [Filesystem](https://github.com/modelcontextprotocol/servers/tree/main/src/filesystem),
[Memory](https://github.com/modelcontextprotocol/servers/tree/main/src/memory),
[Everything](https://github.com/modelcontextprotocol/servers/tree/main/src/everything).

The reference SDK was 1.32.0. All services ran against disposable local fixtures.
Filesystem was restricted to the temporary fixture directory. Memory contained one
project record. No personal files, external accounts or production data were used.

## Actual operations and compatibility

1. Filesystem `read_text_file` read `plan.json`: project alpha, base 17.
2. Memory `search_nodes` found alpha's stored offset, 25.
3. Everything `get-sum` added those retrieved values and returned 42.

Each call was made directly through MCP and through a generated Rhyven connector.
All three result objects matched exactly. The later calls' inputs were derived
from the earlier actual results, not hardcoded independently of the workflow.

A local test-only HTTP-to-stdio gateway used the official SDK to preserve upstream
tool definitions, initialization instructions and results. It was necessary because
Rhyven's connector currently accepts HTTP MCP, while these reference servers can
run over stdio. The benchmark provisions that gateway and separately installed
servers; the Rhyven wrapper does not install them. Gateway setup or latency is not
part of the input-token comparison. All test processes are closed afterward.

We imported six tools into three apps:

- Filesystem: `read_text_file`, `list_directory`.
- Memory: `search_nodes`, `open_nodes`.
- Everything: `echo`, `get-sum`.

**The wrapper did not expose all 36 upstream tools.** The same-six direct baseline
controls for that selection. The full-server comparison measures the effect of
selective, deferred discovery versus eagerly loading a broader tool catalog; it
is not evidence of savings at identical full-server capability coverage.

## Token methodology

Real calls and responses feed a fixed, synthetic model-input schedule. Token counts
use tiktoken 0.12.0 with `o200k_base` and `cl100k_base`. **No language model was invoked.**
These are visible-context token measurements, not provider-reported billing,
reasoning-token measurements, reasoning accuracy, or latency benchmarks.

We include tool definitions, upstream initialization guides, Rhyven's connection
guide, category discovery, complete selected-function descriptions and app guides,
actual argument/result messages, repeated conversation history and the final-answer
input. Tools are supplied on each modeled turn. There is no prompt-cache discount,
compaction, speculative failure, or batching. Hidden harness instructions and
provider-specific framing are excluded.

The single-server task reads the plan while all three servers/apps are available.
The cross-server task performs the complete workflow above. Direct execution needs
two and four modeled inputs respectively. Cold Rhyven execution needs four and
eight, including categories and per-app descriptions. A known-category variant
skips categories. Exact function names are assumed known; search mistakes or
additional discovery are not modeled.

We report two serialization assumptions:

1. **Function-schema-only:** expose name, description and inputSchema for every
   tool on both paths. Omit optional top-level MCP metadata such as annotations,
   titles and outputSchema. This is the more conservative headline comparison.
2. **Full MCP definitions:** count the complete raw definitions as returned by
   tools/list. This matches the earlier benchmark but can overstate what a host
   actually sends to its model.

The task-filtered direct baseline is optimistic: it knows which functions and
server guides will be needed without charging selection/discovery overhead.
It is a useful lower bound, not a measured implementation of automatic tool search.

## Conservative results: function schemas only

Cumulative visible input tokens (`o200k_base`):

| Approach | Single active server | Cross-server workflow |
|---|---:|---:|
| Direct MCP, all 36 tools | 8,290 | 17,051 |
| Rhyven, cold discovery | 5,026 | 15,578 |
| Rhyven, categories already known | 2,263 | 9,658 |
| Direct MCP, same six tools as wrappers | 1,980 | 4,431 |
| Direct MCP, only task-required tools/guides | 546 | 3,507 |

Against eager loading of all 36 tools, cold Rhyven reduces visible input by
**39.4% for the single-server task** and **8.6% for the cross-server workflow**.
It does not beat direct MCP filtered to the same six tools in either cold test.

The `cl100k_base` results and full per-turn counts are in the
[recorded results](benchmarks/multi-mcp-tokens.json).

## Full raw MCP definitions

| Approach | Single active server | Cross-server workflow |
|---|---:|---:|
| Direct MCP, all 36 tools | 14,896 | 30,263 |
| Rhyven, cold discovery | 5,026 | 15,578 |
| Direct MCP, same six tools | 3,440 | 7,351 |
| Direct MCP, only task-required tools/guides | 688 | 4,971 |

Under this assumption, cold savings versus all tools are 66.3% and 48.5%.
The difference between these two tables is substantial. Do not advertise the
larger percentage without saying that it counts full raw MCP metadata.

Repeated-task scenarios retain earlier guides, discovery and tool-call history;
they do not treat discovery as free context. They reuse the captured deterministic
results and exclude previous final-answer text equally on both paths. All repeat
counts are in the JSON report.

## Conclusion

This test confirms visible-input savings against eagerly loading several full tool
catalogs under the stated serialization assumptions. It does not establish a
universal token advantage for the three-tool interface. Filtering native tools
also saves context, and is cheaper here when the required capabilities are known.
Actual billing depends on the host's tool serialization, prompt caching, discovery
strategy and model behavior. A live-model test is still required before claiming
measured API-cost or task-success improvements.

## Reproduce

Install the pinned reference packages separately in a disposable test directory:

```bash
npm install --prefix /tmp/rhyven-multi-test --ignore-scripts --no-audit --no-fund \
  @modelcontextprotocol/server-filesystem@2026.8.31 \
  @modelcontextprotocol/server-memory@2026.8.31 \
  @modelcontextprotocol/server-everything@2026.8.31 \
  @modelcontextprotocol/sdk@1.32.0
```

Build the local feature branch, then use a Python environment with tiktoken 0.12.0:

```bash
cargo build --bin rhyven
python3 qa/multi_mcp_benchmark.py target/debug/rhyven \
  /tmp/rhyven-multi-test/node_modules /tmp/rhyven-multi-results
```

The script creates and removes isolated fixtures, launches the gateway and reference
servers, imports local test apps, verifies matching results, and writes `trace.json`
and `results.json`. It does not install dependencies or submit/publish apps.
Temporary paths and endpoint strings are normalized in captured inputs. Package
hashes still reflect the fresh endpoint, so small token variations across runs are
expected; the recorded capture SHA-256 identifies the exact measured run.
