# Rhyven 0.9.5 verification

The Linux x86-64 release candidate passed all 113 locked Rust workspace tests, formatting and Clippy with Rust 1.90.0.

| Area | Verified behavior |
| --- | --- |
| Task ownership | Two workers raced for one claim. The successor recovered the checkpoint and the provider ticket. The provider received one ticket request, and the expired worker could not complete the task. |
| Prompt enrichment | Codex CLI, Claude Code and Hermes each delivered a cited passage to a local model endpoint. The exported adapter handled long Hermes turn IDs and confirmed receipt through MCP. Disabled and unrelated prompts preserved the user text. Repeated submissions did not add another copy. |
| Collection search | Manual search and prompt enrichment cited the same file after a paginated repository import. A failed import preserved the last snapshot. Refresh removed deleted files. Correcting a note kept the old text out of enrichment before reindexing. |
| Apps | Design Review and Repository File Ingestion each passed four contract cases. Upgrades from the published File RAG 0.2.0 and Wobbegong 0.3.1 packages preserved sources, indexes and summaries. |
| Containers | Messaging passed delivery, restart, restore and reinstall checks. Change Verifier retained shared results after restart. Starter Runner completed a human question and restored its completed task. The documentation tool passed with all nine bundled language servers. |
| Distribution | The public source export built on Linux x86-64 with glibc and musl. The portable build passed search, enrichment and handoff checks. |

The final dependency scan reported no known vulnerabilities in the lockfile. Spreadsheet import and backup/restore tests passed with the updated ZIP and TLS libraries.

The harness tests used deterministic replies from local endpoints. The [token comparison](benchmarks/prompt-enrichment-095.json) records the results of three synthetic questions. The [release guide](release-0.9.5.md) covers setup and operation.
