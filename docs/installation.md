# Installation and dependency setup

The bootstrap installs the marketplace TUI, CLI, local runtime and adapters as one
prebuilt executable. Rust and host Python are not required. Container support is
optional; Python and other app dependencies stay in their images.

## Platform status

**Linux is the supported platform for the current preview. macOS is a feedback
preview for Intel and Apple Silicon; full Mac container acceptance is still pending.**

Mac testers: please report your macOS version, chip type, Docker Desktop version,
installation result, and any problems with app execution or persistent data. Include
`rhyven doctor` output and the relevant error, with private details removed.

Linux dependency automation currently targets Ubuntu/Debian. Other distributions
can reuse an existing compatible engine. WSL follows the Linux route; native
Windows is not advertised as supported. Linux support does not imply that every
Docker/rootless configuration passes the required capability checks.

## Install and run

The engine is open source under Apache-2.0. Customers can download a prebuilt executable
from **rhyvenai.com**, with no GitHub account, repository access or source build.
The installer and download folder are prepared locally; **the public downloads
have not been published yet**. After deployment, the customer command will be:

```bash
curl -fsSL https://rhyvenai.com/install.sh | bash -s -- --containers
```

This installs Rhyven and initiates Docker dependency setup with user approval.
Omit `--containers` for marketplace/declarative-only use. Rust and host Python
are not required. Bash, curl, OpenSSL and a SHA-256 utility are needed by the bootstrap.

Open a new terminal if the installer added Rhyven to PATH, then launch the TUI:

```bash
rhyven
```

To connect an agent, run `rhyven --collection my-project connect --client codex`
(also Claude Code, Cursor, VS Code, Cline and generic clients). `rhyven --agent`
returns connection instructions. See [agent connection](harnesses.md).

The public runtime download is separate from marketplace app distribution.
The public registry is `rhyven-ai/registry`. Refresh it without GitHub authentication:

```bash
rhyven registry-refresh rhyven-ai/registry --anonymous
```

The agent marketplace uses that metadata directly. For CLI `search`, `inspect`
and TUI browsing, run `rhyven registry-sync rhyven-ai/registry --anonymous` to
explicitly download app manifests into the package cache. Sync does not install
apps or pull container images.

The website serves signed `0.4.0-rc.8` binaries for Linux x86-64/ARM64 and
macOS Intel/Apple Silicon. The installer checks the signed manifest and selected
binary before replacing an existing installation. See [release status](release-status.md)
for platform support and release boundaries.

### Build from source

With Rust 1.90 or newer and a C compiler installed:

```bash
git clone https://github.com/rhyven-ai/rhyven.git
cd rhyven
cargo build --release --locked --bin rhyven
./target/release/rhyven
```

`rhyven license` prints the Apache license and notices without initializing app state.
Use `rhyven license --third-party` for the embedded dependency notices.

### Offline installation

From a downloaded release directory containing the platform executable,
`SHA256SUMS`, `SHA256SUMS.sig` and the signed `install.sh`:

```bash
bash install.sh --from-dir . --containers
```

`--yes` explicitly approves the dependency setup described by the installer; it
neither accepts Docker Desktop terms nor grants permission to marketplace apps.

The default destination is `~/.local/bin/rhyven`. The bootstrap adds that directory
to `.profile` and the detected Bash/Zsh startup file; open a new terminal afterward.
Use `--bin-dir /absolute/path` or `--no-modify-path` for managed environments.
State defaults to `~/.rhyven`; `RHYVEN_HOME` selects a different state home.
Release signatures, checksums and executable compatibility are verified before replacing an existing
binary. Existing collections, selected collection and configuration are retained.

Rerun the installation command to install the latest published runtime. The
installer at the domain root is pinned to its matching release. Use
`--version vVERSION` for an available specific release or `--version latest` to
resolve the published `VERSION` once before fetching files. A custom mirror can
be selected using `--download-base-url HTTPS_URL` or `RHYVEN_DOWNLOAD_BASE_URL`.
TUI app updates update individual apps; they do not replace the Rhyven executable.

## Setup and resume

```bash
rhyven setup --containers --plan
rhyven setup --containers
rhyven doctor
```

`--plan` reports the proposed routes without changes. Setup reuses a compatible
engine. It initializes the global collection without changing the selected CLI
collection. It records `ready` or `pending` in `<home>/setup-state.json`; pending
means the binary is usable but container setup needs attention. Inspect the JSON
status; a pending report is not a claim that containers are ready. Rerun the same
setup command (and the same `--home` if customized) after login/restart/onboarding.
Interrupted installation can also be resumed by rerunning the bootstrap.

Supported automatic dependency routes implemented:

- Ubuntu/Debian: install Docker CE from its signed apt repository, enable the
  service, and add the current user to the Docker group after approval. This group
  grants root-equivalent access. Log out of all sessions or reboot before use.
  A new terminal/SSH session alone may leave the systemd user manager with old
  groups: CLI Docker access can work while a user service still gets permission
  denied. Restart the login session/user manager or reboot, then resume setup.
- Existing Linux rootless/default engine: start its service after approval. Do not
  switch contexts, remove conflicting packages, modify cgroup delegation or
  silently relax runtime restrictions.
