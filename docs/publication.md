# Publishing the website and release binaries

The engine and website source are Apache-2.0 in
[`rhyven-ai/rhyven`](https://github.com/rhyven-ai/rhyven). Rhyven app source is
also published in [`rhyven-ai/apps`](https://github.com/rhyven-ai/apps), and
[`rhyven-ai/registry`](https://github.com/rhyven-ai/registry) distributes app
packages and metadata. Customer workloads and state remain on customer infrastructure.

Source publication and binary promotion are separate steps. The rc.8 source
includes the open-source license and embedded notices. The public runtime and
registry validator are still rc.6; do not advertise new candidate features as
available in that binary. See [release status](release-status.md).

## Prepare a release

1. Run workspace tests, transport/installer checks, browser checks and
   `python3 scripts/security-scan.py`. Review container image reports separately;
   a Rust dependency scan does not cover Docker images.
2. Tag the reviewed source with a distinct version. Build with Rust 1.90,
   `--locked`, the release profile and path remapping for the checkout, Cargo home
   and Rustup home. `.github/workflows/release-binaries.yml` defines the build matrix.
3. Produce intermediate artifacts with `scripts/package-release.sh`. The package
   contains the executable, installer, version, Apache `LICENSE`, `NOTICE` and
   `THIRD_PARTY_NOTICES.txt`. The binary also exposes these through `rhyven license`.
4. Sign and stage downloads and the website using an owner-controlled signing key:

   ```bash
   python3 scripts/stage-downloads.py --base-url https://rhyvenai.com \
     --signing-key /secure/path/release-key.pem \
     --out dist/public-downloads-rc8 dist/linux-x86_64-rc8
   python3 scripts/stage-site.py --downloads dist/public-downloads-rc8 --out dist/site-rc8
   ```

   Both commands refuse to overwrite existing output. Include only platform
   artifacts that passed acceptance. macOS remains a feedback preview.
5. Review the exact output, signatures, license notices, links and product claims.
   Deploy only the staged site, never the checkout or all of `dist/`. App state,
   credentials, signing keys, development archives and raw audit reports must stay
   out of public artifacts. Open source does not make those files public.

## Container packages and registry compatibility

The app-image workflow defaults to building, testing and scanning without publishing.
Image publication requires an explicit maintainer dispatch with `audit_only=false`.
It builds the runtime from the checked-out source, tests behavior and scans the
image before pushing. Each package pins an immutable image digest.

Promote compatible versions of the runtime and registry validator together.
Update the public registry's pinned validator hash and acceptance workflow when
promoting a new binary. Only then list packages requiring newer schema features.
Add new app versions rather than overwriting existing releases. Complete anonymous
installation acceptance, update `website/data/availability.json`, regenerate its
catalog and stage the final website. A new Dockerfile does not fix an old image
already published under a different digest.

## Host configuration

Use static HTTPS hosting with automatic certificate renewal and account MFA.
The staged `_headers` file specifies CSP, HSTS, frame protection, MIME protection,
referrer policy and restricted browser capabilities. Configure equivalent response
headers if the chosen host ignores that file.

Serve JavaScript as JavaScript, CSS as `text/css`, JSON as `application/json`,
`.sh` and skill `.md` files as `text/plain`, and binaries/signatures as
`application/octet-stream`. Disable directory listings. Missing downloads must
return 404, not an HTML fallback. Root `install.sh`, `VERSION`, HTML, JavaScript
and catalog files must revalidate caches. Immutable versioned release folders can
be cached for one year and must never be replaced. Upload the versioned release
before updating the root installer and `VERSION` together.

Do not expose the local preview server or runtime REST port as the public website.
The site needs no backend, cookies, analytics or customer credentials.

## Acceptance on the actual domain

- Check TLS, security headers, license downloads and real 404 responses.
- Verify that `/.git/config`, local state paths and development archives are absent.
- Install on a fresh supported Linux system. Verify the version, dependency prompts,
  agent connection and selected collection.
- Search the registry; review permissions; approve, install and use an app. Exercise
  updates, backup/restore and retained state after removal/reinstallation.
- Repeat the container journey with the exact image digests being published.
- Check all five skills, the usage rule, source links, copy buttons and downloads.
- Confirm private vulnerability reporting works in the engine repository.

The installer trusts its embedded public key. Initial delivery of that key still
trusts the HTTPS origin. Publish its fingerprint through a separate trusted channel
and retain a known-good key for independent verification. Keep private signing keys
offline, access-restricted and excluded from Git, CI logs and release output.
