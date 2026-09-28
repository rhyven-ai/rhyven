# Publishing runtime releases and app packages

The engine is Apache-2.0 in [rhyven-ai/rhyven](https://github.com/rhyven-ai/rhyven).
App source lives in [rhyven-ai/apps](https://github.com/rhyven-ai/apps), and
[rhyven-ai/registry](https://github.com/rhyven-ai/registry) distributes app metadata
and packages. Customer workloads and state stay on customer infrastructure.

## Runtime releases

1. Run workspace tests, transport/installer checks, the source export tests and
   `python3 scripts/security-scan.py`. Review app container reports separately.
2. Build a distinct version with Rust 1.90, `--locked`, the release profile and
   path remapping. `packaging/release-binaries.yml` defines the build matrix.
3. Package each accepted platform with `scripts/package-release.sh`. Intermediate
   artifacts include the executable, installer, version, Apache license, notices
   and third-party license text.
4. Sign the accepted artifacts using an owner-controlled private key:

   ```sh
   python3 scripts/stage-downloads.py --base-url https://rhyvenai.com \
     --signing-key /secure/path/release-key.pem \
     --out dist/public-downloads-rc8 dist/linux-x86_64-rc8
   ```

5. Deliver only that signed download tree through the separately maintained
   deployment process. Do not upload source checkouts, customer state, credentials
   or signing keys. Never overwrite a published version directory.
6. Verify hosted signatures and checksums before updating the public registry's
   pinned validator. Runtime and registry validation must accept the same app
   contract. See [release status](release-status.md) for the promoted versions.

The installer verifies its embedded public key. Initial delivery still trusts
HTTPS and the download origin. Publish the key fingerprint through a separately
trusted channel and retain a known-good key for independent verification. Keep
private keys offline, access-restricted and excluded from Git and build logs.

## App packages

Image publication requires explicit maintainer dispatch with `audit_only=false`.
The image workflow builds the matching runtime, tests app behavior and scans the
image before a push. Packages pin immutable image digests.

Promote compatible runtime/validator versions before accepting new schema
features. Release new app versions instead of replacing published packages or
image digests. A changed Dockerfile does not repair an already published image.
Complete anonymous installation acceptance against the exact packages and images.

## Acceptance

- `qa/direct_download_check.py` tests signed installation, tampering rejection and
  retained state using temporary local HTTPS hosting.
- `packaging/installer-vm-acceptance.yml` tests the public installer in a fresh VM.
- `packaging/public-app-acceptance.yml` verifies registry packages and container
  execution using the published runtime.
- Exercise connection verification, permission review, installation, discovery,
  app operations, updates, backup/restore and retained state after reinstall.

Website source, branding assets and browser tests are maintained separately from
this engine repository. Agent usage and authoring instructions remain in `skills/`.