- macOS: download the architecture-matching Docker Desktop DMG, verify vendor
  code signing and Gatekeeper assessment, invoke its installer, and open onboarding.
  Administrator prompts and Docker terms are handled by the user. Docker Desktop
  has its own licensing requirements. Physical-Mac container acceptance remains
  required even when the bootstrap and binary build tests pass.
- Other Linux distributions: reuse a compatible engine; missing-engine installation
  currently requires distribution-specific setup. Native Windows bootstrap remains
  outside the supported preview; WSL follows the Linux route.

Dependency installation does not install any marketplace app, mount project files,
or execute publisher code. Explicit image/app permission reviews are unchanged.
Registry authentication, DNS/proxy configuration, unsupported cgroup setups and
other device configuration issues remain user-controlled troubleshooting tasks.

## Release building and verification

This is the maintainer workflow. Customers only need the install command above.
Build from the reviewed source tag; publish the distribution files to static HTTPS
hosting at `rhyvenai.com` alongside the website. No customer-facing source
repository or GitHub release API is required.

`packaging/release-binaries.yml` builds Linux x86-64/ARM64 with musl and macOS
Intel/Apple Silicon, runs Rust and installer tests, and uploads per-platform
build artifacts. It does not publish automatically. The Linux musl binaries avoid
a dependency on the destination's glibc version. A normal local release build
is useful for local tests but does not establish portable Linux compatibility.

For an already-built release executable:

```bash
bash scripts/package-release.sh target/release/rhyven dist/local-release
python3 scripts/stage-downloads.py \
  --signing-key .release-signing/release-2026.pem \
  --base-url https://rhyvenai.com \
  --out dist/public-downloads \
  dist/local-release
```

For a release covering multiple platforms, pass all artifact directories from
the release build to `stage-downloads.py`. They must declare the same version
and have valid checksums. The assembler copies only supported binaries, rejects
conflicting artifacts, stamps the download origin and version into the installer,
and regenerates checksums. It refuses to overwrite an existing output directory.
It never uploads files or copies the repository, app state or credentials.

Assemble matching, tested artifacts before offering downloads for each platform.
Staging directories under `dist/` are ignored by Git.

Example output for the current candidate:

```text
public-downloads/
  install.sh
  VERSION
  releases/
    v0.4.0-rc.8/
      install.sh
      VERSION
      SHA256SUMS
      SHA256SUMS.sig
      release-key.pem
      LICENSE
      NOTICE
      THIRD_PARTY_NOTICES.txt
      rhyven-linux-x86_64
      ...other platform binaries supplied to the assembler
```

Upload the immutable versioned release folder first. Then replace root `install.sh`
and `VERSION`, using short or disabled caching for those two mutable files.
Serve assets over HTTPS with their exact filenames and without an HTML fallback
for missing downloads. Confirm the checksum and complete an installation from
the hosted URL before removing the website's pending-download notice. Do not
serve the repository root. Retain older versioned releases for pinned installs.

Keep marketplace validation compatible with the distributed runtime before
listing apps that need a new schema or execution feature. The public download
host does not host customer app data or workloads.

Run local checks with:

```bash
shellcheck scripts/*.sh
python3 qa/installer_check.py target/release/rhyven
python3 qa/direct_download_check.py target/release/rhyven
python3 qa/container_host_check.py target/release/rhyven
python3 qa/setup_routes_check.py
```

Installer regression tests use a real binary, isolated state and inert dependency
substitutes. The direct-download test serves staged files through a temporary
local HTTPS server; it does not contact the public domain. These checks do not
establish that every operating system's dependency installer works.

Dependency setup follows the upstream [Ubuntu](https://docs.docker.com/engine/install/ubuntu/),
[Debian](https://docs.docker.com/engine/install/debian/), and
[macOS](https://docs.docker.com/desktop/setup/install/mac-install/) installation routes.

## Signed releases and publication

Starting with the rc.7 candidate, the staged installer embeds the release public
key and requires an RSA/SHA-256 signature on `SHA256SUMS` before executing a
binary. It rejects unsigned or modified manifests; `--from-dir` uses the same
verification. The source installer has no trusted key until staged: developers
must pass `--public-key TRUSTED_PEM_FILE` obtained independently. Earlier rc.6
public assets remain historical, checksum-only previews.

The initial installer still depends on HTTPS and control of rhyvenai.com. Someone
who replaces both that script and its embedded key can defeat bootstrap trust.
For independent verification, retain a known-good installer/key and compare the
public-key fingerprint through a separate trusted channel. The published key
beside a binary is informational; downloading it from the same compromised mirror
is not independent verification. Version pins prevent accidental mixing; selecting
an older signed version intentionally remains possible.

Build artifacts from `package-release.sh` are intermediate, unsigned input.
Use `stage-downloads.py --signing-key ...` to create customer/offline release folders.
Keep the private key outside release folders, restricted to its OS owner, backed up
offline and excluded from Git and CI. Only public keys enter deployment output.
Do not overwrite published version directories. Include `THIRD_PARTY_NOTICES.txt`.

Prepare the full site without exposing the source checkout:

```bash
python3 scripts/stage-site.py --downloads dist/public-downloads-rc8 --out dist/site-rc8
```

See [publication runbook](publication.md) for hosting and final acceptance.
