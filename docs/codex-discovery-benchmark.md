# Live Codex discovery and reuse benchmark

Measured on the local 0.5 candidate, 2026-10-03. These discovery changes are now included in Rhyven 0.5.0; the measurements below retain their original test conditions.

Codex CLI 0.160.0, requested model `gpt-6-astra`, medium reasoning. Two sessions
per setup, three tasks per session; second repetition reverses setup order.
Direct MCP is filtered to the same six actions wrapped by Rhyven. The old binary
represents local commit `10264e6`; the new build adds compact categories, aliases,
action keywords, optional function indexes, batch describe, hashes/freshness,
lookup suggestions and schema-reuse instructions. Both Rhyven builds retain the
same three-tool interface. No install or publication operation is benchmarked.

## Results

Means across two sessions per setup; input includes cached tokens and all Codex
instructions/history. Each task column is incremental usage, not the cumulative
counter returned by `codex exec resume`.

| Setup | First task input | Second task input | Third task input | Entire session input | MCP calls by task |
|---|---:|---:|---:|---:|---|
| direct_filtered | 110,550 | 98,376 | 100,430 | 309,357 | 3/3/3 |
| before | 180,803 | 123,195 | 126,591 | 430,589 | 8/3/3 |
| after | 173,426 | 115,014 | 118,428 | 406,868 | 5/3/3 |

The new build uses **5.5% less session input** than old Rhyven,
but **31.5% more** than filtered direct MCP.
First-task input improves by 4.1% relative to old Rhyven.
All 18 tasks returned the correct current values and executed arithmetic through
MCP. No shell shortcut was observed.

Both new-build sessions used one category call, one batch describe and three
actions on the first task. The add/sum alias eliminated the previous failed
search and fallback. Repeat tasks used three actions with no discovery. Old
Rhyven also reused descriptions on follow-ups: reuse is useful, but these results
do not establish it as an entirely new improvement.

Fewer MCP calls did not yield proportional token savings. Codex still carries its
instructions, schemas and retained tool results through model decisions. This
benchmark measures the combined changes, not separate causal effects of each
feature. Optional index/hash checks were covered by tests but were not selected
by Codex in this workflow.

| Setup | Session cached input | Session uncached input | Session output | Session seconds |
|---|---:|---:|---:|---:|
| direct_filtered | 287,744.0 | 21,613.0 | 505.5 | 54.3 |
| before | 398,208.0 | 32,381.0 | 842.0 | 74.8 |
| after | 377,344.0 | 29,524.0 | 785.0 | 93.5 |

## Method and limits

Filesystem, Memory and Everything use official reference packages 2026.8.31 and
SDK 1.32.0, behind the same loopback HTTP gateway. Memory stores alpha/offset=25.
Before each task, the fixture file's base changes to 17, 31, then 46. Codex must
read it and call the upstream sum tool, returning 42, 56, then 71. The prompt is
identical between setups; follow-ups explicitly say the source data changed.
No correct answers or function names are supplied in the prompt.

All sessions use read-only sandbox, an empty working directory and ignored user
configuration; existing ChatGPT authentication is retained. Only this isolated
Rhyven test server's three MCP tools receive per-tool approval configuration for
unattended execution. This is not a production blanket-approval recommendation.
Sessions persist so `resume` can reuse their history. No user configuration,
production state, registry or public repository is changed.

Actual JSONL `turn.completed.usage` is recorded. Resume reports cumulative
session usage (confirmed against matching rollout token_count totals); this
report subtracts the previous task counter. Output and cache counters are also
cumulative in raw logs. Cache warmth, backend scheduling and exact model messages
are uncontrolled. No dollar-price or consistent latency advantage is claimed.
Two sessions and one small workflow are exploratory, not evidence of savings for
all tasks or harnesses. No raw model-request capture was used.

Results: [machine-readable measurements](benchmarks/codex-discovery-reuse.json).
Raw local traces: `/tmp/rhyven-codex-discovery-v2/`.

## Reproduce

Provide the old binary and separately installed reference dependencies. This
uses the current Codex login and consumes model usage. Use a new output directory.

```sh
python3 qa/codex_mcp_benchmark.py target/debug/rhyven /path/to/old/rhyven \
  /path/to/node_modules /tmp/new-codex-benchmark --repeats 2 --model gpt-6-astra
```

The driver records synthetic session traces and fails on invalid task results.
Subtract preceding cumulative usage before comparing individual resumed tasks.
Codex behavior is documented in [non-interactive mode](https://learn.chatgpt.com/docs/non-interactive-mode)
and [MCP configuration](https://learn.chatgpt.com/docs/extend/mcp?surface=cli).
