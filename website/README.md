# Rhyven local website

A static, local site with five top-level pages: Marketplace, Corvid, Razorback,
Docs and Skills. No framework, build step, npm install, credentials or runtime service is
required to view it. All fonts, imagery and data are served locally. There are no
analytics or external requests.

From the repository root:

```sh
python3 website/serve.py
```

Open **http://localhost:5173**. The server binds to loopback and serves only this
website directory. Stop with Ctrl+C; use `--port 5174` if needed.

## What works

- Hash-based navigation, browser back/forward and direct documentation links.
- Marketplace search, type filters, expandable engine sections and app details.
- Real local manifest descriptions, versions, permissions, objects and actions.
- Copyable install examples and an interactive three-step interoperability example.
- Corvid harness page marked in progress, with a generated pixel-art murder of crows.
- Razorback coming-soon emblem and construction tape.
- Thirteen documentation topics with text search, copyable code and the planned one-line binary installer at `rhyvenai.com/install.sh`.
  **Declarative features** (`#docs/declarative-features`) covers schemas, actions,
  every expression/query operator, relationships, state, tests and recovery,
  with examples and explicit limits.
- Five downloadable agent skills: using Rhyven, publishing, declarative apps,
  on-demand containers and persistent services. Each previews and copies the exact
  SKILL.md file, displays its line count (maximum 500), and supports direct links.
  Use Rhyven also offers a shorter RULE.md for project instructions. Both container
  authoring skills include Dockerfile instructions.
- Responsive layouts, keyboard navigation, native dialogs, visible focus states,
  screen-reader announcements and reduced-motion support.

The website is a catalog/documentation preview. It does not install apps or call
the runtime, registry or GitHub. There are no invented repository stars, user
counts, certification badges or working public-download claims. App availability
comes from `data/availability.json`, an explicit snapshot of published app versions.
Unpublished apps show a pending notice instead of installation commands for unreleased packages.

## Update catalog content

```sh
python3 website/scripts/sync-catalog.py
```

This regenerates `data/catalog.json` from the five declarative packages plus
Rhyven Repo Documentation Tool and Messaging, with the current Cargo workspace
version. Update `data/availability.json` only after an app version is published
and its packages/images can be downloaded anonymously. An exact ID/version
match enables public marketplace installation instructions. Editorial card
summaries and website docs live in `app.js` and `docs.js`. Update the documented
source/distribution status when releasing a new build.

## Prepare public downloads

The engine and website source are Apache-2.0 at `rhyven-ai/rhyven`. Customer installation downloads prebuilt
binaries directly from `https://rhyvenai.com`; it requires no GitHub account.
Getting started shows that command followed by `rhyven`, and clearly marks the
domain installer as not yet deployed. The Linux x86-64 preview binary and all
seven app packages are available in the public `rhyven-ai/registry`. The earlier public
container images passed anonymous fresh-host installation; the newer container
versions shown in this source catalog are still pending publication.

Use the staging command to prepare the installer, checksums and versioned binaries:

```sh
python3 scripts/stage-downloads.py --signing-key PRIVATE_KEY_PATH --base-url https://rhyvenai.com --out NEW_DIRECTORY ARTIFACT_DIRECTORY
```

See [the distribution guide](../docs/installation.md#release-building-and-verification)
for full commands and deployment order. This does not upload anything. Serve that
directory alongside this website at the domain root; no source checkout or GitHub
API is needed by customers. Remove the pending-download notice only after the
hosted installation is verified.

## Files

- `index.html`: page structure and small inline icon symbols.
- `styles.css`: shared styling, responsive layouts and motion preferences.
- `app.js`: navigation, catalog, details, search, clipboard and workflow example.
- `docs.js`: detailed documentation and shared rendering helpers.
- `skills.js`: skill navigation, loading, source preview and download links.
- `skills/*/SKILL.md`: portable agent instructions with YAML frontmatter. Keep each
  at 500 lines or fewer. Edit these files directly; previews and clipboard text
  load them without a second copy. Add new skills to `skills.js` and the explicit
  public-file allowlist in `scripts/stage-site.py`.
- `skills/use-rhyven/RULE.md`: the shorter usage rule, available at
  `#skills/use-rhyven/rule`. The skill uses `#skills/use-rhyven`. Both are also
  explicitly selected by the clean app-source export script.
- `assets/rhyven-brand.png`: the user's original brand image, unchanged. CSS crops
  the displayed viewport to the supplied icon; the old slogan is not displayed.
- `assets/corvid-crows-pixel.png`: generated pixel-art website background; provenance are in `assets/ASSETS.md`.
- `assets/fonts/`: locally served Lato fonts with their OFL notice.

## Browser verification

The optional QA script uses Playwright and axe-core. Install these tools outside
this project if desired; they are not website runtime dependencies:

```sh
npm install --ignore-scripts --prefix /tmp/rhyven-website-check playwright @axe-core/playwright
/tmp/rhyven-website-check/node_modules/.bin/playwright install chromium
NODE_PATH=/tmp/rhyven-website-check/node_modules node qa/website_check.cjs
```

Run the local server first. The browser check covers navigation, filters, search,
accordion/detail behavior, clipboard, docs links, skill downloads/copy/line limits,
loading failures and retry, mobile overflow and accessibility.
Screenshots are written to `/tmp/rhyven-website-screenshots`.

Verified on 2026-09-27 in headless Chromium at 390, 768 and 1440 pixels:
all five pages, the five skills and usage rule, the seven-app catalog, app details, search/filter/accordion controls, clipboard,
keyboard navigation and all thirteen documentation topics passed. Automated axe
WCAG A/AA checks reported no violations in the tested views, with no browser
errors or external requests. Desktop and mobile screenshots were also inspected.
Automated checks do not replace a full assistive-technology audit.

The Use Rhyven skill also passed rc.7 CLI checks in a temporary home: connection
instructions, collection identity, marketplace search/inspection and refusal to
apply an unapproved request. Its task-creation example passed an isolated app
conformance test. These checks downloaded no apps and changed no agent settings.

The Declarative features page's rendered stock-withdrawal and issue-search
examples passed 11 isolated runtime cases, including guarded rejection, revision
conflicts and identical retries. Its scaffold/validate/test/package commands
also completed successfully in a temporary home.

The declarative skill's complete stock example also passed the rc.7 binary's
validator, four behavior tests, packaging, isolated installation, discovery,
description, creation and persisted search from a second CLI invocation. Both
container scaffolds validate and their Python source compiles; this website
change did not rerun Docker execution tests. All four files passed the skill
frontmatter validator and remain below 500 lines.

On a minimal Linux host, Playwright may need additional browser system libraries;
that affects the optional QA tools, not the website or its Python preview server.

Production staging and hosting: [publication runbook](../docs/publication.md). Deploy only the output of `scripts/stage-site.py`, never the repository root.
