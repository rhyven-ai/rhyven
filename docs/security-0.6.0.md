# 0.6.0 release security review

The pinned Gitleaks and Trivy checks ran on 2026-10-08. Dependency scanning
reported no vulnerabilities. Gitleaks reported five historical and four current
`generic-api-key` findings in `SOURCE-MANIFEST.json`. Each finding was inspected
against its exact source line: every value was a 64-character SHA-256 source-file
inventory digest, not an authentication credential. No secret rule was disabled
and no history was rewritten. The raw scanner reports remain local.

Registry authorization now checks namespace ownership for pallet submissions as
well as apps. Owner, maintainer and unauthorized-submitter cases are covered by
tests. Package hashes and listing metadata are checked before source is saved;
changed listings require new approval. Downloading pallets never executes them.

Workspace/global isolation, approval requirements, tampered assets and pinned
approval destinations are covered by runtime tests. Native and script actions
remain unsandboxed host execution requiring explicit permission.
