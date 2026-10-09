# Linux packages

Rhyven packages install the CLI, terminal marketplace, runtime, MCP adapter and
embedded skills as `/usr/bin/rhyven`. Rust and Docker are not required to install
or use declarative state apps. Python/Node script apps need their declared host
runtime. Container apps need a compatible Docker engine.

Download the package matching your distribution and CPU from the
[0.7.0 release](https://github.com/rhyven-ai/rhyven/releases/tag/v0.7.0).
`uname -m` prints `x86_64` or `aarch64` on supported machines.

| Distribution family | Package | Install downloaded file |
| --- | --- | --- |
| Debian, Ubuntu, Linux Mint, Pop!_OS | `.deb` | `sudo apt install ./rhyven-0.7.0-1-x86_64.deb` |
| Fedora, RHEL-compatible systems | `.rpm` | `sudo dnf install ./rhyven-0.7.0-1-x86_64.rpm` |
| openSUSE | `.rpm` | `sudo zypper install ./rhyven-0.7.0-1-x86_64.rpm` |
| Arch, EndeavourOS, Manjaro | `.pkg.tar.zst` | `sudo pacman -U ./rhyven-0.7.0-1-x86_64.pkg.tar.zst` |
| Alpine | `.apk` | `sudo apk add --allow-untrusted ./rhyven-0.7.0-1-x86_64.apk` |

Replace `x86_64` with `aarch64` for ARM64. Arch Linux ARM requires separate
acceptance testing; an ARM64 package build is not a claim of official Arch ARM
support. Related distributions can share a format without having been tested
individually. Immutable distributions may require their own package layering
workflow. The signed curl installer remains available for user-local installs.

## Verify before installing

Packages have a separate signed checksum manifest, `PACKAGE-SHA256SUMS` and
`PACKAGE-SHA256SUMS.sig`, using Rhyven's existing RSA release key. These are not
native repository/GPG signatures. Download both alongside your package, and
verify them with your previously trusted Rhyven `release-key.pem`:

```sh
openssl dgst -sha256 -verify release-key.pem \
  -signature PACKAGE-SHA256SUMS.sig PACKAGE-SHA256SUMS
sha256sum --ignore-missing --check PACKAGE-SHA256SUMS
```

Both commands must succeed and the downloaded package must appear as `OK`.
On first use, obtain and review the public key from the Rhyven source repository
(`packaging/release-key.txt`); downloading a key next to a file alone does not
establish independent trust. Alpine's `--allow-untrusted` bypasses its native
key store, so perform the release-signature verification above first. RPM may
also report that the package lacks a native OpenPGP signature.

## Start as your normal user

```sh
rhyven setup
rhyven
```

Or connect an existing agent:

```sh
rhyven --collection my-project connect --client codex
rhyven --collection my-project connect --check
```

Substitute your supported client. Setup installs all skills per user and fetches
the catalog. Package manager hooks do not initialize user data, install Docker,
change groups or start services. Docker setup remains an explicit separate
`rhyven setup --containers` step; automation currently targets Ubuntu/Debian.

## Update or remove

Install the newer downloaded package using the same package manager. No APT,
DNF or pacman repository is added automatically. `rhyven upgrade` recognizes a
system-package installation and directs you to use the package manager rather
than replacing its binary. App updates are separate from runtime updates.

Remove through your package manager (`apt remove rhyven`, `dnf remove rhyven`,
`zypper remove rhyven`, `pacman -R rhyven`, or `apk del rhyven`). User collections
and skills under `~/.rhyven` are retained. Reinstalling can reuse that state.

If a previous curl install exists, `~/.local/bin/rhyven` may precede `/usr/bin`.
Check `type -a rhyven` and `rhyven --version`; do not assume installing a system
package replaced a separate user-local executable. Select the intended path or
remove the obsolete user-local binary after checking it. Do not delete app data.
