# GitHub package distribution

Container packages add optional `execution` metadata to each index entry. It must
exactly match the manifest, including its distributable image digest and secret
names. Registry validation checks these disclosures without running container
tests or pulling images. User installation pulls the image only after consent.
The public registry uses the matching `0.4.0-rc.6` validator binary. See [release status](release-status.md) before using newer source features.

The public registry is `rhyven-ai/registry`. Its `rhyven` publisher namespace is
owned by `rhyven-ai`. It contains only distribution metadata, app packages and
registry validation tooling. The platform source is Apache-2.0 in
[`rhyven-ai/rhyven`](https://github.com/rhyven-ai/rhyven). Rhyven app source is
also available separately in [`rhyven-ai/apps`](https://github.com/rhyven-ai/apps).

The public catalog uses `rhyven/...` app IDs. Rhyven Repo Documentation Tool is
`rhyven/repo-documentation-tool`. Existing installed `official/...` apps and
`community/inventory` retain their IDs and data; new IDs are separate app state.
No app identity or customer database is silently rewritten.

## Use the public registry

Use Rhyven `0.4.0-rc.6` or newer for display names and current registry metadata.
Public discovery and downloads require no GitHub login:

```sh
rhyven registry-sync rhyven-ai/registry --anonymous
rhyven search
rhyven inspect rhyven/work-management
rhyven install rhyven/work-management
```

Review the permissions, then repeat installation with `--accept-permissions`.
Agent installation uses the marketplace's human-approval flow instead of an
agent-supplied approval flag. `registry-refresh` reads metadata and repository
stars without downloading app assets. `registry-sync` additionally downloads and
validates app manifests for CLI `search`, `inspect` and TUI browsing; it does not
install apps or pull container images. Use metadata-only refresh with the agent
marketplace when packages should only be downloaded after installation consent.

Private registries still work using `GH_TOKEN`, `GITHUB_TOKEN` or local GitHub CLI
authentication. Public registry clients can explicitly use `--anonymous`.
Credentials are sent only to allowed GitHub API endpoints, never to release
storage redirects. Failed sync or refresh leaves the previous cache intact.

A Rhyven home has one configured GitHub registry. Changing a previously configured
registry requires a new home or an explicit cache migration; it does not imply
permission to erase existing collections. For a clean public-registry trial:

```sh
rhyven --home /tmp/rhyven-public-trial registry-refresh rhyven-ai/registry --anonymous
```

## Publish through a registry PR

1. Validate and test your app with `rhyven app validate` and `rhyven app test`.
2. Package it with `rhyven app package ./app --out app.json`.
3. Upload that exact file to a GitHub release in the namespace owner's repository.
4. Obtain the numeric asset ID from `gh api repos/OWNER/REPO/releases/tags/TAG`.
5. Generate an index entry:

   ```sh
   rhyven registry-entry app.json --repository OWNER/REPO --asset-id ID
   ```

6. Add the entry to the registry's `index.json` and submit a PR.
7. Automated validation and maintainer review precede merge.

The existing `rhyven app publish` command still publishes locally. GitHub submission uses
the generated entry, GitHub releases and a registry PR; automatic release/PR
creation inside the product CLI is not implemented in this iteration.

## Index contract and validation

Index format 1 contains `publishers` (namespace -> GitHub owner) and `apps`.
Each entry contains name, optional display_name, version, description, publisher, repository, numeric
release asset ID, SHA-256 of the exact asset bytes, permissions, hosting mode,
and trust. This is distribution metadata; app package format 2 is unchanged.
Runtime canonical JSON hashes and distribution byte hashes are distinct.

`registry-validate index.json --base previous-index.json` verifies namespace
ownership, immutable prior entries, raw download hashes, metadata equality,
runtime schema validation and local declarative behavior tests. Remote app
providers are not called. New entries use `Unverified`; the legacy `Community` trust value is accepted for compatibility.
Verified and Certified labels are rejected until a real verification authority
exists. GitHub ownership is account/repository identity, not certification.

The registry workflow runs pinned validation tooling from the trusted base
branch, reads the PR's index as data, and checks the authenticated PR author.
Namespace registration is maintainer-only; an entry must be submitted by its
registered owner or the maintainer. Organizations currently submit through the
maintainer. Enable required status checks and code-owner approval through GitHub
branch protection where the repository's plan permits it. CODEOWNERS by itself
does not enforce review. GitHub records submission and merge history.

Prototype limits: 100 package versions, 1 MiB index and individual package,
30-second HTTP request timeout, eager downloads at sync, append-only index,
and one registry per workspace. Revocation and namespace transfer need explicit
future support. The cache trusts local workspace principals, like the database.

## Verification

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
python3 qa/public_registry_check.py ./target/release/rhyven
```

The public registry check uses temporary state and anonymous downloads. It tests
five app installs through MCP with simulated host acceptance/denial, publisher
labels, stars, useful app calls, retained-state reinstall and a second agent.

## Agent discovery without downloads

Use `registry-refresh OWNER/REPO [--anonymous]` for the agent marketplace. It
caches index metadata and GitHub repository stars without downloading packages.
The agent browses and prepares operations through `rhyven/marketplace`, then
obtains user consent before apply/download. Existing `registry-sync` remains the
explicit eager-download CLI/TUI flow. See [agent marketplace](agent-marketplace.md).

`registry-entry` now includes optional `hosting_details` for remote entries.
Local legacy entries remain supported; remote entries need these disclosures
before an agent can prepare download approval. The package format is unchanged.
The deployed registry validator must be rebuilt and repinned before publishing
remote entries with this new field; this local implementation does not automatically
replace the previously published validator release/workflow.
