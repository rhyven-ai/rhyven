# Release status

The downloadable runtime is 0.4.0-rc.8, introducing the Apache-2.0 engine release.
It includes the CLI, TUI, three-tool MCP, REST, declarative execution, container
actions, supervised services, collections, staged updates and backup/restore.

The public runtime and registry validator are 0.4.0-rc.8. The website installer
at https://rhyvenai.com/install.sh serves signed Linux x86-64/ARM64 and macOS
Intel/Apple Silicon binaries. The macOS builds remain feedback previews.
Newer container app candidates do not replace existing released app versions.

Linux is the supported preview platform; macOS is a feedback target. Native
Windows is not supported. Real-device container acceptance, additional agent
clients and the combined first-time onboarding trial remain release work.
Container acceptance requires Docker and cannot be inferred from unit tests.

The public source has a new history containing reviewed code and documentation.
Older development repositories and artifacts remain archived privately. No
customer data, local runtime database or signing key is part of the source release.

Corvid is in progress as a separate harness. It is not part of the engine release.
Razorback is coming soon; its capabilities have not been announced.
