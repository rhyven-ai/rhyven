# Rhyven 0.8.0

This release focuses on complete apps that agents can use together.

- Declarative apps can import and extract CSV, JSON, text and XLSX files, and
  export CSV, JSON or text. File access is app-scoped and permission-controlled.
- Design Review stores review tasks and proposals for the user's existing
  harness. It does not require a second model connection.
- Change Verifier compares a Python regression test before and after a change
  and runs the candidate's existing tests in a container.
- Customer Onboarding Monitor tracks required documents, replies and deadlines,
  with a durable agent inbox and an optional configured runner webhook.
- Pallet commands, downloads, discovery and the terminal pallet view are retired.
  Apps, app workflows, native actions and bounded app discovery remain.

See [file actions](files.md), [app workflows](composable-capabilities.md), and
[upgrade notes](migration-0.8.md). App packages and images have separate versions.
