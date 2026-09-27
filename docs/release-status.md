# Release status

The source candidate is 0.4.0-rc.8, introducing the Apache-2.0 engine release.
It includes the CLI, TUI, three-tool MCP, REST, declarative execution, container
actions, supervised services, collections, staged updates and backup/restore.

The public binary/registry validator remains 0.4.0-rc.6. The website installer
for rhyvenai.com is prepared but not deployed. Newer source and container app
candidates do not replace existing released artifacts. Align runtime, validator,
app versions and signed downloads before promoting the next binary release.

Linux is the supported preview platform; macOS is a feedback target. Native
Windows is not supported. Real-device container acceptance, additional agent
clients and the combined first-time onboarding trial remain release work.
Container acceptance requires Docker and cannot be inferred from unit tests.

The public source has a new history containing reviewed code and documentation.
Older development repositories and artifacts remain archived privately. No
customer data, local runtime database or signing key is part of the source release.

Corvid is in progress as a separate harness. It is not part of the engine release.
Razorback is coming soon; its capabilities have not been announced.
