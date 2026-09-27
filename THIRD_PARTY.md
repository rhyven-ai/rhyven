# Third-party components

Rust dependencies are pinned in Cargo.lock. The packaged notices directory contains license/copyright files available in the resolved Cargo package sources. SQLite is compiled through rusqlite's bundled feature; HTTPS uses reqwest/rustls. The current preview Linux binaries are built from this source using Rust 1.90.0 and musl. macOS binaries use the native platform toolchain.

```bash
CC=musl-gcc cargo +1.90.0 build --release --locked --target x86_64-unknown-linux-musl
```

Earlier static GNU/Linux builds included the system C library. Its copyright notice is included; corresponding upstream source is available from the [GNU C Library project](https://www.gnu.org/software/libc/) and the matching distribution source package. Rhyven's original code is licensed under Apache-2.0; see LICENSE and NOTICE. Third-party components retain the licenses documented here. Binary releases and standalone MCP exports include THIRD_PARTY_NOTICES.txt. The executable exposes the same notices with `rhyven license --third-party`.
